//! Opt-in native Rust boundary for the bounded DynLex vector kernel.
//! Unsupported PDFs return None so the host can keep its existing renderer.
mod annotation_overlay;
mod prepared;
pub use annotation_overlay::{
    render_native_annotation_overlay, render_native_annotation_overlay_pixels,
    render_native_annotation_overlay_pixels_on_opaque_source,
    render_native_annotation_overlay_pixels_over_premultiplied,
    render_native_annotation_overlay_pixels_with_content, render_native_selection_overlay_pixels,
    render_native_ui_preview_pixels,
};
pub use prepared::DynlexDocument;
#[cfg(feature = "kernel")]
pub use prepared::{PixelViewport, PreparedPage};
#[cfg(feature = "kernel")]
mod appearance_stage;
#[cfg(feature = "kernel")]
pub use appearance_stage::flatten_native_free_text_appearance;
#[cfg(feature = "kernel")]
mod imported_appearance;
#[cfg(feature = "kernel")]
mod imported_text;
#[cfg(feature = "kernel")]
pub use imported_appearance::{
    render_imported_shape_appearances_for_app, render_missing_shape_appearances_for_app,
    render_native_shape_appearances_direct, render_native_shape_appearances_for_app,
    render_single_square_appearance_direct, render_square_appearances_direct,
};
#[cfg(feature = "kernel")]
mod glyph_scene;
#[cfg(feature = "kernel")]
pub use glyph_scene::{
    render_validated_glyph_layer, render_validated_glyph_scene,
    render_validated_glyph_scene_over_opaque, render_validated_glyph_scene_over_opaque_at_origin,
    render_validated_glyph_scene_over_opaque_at_origin_owned,
    render_validated_glyph_scene_over_premultiplied,
    render_validated_glyph_scene_over_premultiplied_at_origin,
    render_validated_glyph_scene_over_premultiplied_at_origin_owned,
};
use opds_engine_probe::{ParsedPdf, Raster, RenderError};
#[cfg(feature = "kernel")]
use opds_opdl_adapter::{
    geometry_v3::page_to_opgm_v3_bytes, page_to_opdl_bytes, page_to_opdl_ordered_bytes,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Hayro,
    Dynlex,
    Pdfium,
    OwnRust,
}

/// PDFium is loaded dynamically only for the opt-in feature. A missing
/// library leaves the native viewer on its existing Hayro route.
pub fn pdfium_available() -> bool {
    #[cfg(feature = "pdfium")]
    {
        opds_pdfium_backend::available()
    }
    #[cfg(not(feature = "pdfium"))]
    {
        false
    }
}

pub fn own_rust_available() -> bool {
    cfg!(feature = "own-rust")
}

impl Default for Backend {
    fn default() -> Self {
        Self::Hayro
    }
}

#[derive(Debug, Clone)]
pub struct RenderOutcome {
    pub raster: Raster,
    pub used: Backend,
    /// True when the PDF body came from Hayro but imported shapes came
    /// from the validated in-process DynLex annotation compositor.
    pub imported_annotations_dynlex: bool,
}

/// Keep Hayro operational for every page/zoom outside the verified DynLex subset.
pub fn render_with_fallback(
    pdf: &ParsedPdf,
    source: &[u8],
    page: usize,
    scale: f32,
    backend: Backend,
) -> Result<Raster, RenderError> {
    Ok(render_with_report(pdf, source, page, scale, backend)?.raster)
}

pub fn render_with_report(
    pdf: &ParsedPdf,
    source: &[u8],
    page: usize,
    scale: f32,
    backend: Backend,
) -> Result<RenderOutcome, RenderError> {
    render_with_report_using(pdf, source, page, scale, backend, || {
        try_render(source, page, scale)
    })
}

fn render_with_report_using(
    pdf: &ParsedPdf,
    source: &[u8],
    page: usize,
    scale: f32,
    backend: Backend,
    dynlex_render: impl FnOnce() -> Result<Option<Bitmap>, String>,
) -> Result<RenderOutcome, RenderError> {
    #[cfg(not(any(feature = "kernel", feature = "pdfium", feature = "own-rust")))]
    let _ = source;
    #[cfg(feature = "own-rust")]
    if backend == Backend::OwnRust {
        if let Ok(raster) = opds_rust_kernel::render(source, page, scale) {
            return Ok(RenderOutcome {
                raster,
                used: Backend::OwnRust,
                imported_annotations_dynlex: false,
            });
        }
    }
    #[cfg(feature = "pdfium")]
    if backend == Backend::Pdfium {
        if let Ok(raster) = opds_pdfium_backend::render(source, page, scale) {
            if let Ok((width, height)) = pdf.page_dimensions(page) {
                if (raster.page_width_pt - width).abs() <= 0.01
                    && (raster.page_height_pt - height).abs() <= 0.01
                {
                    return Ok(RenderOutcome {
                        raster,
                        used: Backend::Pdfium,
                        imported_annotations_dynlex: false,
                    });
                }
            }
        }
    }
    if backend == Backend::Dynlex {
        let started = std::time::Instant::now();
        #[cfg(feature = "kernel")]
        if scale == 1.0 && source.windows(7).any(|window| window == b"/Annots") {
            if let Ok(Some(raster)) = render_imported_shape_appearances_for_app(pdf, source, page) {
                return Ok(RenderOutcome {
                    used: Backend::Hayro,
                    imported_annotations_dynlex: true,
                    raster,
                });
            }
        }
        if let Ok(Some(bitmap)) = dynlex_render() {
            let (page_width_pt, page_height_pt) = pdf.page_dimensions(page)?;
            return Ok(RenderOutcome {
                used: Backend::Dynlex,
                imported_annotations_dynlex: false,
                raster: Raster {
                    width: bitmap.width,
                    height: bitmap.height,
                    rgba: bitmap.rgba,
                    page_width_pt,
                    page_height_pt,
                    total_pages: pdf.page_count(),
                    parse_ms: 0,
                    render_ms: started.elapsed().as_millis(),
                },
            });
        }
    }
    Ok(RenderOutcome {
        used: Backend::Hayro,
        imported_annotations_dynlex: false,
        raster: pdf.render_page(page, scale)?,
    })
}

#[derive(Debug, Clone)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[cfg(feature = "kernel")]
#[link(name = "opds_dynlex_buffer", kind = "static")]
unsafe extern "C" {
    fn opds_render_buffer(
        scene: *const u8,
        scene_len: i32,
        output: *mut u8,
        output_len: i32,
        mode: i32,
    ) -> i32;
}

#[cfg(feature = "kernel")]
#[link(name = "opds_dynlex_annotation", kind = "static")]
unsafe extern "C" {
    fn opds_paint_annotations(
        scene: *const u8,
        scene_len: i32,
        rgba: *mut u8,
        rgba_len: i32,
    ) -> i32;
}

/// Prepare a bounded first-page scene and render the supported zoom subset.
/// Use DynlexDocument in a viewer to retain this preparation across renders.
pub fn try_render(bytes: &[u8], page: usize, scale: f32) -> Result<Option<Bitmap>, String> {
    if page != 1 || !scale.is_finite() || !(0.01..=8.0).contains(&scale) {
        return Ok(None);
    }
    #[cfg(not(feature = "kernel"))]
    {
        let _ = bytes;
        Ok(None)
    }
    #[cfg(feature = "kernel")]
    {
        match PreparedPage::from_pdf(bytes)? {
            Some(prepared) => prepared.render(scale),
            None => Ok(None),
        }
    }
}

#[cfg(feature = "kernel")]
fn prepare_scene(bytes: &[u8]) -> Result<Option<(Vec<u8>, u32, u32)>, String> {
    if let Ok(geometry) = page_to_opgm_v3_bytes(bytes) {
        return validated_scene(&geometry).map(Some);
    }
    let old = page_to_opdl_bytes(bytes)
        .ok()
        .and_then(|scene| validated_rgb_scene(scene).ok());
    let ordered = || {
        page_to_opdl_ordered_bytes(bytes)
            .ok()
            .and_then(|scene| validated_ordered_scene(scene).ok())
    };
    Ok(old.or_else(ordered))
}

