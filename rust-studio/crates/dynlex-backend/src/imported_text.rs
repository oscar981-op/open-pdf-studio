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
    pub scene: Vec<u8>,
    pub clip: [u32; 4],
    pub origin: [u32; 2],
    pub page_size: [u32; 2],
}

/// Stage one selected annotation using the caller's parsed PDF and validated
/// page geometry. All glyphs must fit a single 256×192 tile; no partial paint.
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
    if appearance_size[0] > 256.0 || appearance_size[1] > 192.0 {
        return None;
    }
    let origin = [clip[0], clip[1]];
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
        || form.content.len() > 4096
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
    if inner.len() < 11 || inner.len() > 73 {
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
    let mut records = Vec::new();
    let mut painted = 0;
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
        if encoded.len() % 2 != 0 || encoded.len() > 128 {
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
                let (mut opm, shift) =
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
                text_quad_glyph::rebase_screen_f32(&mut opm, shift, origin).ok()?;
                let mut font_name = [0_u8; 32];
                font_name[..5].copy_from_slice(b"OPDS0");
                records.extend_from_slice(&font_name);
                records.extend_from_slice(&gid.to_le_bytes());
                records.extend_from_slice(&gid.to_le_bytes());
                records.extend_from_slice(&color);
                records.push(0);
                records.extend_from_slice(&u32::try_from(opm.len()).ok()?.to_le_bytes());
                records.extend_from_slice(&opm);
                painted += 1;
                if painted > 32 || records.len() > 140_000 {
                    return None;
                }
            }
            advance += widths.get(&gid).copied().unwrap_or(1000.0) * size / 1000.0;
        }
    }
    if painted == 0 {
        return None;
    }
    let mut scene = Vec::with_capacity(20 + records.len());
    scene.extend_from_slice(b"OPGS");
    for word in [1_u32, 256, 192, painted] {
        scene.extend_from_slice(&word.to_le_bytes());
    }
    scene.extend_from_slice(&records);
    Some(StagedImportedText {
        scene,
        clip,
        origin,
        page_size,
    })
}
