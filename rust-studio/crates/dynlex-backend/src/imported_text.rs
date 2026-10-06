//! Bounded direct staging of a saved Open PDF Studio FreeText /AP into OPGS.
//! Hayro renders the PDF body; DynLex paints only fully validated glyphs.
use crate::imported_appearance::display_point;
use lopdf::{content::Content, Dictionary, Document, Object};
use opds_opdl_adapter::text_quad_glyph::{self, Source};
use std::collections::HashMap;

const FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/LiberationSans-Regular.ttf");

fn number(value: &Object) -> Option<f64> {
    match value {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(f64::from(*value)),
        _ => None,
    }
}

fn quad(value: &Object) -> Option<[f64; 4]> {
    let [a, b, c, d] = value.as_array().ok()?.as_slice() else {
        return None;
    };
    let result = [number(a)?, number(b)?, number(c)?, number(d)?];
    result.iter().all(|v| v.is_finite()).then_some(result)
}

fn name(dict: &Dictionary, key: &[u8], expected: &[u8]) -> bool {
    dict.get(key).ok().and_then(|v| v.as_name().ok()) == Some(expected)
}

fn color(value: &Object) -> Option<u8> {
    let value = number(value)?;
    if !(0.0..=1.0).contains(&value) {
        return None;
    }
    let byte = (value * 255.0).round() as u8;
    let printed = format!("{:.3}", f64::from(byte) / 255.0)
        .parse::<f64>()
        .ok()?;
    ((value - printed).abs() <= 0.00001).then_some(byte)
}

fn embedded_native_font(doc: &Document, resources: &Dictionary) -> Option<HashMap<u16, f64>> {
    if resources.len() != 1 {
        return None;
    }
    let fonts = resources.get(b"Font").ok()?.as_dict().ok()?;
    if fonts.len() != 1 {
        return None;
    }
    let (_, font) = doc.dereference(fonts.get(b"OPDS0").ok()?).ok()?;
    let font = font.as_dict().ok()?;
    if !name(font, b"Type", b"Font")
        || !name(font, b"Subtype", b"Type0")
        || !name(font, b"Encoding", b"Identity-H")
    {
        return None;
    }
    let [descendant] = doc
        .dereference(font.get(b"DescendantFonts").ok()?)
        .ok()?
        .1
        .as_array()
        .ok()?
        .as_slice()
    else {
        return None;
    };
    let (_, cid) = doc.dereference(descendant).ok()?;
    let cid = cid.as_dict().ok()?;
    if !name(cid, b"Subtype", b"CIDFontType2") || !name(cid, b"CIDToGIDMap", b"Identity") {
        return None;
    }
    let (_, descriptor) = doc.dereference(cid.get(b"FontDescriptor").ok()?).ok()?;
    let (_, file) = doc
        .dereference(descriptor.as_dict().ok()?.get(b"FontFile2").ok()?)
        .ok()?;
    let file = file.as_stream().ok()?;
    if file.dict.has(b"Filter")
        || file.dict.has(b"DecodeParms")
        || file.content != FONT_BYTES
        || file.dict.get(b"Length1").ok().and_then(number) != Some(FONT_BYTES.len() as f64)
    {
        return None;
    }
    let mut widths = HashMap::new();
    let entries = cid.get(b"W").ok()?.as_array().ok()?;
    if entries.len() > 4096 || entries.len() % 2 != 0 {
        return None;
    }
    for pair in entries.chunks_exact(2) {
        let gid = u16::try_from(pair[0].as_i64().ok()?).ok()?;
        let [width] = pair[1].as_array().ok()?.as_slice() else {
            return None;
        };
        let width = number(width)?;
        if widths.insert(gid, width).is_some() || !(0.0..=10_000.0).contains(&width) {
            return None;
        }
    }
    Some(widths)
}

pub(crate) struct StagedImportedText {
    pub tiles: Vec<(Vec<u8>, [u32; 2])>,
    pub clip: [u32; 4],
    pub page_size: [u32; 2],
}

struct ImportedGlyph {
    opm: Vec<u8>,
    shift: [f64; 2],
    bounds: [f64; 4],
    gid: u16,
    color: [u8; 3],
}

fn encode_tile(glyphs: &[ImportedGlyph], origin: [u32; 2]) -> Option<Vec<u8>> {
    if glyphs.is_empty() || glyphs.len() > 32 {
        return None;
    }
    let mut records = Vec::new();
    for glyph in glyphs {
        let mut opm = glyph.opm.clone();
        text_quad_glyph::rebase_screen_f32(&mut opm, glyph.shift, origin).ok()?;
        let mut font_name = [0_u8; 32];
        font_name[..5].copy_from_slice(b"OPDS0");
        records.extend_from_slice(&font_name);
        records.extend_from_slice(&glyph.gid.to_le_bytes());
        records.extend_from_slice(&glyph.gid.to_le_bytes());
        records.extend_from_slice(&glyph.color);
        records.push(0);
        records.extend_from_slice(&u32::try_from(opm.len()).ok()?.to_le_bytes());
        records.extend_from_slice(&opm);
        if records.len() + 20 > 140_000 {
            return None;
        }
    }
    let mut scene = Vec::with_capacity(20 + records.len());
    scene.extend_from_slice(b"OPGS");
    for word in [1_u32, 256, 192, glyphs.len() as u32] {
        scene.extend_from_slice(&word.to_le_bytes());
    }
    scene.extend_from_slice(&records);
    Some(scene)
}