#[cfg(feature = "kernel")]
fn render_validated_scene(scene: &[u8], width: u32, height: u32) -> Result<Bitmap, String> {
    let mode = if word(scene, 32)? == 0 { 2 } else { 3 };
    let pixel_bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or("bitmap size overflow")?;
    let mut output = vec![0_u8; 12 + pixel_bytes];
    // SAFETY: private callers validate every command/offset and bound dimensions
    // before this call. The buffer has the ABI header and exactly width*height*4 bytes.
    let code = unsafe {
        opds_render_buffer(
            scene.as_ptr(),
            i32::try_from(scene.len()).map_err(|_| "scene too large")?,
            output.as_mut_ptr(),
            i32::try_from(output.len()).map_err(|_| "output too large")?,
            mode,
        )
    };
    if code != 0 {
        return Err(format!("DynLex kernel status {code}"));
    }
    Ok(Bitmap {
        width,
        height,
        rgba: output.split_off(12),
    })
}

#[cfg(feature = "kernel")]
fn word(bytes: &[u8], offset: usize) -> Result<u32, String> {
    Ok(u32::from_le_bytes(
        bytes
            .get(offset..offset + 4)
            .ok_or("truncated OPGM")?
            .try_into()
            .map_err(|_| "truncated OPGM")?,
    ))
}

#[cfg(feature = "kernel")]
fn validated_scene(geometry: &[u8]) -> Result<(Vec<u8>, u32, u32), String> {
    if geometry.len() < 40
        || geometry.len() > 1576
        || geometry.get(..4) != Some(b"OPGM")
        || word(geometry, 4)? != 3
    {
        return Err("invalid OPGM-v3 header".into());
    }
    let media = [word(geometry, 8)?, word(geometry, 12)?];
    let crop = [
        word(geometry, 16)?,
        word(geometry, 20)?,
        word(geometry, 24)?,
        word(geometry, 28)?,
    ];
    let turn = word(geometry, 32)?;
    let count = word(geometry, 36)?;
    if media.iter().any(|&n| n == 0 || n > 4096)
        || crop[0] >= crop[2]
        || crop[1] >= crop[3]
        || crop[2] > media[0]
        || crop[3] > media[1]
        || !matches!(turn, 0 | 90 | 180 | 270)
        || !(1..=64).contains(&count)
        || geometry.len() != 40 + count as usize * 24
    {
        return Err("invalid OPGM-v3 dimensions/count".into());
    }
    let (width, height) = if matches!(turn, 90 | 270) {
        (crop[3] - crop[1], crop[2] - crop[0])
    } else {
        (crop[2] - crop[0], crop[3] - crop[1])
    };
    if width.checked_mul(height).is_none_or(|n| n > 6_000_000) {
        return Err("OPGM-v3 exceeds pixel bound".into());
    }
    for index in 0..count as usize {
        let at = 40 + index * 24;
        let kind = word(geometry, at)?;
        let rect = [
            word(geometry, at + 4)?,
            word(geometry, at + 8)?,
            word(geometry, at + 12)?,
            word(geometry, at + 16)?,
        ];
        if !matches!(kind, 1 | 3)
            || rect[0] >= rect[2]
            || rect[1] >= rect[3]
            || rect[2] > width
            || rect[3] > height
            || (kind == 3 && rect[2] - rect[0] != 1 && rect[3] - rect[1] != 1)
            || geometry[at + 23] != 255
        {
            return Err("invalid OPGM-v3 command".into());
        }
    }
    let mut scene = Vec::with_capacity(geometry.len());
    scene.extend_from_slice(b"OPDL");
    for value in [2, width, height, 0, 0, width, height, 0, count] {
        scene.extend_from_slice(&value.to_le_bytes());
    }
    scene.extend_from_slice(&geometry[40..]);
    Ok((scene, width, height))
}

/// The v2 adapter flattens up to two RGB8 XObjects before vector commands.
/// Revalidate its bytes before crossing the ABI.
#[cfg(feature = "kernel")]
fn validated_rgb_scene(scene: Vec<u8>) -> Result<(Vec<u8>, u32, u32), String> {
    if scene.len() < 40
        || scene.len() > 3_538_984
        || scene.get(..4) != Some(b"OPDL")
        || word(&scene, 4)? != 2
    {
        return Err("invalid RGB OPDL header".into());
    }
    let width = word(&scene, 8)?;
    let height = word(&scene, 12)?;
    if width == 0
        || height == 0
        || width > 1024
        || height > 1024
        || [
            word(&scene, 16)?,
            word(&scene, 20)?,
            word(&scene, 24)?,
            word(&scene, 28)?,
        ] != [0, 0, width, height]
    {
        return Err("invalid RGB OPDL page".into());
    }
    let image_len = usize::try_from(word(&scene, 32)?).map_err(|e| e.to_string())?;
    let required_rgb = usize::try_from(width)
        .unwrap()
        .checked_mul(usize::try_from(height).unwrap())
        .and_then(|n| n.checked_mul(3))
        .ok_or("RGB byte count overflow")?;
    if image_len != required_rgb {
        return Err("RGB OPDL needs one full-page image".into());
    }
    let count_at = 36_usize
        .checked_add(image_len)
        .ok_or("RGB offset overflow")?;
    let count = usize::try_from(word(&scene, count_at)?).map_err(|e| e.to_string())?;
    if count > 16_384
        || count_at
            .checked_add(4)
            .and_then(|n| count.checked_mul(24).and_then(|m| n.checked_add(m)))
            != Some(scene.len())
    {
        return Err("invalid RGB OPDL commands".into());
    }
    for index in 0..count {
        let at = count_at + 4 + index * 24;
        let kind = word(&scene, at)?;
        let rect = [
            word(&scene, at + 4)?,
            word(&scene, at + 8)?,
            word(&scene, at + 12)?,
            word(&scene, at + 16)?,
        ];
        if !matches!(kind, 1 | 3)
            || rect[0] >= rect[2]
            || rect[1] >= rect[3]
            || rect[2] > width
            || rect[3] > height
            || (kind == 3 && rect[2] - rect[0] != 1 && rect[3] - rect[1] != 1)
            || scene[at + 23] != 255
        {
            return Err("invalid RGB OPDL command".into());
        }
    }
    Ok((scene, width, height))
}

