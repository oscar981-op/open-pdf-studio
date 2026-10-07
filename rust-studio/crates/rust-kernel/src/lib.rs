//! Small, independent pure-Rust PDF raster kernel. The bounded PDF→OPGM
//! adapter supplies validated geometry; this renderer paints its own RGBA
//! pixels and never calls DynLex, Hayro, PDFium or a foreign rasterizer.
#![forbid(unsafe_code)]
use opds_engine_probe::Raster;
use opds_opdl_adapter::{geometry_v3::page_to_opgm_v3_bytes, page_to_opdl_ordered_bytes};
use std::time::Instant;

fn word(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

pub fn render(source: &[u8], page_number: usize, scale: f32) -> Result<Raster, String> {
    if page_number != 1 || scale != 1.0 || source.len() > 16_000_000 {
        return Err("own Rust kernel only supports bounded page 1 at 100%".into());
    }
    let started = Instant::now();
    if let Ok(scene) = page_to_opgm_v3_bytes(source) {
        return render_scene(&scene, started);
    }
    let scene = page_to_opdl_ordered_bytes(source).map_err(|e| e.to_string())?;
    render_ordered_scene(&scene, started)
}

enum OrderedCommand {
    Solid([u32; 4], [u8; 4]),
    Image([u32; 4], std::ops::Range<usize>),
}

/// Independently paint the bounded opaque, ordered OPDL-v3 scene. Newer
/// coverage-mask versions deliberately fall back until their blending has
/// its own pixel proof in this kernel.
fn render_ordered_scene(scene: &[u8], started: Instant) -> Result<Raster, String> {
    if scene.len() < 40
        || scene.len() > 3_538_984
        || scene.get(..4) != Some(b"OPDL")
        || word(scene, 4) != Some(3)
        || word(scene, 32) != Some(0)
    {
        return Err("unsupported ordered OPDL header".into());
    }
    let width = word(scene, 8).ok_or("missing width")?;
    let height = word(scene, 12).ok_or("missing height")?;
    let pixels = width.checked_mul(height).ok_or("page size overflow")?;
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || pixels > 6_000_000
        || [
            word(scene, 16),
            word(scene, 20),
            word(scene, 24),
            word(scene, 28),
        ] != [Some(0), Some(0), Some(width), Some(height)]
    {
        return Err("invalid ordered OPDL page".into());
    }
    let count = word(scene, 36).ok_or("missing command count")? as usize;
    let image_start = count
        .checked_mul(24)
        .and_then(|bytes| 40_usize.checked_add(bytes))
        .ok_or("command table overflow")?;
    if count > 16_384 || image_start > scene.len() {
        return Err("invalid ordered command table".into());
    }
    let mut commands = Vec::with_capacity(count);
    let mut image_bytes = 0_usize;
    let mut image_count = 0_usize;
    for index in 0..count {
        let at = 40 + index * 24;
        let kind = word(scene, at).ok_or("truncated command")?;
        let rect = [
            word(scene, at + 4).ok_or("truncated rectangle")?,
            word(scene, at + 8).ok_or("truncated rectangle")?,
            word(scene, at + 12).ok_or("truncated rectangle")?,
            word(scene, at + 16).ok_or("truncated rectangle")?,
        ];
        if rect[0] >= rect[2] || rect[1] >= rect[3] || rect[2] > width || rect[3] > height {
            return Err("invalid ordered command bounds".into());
        }
        match kind {
            1 | 3 => {
                let ink: [u8; 4] = scene[at + 20..at + 24].try_into().unwrap();
                if ink[3] != 255 || (kind == 3 && rect[2] - rect[0] != 1 && rect[3] - rect[1] != 1)
                {
                    return Err("invalid ordered vector command".into());
                }
                commands.push(OrderedCommand::Solid(rect, ink));
            }
            6 => {
                image_count += 1;
                let offset = word(scene, at + 20).ok_or("truncated image offset")? as usize;
                if image_count > 256 || offset != image_bytes {
                    return Err("invalid ordered image offset or count".into());
                }
                let length = (rect[2] - rect[0])
                    .checked_mul(rect[3] - rect[1])
                    .and_then(|n| n.checked_mul(3))
                    .ok_or("image size overflow")? as usize;
                let start = image_start
                    .checked_add(offset)
                    .ok_or("image offset overflow")?;
                let end = start.checked_add(length).ok_or("image length overflow")?;
                if scene.get(start..end).is_none() {
                    return Err("truncated ordered image".into());
                }
                image_bytes = image_bytes
                    .checked_add(length)
                    .ok_or("image tail overflow")?;
                commands.push(OrderedCommand::Image(rect, start..end));
            }
            _ => return Err("unsupported ordered command".into()),
        }
    }
    if image_start.checked_add(image_bytes) != Some(scene.len()) {
        return Err("invalid ordered image tail".into());
    }
    let mut rgba = vec![255; pixels as usize * 4];
    for command in commands {
        match command {
            OrderedCommand::Solid([left, top, right, bottom], ink) => {
                for y in top..bottom {
                    for x in left..right {
                        let dst = ((y * width + x) * 4) as usize;
                        rgba[dst..dst + 4].copy_from_slice(&ink);
                    }
                }
            }
            OrderedCommand::Image([left, top, right, bottom], range) => {
                let rgb = &scene[range];
                let image_width = right - left;
                for y in top..bottom {
                    for x in left..right {
                        let src = (((y - top) * image_width + (x - left)) * 3) as usize;
                        let dst = ((y * width + x) * 4) as usize;
                        rgba[dst..dst + 3].copy_from_slice(&rgb[src..src + 3]);
                    }
                }
            }
        }
    }
    Ok(Raster {
        width,
        height,
        rgba,
        page_width_pt: width as f32,
        page_height_pt: height as f32,
        total_pages: 1,
        parse_ms: 0,
        render_ms: started.elapsed().as_millis(),
    })
}

fn render_scene(scene: &[u8], started: Instant) -> Result<Raster, String> {
    if scene.get(..4) != Some(b"OPGM") || word(scene, 4) != Some(3) {
        return Err("invalid OPGM-v3 header".into());
    }
    let media = [word(scene, 8), word(scene, 12)];
    let crop = [
        word(scene, 16),
        word(scene, 20),
        word(scene, 24),
        word(scene, 28),
    ];
    let turn = word(scene, 32).ok_or("missing page rotation")?;
    let count = word(scene, 36).ok_or("missing command count")? as usize;
    let [Some(x0), Some(y0), Some(x1), Some(y1)] = crop else {
        return Err("missing CropBox".into());
    };
    if media.iter().any(Option::is_none)
        || x0 >= x1
        || y0 >= y1
        || x1 > media[0].unwrap()
        || y1 > media[1].unwrap()
        || !matches!(turn, 0 | 90 | 180 | 270)
        || !(1..=64).contains(&count)
        || scene.len() != 40 + count * 24
    {
        return Err("invalid OPGM-v3 geometry or length".into());
    }
    let (width, height) = if matches!(turn, 90 | 270) {
        (y1 - y0, x1 - x0)
    } else {
        (x1 - x0, y1 - y0)
    };
    if width == 0
        || height == 0
        || width > 4096
        || height > 4096
        || u64::from(width) * u64::from(height) > 6_000_000
    {
        return Err("own Rust kernel bitmap exceeds limit".into());
    }
    // Validate every command before allocating or painting any output.
    let mut commands = Vec::with_capacity(count);
    for index in 0..count {
        let at = 40 + index * 24;
        let kind = word(scene, at).ok_or("truncated command")?;
        let rect = [
            word(scene, at + 4),
            word(scene, at + 8),
            word(scene, at + 12),
            word(scene, at + 16),
        ];
        let [Some(left), Some(top), Some(right), Some(bottom)] = rect else {
            return Err("truncated rectangle".into());
        };
        let ink: [u8; 4] = scene[at + 20..at + 24].try_into().unwrap();
        if !matches!(kind, 1 | 3)
            || left >= right
            || top >= bottom
            || right > width
            || bottom > height
            || ink[3] != 255
            || (kind == 3 && right - left != 1 && bottom - top != 1)
        {
            return Err("unsupported OPGM-v3 command".into());
        }
        commands.push(([left, top, right, bottom], ink));
    }
    let mut rgba = vec![255; width as usize * height as usize * 4];
    for ([left, top, right, bottom], ink) in commands {
        for y in top..bottom {
            let row = y as usize * width as usize * 4;
            for x in left..right {
                let offset = row + x as usize * 4;
                rgba[offset..offset + 4].copy_from_slice(&ink);
            }
        }
    }
    Ok(Raster {
        width,
        height,
        rgba,
        page_width_pt: width as f32,
        page_height_pt: height as f32,
        total_pages: 1,
        parse_ms: 0,
        render_ms: started.elapsed().as_millis(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Document, Object};
    use opds_engine_probe::ParsedPdf;
    use std::sync::Arc;

    const SIMPLE: &[u8] =
        include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf");
    const RASTER: &[u8] =
        include_bytes!("../../dynlex-backend/tests/fixtures/placed-raster-flate.pdf");
    const HYBRID: &[u8] =
        include_bytes!("../../dynlex-backend/tests/fixtures/two-images-hybrid.pdf");
    const ROTATED: &[u8] =
        include_bytes!("../../dynlex-backend/tests/fixtures/rotated-cropped-image-90.pdf");
    const MANY_IMAGES: &[u8] =
        include_bytes!("../../dynlex-backend/tests/fixtures/three-images-layered.pdf");

    #[test]
    fn rejects_bad_requests_and_corrupt_scenes() {
        assert!(render(SIMPLE, 0, 1.0).is_err());
        assert!(render(SIMPLE, 1, 1.25).is_err());
        let now = Instant::now();
        assert!(render_scene(b"OPGM", now).is_err());
        let mut scene = page_to_opgm_v3_bytes(SIMPLE).unwrap();
        scene.pop();
        assert!(render_scene(&scene, now).is_err());
    }

    #[test]
    fn simple_page_matches_hayro_exactly() {
        for turn in [0, 90, 180, 270] {
            let mut source = Document::load_mem(SIMPLE).unwrap();
            let page_id = *source.get_pages().get(&1).unwrap();
            source
                .get_object_mut(page_id)
                .unwrap()
                .as_dict_mut()
                .unwrap()
                .set("Rotate", turn);
            let mut bytes = Vec::new();
            source.save_to(&mut bytes).unwrap();
            let actual = render(&bytes, 1, 1.0).unwrap();
            let pdf = ParsedPdf::open(Arc::new(bytes)).unwrap();
            let expected = pdf.render_page(1, 1.0).unwrap();
            assert_eq!(
                (actual.width, actual.height),
                (expected.width, expected.height)
            );
            assert_eq!(actual.rgba, expected.rgba, "rotation={turn}");
        }
    }

    #[test]
    fn page_with_annotations_is_not_silently_rendered_without_them() {
        let mut source = Document::load_mem(SIMPLE).unwrap();
        let page_id = *source.get_pages().get(&1).unwrap();
        source
            .get_object_mut(page_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Annots", Object::Array(vec![]));
        let mut bytes = Vec::new();
        source.save_to(&mut bytes).unwrap();
        assert!(render(&bytes, 1, 1.0).is_err());
    }

    #[test]
    fn ordered_images_and_vector_layers_match_hayro() {
        for source in [RASTER, HYBRID, ROTATED, MANY_IMAGES] {
            let scene = page_to_opdl_ordered_bytes(source).unwrap();
            assert_eq!(word(&scene, 4), Some(3));
            let actual = render(source, 1, 1.0).unwrap();
            let expected = ParsedPdf::open(Arc::new(source.to_vec()))
                .unwrap()
                .render_page(1, 1.0)
                .unwrap();
            assert_eq!(
                (actual.width, actual.height),
                (expected.width, expected.height)
            );
            assert_eq!(actual.rgba, expected.rgba);
        }
    }

    #[test]
    fn ordered_images_reject_aliases_truncation_and_unknown_coverage() {
        let scene = page_to_opdl_ordered_bytes(HYBRID).unwrap();
        let now = Instant::now();
        let mut truncated = scene.clone();
        truncated.pop();
        assert!(render_ordered_scene(&truncated, now).is_err());
        let image_at = (0..word(&scene, 36).unwrap() as usize)
            .map(|index| 40 + index * 24)
            .find(|&at| word(&scene, at) == Some(6))
            .unwrap();
        let mut aliased = scene.clone();
        aliased[image_at + 20..image_at + 24].copy_from_slice(&1_u32.to_le_bytes());
        assert!(render_ordered_scene(&aliased, now).is_err());
        let mut coverage = scene;
        coverage[4..8].copy_from_slice(&6_u32.to_le_bytes());
        assert!(render_ordered_scene(&coverage, now).is_err());
    }
}