/// Stage one selected annotation using the caller's parsed PDF and validated
/// page geometry. Every glyph must fit one of at most sixteen 256×192 tiles;
/// all tiles are validated before the caller paints any of them.
pub(crate) fn stage_saved_native_freetext(
    doc: &Document,
    annotation: &Dictionary,
    crop: [f64; 4],
    rotation: u16,
) -> Option<StagedImportedText> {
    let [raw_w, raw_h] = [crop[2] - crop[0], crop[3] - crop[1]];
    let [visible_w, visible_h] = if matches!(rotation, 90 | 270) {
        [raw_h, raw_w]
    } else {
        [raw_w, raw_h]
    };
    let page_size = [visible_w as u32, visible_h as u32];
    if !name(annotation, b"Subtype", b"FreeText")
        || annotation.get(b"F").ok().and_then(number) != Some(4.0)
        || annotation.iter().any(|(key, _)| {
            !matches!(
                key.as_slice(),
                b"Type"
                    | b"Subtype"
                    | b"Rect"
                    | b"F"
                    | b"NM"
                    | b"Contents"
                    | b"C"
                    | b"Q"
                    | b"DA"
                    | b"AP"
            )
        })
        || !annotation
            .get(b"NM")
            .ok()?
            .as_str()
            .ok()?
            .starts_with(b"opds-rust-")
    {
        return None;
    }
    let [x0, y0, x1, y1] = quad(annotation.get(b"Rect").ok()?)?;
    if x0 < crop[0]
        || y0 < crop[1]
        || x1 > crop[2]
        || y1 > crop[3]
        || x1 <= x0
        || y1 <= y0
        || [x0, y0, x1, y1].iter().any(|v| v.fract() != 0.0)
    {
        return None;
    }
    let p0 = display_point(crop, rotation, x0, y0);
    let p1 = display_point(crop, rotation, x1, y1);
    let clip = [
        p0[0].min(p1[0]) as u32,
        p0[1].min(p1[1]) as u32,
        p0[0].max(p1[0]) as u32,
        p0[1].max(p1[1]) as u32,
    ];
    let appearance_size = [f64::from(clip[2] - clip[0]), f64::from(clip[3] - clip[1])];
    if appearance_size[0] > 4096.0 || appearance_size[1] > 4096.0 {
        return None;
    }
    let ap = annotation.get(b"AP").ok()?.as_dict().ok()?;
    if ap.len() != 1 {
        return None;
    }
    let (_, form) = doc.dereference(ap.get(b"N").ok()?).ok()?;
    let form = form.as_stream().ok()?;
    if !name(&form.dict, b"Type", b"XObject")
        || !name(&form.dict, b"Subtype", b"Form")
        || form.dict.iter().any(|(key, _)| {
            !matches!(
                key.as_slice(),
                b"Type" | b"Subtype" | b"BBox" | b"Resources" | b"Length"
            )
        })
        || form.dict.has(b"Filter")
        || form.dict.has(b"Matrix")
        || form.dict.has(b"Group")
        || form.content.len() > 20_000
        || quad(form.dict.get(b"BBox").ok()?)? != [0.0, 0.0, x1 - x0, y1 - y0]
    {
        return None;
    }
    if form.content.iter().any(|byte| {
        !byte.is_ascii_alphanumeric()
            && !byte.is_ascii_whitespace()
            && !matches!(byte, b'/' | b'<' | b'>' | b'-' | b'+' | b'.')
    }) || form
        .content
        .iter()
        .rev()
        .find(|byte| !byte.is_ascii_whitespace())
        != Some(&b'Q')
    {
        return None;
    }
    let widths = embedded_native_font(&doc, form.dict.get(b"Resources").ok()?.as_dict().ok()?)?;
    let operations = Content::decode(&form.content).ok()?.operations;
    let inner = if rotation == 0 {
        operations.as_slice()
    } else {
        if operations.len() < 14
            || operations[0].operator != "q"
            || operations[1].operator != "cm"
            || operations.last()?.operator != "Q"
            || !operations[0].operands.is_empty()
            || !operations.last()?.operands.is_empty()
        {
            return None;
        }
        let expected = match rotation {
            90 => [0.0, 1.0, -1.0, 0.0, appearance_size[1], 0.0],
            180 => [-1.0, 0.0, 0.0, -1.0, appearance_size[0], appearance_size[1]],
            270 => [0.0, -1.0, 1.0, 0.0, 0.0, appearance_size[0]],
            _ => return None,
        };
        if operations[1]
            .operands
            .iter()
            .map(number)
            .collect::<Option<Vec<_>>>()?
            != expected
        {
            return None;
        }
        &operations[2..operations.len() - 1]
    };
    if inner.len() < 11 || inner.len() > 267 {
        return None;
    }
    let head = &inner[..7];
    if head
        .iter()
        .map(|op| op.operator.as_str())
        .collect::<Vec<_>>()
        != ["q", "re", "W", "n", "BT", "Tf", "rg"]
        || !head[0].operands.is_empty()
        || !head[2].operands.is_empty()
        || !head[3].operands.is_empty()
        || !head[4].operands.is_empty()
        || head[1]
            .operands
            .iter()
            .map(number)
            .collect::<Option<Vec<_>>>()?
            != [0.0, 0.0, appearance_size[0], appearance_size[1]]
        || head[5].operands.len() != 2
        || head[5].operands[0].as_name().ok()? != b"OPDS0"
        || head[6].operands.len() != 3
    {
        return None;
    }
    let size = number(&head[5].operands[1])?;
    if !(8.0..=72.0).contains(&size) || size.fract() != 0.0 {
        return None;
    }
    let color = head[6]
        .operands
        .iter()
        .map(color)
        .collect::<Option<Vec<_>>>()?;
    if inner[inner.len() - 2].operator != "ET"
        || inner.last()?.operator != "Q"
        || !inner[inner.len() - 2].operands.is_empty()
        || !inner.last()?.operands.is_empty()
    {
        return None;
    }
    let text = &inner[7..inner.len() - 2];
    if text.len() % 2 != 0 {
        return None;
    }
    let mut glyphs = Vec::new();
    for pair in text.chunks_exact(2) {
        if pair[0].operator != "Tm"
            || pair[1].operator != "Tj"
            || pair[0].operands.len() != 6
            || pair[1].operands.len() != 1
        {
            return None;
        }
        let matrix = pair[0]
            .operands
            .iter()
            .map(number)
            .collect::<Option<Vec<_>>>()?;
        if matrix[..4] != [1.0, 0.0, 0.0, 1.0] {
            return None;
        }
        let encoded = pair[1].operands[0].as_str().ok()?;
        if encoded.len() % 2 != 0 || encoded.len() > 512 {
            return None;
        }
        let mut advance = 0.0;
        for code in encoded.chunks_exact(2) {
            let gid = u16::from_be_bytes([code[0], code[1]]);
            let screen_x = f64::from(clip[0]) + matrix[4] + advance;
            let screen_y = f64::from(clip[3]) - matrix[5];
            if gid == 0 {
                return None;
            }
            // Liberation Sans's GID 3 is the blank space; it advances the
            // text cursor but contributes no shape to OPGS.
            if gid != 3 {
                let transform = [size / 2048.0, 0.0, 0.0, -size / 2048.0, screen_x, screen_y];
                let (opm, shift) =
                    text_quad_glyph::stage(FONT_BYTES, gid, transform, Source::SkrifaAtThousand)
                        .map(|(opm, shift, _)| (opm, shift))
                        .or_else(|_| text_quad_glyph::stage_line_only(FONT_BYTES, gid, transform))
                        .ok()?;
                let bounds = text_quad_glyph::absolute_bounds(&opm, shift).ok()?;
                if bounds[0] < f64::from(clip[0])
                    || bounds[1] < f64::from(clip[1])
                    || bounds[2] > f64::from(clip[2])
                    || bounds[3] > f64::from(clip[3])
                {
                    return None;
                }
                glyphs.push(ImportedGlyph {
                    opm,
                    shift,
                    bounds: bounds.map(|value| f64::from(value as f32)),
                    gid,
                    color: [color[0], color[1], color[2]],
                });
                if glyphs.len() > 128 {
                    return None;
                }
            }
            advance += widths.get(&gid).copied().unwrap_or(1000.0) * size / 1000.0;
        }
    }
    if glyphs.is_empty() {
        return None;
    }
    let mut tiles = Vec::new();
    let mut start = 0;
    while start < glyphs.len() {
        let mut end = start;
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        while end < glyphs.len() && end - start < 32 {
            let glyph = &glyphs[end];
            let next = [
                bounds[0].min(glyph.bounds[0]),
                bounds[1].min(glyph.bounds[1]),
                bounds[2].max(glyph.bounds[2]),
                bounds[3].max(glyph.bounds[3]),
            ];
            let ox = next[0].floor();
            let oy = next[1].floor();
            if !ox.is_finite()
                || !oy.is_finite()
                || ox < 0.0
                || oy < 0.0
                || ox > f64::from(u32::MAX)
                || oy > f64::from(u32::MAX)
                || next[2] > ox + 256.0
                || next[3] > oy + 192.0
            {
                if end == start {
                    return None;
                }
                break;
            }
            bounds = next;
            end += 1;
        }
        if tiles.len() == 16 {
            return None;
        }
        let origin = [bounds[0].floor() as u32, bounds[1].floor() as u32];
        tiles.push((encode_tile(&glyphs[start..end], origin)?, origin));
        start = end;
    }
    Some(StagedImportedText {
        tiles,
        clip,
        page_size,
    })
}
