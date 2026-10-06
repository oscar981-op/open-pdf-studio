//! First bounded DynLex paint path for native annotations over an opaque page.
//! Editing, selection handles and unsupported annotation kinds remain with the
//! host until their visual output is independently checked against PDF /AP.
use crate::Bitmap;
use opds_engine_probe::Raster;
use opds_native_core::annotations::Annotation;
#[cfg(feature = "kernel")]
use opds_native_core::annotations::{Kind, Rect};

#[cfg(feature = "kernel")]
fn whole(value: f64, limit: u32) -> Option<u32> {
    (value.is_finite() && value >= 0.0 && value <= f64::from(limit) && value.fract() == 0.0)
        .then(|| value as u32)
}

#[cfg(feature = "kernel")]
fn pixel_rect(bounds: Rect, width: u32, height: u32) -> Option<[u32; 4]> {
    let x0 = whole(bounds.x, width)?;
    let y0 = whole(bounds.y, height)?;
    let x1 = whole(bounds.x + bounds.width, width)?;
    let y1 = whole(bounds.y + bounds.height, height)?;
    (x1 >= x0 + 4 && y1 >= y0 + 4).then_some([x0, y0, x1, y1])
}

#[cfg(feature = "kernel")]
fn command(scene: &mut Vec<u8>, kind: u32, rect: [u32; 4], tail: [u8; 4]) {
    for value in [kind, rect[0], rect[1], rect[2], rect[3]] {
        scene.extend_from_slice(&value.to_le_bytes());
    }
    scene.extend_from_slice(&tail);
}

/// Return `None` rather than partially painting a document when any native
/// annotation is outside the verified 1px/page-point Square subset.
pub fn render_native_annotation_overlay(
    base: &Raster,
    annotations: &[Annotation],
    page: usize,
) -> Result<Option<Bitmap>, String> {
    render_native_annotation_overlay_pixels(
        &base.rgba,
        base.width,
        base.height,
        (base.page_width_pt, base.page_height_pt),
        annotations,
        page,
    )
}

/// Borrow the page pixels. DynLex paints the bounded marks on one cloned RGBA
/// output, without materializing a second full-page OPDL RGB payload.
pub fn render_native_annotation_overlay_pixels(
    base_rgba: &[u8],
    width: u32,
    height: u32,
    page_points: (f32, f32),
    annotations: &[Annotation],
    page: usize,
) -> Result<Option<Bitmap>, String> {
    #[cfg(not(feature = "kernel"))]
    {
        let _ = (base_rgba, width, height, page_points, annotations, page);
        return Ok(None);
    }
    #[cfg(feature = "kernel")]
    {
        if width == 0
            || height == 0
            || width > 4096
            || height > 4096
            || u64::from(width) * u64::from(height) > 6_000_000
            || page_points.0 != width as f32
            || page_points.1 != height as f32
            || base_rgba.len() != (width as usize) * (height as usize) * 4
            || base_rgba.chunks_exact(4).any(|pixel| pixel[3] != 255)
        {
            return Ok(None);
        }
        let marks: Vec<_> = annotations
            .iter()
            .filter(|item| item.page == page)
            .collect();
        if marks.is_empty() || marks.len() > 64 {
            return Ok(None);
        }
        let mut rectangles = Vec::with_capacity(marks.len());
        for mark in marks {
            let Kind::Rectangle { color_rgb } = &mark.kind else {
                return Ok(None);
            };
            let Some(bounds) = pixel_rect(mark.bounds, width, height) else {
                return Ok(None);
            };
            if *color_rgb == [255, 226, 67]
                && (bounds[1]..bounds[3]).any(|y| {
                    (bounds[0]..bounds[2]).any(|x| {
                        let at = ((y * width + x) as usize) * 4;
                        base_rgba[at..at + 3] != [255, 255, 255]
                    })
                })
            {
                // The PDF transparency group uses a different 8-bit rounding
                // path over colored page content. Only white underlays have
                // been proved byte-identical so far.
                return Ok(None);
            }
            if rectangles
                .iter()
                .any(|(prior, prior_color): &([u32; 4], [u8; 3])| {
                    (bounds[0] < prior[2]
                        && prior[0] < bounds[2]
                        && bounds[1] < prior[3]
                        && prior[1] < bounds[3])
                        && (*color_rgb == [255, 226, 67] || *prior_color == [255, 226, 67])
                })
            {
                // Overlapping highlight appearances use a separate PDF
                // transparency group. Its edge rounding is not represented
                // by the current opaque-page OPAN commands.
                return Ok(None);
            }
            rectangles.push((bounds, *color_rgb));
        }
        let mut scene = Vec::with_capacity(20 + rectangles.len() * 24);
        scene.extend_from_slice(b"OPAN");
        for value in [1, width, height, rectangles.len() as u32] {
            scene.extend_from_slice(&value.to_le_bytes());
        }
        for ([x0, y0, x1, y1], color) in rectangles {
            if color == [255, 226, 67] {
                // PDF /ca 0.35 rounds to the 8-bit compositor's 90/255 step.
                command(
                    &mut scene,
                    1,
                    [x0, y0, x1, y1],
                    [color[0], color[1], color[2], 90],
                );
            } else {
                command(
                    &mut scene,
                    2,
                    [x0, y0, x1, y1],
                    [color[0], color[1], color[2], 255],
                );
            }
        }
        debug_assert!(scene.len() <= 1_556);
        let mut output = base_rgba.to_vec();
        // SAFETY: Rust checked the full mark list, sizes and pixel buffer.
        // The ABI wrapper rechecks lengths before invoking the DynLex painter.
        let status = unsafe {
            super::opds_paint_annotations(
                scene.as_ptr(),
                i32::try_from(scene.len()).map_err(|_| "annotation scene too large")?,
                output.as_mut_ptr(),
                i32::try_from(output.len()).map_err(|_| "annotation bitmap too large")?,
            )
        };
        if status != 0 {
            return Err(format!("DynLex annotation kernel status {status}"));
        }
        Ok(Some(Bitmap {
            width,
            height,
            rgba: output,
        }))
    }
}

#[cfg(all(test, feature = "kernel"))]
mod tests {
    #[test]
    fn annotation_abi_rejects_null_short_and_mismatched_buffers() {
        let mut scene = Vec::from(&b"OPAN"[..]);
        for value in [1_u32, 4, 4, 0] {
            scene.extend_from_slice(&value.to_le_bytes());
        }
        let mut pixels = [255_u8; 64];
        // SAFETY: Only the ABI's own rejection paths are exercised; it must
        // reject before dereferencing a null or undersized allocation.
        unsafe {
            assert_eq!(
                super::super::opds_paint_annotations(
                    std::ptr::null(),
                    scene.len() as i32,
                    pixels.as_mut_ptr(),
                    pixels.len() as i32,
                ),
                2
            );
            assert_eq!(
                super::super::opds_paint_annotations(
                    scene.as_ptr(),
                    scene.len() as i32,
                    pixels.as_mut_ptr(),
                    63,
                ),
                2
            );
            scene[16..20].copy_from_slice(&1_u32.to_le_bytes());
            assert_eq!(
                super::super::opds_paint_annotations(
                    scene.as_ptr(),
                    scene.len() as i32,
                    pixels.as_mut_ptr(),
                    pixels.len() as i32,
                ),
                2
            );
        }
    }
}
