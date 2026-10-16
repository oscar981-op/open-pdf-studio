//! Bounded in-process glyph-run entry point. Text extraction/placement remains
//! experimental; this only renders a validated OPGS tile with DynLex coverage.
use crate::Bitmap;

#[path = "../../../dynlex-engine-poc/rgba/glyph_run_core.rs"]
mod core;

pub fn render_validated_glyph_scene(scene: &[u8]) -> Result<Bitmap, String> {
    let rgba = core::render_opgs(scene).map_err(|error| error.to_string())?;
    if rgba.len() != 256 * 192 * 4 {
        return Err("DynLex glyph scene produced an invalid tile".into());
    }
    Ok(Bitmap {
        width: 256,
        height: 192,
        rgba,
    })
}

/// Return the validated glyph group's premultiplied RGBA tile without page
/// composition. The same DynLex coverage kernel paints every glyph.
pub fn render_validated_glyph_layer(scene: &[u8]) -> Result<Bitmap, String> {
    let rgba = core::render_opgs_layer(scene).map_err(|error| error.to_string())?;
    if rgba.len() != 256 * 192 * 4 {
        return Err("DynLex glyph layer produced an invalid tile".into());
    }
    Ok(Bitmap {
        width: 256,
        height: 192,
        rgba,
    })
}

/// Paint the validated glyphs directly over an opaque page, in PDF content
/// order, using the separate DynLex 8-bit compositor for each glyph.
pub fn render_validated_glyph_scene_over_opaque(
    scene: &[u8],
    base_rgba: &[u8],
    width: u32,
    height: u32,
    clip: [u32; 4],
) -> Result<Bitmap, String> {
    let rgba = core::render_opgs_over_opaque(scene, base_rgba, width, height, clip)
        .map_err(|error| error.to_string())?;
    Ok(Bitmap {
        width,
        height,
        rgba,
    })
}

/// Paint a 256×192 validated glyph tile at an integral position on a bounded
/// larger opaque page. OPGC-v2 carries the tile origin to DynLex explicitly.
pub fn render_validated_glyph_scene_over_opaque_at_origin(
    scene: &[u8],
    base_rgba: &[u8],
    width: u32,
    height: u32,
    clip: [u32; 4],
    origin: [u32; 2],
) -> Result<Bitmap, String> {
    let rgba =
        core::render_opgs_over_opaque_at_origin(scene, base_rgba, width, height, clip, origin)
            .map_err(|error| error.to_string())?;
    Ok(Bitmap {
        width,
        height,
        rgba,
    })
}

/// Consume the already validated opaque page to avoid a full-page copy for
/// each successive text tile in a large annotation.
pub fn render_validated_glyph_scene_over_opaque_at_origin_owned(
    scene: &[u8],
    base_rgba: Vec<u8>,
    width: u32,
    height: u32,
    clip: [u32; 4],
    origin: [u32; 2],
) -> Result<Bitmap, String> {
    let rgba = core::render_opgs_over_opaque_at_origin_owned(
        scene, base_rgba, width, height, clip, origin,
    )
    .map_err(|error| error.to_string())?;
    Ok(Bitmap {
        width,
        height,
        rgba,
    })
}

/// Diagnostic path: render glyphs into an already-premultiplied PDF content
/// layer. The caller is responsible for final paper composition.
pub fn render_validated_glyph_scene_over_premultiplied(
    scene: &[u8],
    base_rgba: &[u8],
    width: u32,
    height: u32,
    clip: [u32; 4],
) -> Result<Bitmap, String> {
    let rgba = core::render_opgs_over_premultiplied(scene, base_rgba, width, height, clip)
        .map_err(|error| error.to_string())?;
    Ok(Bitmap {
        width,
        height,
        rgba,
    })
}

