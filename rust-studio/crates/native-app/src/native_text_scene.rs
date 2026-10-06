//! Direct, bounded native Text Box -> OPGS staging for the opt-in DynLex path.
//! The model and same Liberation Sans bytes used by Save As are the inputs.
use super::text_layout;
use lopdf::{content::Content, Document, Object};
use opds_native_core::annotations::{Annotation, Kind, TextAlign, TEXT_FONT_SIZES};
use opds_opdl_adapter::text_quad_glyph::{self, Source};

#[derive(Clone, Copy, PartialEq, Eq)]
enum PagePaint {
    Blank,
    OpaqueFill,
    Mixed,
}

fn numeric(object: &Object) -> Option<f64> {
    match object {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(f64::from(*value)),
        _ => None,
    }
}

fn supported_page_paint(source: &[u8], page: usize, width: u32, height: u32) -> Option<PagePaint> {
    if source.len() > 16_000_000 {
        return None;
    }
    let Ok(doc) = Document::load_mem(source) else {
        return None;
    };
    if doc.is_encrypted() {
        return None;
    }
    let Some(page_number) = page
        .checked_add(1)
        .and_then(|number| u32::try_from(number).ok())
    else {
        return None;
    };
    let Some(page_id) = doc.get_pages().get(&page_number).copied() else {
        return None;
    };
    if doc
        .get_dictionary(page_id)
        .is_ok_and(|node| node.has(b"Annots") || node.has(b"Group") || node.has(b"UserUnit"))
    {
        // An existing /AP on the source page must never be painted twice.
        return None;
    }
    let Ok((crop, rotation)) = super::pdf_save::print_page_geometry(&doc, page_id) else {
        return None;
    };
    let visible = if matches!(rotation, 90 | 270) {
        [crop[3] - crop[1], crop[2] - crop[0]]
    } else {
        [crop[2] - crop[0], crop[3] - crop[1]]
    };
    if visible != [f64::from(width), f64::from(height)] {
        return None;
    }
    // Keep this classifier independent of PDF decompression. A tiny single
    // unfiltered stream is enough to prove blank/full-page opaque content;
    // everything else retains the normal annotation overlay.
    let contents = doc.get_page_contents(page_id);
    let [content_id] = contents.as_slice() else {
        return None;
    };
    let stream = doc.get_object(*content_id).ok()?.as_stream().ok()?;
    if stream.content.len() > 1024
        || stream.dict.has(b"Filter")
        || stream.dict.has(b"DecodeParms")
        || stream.dict.get(b"Length").ok().and_then(numeric)? != stream.content.len() as f64
    {
        return None;
    }
    let operations = Content::decode(&stream.content).ok()?.operations;
    if operations.is_empty() {
        return Some(PagePaint::Blank);
    }
    let mixed = || {
        let (direct, inherited) = doc.get_page_resources(page_id).ok()?;
        (source.len() <= 2_000_000
            && operations.len() <= 64
            && operations.iter().all(|operation| {
                matches!(
                    operation.operator.as_str(),
                    "rg" | "RG" | "w" | "re" | "f" | "m" | "l" | "S"
                )
            })
            && direct.is_none_or(lopdf::Dictionary::is_empty)
            && inherited.iter().all(|id| {
                doc.get_dictionary(*id)
                    .is_ok_and(lopdf::Dictionary::is_empty)
            }))
        .then_some(PagePaint::Mixed)
    };
    if operations.len() != 3
        || operations[0].operator != "rg"
        || operations[1].operator != "re"
        || operations[2].operator != "f"
        || operations[0].operands.len() != 3
        || operations[1].operands.len() != 4
        || !operations[2].operands.is_empty()
    {
        return mixed();
    }
    let color = operations[0]
        .operands
        .iter()
        .map(numeric)
        .collect::<Option<Vec<_>>>()?;
    if color.iter().any(|value| !(0.0..=1.0).contains(value)) {
        return mixed();
    }
    let rect = operations[1]
        .operands
        .iter()
        .map(numeric)
        .collect::<Option<Vec<_>>>()?;
    if rect[2] <= 0.0
        || rect[3] <= 0.0
        || rect[0] > crop[0]
        || rect[1] > crop[1]
        || rect[0] + rect[2] < crop[2]
        || rect[1] + rect[3] < crop[3]
    {
        return mixed();
    }
    Some(PagePaint::OpaqueFill)
}