/// Ordered OPDL-v3/v5/v6 stores images after the fixed-width command table; kinds 6/7/8 carry
/// byte offsets into the trailing RGB/binary-alpha blobs. Offsets are contiguous, so the
/// DynLex kernel cannot read unvalidated gaps, aliases or truncated images.
#[cfg(feature = "kernel")]
fn validated_ordered_scene(scene: Vec<u8>) -> Result<(Vec<u8>, u32, u32), String> {
    if scene.len() < 40
        || scene.len() > 3_538_984
        || scene.get(..4) != Some(b"OPDL")
        || !matches!(word(&scene, 4)?, 3 | 5 | 6)
        || word(&scene, 32)? != 0
    {
        return Err("invalid ordered OPDL header".into());
    }
    let width = word(&scene, 8)?;
    let height = word(&scene, 12)?;
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || width
            .checked_mul(height)
            .is_none_or(|pixels| pixels > 6_000_000)
        || [
            word(&scene, 16)?,
            word(&scene, 20)?,
            word(&scene, 24)?,
            word(&scene, 28)?,
        ] != [0, 0, width, height]
    {
        return Err("invalid ordered OPDL page".into());
    }
    let count = usize::try_from(word(&scene, 36)?).map_err(|e| e.to_string())?;
    let version = word(&scene, 4)?;
    let image_start = count
        .checked_mul(24)
        .and_then(|n| 40_usize.checked_add(n))
        .ok_or("ordered command overflow")?;
    if count > 16_384 || image_start > scene.len() {
        return Err("invalid ordered OPDL command table".into());
    }
    let mut image_bytes = 0_usize;
    let mut images = 0_usize;
    for index in 0..count {
        let at = 40 + index * 24;
        let kind = word(&scene, at)?;
        let rect = [
            word(&scene, at + 4)?,
            word(&scene, at + 8)?,
            word(&scene, at + 12)?,
            word(&scene, at + 16)?,
        ];
        if rect[0] >= rect[2] || rect[1] >= rect[3] || rect[2] > width || rect[3] > height {
            return Err("invalid ordered command bounds".into());
        }
        match kind {
            1 | 3 => {
                if scene[at + 23] != 255
                    || (kind == 3 && rect[2] - rect[0] != 1 && rect[3] - rect[1] != 1)
                {
                    return Err("invalid ordered vector command".into());
                }
            }
            6 | 7 | 8 => {
                if (kind == 7 && version < 5) || (kind == 8 && version != 6) {
                    return Err("image coverage kind requires a newer ordered OPDL version".into());
                }
                images += 1;
                let offset = usize::try_from(word(&scene, at + 20)?).map_err(|e| e.to_string())?;
                if images > 256 || offset != image_bytes {
                    return Err("invalid ordered image offset or count".into());
                }
                let length = (rect[2] - rect[0])
                    .checked_mul(rect[3] - rect[1])
                    .and_then(|n| n.checked_mul(if kind == 6 { 3 } else { 4 }))
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or("ordered image size overflow")?;
                let start = image_start
                    .checked_add(offset)
                    .ok_or("ordered image size overflow")?;
                let end = start
                    .checked_add(length)
                    .ok_or("ordered image size overflow")?;
                let bytes = scene.get(start..end).ok_or("truncated ordered image")?;
                if kind == 7
                    && bytes
                        .chunks_exact(4)
                        .any(|pixel| !matches!(pixel[3], 0 | 255))
                {
                    return Err("unsupported ordered image alpha".into());
                }
                image_bytes = image_bytes
                    .checked_add(length)
                    .ok_or("ordered image size overflow")?;
            }
            _ => return Err("unsupported ordered command".into()),
        }
    }
    // A valid PDF may issue an image Do whose active clip excludes every
    // pixel. Its ordered scene then has no image payload (and may be blank).
    if image_start.checked_add(image_bytes) != Some(scene.len()) {
        return Err("invalid ordered image tail".into());
    }
    Ok((scene, width, height))
}