/// Paint a validated glyph tile at an integral position on a bounded larger
/// premultiplied PDF-content layer. Paper composition remains with the caller.
pub fn render_validated_glyph_scene_over_premultiplied_at_origin(
    scene: &[u8],
    base_rgba: &[u8],
    width: u32,
    height: u32,
    clip: [u32; 4],
    origin: [u32; 2],
) -> Result<Bitmap, String> {
    let rgba = core::render_opgs_over_premultiplied_at_origin(
        scene, base_rgba, width, height, clip, origin,
    )
    .map_err(|error| error.to_string())?;
    Ok(Bitmap {
        width,
        height,
        rgba,
    })
}

pub fn render_validated_glyph_scene_over_premultiplied_at_origin_owned(
    scene: &[u8],
    base_rgba: Vec<u8>,
    width: u32,
    height: u32,
    clip: [u32; 4],
    origin: [u32; 2],
) -> Result<Bitmap, String> {
    let rgba = core::render_opgs_over_premultiplied_at_origin_owned(
        scene, base_rgba, width, height, clip, origin,
    )
    .map_err(|error| error.to_string())?;
    Ok(Bitmap {
        width,
        height,
        rgba,
    })
}

/// Use the same validated DynLex area/compositing kernels for a geometric
/// Form path. The caller supplies a real OPM2 outline, never a fake glyph.
pub fn render_validated_polygon_over_premultiplied_at_origin_owned(
    outline: &[u8],
    color: [u8; 3],
    base_rgba: Vec<u8>,
    width: u32,
    height: u32,
    clip: [u32; 4],
    origin: [u32; 2],
) -> Result<Bitmap, String> {
    let rgba = core::render_opm_over_premultiplied_at_origin_owned(
        outline, color, base_rgba, width, height, clip, origin,
    )
    .map_err(|error| error.to_string())?;
    Ok(Bitmap {
        width,
        height,
        rgba,
    })
}