/// Experimental bounded mixed-content path. Hayro supplies the source
/// content's transparency as an oracle; DynLex paints every glyph into that
/// premultiplied layer. Reconstructing the visible base must be byte exact.
pub fn eligible_mixed_page(source_pdf: &[u8], page: usize, width: u32, height: u32) -> bool {
    supported_page_paint(source_pdf, page, width, height) == Some(PagePaint::Mixed)
}

/// All visible page pixels come from one validated opaque PDF fill. Ordered
/// native shapes may therefore be painted before or after a Text Box while
/// keeping the same opaque glyph-compositor input contract.
pub fn eligible_opaque_page(source_pdf: &[u8], page: usize, width: u32, height: u32) -> bool {
    supported_page_paint(source_pdf, page, width, height) == Some(PagePaint::OpaqueFill)
}

/// Return a source PDF-content layer only when its premultiplied pixels
/// reconstruct the displayed white-paper bitmap byte for byte. Blank pages
/// need no async render; all other partial pages require a page-bound layer.
pub fn verified_premultiplied_page_layer(
    source_pdf: &[u8],
    page: usize,
    base_rgba: &[u8],
    content_rgba: Option<&[u8]>,
    width: u32,
    height: u32,
    page_points: (f32, f32),
) -> Option<Vec<u8>> {
    if width == 0
        || width > 4096
        || height == 0
        || height > 4096
        || u64::from(width) * u64::from(height) > 6_000_000
        || page_points != (width as f32, height as f32)
        || base_rgba.len() != width as usize * height as usize * 4
        || base_rgba.chunks_exact(4).any(|pixel| pixel[3] != 255)
    {
        return None;
    }
    match supported_page_paint(source_pdf, page, width, height)? {
        PagePaint::Blank => base_rgba
            .chunks_exact(4)
            .all(|pixel| pixel == [255; 4])
            .then(|| vec![0; base_rgba.len()]),
        PagePaint::Mixed => {
            let content = content_rgba.filter(|layer| layer.len() == base_rgba.len())?;
            content
                .chunks_exact(4)
                .zip(base_rgba.chunks_exact(4))
                .all(|(source, visible)| {
                    let paper = 255 - source[3];
                    source[..3]
                        .iter()
                        .zip(&visible[..3])
                        .all(|(channel, target)| {
                            *channel <= source[3] && channel.saturating_add(paper) == *target
                        })
                })
                .then(|| content.to_vec())
        }
        PagePaint::OpaqueFill => None,
    }
}

/// An async content render can prove opacity under a colored Highlight only
/// for this small, independently classified source subset. The compositor
/// still checks every covered source pixel after the render completes.
pub fn eligible_annotation_content_layer(
    source_pdf: &[u8],
    page: usize,
    width: u32,
    height: u32,
) -> bool {
    matches!(
        supported_page_paint(source_pdf, page, width, height),
        Some(PagePaint::Mixed | PagePaint::OpaqueFill)
    )
}

