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

/// The optional transparent PDF-content layer proves that a colored source
/// under a Highlight is fully opaque before the annotation is composited.
/// The caller must bind this layer to the same rendered page and scale.
pub fn render_native_annotation_overlay_pixels_with_content(
    base_rgba: &[u8],
    content_rgba: &[u8],
    width: u32,
    height: u32,
    page_points: (f32, f32),
    annotations: &[Annotation],
    page: usize,
) -> Result<Option<Bitmap>, String> {
    render_native_annotation_overlay_pixels_inner(
        base_rgba,
        Some(content_rgba),
        width,
        height,
        page_points,
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
    render_native_annotation_overlay_pixels_inner(
        base_rgba,
        None,
        width,
        height,
        page_points,
        annotations,
        page,
    )
}

fn render_native_annotation_overlay_pixels_inner(
    base_rgba: &[u8],
    content_rgba: Option<&[u8]>,
    width: u32,
    height: u32,
    page_points: (f32, f32),
    annotations: &[Annotation],
    page: usize,
) -> Result<Option<Bitmap>, String> {
    #[cfg(not(feature = "kernel"))]
    {
        let _ = (
            base_rgba,
            content_rgba,
            width,
            height,
            page_points,
            annotations,
            page,
        );
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
            if *color_rgb == [255, 226, 67] {
                let mut colored_underlay = false;
                for y in bounds[1]..bounds[3] {
                    for x in bounds[0]..bounds[2] {
                        let at = ((y * width + x) as usize) * 4;
                        if base_rgba[at..at + 3] != [255, 255, 255] {
                            colored_underlay = true;
                            break;
                        }
                    }
                    if colored_underlay {
                        break;
                    }
                }
                if colored_underlay {
                    let Some(content) = content_rgba.filter(|layer| layer.len() == base_rgba.len())
                    else {
                        return Ok(None);
                    };
                    for y in bounds[1]..bounds[3] {
                        for x in bounds[0]..bounds[2] {
                            let at = ((y * width + x) as usize) * 4;
                            if content[at + 3] != 255
                                || content[at..at + 3] != base_rgba[at..at + 3]
                            {
                                // Compositing over a translucent PDF source
                                // group can differ by thousands of 8-bit
                                // channels after the viewer adds white paper.
                                return Ok(None);
                            }
                        }
                    }
                }
            }
            rectangles.push((bounds, *color_rgb));
        }
        let mut scene = Vec::with_capacity(20 + rectangles.len() * 9 * 24);
        scene.extend_from_slice(b"OPAN");
        for value in [1, width, height, 0] {
            scene.extend_from_slice(&value.to_le_bytes());
        }
        let mut command_count = 0_u32;
        for ([x0, y0, x1, y1], color) in rectangles {
            if color == [255, 226, 67] {
                // This native /AP's 0.35 opacity and 1 0.886 0.263 rg paint
                // produce premultiplied [89,79,23,89] in Hayro. Partial
                // four-pixel tiles use a separately rounded masked blend.
                // Partitioning once here keeps both paint paths in DynLex.
                // Vello's start tile goes through its mask path even when
                // the left edge happens to sit exactly on a tile boundary.
                let left_full = (x0 / 4 + 1) * 4;
                let right_full = (x1 / 4) * 4;
                let top_full = y0.div_ceil(4) * 4;
                let bottom_full = (y1 / 4) * 4;
                let mut xs = [x0, left_full.min(x1), right_full.max(x0), x1];
                let mut ys = [y0, top_full.min(y1), bottom_full.max(y0), y1];
                xs.sort_unstable();
                ys.sort_unstable();
                for [y_start, y_end] in ys.windows(2).map(|window| [window[0], window[1]]) {
                    for [x_start, x_end] in xs.windows(2).map(|window| [window[0], window[1]]) {
                        if x_start == x_end || y_start == y_end {
                            continue;
                        }
                        let masked = x_start < left_full
                            || x_start >= right_full
                            || y_start < top_full
                            || y_start >= bottom_full;
                        command(
                            &mut scene,
                            if masked { 3 } else { 4 },
                            [x_start, y_start, x_end, y_end],
                            [89, 79, 23, 89],
                        );
                        command_count += 1;
                    }
                }
            } else {
                command(
                    &mut scene,
                    2,
                    [x0, y0, x1, y1],
                    [color[0], color[1], color[2], 255],
                );
                command_count += 1;
            }
        }
        scene[16..20].copy_from_slice(&command_count.to_le_bytes());
        debug_assert!(command_count <= 576 && scene.len() <= 13_844);
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
    use super::*;

    #[test]
    fn sixty_four_disjoint_highlights_fit_expanded_command_bound() {
        let marks: Vec<_> = (0..64)
            .map(|index| Annotation {
                id: index + 1,
                page: 0,
                bounds: Rect {
                    x: (5 + (index % 8) * 16) as f64,
                    y: (7 + (index / 8) * 16) as f64,
                    width: 9.0,
                    height: 9.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [255, 226, 67],
                },
            })
            .collect();
        let white = vec![255_u8; 256 * 256 * 4];
        let output =
            render_native_annotation_overlay_pixels(&white, 256, 256, (256.0, 256.0), &marks, 0)
                .unwrap()
                .expect("64 disjoint Highlights must fit the bounded OPAN scene");
        assert_ne!(output.rgba, white);
    }

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
            scene.resize(20 + 577 * 24, 0);
            scene[16..20].copy_from_slice(&577_u32.to_le_bytes());
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
