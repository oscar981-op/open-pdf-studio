//! Opt-in native Rust boundary for the bounded DynLex vector kernel.
//! Unsupported PDFs return None so the host can keep its existing renderer.
use opds_engine_probe::{ParsedPdf, Raster, RenderError};
#[cfg(feature = "kernel")]
use opds_opdl_adapter::geometry_v3::page_to_opgm_v3_bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    Hayro,
    Dynlex,
}

/// Keep Hayro operational for every page/zoom outside the verified DynLex subset.
pub fn render_with_fallback(
    pdf: &ParsedPdf,
    source: &[u8],
    page: usize,
    scale: f32,
    backend: Backend,
) -> Result<Raster, RenderError> {
    if backend == Backend::Dynlex {
        let started = std::time::Instant::now();
        if let Ok(Some(bitmap)) = try_render(source, page, scale) {
            let (page_width_pt, page_height_pt) = pdf.page_dimensions(page)?;
            return Ok(Raster {
                width: bitmap.width,
                height: bitmap.height,
                rgba: bitmap.rgba,
                page_width_pt,
                page_height_pt,
                total_pages: pdf.page_count(),
                parse_ms: 0,
                render_ms: started.elapsed().as_millis(),
            });
        }
    }
    pdf.render_page(page, scale)
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
        let geometry = match page_to_opgm_v3_bytes(bytes) {
            Ok(geometry) => geometry,
            Err(_) => return Ok(None),
        };
        let (scene, width, height) = validated_scene(&geometry)?;
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
                2,
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
            || !geometry[at + 20..at + 23]
                .iter()
                .all(|&n| n == 0 || n == 255)
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
        let hayro = pdf.render_page(1, 1.0).unwrap();
        assert_eq!(direct.rgba, routed.rgba);
        assert_eq!(routed.rgba, hayro.rgba);
    }
}