pub fn render_mixed_page_text_box(
    source_pdf: &[u8],
    page: usize,
    base_rgba: &[u8],
    content_rgba: &[u8],
    width: u32,
    height: u32,
    page_points: (f32, f32),
    mark: &Annotation,
) -> Result<Option<Vec<u8>>, String> {
    if width == 0
        || width > 256
        || height == 0
        || height > 4096
        || page_points != (width as f32, height as f32)
        || base_rgba.len() != width as usize * height as usize * 4
        || content_rgba.len() != base_rgba.len()
        || base_rgba.chunks_exact(4).any(|pixel| pixel[3] != 255)
        || mark.bounds.x + mark.bounds.width > f64::from(width)
        || mark.bounds.y + mark.bounds.height > f64::from(height)
        || supported_page_paint(source_pdf, page, width, height) != Some(PagePaint::Mixed)
    {
        return Ok(None);
    }
    if content_rgba
        .chunks_exact(4)
        .zip(base_rgba.chunks_exact(4))
        .any(|(source, visible)| {
            let paper = 255 - source[3];
            visible[3] != 255
                || source[..3]
                    .iter()
                    .zip(&visible[..3])
                    .any(|(channel, target)| channel.saturating_add(paper) != *target)
        })
    {
        return Ok(None);
    }
    let scene = match stage_native_text_box_scene(mark) {
        Ok(scene) => scene,
        Err(_) => return Ok(None),
    };
    let clip = [
        mark.bounds.x as u32,
        mark.bounds.y as u32,
        (mark.bounds.x + mark.bounds.width) as u32,
        (mark.bounds.y + mark.bounds.height) as u32,
    ];
    let mut output = match opds_dynlex_backend::render_validated_glyph_scene_over_premultiplied(
        &scene,
        content_rgba,
        width,
        height,
        clip,
    ) {
        Ok(bitmap) => bitmap.rgba,
        Err(_) => return Ok(None),
    };
    for pixel in output.chunks_exact_mut(4) {
        let paper = 255 - pixel[3];
        for channel in &mut pixel[..3] {
            *channel = channel.saturating_add(paper);
        }
        pixel[3] = 255;
    }
    Ok(Some(output))
}

/// Paint one validated run on a blank or provably opaque single-fill page at
/// one pixel per point. Other page states retain the existing Iced overlay.
pub fn render_opaque_page_text_box(
    source_pdf: &[u8],
    page: usize,
    base_rgba: &[u8],
    width: u32,
    height: u32,
    page_points: (f32, f32),
    mark: &Annotation,
) -> Result<Option<Vec<u8>>, String> {
    if width == 0
        || width > 256
        || height == 0
        || height > 4096
        || page_points != (width as f32, height as f32)
        || base_rgba.len() != width as usize * height as usize * 4
        || base_rgba.chunks_exact(4).any(|pixel| pixel[3] != 255)
        || mark.bounds.x + mark.bounds.width > f64::from(width)
        || mark.bounds.y + mark.bounds.height > f64::from(height)
    {
        return Ok(None);
    }
    let Some(page_paint) = supported_page_paint(source_pdf, page, width, height) else {
        return Ok(None);
    };
    if page_paint == PagePaint::Blank
        && base_rgba
            .chunks_exact(4)
            .any(|pixel| pixel != [255, 255, 255, 255])
    {
        return Ok(None);
    }
    let scene = match stage_native_text_box_scene(mark) {
        Ok(scene) => scene,
        Err(_) => return Ok(None),
    };
    let clip = [
        mark.bounds.x as u32,
        mark.bounds.y as u32,
        (mark.bounds.x + mark.bounds.width) as u32,
        (mark.bounds.y + mark.bounds.height) as u32,
    ];
    if page_paint == PagePaint::OpaqueFill {
        return match opds_dynlex_backend::render_validated_glyph_scene_over_opaque(
            &scene, base_rgba, width, height, clip,
        ) {
            Ok(bitmap) => Ok(Some(bitmap.rgba)),
            Err(_) => Ok(None),
        };
    }
    if page_paint != PagePaint::Blank {
        return Ok(None);
    }
    let tile = opds_dynlex_backend::render_validated_glyph_scene(&scene)?;
    let (left, top, right, bottom) = (
        clip[0] as usize,
        clip[1] as usize,
        clip[2] as usize,
        clip[3] as usize,
    );
    for y in 0..tile.height as usize {
        for x in 0..tile.width as usize {
            if (x < left || x >= right || y < top || y >= bottom)
                && tile.rgba[(y * tile.width as usize + x) * 4..][..4] != [255; 4]
            {
                return Ok(None);
            }
        }
    }
    let mut output = base_rgba.to_vec();
    for y in 0..height.min(tile.height) as usize {
        let dst = y * width as usize * 4;
        let src = y * tile.width as usize * 4;
        output[dst..dst + width as usize * 4]
            .copy_from_slice(&tile.rgba[src..src + width as usize * 4]);
    }
    Ok(Some(output))
}