/// Paint already validated opaque RGB8 appearances over an opaque page.
/// Both the paper/background and the images are composed by the DynLex ordered
/// image kernel; Rust only serializes and validates the bounded scene.
#[cfg(feature = "kernel")]
fn paint_rgb_appearances(
    mut base: Vec<u8>,
    width: u32,
    height: u32,
    images: &[([u32; 4], &[u8])],
) -> Result<Option<Vec<u8>>, String> {
    let pixels = usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
        .ok_or("appearance page size overflow")?;
    if width > 4096 || height > 4096 || pixels > 6_000_000 || base.len() != pixels * 4 {
        return Ok(None);
    }
    if images.is_empty() || images.len() > 8 {
        return Ok(None);
    }
    let mut total_image_bytes = 0_usize;
    for (rect, image) in images {
        let image_pixels =
            usize::try_from(rect[2].checked_sub(rect[0]).ok_or("invalid image width")?)
                .ok()
                .and_then(|w| {
                    usize::try_from(rect[3].checked_sub(rect[1])?)
                        .ok()
                        .and_then(|h| w.checked_mul(h))
                })
                .ok_or("appearance image size overflow")?;
        if image_pixels == 0
            || image_pixels > 65_536
            || image.len() != image_pixels * 3
            || rect[2] > width
            || rect[3] > height
        {
            return Ok(None);
        }
        total_image_bytes += image.len();
    }
    if total_image_bytes > 1_000_000 {
        return Ok(None);
    }
    // Keep each OPDL RGB scene below its validated 3.5 MiB input bound.
    // Images are grouped in PDF order while their union fits a 700k-pixel
    // tile; a distant image starts the next tile and reads the updated page.
    let mut start = 0;
    while start < images.len() {
        let mut bounds = images[start].0;
        let mut end = start + 1;
        while end < images.len() {
            let next = images[end].0;
            let union = [
                bounds[0].min(next[0]),
                bounds[1].min(next[1]),
                bounds[2].max(next[2]),
                bounds[3].max(next[3]),
            ];
            let union_pixels = u64::from(union[2] - union[0]) * u64::from(union[3] - union[1]);
            if union_pixels > 700_000 {
                break;
            }
            bounds = union;
            end += 1;
        }
        let tile_width = (bounds[2] - bounds[0]) as usize;
        let tile_height = (bounds[3] - bounds[1]) as usize;
        let tile_pixels = tile_width * tile_height;
        let image_bytes = images[start..end]
            .iter()
            .map(|(_, image)| image.len())
            .sum::<usize>();
        let mut rgb = Vec::with_capacity(tile_pixels * 3);
        for y in bounds[1]..bounds[3] {
            let row = (y as usize * width as usize + bounds[0] as usize) * 4;
            for pixel in base[row..row + tile_width * 4].chunks_exact(4) {
                if pixel[3] != 255 {
                    return Ok(None);
                }
                rgb.extend_from_slice(&pixel[..3]);
            }
        }
        let mut scene = Vec::with_capacity(40 + (end - start + 1) * 24 + rgb.len() + image_bytes);
        scene.extend_from_slice(b"OPDL");
        for value in [
            3,
            tile_width as u32,
            tile_height as u32,
            0,
            0,
            tile_width as u32,
            tile_height as u32,
            0,
            (end - start + 1) as u32,
        ] {
            scene.extend_from_slice(&value.to_le_bytes());
        }
        let mut offset = rgb.len() as u32;
        // The first command lays down the current page tile. The remaining
        // commands carry image offsets contiguous after that RGB payload.
        scene.extend_from_slice(&6_u32.to_le_bytes());
        for value in [0, 0, tile_width as u32, tile_height as u32, 0] {
            scene.extend_from_slice(&value.to_le_bytes());
        }
        for (rect, image) in &images[start..end] {
            scene.extend_from_slice(&6_u32.to_le_bytes());
            for value in [
                rect[0] - bounds[0],
                rect[1] - bounds[1],
                rect[2] - bounds[0],
                rect[3] - bounds[1],
                offset,
            ] {
                scene.extend_from_slice(&value.to_le_bytes());
            }
            offset += image.len() as u32;
        }
        scene.append(&mut rgb);
        for (_, image) in &images[start..end] {
            scene.extend_from_slice(image);
        }
        let (scene, actual_width, actual_height) = validated_ordered_scene(scene)?;
        if actual_width != tile_width as u32 || actual_height != tile_height as u32 {
            return Err("appearance tile dimensions changed".into());
        }
        let mut output = vec![0_u8; 12 + tile_pixels * 4];
        // SAFETY: validated_ordered_scene checks every byte span and command;
        // output reserves the ABI header and exact RGBA tile payload.
        let status = unsafe {
            opds_render_buffer(
                scene.as_ptr(),
                i32::try_from(scene.len()).map_err(|_| "appearance scene too large")?,
                output.as_mut_ptr(),
                i32::try_from(output.len()).map_err(|_| "appearance bitmap too large")?,
                3,
            )
        };
        if status != 0 {
            return Err(format!("DynLex appearance image status {status}"));
        }
        for (tile_y, y) in (bounds[1]..bounds[3]).enumerate() {
            let destination = (y as usize * width as usize + bounds[0] as usize) * 4;
            let source = 12 + tile_y * tile_width * 4;
            base[destination..destination + tile_width * 4]
                .copy_from_slice(&output[source..source + tile_width * 4]);
        }
        start = end;
    }
    Ok(Some(base))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[cfg(feature = "kernel")]
    #[test]
    fn distant_imported_rgb_images_use_ordered_tiles_on_large_page() {
        let (width, height) = (1200_u32, 1000_u32);
        let mut base = vec![0_u8; width as usize * height as usize * 4];
        for pixel in base.chunks_exact_mut(4) {
            pixel.copy_from_slice(&[20, 30, 40, 255]);
        }
        let red = vec![255, 0, 0].repeat(4);
        let blue = vec![0, 0, 255].repeat(4);
        let images = [
            ([5, 5, 7, 7], red.as_slice()),
            ([1100, 900, 1102, 902], blue.as_slice()),
        ];
        let result = paint_rgb_appearances(base.clone(), width, height, &images)
            .unwrap()
            .unwrap();
        for (bounds, color) in [
            ([5, 5, 7, 7], [255, 0, 0, 255]),
            ([1100, 900, 1102, 902], [0, 0, 255, 255]),
        ] {
            for y in bounds[1]..bounds[3] {
                for x in bounds[0]..bounds[2] {
                    let at = (y * width + x) as usize * 4;
                    base[at..at + 4].copy_from_slice(&color);
                }
            }
        }
        assert_eq!(result, base);
    }
    #[test]
    fn unsupported_requests_fall_back() {
        assert!(try_render(b"", 2, 1.0).unwrap().is_none());
        assert!(try_render(b"", 1, 0.5).unwrap().is_none());
        assert!(try_render(b"", 1, 1.0).unwrap().is_none());
    }
    #[test]
    fn unsupported_text_pdf_uses_hayro() {
        let bytes =
            include_bytes!("../../../../test pdf-bestanden/tekstrotatie-extern.pdf").to_vec();
        let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        let a = render_with_fallback(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
        let report = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
        assert_eq!(report.used, Backend::Hayro);
        let b = pdf.render_page(1, 1.0).unwrap();
        assert_eq!((a.width, a.height, a.rgba), (b.width, b.height, b.rgba));
    }
    #[cfg(feature = "kernel")]
    #[test]
    fn supported_pdf_uses_dynlex_and_matches_hayro() {
        let bytes = include_bytes!("../tests/fixtures/simple-fill-stroke.pdf").to_vec();
        let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        let direct = try_render(&bytes, 1, 1.0).unwrap().unwrap();
        let routed = render_with_fallback(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
        let report = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
        assert_eq!(report.used, Backend::Dynlex);
        let hayro = pdf.render_page(1, 1.0).unwrap();
        assert_eq!(direct.rgba, routed.rgba);
        assert_eq!(routed.rgba, hayro.rgba);
    }

    #[cfg(feature = "own-rust")]
    #[test]
    fn own_rust_backend_is_distinct_and_falls_back_outside_its_subset() {
        let bytes = include_bytes!("../tests/fixtures/simple-fill-stroke.pdf").to_vec();
        let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        let selected = render_with_report(&pdf, &bytes, 1, 1.0, Backend::OwnRust).unwrap();
        assert_eq!(selected.used, Backend::OwnRust);
        assert_eq!(selected.raster.rgba, pdf.render_page(1, 1.0).unwrap().rgba);
        let fallback = render_with_report(&pdf, &bytes, 1, 1.25, Backend::OwnRust).unwrap();
        assert_eq!(fallback.used, Backend::Hayro);
        assert_eq!(fallback.raster.rgba, pdf.render_page(1, 1.25).unwrap().rgba);
        let hybrid = include_bytes!("../tests/fixtures/two-images-hybrid.pdf").to_vec();
        let hybrid_pdf = ParsedPdf::open(Arc::new(hybrid.clone())).unwrap();
        let selected = render_with_report(&hybrid_pdf, &hybrid, 1, 1.0, Backend::OwnRust).unwrap();
        assert_eq!(selected.used, Backend::OwnRust);
        assert_eq!(
            selected.raster.rgba,
            hybrid_pdf.render_page(1, 1.0).unwrap().rgba
        );
    }

    #[cfg(feature = "pdfium")]
    #[test]
    fn pdfium_backend_reports_the_renderer_actually_used() {
        let bytes = include_bytes!("../tests/fixtures/simple-fill-stroke.pdf").to_vec();
        let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        let selected = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Pdfium).unwrap();
        assert_eq!(
            selected.used,
            if pdfium_available() {
                Backend::Pdfium
            } else {
                Backend::Hayro
            }
        );
        assert_eq!(
            selected.raster.rgba.len(),
            selected.raster.width as usize * selected.raster.height as usize * 4
        );
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn fractional_device_rgb_uses_dynlex_and_matches_hayro() {
        let bytes = include_bytes!("../tests/fixtures/fractional-rgb-fill-stroke.pdf").to_vec();
        let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        let result = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
        assert_eq!(result.used, Backend::Dynlex);
        let hayro = pdf.render_page(1, 1.0).unwrap();
        assert_eq!(result.raster.rgba, hayro.rgba);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn saved_graphics_state_uses_dynlex_and_matches_hayro() {
        let bytes = include_bytes!("../tests/fixtures/stateful-rgb-fill-stroke.pdf").to_vec();
        let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        let result = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
        assert_eq!(result.used, Backend::Dynlex);
        let hayro = pdf.render_page(1, 1.0).unwrap();
        assert_eq!(result.raster.rgba, hayro.rgba);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn full_page_rgb_image_and_vector_overlay_match_hayro() {
        for bytes in [
            include_bytes!("../tests/fixtures/full-page-raster.pdf").as_slice(),
            include_bytes!("../tests/fixtures/full-page-hybrid.pdf").as_slice(),
            include_bytes!("../tests/fixtures/full-page-raster-flate.pdf").as_slice(),
            include_bytes!("../tests/fixtures/full-page-hybrid-flate.pdf").as_slice(),
        ] {
            let bytes = bytes.to_vec();
            let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
            let result = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
            assert_eq!(result.used, Backend::Dynlex);
            let hayro = pdf.render_page(1, 1.0).unwrap();
            assert_eq!(result.raster.rgba.len(), hayro.rgba.len());
            assert!(result.raster.rgba == hayro.rgba, "RGB image pixels differ");
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn placed_rgb_image_and_vector_overlay_match_hayro() {
        for bytes in [
            include_bytes!("../tests/fixtures/placed-raster.pdf").as_slice(),
            include_bytes!("../tests/fixtures/placed-hybrid.pdf").as_slice(),
            include_bytes!("../tests/fixtures/placed-raster-flate.pdf").as_slice(),
            include_bytes!("../tests/fixtures/placed-hybrid-flate.pdf").as_slice(),
        ] {
            let bytes = bytes.to_vec();
            let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
            let result = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
            assert_eq!(result.used, Backend::Dynlex);
            let hayro = pdf.render_page(1, 1.0).unwrap();
            assert_eq!(result.raster.rgba, hayro.rgba);
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn overlapping_rgb_images_then_vector_overlay_match_hayro() {
        for bytes in [
            include_bytes!("../tests/fixtures/two-images-raster.pdf").as_slice(),
            include_bytes!("../tests/fixtures/two-images-hybrid.pdf").as_slice(),
            include_bytes!("../tests/fixtures/two-images-raster-flate.pdf").as_slice(),
            include_bytes!("../tests/fixtures/two-images-hybrid-flate.pdf").as_slice(),
        ] {
            let bytes = bytes.to_vec();
            let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
            let result = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
            assert_eq!(result.used, Backend::Dynlex);
            let hayro = pdf.render_page(1, 1.0).unwrap();
            assert_eq!(result.raster.rgba, hayro.rgba);
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn ordered_images_after_vector_content_match_hayro() {
        for (name, bytes) in [
            (
                "one-after-vector",
                include_bytes!("../tests/fixtures/placed-raster-after-vector.pdf").as_slice(),
            ),
            (
                "second-after-vector",
                include_bytes!("../tests/fixtures/two-images-wrong-order.pdf").as_slice(),
            ),
            (
                "third-after-images",
                include_bytes!("../tests/fixtures/three-images-layered.pdf").as_slice(),
            ),
        ] {
            let bytes = bytes.to_vec();
            let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
            let result = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
            assert_eq!(result.used, Backend::Dynlex);
            let hayro = pdf.render_page(1, 1.0).unwrap();
            let mismatch = result
                .raster
                .rgba
                .iter()
                .zip(&hayro.rgba)
                .position(|(a, b)| a != b);
            assert_eq!(
                mismatch,
                None,
                "{name} first differing RGBA byte; got {:?}, expected {:?}",
                mismatch.map(|i| result.raster.rgba[i]),
                mismatch.map(|i| hayro.rgba[i])
            );
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn many_named_rgb_images_and_vector_layers_match_hayro() {
        for bytes in [
            include_bytes!("../tests/fixtures/many-images-raw.pdf").as_slice(),
            include_bytes!("../tests/fixtures/many-images-mixed-flate.pdf").as_slice(),
        ] {
            let bytes = bytes.to_vec();
            let scene = page_to_opdl_ordered_bytes(&bytes).unwrap();
            assert_eq!(word(&scene, 4).unwrap(), 3);
            assert!(validated_ordered_scene(scene).is_ok());
            let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
            let result = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
            assert_eq!(result.used, Backend::Dynlex);
            let hayro = pdf.render_page(1, 1.0).unwrap();
            let mismatch = result
                .raster
                .rgba
                .iter()
                .zip(&hayro.rgba)
                .position(|(a, b)| a != b);
            assert_eq!(mismatch, None, "first differing RGBA byte");
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn scaled_rgb_images_match_hayro() {
        for (name, bytes) in [
            (
                "up-raster",
                include_bytes!("../tests/fixtures/scaled-up-raster.pdf").as_slice(),
            ),
            (
                "up-hybrid",
                include_bytes!("../tests/fixtures/scaled-up-hybrid.pdf").as_slice(),
            ),
            (
                "down-raster",
                include_bytes!("../tests/fixtures/scaled-down-raster.pdf").as_slice(),
            ),
            (
                "down-hybrid",
                include_bytes!("../tests/fixtures/scaled-down-hybrid.pdf").as_slice(),
            ),
            (
                "wide-strip",
                include_bytes!("../tests/fixtures/scaled-strip-flate.pdf").as_slice(),
            ),
            (
                "odd-upscale",
                include_bytes!("../tests/fixtures/scaled-up-odd.pdf").as_slice(),
            ),
            (
                "mixed-scale",
                include_bytes!("../tests/fixtures/scaled-mixed.pdf").as_slice(),
            ),
            (
                "large-scaled-strip-hybrid",
                include_bytes!("../tests/fixtures/large-scaled-strip-hybrid.pdf").as_slice(),
            ),
        ] {
            let pdf = ParsedPdf::open(Arc::new(bytes.to_vec())).unwrap();
            let result = render_with_report(&pdf, bytes, 1, 1.0, Backend::Dynlex).unwrap();
            assert_eq!(result.used, Backend::Dynlex, "{name}");
            let expected = pdf.render_page(1, 1.0).unwrap();
            assert_eq!(result.raster.rgba.len(), expected.rgba.len(), "{name}");
            let mismatch = result
                .raster
                .rgba
                .iter()
                .zip(&expected.rgba)
                .position(|(a, b)| a != b);
            assert_eq!(mismatch, None, "{name}: first differing RGBA byte");
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn inherited_rotation_and_crop_orient_images_and_vectors() {
        for (turn, bytes) in [
            (
                0,
                include_bytes!("../tests/fixtures/rotated-cropped-image-0.pdf").as_slice(),
            ),
            (
                90,
                include_bytes!("../tests/fixtures/rotated-cropped-image-90.pdf").as_slice(),
            ),
            (
                180,
                include_bytes!("../tests/fixtures/rotated-cropped-image-180.pdf").as_slice(),
            ),
            (
                270,
                include_bytes!("../tests/fixtures/rotated-cropped-image-270.pdf").as_slice(),
            ),
        ] {
            let pdf = ParsedPdf::open(Arc::new(bytes.to_vec())).unwrap();
            let rendered = render_with_report(&pdf, bytes, 1, 1.0, Backend::Dynlex).unwrap();
            assert_eq!(rendered.used, Backend::Dynlex, "turn {turn}");
            let expected = pdf.render_page(1, 1.0).unwrap();
            assert_eq!(rendered.raster.rgba.len(), expected.rgba.len());
            let mismatch = rendered
                .raster
                .rgba
                .iter()
                .zip(&expected.rgba)
                .position(|(a, b)| a != b);
            assert_eq!(mismatch, None, "turn {turn}: first differing RGBA byte");
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn large_rotated_cropped_image_matches_hayro() {
        let bytes = include_bytes!("../tests/fixtures/large-rotated-cropped-image.pdf");
        let pdf = ParsedPdf::open(Arc::new(bytes.to_vec())).unwrap();
        let rendered = render_with_report(&pdf, bytes, 1, 1.0, Backend::Dynlex).unwrap();
        assert_eq!(rendered.used, Backend::Dynlex);
        assert_eq!(
            (rendered.raster.width, rendered.raster.height),
            (1984, 1484)
        );
        let expected = pdf.render_page(1, 1.0).unwrap();
        assert_eq!(rendered.raster.rgba, expected.rgba);
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn flate_png_predictor_images_match_hayro() {
        for (name, bytes) in [
            (
                "raster",
                include_bytes!("../tests/fixtures/predictor-raster.pdf").as_slice(),
            ),
            (
                "hybrid",
                include_bytes!("../tests/fixtures/predictor-hybrid.pdf").as_slice(),
            ),
            (
                "rotated",
                include_bytes!("../tests/fixtures/predictor-rotated-cropped.pdf").as_slice(),
            ),
            (
                "large-strip",
                include_bytes!("../tests/fixtures/predictor-large-strip.pdf").as_slice(),
            ),
        ] {
            let pdf = ParsedPdf::open(Arc::new(bytes.to_vec())).unwrap();
            let rendered = render_with_report(&pdf, bytes, 1, 1.0, Backend::Dynlex).unwrap();
            assert_eq!(rendered.used, Backend::Dynlex, "{name}");
            let expected = pdf.render_page(1, 1.0).unwrap();
            assert_eq!(rendered.raster.rgba, expected.rgba, "{name}");
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn composed_axis_ctm_and_graphics_restore_match_hayro() {
        for (name, bytes) in [
            (
                "global raster",
                include_bytes!("../tests/fixtures/global-ctm-raster.pdf").as_slice(),
            ),
            (
                "global hybrid",
                include_bytes!("../tests/fixtures/global-ctm-hybrid.pdf").as_slice(),
            ),
            (
                "restored matrix",
                include_bytes!("../tests/fixtures/global-ctm-restore.pdf").as_slice(),
            ),
            (
                "restored color",
                include_bytes!("../tests/fixtures/global-ctm-style-restore.pdf").as_slice(),
            ),
        ] {
            let pdf = ParsedPdf::open(Arc::new(bytes.to_vec())).unwrap();
            let rendered = render_with_report(&pdf, bytes, 1, 1.0, Backend::Dynlex).unwrap();
            assert_eq!(rendered.used, Backend::Dynlex, "{name}");
            let expected = pdf.render_page(1, 1.0).unwrap();
            assert_eq!(rendered.raster.rgba, expected.rgba, "{name}");
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn fractional_axis_images_match_hayro() {
        for (name, bytes) in [
            (
                "upscale grid",
                include_bytes!("../tests/fixtures/fractional-grid-up.pdf").as_slice(),
            ),
            (
                "downscale grid",
                include_bytes!("../tests/fixtures/fractional-grid-down.pdf").as_slice(),
            ),
            (
                "prior vector",
                include_bytes!("../tests/fixtures/fractional-over-blue.pdf").as_slice(),
            ),
            (
                "real-style predictor strip",
                include_bytes!("../tests/fixtures/fractional-real-style-strip.pdf").as_slice(),
            ),
            (
                "crop 0",
                include_bytes!("../tests/fixtures/fractional-crop-rotate-0.pdf").as_slice(),
            ),
            (
                "crop 90",
                include_bytes!("../tests/fixtures/fractional-crop-rotate-90.pdf").as_slice(),
            ),
            (
                "crop 180",
                include_bytes!("../tests/fixtures/fractional-crop-rotate-180.pdf").as_slice(),
            ),
            (
                "crop 270",
                include_bytes!("../tests/fixtures/fractional-crop-rotate-270.pdf").as_slice(),
            ),
            (
                "mixed scale",
                include_bytes!("../tests/fixtures/fractional-mixed-scale.pdf").as_slice(),
            ),
            (
                "layer order",
                include_bytes!("../tests/fixtures/fractional-layered.pdf").as_slice(),
            ),
            (
                "mixed image records",
                include_bytes!("../tests/fixtures/fractional-mixed-image-records.pdf").as_slice(),
            ),
            (
                "existing rectangular clip",
                include_bytes!("../tests/fixtures/fractional-clipped.pdf").as_slice(),
            ),
        ] {
            let pdf = ParsedPdf::open(Arc::new(bytes.to_vec())).unwrap();
            let rendered = render_with_report(&pdf, bytes, 1, 1.0, Backend::Dynlex).unwrap();
            assert_eq!(rendered.used, Backend::Dynlex, "{name}");
            let expected = pdf.render_page(1, 1.0).unwrap();
            assert_eq!(rendered.raster.rgba, expected.rgba, "{name}");
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn indexed_device_rgb_images_match_hayro() {
        for (name, bytes) in [
            (
                "raw raster",
                include_bytes!("../tests/fixtures/indexed-raw-raster.pdf").as_slice(),
            ),
            (
                "Flate hybrid",
                include_bytes!("../tests/fixtures/indexed-flate-hybrid.pdf").as_slice(),
            ),
            (
                "rotate and crop",
                include_bytes!("../tests/fixtures/indexed-rotate-crop.pdf").as_slice(),
            ),
            (
                "256-color large",
                include_bytes!("../tests/fixtures/indexed-256-palette-large.pdf").as_slice(),
            ),
        ] {
            let pdf = ParsedPdf::open(Arc::new(bytes.to_vec())).unwrap();
            let rendered = render_with_report(&pdf, bytes, 1, 1.0, Backend::Dynlex).unwrap();
            assert_eq!(rendered.used, Backend::Dynlex, "{name}");
            let expected = pdf.render_page(1, 1.0).unwrap();
            if rendered.raster.rgba != expected.rgba {
                let first = rendered
                    .raster
                    .rgba
                    .iter()
                    .zip(&expected.rgba)
                    .position(|(actual, wanted)| actual != wanted)
                    .unwrap();
                let mismatches = rendered
                    .raster
                    .rgba
                    .iter()
                    .zip(&expected.rgba)
                    .filter(|(actual, wanted)| actual != wanted)
                    .count();
                let start = first / 4 * 4;
                panic!("{name}: first RGBA byte {first}, mismatch bytes {mismatches}, actual {:?}, expected {:?}",
                    &rendered.raster.rgba[start..start+4], &expected.rgba[start..start+4]);
            }
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn invalid_indexed_images_fall_back() {
        for bytes in [
            include_bytes!("../tests/fixtures/indexed-invalid-index.pdf").as_slice(),
            include_bytes!("../tests/fixtures/indexed-invalid-palette.pdf").as_slice(),
            include_bytes!("../tests/fixtures/indexed-unsupported-decode.pdf").as_slice(),
        ] {
            assert!(page_to_opdl_ordered_bytes(bytes).is_err());
            assert!(try_render(bytes, 1, 1.0).unwrap().is_none());
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn unproven_fractional_image_states_fall_back() {
        for bytes in [
            include_bytes!("../tests/fixtures/fractional-interpolate-unsupported.pdf").as_slice(),
        ] {
            assert!(page_to_opdl_ordered_bytes(bytes).is_err());
            assert!(try_render(bytes, 1, 1.0).unwrap().is_none());
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn bounded_clipped_images_match_hayro() {
        for (name, bytes) in [
            (
                "fractional RGB",
                include_bytes!("../tests/fixtures/clipped-fractional-rgb.pdf").as_slice(),
            ),
            (
                "rotate CropBox",
                include_bytes!("../tests/fixtures/clipped-fractional-rotate-crop.pdf").as_slice(),
            ),
            (
                "double clip",
                include_bytes!("../tests/fixtures/clipped-double-intersection.pdf").as_slice(),
            ),
            (
                "q/Q restoration",
                include_bytes!("../tests/fixtures/clipped-restore-after-image.pdf").as_slice(),
            ),
            (
                "Indexed RGB",
                include_bytes!("../tests/fixtures/clipped-indexed-rgb.pdf").as_slice(),
            ),
            (
                "m/l/h rectangle",
                include_bytes!("../tests/fixtures/clipped-path-rectangle.pdf").as_slice(),
            ),
            (
                "m/l/h rectangle Rotate/CropBox",
                include_bytes!("../tests/fixtures/clipped-path-rectangle-rotate.pdf").as_slice(),
            ),
            (
                "m/l/h rectangle W*",
                include_bytes!("../tests/fixtures/clipped-path-rectangle-evenodd.pdf").as_slice(),
            ),
            (
                "fractional m/l/h rectangle Rotate/CropBox",
                include_bytes!("../tests/fixtures/clipped-path-subpixel-rotate.pdf").as_slice(),
            ),
            (
                "convex triangle",
                include_bytes!("../tests/fixtures/clipped-triangle.pdf").as_slice(),
            ),
            (
                "convex triangle Rotate/CropBox",
                include_bytes!("../tests/fixtures/clipped-triangle-rotate.pdf").as_slice(),
            ),
            (
                "convex triangle W*",
                include_bytes!("../tests/fixtures/clipped-triangle-evenodd.pdf").as_slice(),
            ),
            (
                "convex four-corner clip",
                include_bytes!("../tests/fixtures/clipped-diamond.pdf").as_slice(),
            ),
            (
                "simple concave even-odd clip",
                include_bytes!("../tests/fixtures/clipped-concave.pdf").as_slice(),
            ),
            (
                "simple concave Rotate/CropBox clip",
                include_bytes!("../tests/fixtures/clipped-concave-rotate.pdf").as_slice(),
            ),
            (
                "two-contour even-odd hole",
                include_bytes!("../tests/fixtures/clipped-two-contour-hole.pdf").as_slice(),
            ),
            (
                "two-contour hole Rotate/CropBox",
                include_bytes!("../tests/fixtures/clipped-two-contour-hole-rotate.pdf").as_slice(),
            ),
            (
                "two-contour hole restored before later image",
                include_bytes!("../tests/fixtures/clipped-two-contour-hole-restored.pdf")
                    .as_slice(),
            ),
            (
                "two-contour hole with reversed path order",
                include_bytes!("../tests/fixtures/clipped-two-contour-hole-reversed.pdf")
                    .as_slice(),
            ),
            (
                "three contours with two holes",
                include_bytes!("../tests/fixtures/clipped-multi-two-holes.pdf").as_slice(),
            ),
            (
                "three nested contours with restored island",
                include_bytes!("../tests/fixtures/clipped-three-contour-nested.pdf").as_slice(),
            ),
            (
                "three contours with two holes Rotate/CropBox",
                include_bytes!("../tests/fixtures/clipped-multi-two-holes-rotate.pdf").as_slice(),
            ),
            (
                "disjoint contours",
                include_bytes!("../tests/fixtures/clipped-multi-disjoint.pdf").as_slice(),
            ),
            (
                "disjoint contours Rotate/CropBox",
                include_bytes!("../tests/fixtures/clipped-multi-disjoint-rotate.pdf").as_slice(),
            ),
            (
                "four contours with two islands and holes",
                include_bytes!("../tests/fixtures/clipped-multi-four-contours.pdf").as_slice(),
            ),
            (
                "four contours Rotate/CropBox",
                include_bytes!("../tests/fixtures/clipped-multi-four-contours-rotate.pdf")
                    .as_slice(),
            ),
            (
                "twelve-vertex convex outer contour",
                include_bytes!("../tests/fixtures/clipped-multi-twelve-vertices.pdf").as_slice(),
            ),
            (
                "concave outer contour with hole",
                include_bytes!("../tests/fixtures/clipped-multi-concave-hole.pdf").as_slice(),
            ),
            (
                "concave outer contour with hole Rotate/CropBox",
                include_bytes!("../tests/fixtures/clipped-multi-concave-hole-rotate.pdf")
                    .as_slice(),
            ),
            (
                "long 67-vertex concave outer contour",
                include_bytes!("../tests/fixtures/clipped-multi-long-stair.pdf").as_slice(),
            ),
            (
                "isolated real W* shape under global CTM",
                include_bytes!("../tests/fixtures/clipped-real-shape-582.pdf").as_slice(),
            ),
            (
                "convex clip restored before later image",
                include_bytes!("../tests/fixtures/clipped-triangle-restored.pdf").as_slice(),
            ),
            (
                "subpixel RGB",
                include_bytes!("../tests/fixtures/clipped-subpixel-rgb.pdf").as_slice(),
            ),
            (
                "subpixel image edge",
                include_bytes!("../tests/fixtures/clipped-subpixel-image.pdf").as_slice(),
            ),
            (
                "subpixel Rotate/CropBox",
                include_bytes!("../tests/fixtures/clipped-subpixel-rotate.pdf").as_slice(),
            ),
            (
                "subpixel Rotate 180",
                include_bytes!("../tests/fixtures/clipped-subpixel-rotate-180.pdf").as_slice(),
            ),
            (
                "subpixel Rotate 270",
                include_bytes!("../tests/fixtures/clipped-subpixel-rotate-270.pdf").as_slice(),
            ),
            (
                "subpixel global CTM",
                include_bytes!("../tests/fixtures/clipped-subpixel-global-ctm.pdf").as_slice(),
            ),
            (
                "subpixel even-odd rectangle",
                include_bytes!("../tests/fixtures/clipped-subpixel-evenodd.pdf").as_slice(),
            ),
        ] {
            let pdf = ParsedPdf::open(Arc::new(bytes.to_vec())).unwrap();
            let rendered = render_with_report(&pdf, bytes, 1, 1.0, Backend::Dynlex).unwrap();
            assert_eq!(rendered.used, Backend::Dynlex, "{name}");
            let expected = pdf.render_page(1, 1.0).unwrap();
            if rendered.raster.rgba != expected.rgba {
                let first = rendered
                    .raster
                    .rgba
                    .iter()
                    .zip(&expected.rgba)
                    .position(|(actual, wanted)| actual != wanted)
                    .unwrap();
                let mismatches = rendered
                    .raster
                    .rgba
                    .iter()
                    .zip(&expected.rgba)
                    .filter(|(actual, wanted)| actual != wanted)
                    .count();
                let start = first / 4 * 4;
                panic!("{name}: first RGBA byte {first}, mismatch bytes {mismatches}, actual {:?}, expected {:?}",
                    &rendered.raster.rgba[start..start+4], &expected.rgba[start..start+4]);
            }
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn unsupported_image_clips_fall_back() {
        for bytes in [
            include_bytes!("../tests/fixtures/clipped-oversized-polygon-unsupported.pdf")
                .as_slice(),
            include_bytes!("../tests/fixtures/clipped-path-open-unsupported.pdf").as_slice(),
            include_bytes!("../tests/fixtures/clipped-path-bowtie-unsupported.pdf").as_slice(),
            include_bytes!("../tests/fixtures/clipped-subpixel-unsupported-fill.pdf").as_slice(),
            include_bytes!("../tests/fixtures/clipped-subpixel-ambiguous.pdf").as_slice(),
            include_bytes!("../tests/fixtures/clipped-polygon-ambiguous.pdf").as_slice(),
            include_bytes!("../tests/fixtures/clipped-polygon-ambiguous-rotate.pdf").as_slice(),
            include_bytes!("../tests/fixtures/clipped-multi-half-step-unsupported.pdf").as_slice(),
            include_bytes!("../tests/fixtures/clipped-polygon-fill-unsupported.pdf").as_slice(),
            include_bytes!("../tests/fixtures/clipped-polygon-intersection-unsupported.pdf")
                .as_slice(),
            include_bytes!("../tests/fixtures/clipped-two-contour-nonzero-unsupported.pdf")
                .as_slice(),
            include_bytes!("../tests/fixtures/clipped-two-contour-overlap-unsupported.pdf")
                .as_slice(),
            include_bytes!("../tests/fixtures/clipped-two-contour-triangle-hole.pdf").as_slice(),
            include_bytes!("../tests/fixtures/clipped-two-contour-open-unsupported.pdf").as_slice(),
            include_bytes!("../tests/fixtures/clipped-multi-crossing-unsupported.pdf").as_slice(),
            include_bytes!("../tests/fixtures/clipped-multi-five-unsupported.pdf").as_slice(),
            include_bytes!("../tests/fixtures/clipped-real-shape-338.pdf").as_slice(),
            include_bytes!("../tests/fixtures/clipped-real-shape-338-rotate-crop.pdf").as_slice(),
        ] {
            assert!(page_to_opdl_ordered_bytes(bytes).is_err());
            assert!(try_render(bytes, 1, 1.0).unwrap().is_none());
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn fully_clipped_images_are_ordered_noops() {
        for (name, bytes) in [
            (
                "aligned",
                include_bytes!("../tests/fixtures/clipped-fully-outside.pdf").as_slice(),
            ),
            (
                "fractional",
                include_bytes!("../tests/fixtures/clipped-subpixel-fully-outside.pdf").as_slice(),
            ),
            (
                "later visible image",
                include_bytes!("../tests/fixtures/clipped-fully-outside-then-visible.pdf")
                    .as_slice(),
            ),
        ] {
            let scene = page_to_opdl_ordered_bytes(bytes);
            assert!(scene.is_ok(), "{name}: {:?}", scene.err());
            let pdf = ParsedPdf::open(Arc::new(bytes.to_vec())).unwrap();
            let rendered = render_with_report(&pdf, bytes, 1, 1.0, Backend::Dynlex).unwrap();
            assert_eq!(rendered.used, Backend::Dynlex, "{name}");
            let expected = pdf.render_page(1, 1.0).unwrap();
            assert_eq!(rendered.raster.rgba, expected.rgba, "{name}");
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn unsupported_composed_ctms_fall_back() {
        let original = include_bytes!("../tests/fixtures/global-ctm-raster.pdf");
        for (old, new) in [
            (b"0.1 0 0 0.1".as_slice(), b"0.1 1 0 0.1".as_slice()),
            (b"0.1 0 0 0.1".as_slice(), b"0.0 0 0 0.1".as_slice()),
            (b"0.1 0 0 0.1".as_slice(), b"9.9 0 0 0.1".as_slice()),
        ] {
            let mut bytes = original.to_vec();
            let at = bytes
                .windows(old.len())
                .position(|window| window == old)
                .unwrap();
            bytes[at..at + old.len()].copy_from_slice(new);
            assert!(page_to_opdl_ordered_bytes(&bytes).is_err());
            assert!(try_render(&bytes, 1, 1.0).unwrap().is_none());
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn invalid_png_predictor_parameters_fall_back() {
        let original = include_bytes!("../tests/fixtures/predictor-raster.pdf");
        for (old, new) in [
            (b"/Columns 320".as_slice(), b"/Columns 319".as_slice()),
            (b"/Predictor 15".as_slice(), b"/Predictor 12".as_slice()),
            (b"/Colors 3".as_slice(), b"/Colors 4".as_slice()),
        ] {
            let mut bytes = original.to_vec();
            let at = bytes
                .windows(old.len())
                .position(|window| window == old)
                .unwrap();
            bytes[at..at + old.len()].copy_from_slice(new);
            assert!(page_to_opdl_ordered_bytes(&bytes).is_err());
            assert!(try_render(&bytes, 1, 1.0).unwrap().is_none());
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn scaled_images_outside_validated_subset_fall_back() {
        for bytes in [
            include_bytes!("../tests/fixtures/scaled-interpolate-unsupported.pdf").as_slice(),
            include_bytes!("../tests/fixtures/scaled-source-oversized.pdf").as_slice(),
            include_bytes!("../tests/fixtures/large-page-pixel-limit.pdf").as_slice(),
            include_bytes!("../tests/fixtures/large-page-side-limit.pdf").as_slice(),
            include_bytes!("../tests/fixtures/image-invalid-rotation.pdf").as_slice(),
            include_bytes!("../tests/fixtures/image-invalid-crop.pdf").as_slice(),
            include_bytes!("../tests/fixtures/image-inherited-userunit.pdf").as_slice(),
        ] {
            assert!(page_to_opdl_ordered_bytes(bytes).is_err());
            assert!(try_render(bytes, 1, 1.0).unwrap().is_none());
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn rgb_scene_boundary_rejects_corruption() {
        let bytes = include_bytes!("../tests/fixtures/full-page-hybrid.pdf");
        let scene = page_to_opdl_bytes(bytes).unwrap();
        assert!(validated_rgb_scene(scene.clone()).is_ok());
        let mut truncated = scene.clone();
        truncated.pop();
        assert!(validated_rgb_scene(truncated).is_err());
        let mut bad_image_length = scene.clone();
        bad_image_length[32..36].copy_from_slice(&0_u32.to_le_bytes());
        assert!(validated_rgb_scene(bad_image_length).is_err());
        let mut bad_alpha = scene.clone();
        let at = scene.len() - 1;
        bad_alpha[at] = 0;
        assert!(validated_rgb_scene(bad_alpha).is_err());
        let mut bad_kind = scene;
        let image_len = usize::try_from(word(&bad_kind, 32).unwrap()).unwrap();
        bad_kind[40 + image_len..44 + image_len].copy_from_slice(&9_u32.to_le_bytes());
        assert!(validated_rgb_scene(bad_kind).is_err());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn ordered_scene_boundary_rejects_corruption() {
        let bytes = include_bytes!("../tests/fixtures/two-images-wrong-order.pdf");
        let scene = page_to_opdl_ordered_bytes(bytes).unwrap();
        assert!(validated_ordered_scene(scene.clone()).is_ok());
        let mut truncated = scene.clone();
        truncated.pop();
        assert!(validated_ordered_scene(truncated).is_err());
        let mut appended = scene.clone();
        appended.push(0);
        assert!(validated_ordered_scene(appended).is_err());
        let mut bad_kind = scene.clone();
        bad_kind[40..44].copy_from_slice(&99_u32.to_le_bytes());
        assert!(validated_ordered_scene(bad_kind).is_err());
        let mut bad_offset = scene.clone();
        let image_command = (0..word(&scene, 36).unwrap() as usize)
            .map(|i| 40 + i * 24)
            .find(|at| word(&scene, *at).unwrap() == 6)
            .unwrap();
        bad_offset[image_command + 20..image_command + 24].copy_from_slice(&1_u32.to_le_bytes());
        assert!(validated_ordered_scene(bad_offset).is_err());
        let mut bad_bounds = scene;
        bad_bounds[image_command + 12..image_command + 16].copy_from_slice(&2048_u32.to_le_bytes());
        assert!(validated_ordered_scene(bad_bounds).is_err());

        let make_image_count = |count: u32| {
            let mut scene = Vec::new();
            scene.extend_from_slice(b"OPDL");
            for value in [3, 1, 1, 0, 0, 1, 1, 0, count] {
                scene.extend_from_slice(&value.to_le_bytes());
            }
            for index in 0..count {
                for value in [6, 0, 0, 1, 1, index * 3] {
                    scene.extend_from_slice(&value.to_le_bytes());
                }
            }
            scene.resize(scene.len() + count as usize * 3, 0);
            scene
        };
        assert!(validated_ordered_scene(make_image_count(256)).is_ok());
        assert!(validated_ordered_scene(make_image_count(257)).is_err());

        let base =
            page_to_opdl_ordered_bytes(include_bytes!("../tests/fixtures/scaled-up-raster.pdf"))
                .unwrap();
        for (width, height) in [(4097_u32, 1_u32), (3000, 3000)] {
            let mut oversized = base.clone();
            oversized[8..12].copy_from_slice(&width.to_le_bytes());
            oversized[12..16].copy_from_slice(&height.to_le_bytes());
            oversized[24..28].copy_from_slice(&width.to_le_bytes());
            oversized[28..32].copy_from_slice(&height.to_le_bytes());
            assert!(validated_ordered_scene(oversized).is_err());
        }
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn binary_alpha_scene_boundary_rejects_corruption() {
        let bytes = include_bytes!("../tests/fixtures/fractional-grid-up.pdf");
        let scene = page_to_opdl_ordered_bytes(bytes).unwrap();
        assert_eq!(word(&scene, 4).unwrap(), 5);
        assert_eq!(word(&scene, 40).unwrap(), 7);
        assert!(validated_ordered_scene(scene.clone()).is_ok());
        let mut wrong_version = scene.clone();
        wrong_version[4..8].copy_from_slice(&3_u32.to_le_bytes());
        assert!(validated_ordered_scene(wrong_version).is_err());
        let mut invalid_alpha = scene.clone();
        let image_start = 40 + word(&scene, 36).unwrap() as usize * 24;
        invalid_alpha[image_start + 3] = 128;
        assert!(validated_ordered_scene(invalid_alpha).is_err());
        let mut truncated = scene;
        truncated.pop();
        assert!(validated_ordered_scene(truncated).is_err());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn fractional_clip_scene_boundary_rejects_corruption() {
        let pdf = include_bytes!("../tests/fixtures/clipped-subpixel-rgb.pdf");
        let scene = page_to_opdl_ordered_bytes(pdf).unwrap();
        assert_eq!(word(&scene, 4).unwrap(), 6);
        let image_at = (0..word(&scene, 36).unwrap() as usize)
            .map(|index| 40 + index * 24)
            .find(|at| word(&scene, *at).unwrap() == 8)
            .unwrap();
        assert!(validated_ordered_scene(scene.clone()).is_ok());
        let mut old_version = scene.clone();
        old_version[4..8].copy_from_slice(&5_u32.to_le_bytes());
        assert!(validated_ordered_scene(old_version).is_err());
        let mut bad_kind = scene.clone();
        bad_kind[image_at..image_at + 4].copy_from_slice(&9_u32.to_le_bytes());
        assert!(validated_ordered_scene(bad_kind).is_err());
        let mut bad_offset = scene.clone();
        bad_offset[image_at + 20..image_at + 24].copy_from_slice(&1_u32.to_le_bytes());
        assert!(validated_ordered_scene(bad_offset).is_err());
        let mut truncated = scene;
        truncated.pop();
        assert!(validated_ordered_scene(truncated).is_err());
    }

    #[cfg(feature = "kernel")]
    #[test]
    fn inherited_rotation_works_and_unsupported_image_states_fall_back() {
        let rotated = include_bytes!("../tests/fixtures/full-page-inherited-rotate.pdf").to_vec();
        assert!(try_render(&rotated, 1, 1.0).unwrap().is_some());
        let pdf = ParsedPdf::open(Arc::new(rotated.clone())).unwrap();
        let result = render_with_report(&pdf, &rotated, 1, 1.0, Backend::Dynlex).unwrap();
        assert_eq!(result.used, Backend::Dynlex);
        let hayro = pdf.render_page(1, 1.0).unwrap();
        assert_eq!(result.raster.rgba, hayro.rgba);

        let filtered = include_bytes!("../tests/fixtures/full-page-filtered-unsupported.pdf");
        assert!(try_render(filtered, 1, 1.0).unwrap().is_none());
        let predictor =
            include_bytes!("../tests/fixtures/full-page-flate-predictor-unsupported.pdf");
        assert!(try_render(predictor, 1, 1.0).unwrap().is_none());
        let oversized = include_bytes!("../tests/fixtures/full-page-flate-oversized.pdf");
        assert!(try_render(oversized, 1, 1.0).unwrap().is_none());
        let outside = include_bytes!("../tests/fixtures/placed-raster-outside.pdf");
        assert!(try_render(outside, 1, 1.0).unwrap().is_none());
        let fractional = include_bytes!("../tests/fixtures/placed-raster-fractional.pdf");
        let pdf = ParsedPdf::open(Arc::new(fractional.to_vec())).unwrap();
        let result = render_with_report(&pdf, fractional, 1, 1.0, Backend::Dynlex).unwrap();
        assert_eq!(result.used, Backend::Dynlex);
        assert_eq!(result.raster.rgba, pdf.render_page(1, 1.0).unwrap().rgba);
    }
}
