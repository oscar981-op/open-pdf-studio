//! Opt-in native Rust boundary for the bounded DynLex vector kernel.
//! Unsupported PDFs return None so the host can keep its existing renderer.
use opds_engine_probe::{ParsedPdf, Raster, RenderError};
#[cfg(feature = "kernel")]
use opds_opdl_adapter::{
    geometry_v3::page_to_opgm_v3_bytes, page_to_opdl_bytes, page_to_opdl_ordered_bytes,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Hayro,
    Dynlex,
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
    if backend == Backend::Dynlex {
        let started = std::time::Instant::now();
        if let Ok(Some(bitmap)) = try_render(source, page, scale) {
            let (page_width_pt, page_height_pt) = pdf.page_dimensions(page)?;
            return Ok(RenderOutcome {
                used: Backend::Dynlex,
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

/// Only a first-page 72-dpi rendering is supported by OPGM-v3.
/// Every unsupported PDF shape or requested zoom is left to the host backend.
pub fn try_render(bytes: &[u8], page: usize, scale: f32) -> Result<Option<Bitmap>, String> {
    if page != 1 || scale != 1.0 {
        return Ok(None);
    }
    #[cfg(not(feature = "kernel"))]
    {
        let _ = bytes;
        Ok(None)
    }
    #[cfg(feature = "kernel")]
    {
        let (scene, width, height) = if let Ok(geometry) = page_to_opgm_v3_bytes(bytes) {
            validated_scene(&geometry)?
        } else {
            let old = page_to_opdl_bytes(bytes)
                .ok()
                .and_then(|scene| validated_rgb_scene(scene).ok());
            let ordered = || {
                page_to_opdl_ordered_bytes(bytes)
                    .ok()
                    .and_then(|scene| validated_ordered_scene(scene).ok())
            };
            match old.or_else(ordered) {
                Some(validated) => validated,
                None => return Ok(None),
            }
        };
        let mode = if word(&scene, 32)? == 0 { 2 } else { 3 };
        let pixel_bytes = usize::try_from(width)
            .unwrap()
            .checked_mul(usize::try_from(height).unwrap())
            .and_then(|n| n.checked_mul(4))
            .ok_or("bitmap size overflow")?;
        let mut output = vec![0_u8; 12 + pixel_bytes];
        // SAFETY: validated_scene bounds every command, dimensions and scene length.
        // The output has a 12-byte ABI header and exactly width*height*4 pixel bytes.
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
        Ok(Some(Bitmap {
            width,
            height,
            rgba: output.split_off(12),
        }))
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
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
    fn rectangular_clipped_images_match_hayro() {
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
            include_bytes!("../tests/fixtures/clipped-nonrect-unsupported.pdf").as_slice(),
            include_bytes!("../tests/fixtures/clipped-subpixel-unsupported-fill.pdf").as_slice(),
            include_bytes!("../tests/fixtures/clipped-subpixel-ambiguous.pdf").as_slice(),
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