pub fn stage_native_text_box_scene(mark: &Annotation) -> Result<Vec<u8>, String> {
    stage_native_text_box_scene_at_origin(mark, [0, 0])
}

/// Stage one native Text Box into a 256×192 glyph tile at an integral page
/// origin. The origin is carried separately by OPGC-v2 at composition time.
pub fn stage_native_text_box_scene_at_origin(
    mark: &Annotation,
    origin: [u32; 2],
) -> Result<Vec<u8>, String> {
    let Kind::Text {
        value,
        color_rgb,
        font_size,
        align,
    } = &mark.kind
    else {
        return Err("not a native Text Box".into());
    };
    let bounds = mark.bounds;
    if !TEXT_FONT_SIZES.contains(font_size)
        || [bounds.x, bounds.y, bounds.width, bounds.height]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0 || v.fract() != 0.0)
        || bounds.width < 4.0
        || bounds.height < f64::from(*font_size)
        || bounds.x < f64::from(origin[0])
        || bounds.y < f64::from(origin[1])
        || bounds.x + bounds.width > f64::from(origin[0]) + 256.0
        || bounds.y + bounds.height > f64::from(origin[1]) + 192.0
    {
        return Err("Text Box is outside proven glyph tile geometry".into());
    }
    let face = text_layout::font_face().ok_or("native embedded font unavailable")?;
    let lines = text_layout::wrap_text(value, bounds.width, f64::from(*font_size), face);
    if value.chars().count() > 64
        || !(1..=4).contains(&lines.len())
        || value.chars().any(|ch| ch.is_control() && ch != '\n')
    {
        return Err("Text Box run or wrapping is outside proven tile".into());
    }
    let units = f64::from(face.units_per_em());
    let size = f64::from(*font_size);
    let mut records = Vec::new();
    let mut glyph_count = 0_usize;
    for (index, line) in lines.iter().enumerate() {
        // Save As sums unscaled advances first, then scales the line once.
        let advance_units = line.text.chars().try_fold(0_u64, |total, ch| {
            let gid = face.glyph_index(ch).ok_or("native glyph missing")?;
            let advance = face
                .glyph_hor_advance(gid)
                .ok_or("native glyph width missing")?;
            total
                .checked_add(u64::from(advance))
                .ok_or("native line advance exceeds bound")
        })?;
        let width = advance_units as f64 * size / units;
        let x = match align {
            TextAlign::Left => 0.0,
            TextAlign::Center => (bounds.width - width) / 2.0,
            TextAlign::Right => bounds.width - width,
        };
        // Save As writes every visual line's text matrix to three decimals.
        let line_x = format!("{x:.3}")
            .parse::<f64>()
            .map_err(|_| "invalid native line origin")?;
        let mut offset = 0.0_f64;
        for ch in line.text.chars() {
            let code = ch as u32;
            let allowed = (0x20..=0x024F).contains(&code)
                || (0x0370..=0x052F).contains(&code)
                || (0x2000..=0x206F).contains(&code)
                || code == 0x20AC;
            if !allowed {
                return Err("Text Box script unsupported by native Save As".into());
            }
            let gid = face.glyph_index(ch).ok_or("native glyph missing")?;
            let advance = face
                .glyph_hor_advance(gid)
                .ok_or("native glyph width missing")?;
            if ch != ' ' {
                let matrix = [
                    size / units,
                    0.0,
                    0.0,
                    -size / units,
                    bounds.x + line_x + offset,
                    bounds.y + size + index as f64 * (size + 4.0),
                ];
                let (mut opm, old_shift) = text_quad_glyph::stage(
                    text_layout::FONT_BYTES,
                    gid.0,
                    matrix,
                    Source::SkrifaAtThousand,
                )
                .map(|(scene, shift, _)| (scene, shift))
                .or_else(|_| {
                    text_quad_glyph::stage_line_only(text_layout::FONT_BYTES, gid.0, matrix)
                })
                .map_err(|error| error.to_string())?;
                let glyph = text_quad_glyph::absolute_bounds(&opm, old_shift)
                    .map_err(|error| error.to_string())?;
                if glyph[0] < bounds.x
                    || glyph[1] < bounds.y
                    || glyph[2] > bounds.x + bounds.width
                    || glyph[3] > bounds.y + bounds.height
                {
                    return Err("native glyph would require Text Box clipping".into());
                }
                if origin == [0, 0] {
                    text_quad_glyph::rebase(&mut opm, old_shift, [0.0, 0.0])
                } else {
                    text_quad_glyph::rebase_screen_f32(&mut opm, old_shift, origin)
                }
                .map_err(|error| error.to_string())?;
                let mut name = [0_u8; 32];
                name[..5].copy_from_slice(b"OPDS0");
                records.extend_from_slice(&name);
                records.extend_from_slice(&gid.0.to_le_bytes()); // Identity CID
                records.extend_from_slice(&gid.0.to_le_bytes()); // Identity GID
                records.extend_from_slice(color_rgb);
                records.push(0);
                records.extend_from_slice(&(opm.len() as u32).to_le_bytes());
                records.extend_from_slice(&opm);
                glyph_count += 1;
                if records.len() + 20 > 140_000 {
                    return Err("native glyph scene exceeds bound".into());
                }
            }
            // The PDF CID width is a lopdf Real (f32), not an unrounded
            // TTF advance. Match each Tj text cursor exactly.
            let cid_width = (f64::from(advance) * 1000.0 / units) as f32;
            offset += f64::from(cid_width) * size / 1000.0;
        }
    }
    // OPGS v1 is deliberately bounded to a small number of painted glyphs.
    // Spaces and blank visual lines affect placement but create no records.
    if !(2..=32).contains(&glyph_count) {
        return Err("native painted glyph count outside OPGS limit".into());
    }
    let mut scene = Vec::with_capacity(20 + records.len());
    scene.extend_from_slice(b"OPGS");
    for word in [1_u32, 256, 192, glyph_count as u32] {
        scene.extend_from_slice(&word.to_le_bytes());
    }
    scene.extend_from_slice(&records);
    Ok(scene)
}

