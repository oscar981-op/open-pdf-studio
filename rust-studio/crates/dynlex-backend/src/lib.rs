//! Opt-in native Rust boundary for the bounded DynLex vector kernel.
//! Unsupported PDFs return None so the host can keep its existing renderer.
use opds_engine_probe::{ParsedPdf, Raster, RenderError};
#[cfg(feature = "kernel")]
use opds_opdl_adapter::{geometry_v3::page_to_opgm_v3_bytes, page_to_opdl_bytes};

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
            let scene = match page_to_opdl_bytes(bytes) {
                Ok(scene) => scene,
                Err(_) => return Ok(None),
            };
            match validated_rgb_scene(scene) {
                Ok(validated) => validated,
                Err(_) => return Ok(None),
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

/// The older PDF adapter supports only one unfiltered, full-page RGB8 XObject
/// before its vector commands. Revalidate its bytes before crossing the ABI.
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
    fn unsupported_image_state_never_enters_dynlex() {
        let rotated = include_bytes!("../tests/fixtures/full-page-inherited-rotate.pdf").to_vec();
        assert!(try_render(&rotated, 1, 1.0).unwrap().is_none());
        let pdf = ParsedPdf::open(Arc::new(rotated.clone())).unwrap();
        let result = render_with_report(&pdf, &rotated, 1, 1.0, Backend::Dynlex).unwrap();
        assert_eq!(result.used, Backend::Hayro);
        let hayro = pdf.render_page(1, 1.0).unwrap();
        assert_eq!(result.raster.rgba, hayro.rgba);

        let filtered = include_bytes!("../tests/fixtures/full-page-filtered-unsupported.pdf");
        assert!(try_render(filtered, 1, 1.0).unwrap().is_none());
    }
}