pub(crate) fn validate_polygon_outline(outline: &[u8]) -> bool {
    core::validate_opm_outline(outline).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_scene_cannot_reach_glyph_kernel() {
        assert!(render_validated_glyph_scene(b"OPGS").is_err());
        let mut scene = b"OPGS".to_vec();
        for word in [1_u32, 256, 192, 1] {
            scene.extend_from_slice(&word.to_le_bytes());
        }
        assert!(render_validated_glyph_scene(&scene).is_err());
    }

    #[test]
    fn premultiplied_page_validation_rejects_unassociated_color() {
        let scene =
            include_bytes!("../../../dynlex-engine-poc/rgba/fixtures/native-blue-omega.opgs");
        let mut invalid = vec![0_u8; 200 * 192 * 4];
        invalid[0] = 1;
        assert!(render_validated_glyph_scene_over_premultiplied(
            scene,
            &invalid,
            200,
            192,
            [20, 30, 130, 88],
        )
        .is_err());
    }

    #[test]
    fn movable_glyph_tile_respects_page_bounds_and_source_alpha() {
        let scene =
            include_bytes!("../../../dynlex-engine-poc/rgba/fixtures/native-blue-omega.opgs");
        let white = vec![255_u8; 595 * 842 * 4];
        let output = render_validated_glyph_scene_over_opaque_at_origin(
            scene,
            &white,
            595,
            842,
            [320, 430, 430, 488],
            [300, 400],
        )
        .unwrap();
        assert_eq!(output.rgba.len(), white.len());
        assert!(output.rgba.chunks_exact(4).any(|pixel| pixel != [255; 4]));
        assert!(render_validated_glyph_scene_over_opaque_at_origin(
            scene,
            &white,
            595,
            842,
            [320, 430, 430, 488],
            [400, 400],
        )
        .is_err());
        let mut invalid_layer = vec![0_u8; white.len()];
        invalid_layer[0] = 1;
        assert!(render_validated_glyph_scene_over_premultiplied_at_origin(
            scene,
            &invalid_layer,
            595,
            842,
            [320, 430, 430, 488],
            [300, 400],
        )
        .is_err());
    }

    #[test]
    fn one_glyph_tile_is_valid_but_empty_scene_is_rejected() {
        let source =
            include_bytes!("../../../dynlex-engine-poc/rgba/fixtures/native-blue-omega.opgs");
        let length = u32::from_le_bytes(source[60..64].try_into().unwrap()) as usize;
        let mut single = source[..20 + 44 + length].to_vec();
        single[16..20].copy_from_slice(&1_u32.to_le_bytes());
        assert_eq!(
            render_validated_glyph_scene(&single).unwrap().rgba.len(),
            256 * 192 * 4
        );
        single[16..20].copy_from_slice(&0_u32.to_le_bytes());
        assert!(render_validated_glyph_scene(&single).is_err());
    }

    #[test]
    fn saved_native_blue_omega_scene_matches_hayro_oracle() {
        // Generated by stage_type0_text from the saved /FreeText fixture in
        // pdf_save.rs. The expected checksum was independently confirmed by
        // Hayro on its isolated 256x192 oracle PDF.
        let scene =
            include_bytes!("../../../dynlex-engine-poc/rgba/fixtures/native-blue-omega.opgs");
        let result = render_validated_glyph_scene(scene).unwrap();
        assert_eq!((result.width, result.height), (256, 192));
        let hash = result
            .rgba
            .iter()
            .fold(0xcbf29ce484222325_u64, |hash, &byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
        assert_eq!(hash, 0x79c4c5f51fbacaba);
        assert!(result.rgba.chunks_exact(4).all(|pixel| pixel[3] == 255));
        for prefix in 0..scene.len() {
            assert!(
                render_validated_glyph_scene(&scene[..prefix]).is_err(),
                "truncated OPGS accepted at {prefix} bytes"
            );
        }
    }
}

// Research-only caller has validated OPNW structure and aligned coordinates.
// This does not change OPGS/OPM2 acceptance or native dispatch.
pub(crate) fn paint_experimental_winding(
    words: Vec<u64>,
    color: [u8; 3],
    output: Vec<u8>,
    width: u32,
    height: u32,
    origin: [u32; 2],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    core::render_outlines_over_page_owned(
        vec![(words, color)],
        output,
        width,
        height,
        [0, 0, width, height],
        origin,
        true,
        true,
    )
}

/// Private typed-scene bridge; raw outlines cannot enter through a public API.
pub(crate) fn render_prepared_winding_page(
    width: u32,
    height: u32,
    scenes: &[(&[u64], [u8; 3], [u32; 2], [u32; 4])],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    core::render_prepared_page(width, height, scenes)
}

/// Per-paint rectangle masks for the validated v3 prepared scene. The
/// separate probe called this first; native replay now uses the same math.
pub(crate) fn render_prepared_winding_page_fractional(
    width: u32,
    height: u32,
    scenes: &[(&[u64], [u8; 3], [u32; 2], [u32; 4])],
    fractional_clips: &[Option<[f64; 4]>],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    core::render_prepared_page_fractional(width, height, scenes, fractional_clips)
}

pub(crate) fn render_prepared_winding_page_fractional_over_opaque(
    width: u32,
    height: u32,
    scenes: &[(&[u64], [u8; 3], [u32; 2], [u32; 4])],
    fractional_clips: &[Option<[f64; 4]>],
    background: Vec<u8>,
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    core::render_prepared_page_fractional_over_opaque(
        width,
        height,
        scenes,
        fractional_clips,
        background,
    )
}

/// Diagnostic split timings only; ordinary replay does not read a clock per paint.
pub(crate) fn profile_prepared_winding_page(
    width: u32,
    height: u32,
    scenes: &[(&[u64], [u8; 3], [u32; 2], [u32; 4])],
    profile: &mut [std::time::Duration; 4],
) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    core::render_prepared_page_profiled(width, height, scenes, Some(profile))
}