#[cfg(test)]
mod tests {
    use super::*;
    use opds_native_core::annotations::Rect;

    #[test]
    fn native_model_stages_same_glyphs_as_saved_appearance() {
        let mark = Annotation {
            id: 321,
            page: 0,
            bounds: Rect {
                x: 20.0,
                y: 30.0,
                width: 110.0,
                height: 58.0,
            },
            kind: Kind::Text {
                value: "ΩΩΩ".into(),
                color_rgb: [0, 80, 240],
                font_size: 20,
                align: TextAlign::Center,
            },
        };
        let staged = stage_native_text_box_scene(&mark).unwrap();
        let saved =
            include_bytes!("../../../dynlex-engine-poc/rgba/fixtures/native-blue-omega.opgs");
        let result = opds_dynlex_backend::render_validated_glyph_scene(&staged).unwrap();
        let reference = opds_dynlex_backend::render_validated_glyph_scene(saved).unwrap();
        assert_eq!((result.width, result.height), (256, 192));
        let differences: Vec<_> = result
            .rgba
            .iter()
            .zip(&reference.rgba)
            .enumerate()
            .filter_map(|(index, (actual, expected))| {
                (actual != expected).then_some((index, *actual, *expected))
            })
            .collect();
        assert!(
            differences.is_empty(),
            "native glyph pixels differ: {} bytes, first {:?}",
            differences.len(),
            &differences[..differences.len().min(16)]
        );
        let mut multiline = mark.clone();
        if let Kind::Text { value, .. } = &mut multiline.kind {
            *value = "Ω\nΩ".into();
        }
        assert!(stage_native_text_box_scene(&multiline).is_ok());
        let mut clipped = multiline.clone();
        clipped.bounds.height = 25.0;
        assert!(stage_native_text_box_scene(&clipped).is_err());
        let mut outside = mark.clone();
        outside.bounds.y = 150.0;
        assert!(stage_native_text_box_scene(&outside).is_err());
        let mut missing = mark;
        if let Kind::Text { value, .. } = &mut missing.kind {
            *value = "🦀🦀".into();
        }
        assert!(stage_native_text_box_scene(&missing).is_err());
    }
}
