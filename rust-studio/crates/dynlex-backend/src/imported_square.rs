//! Experimental direct DynLex paint of one imported, native-shaped Square /AP.
//! Hayro still renders the page body and is an independent whole-page oracle.
//! This read-only probe is not the general imported annotation renderer.
use crate::{annotation_overlay::render_native_annotation_overlay, Bitmap};
use lopdf::{content::Content, Dictionary, Document, Object, ObjectId};
use opds_engine_probe::ParsedPdf;
use opds_native_core::annotations::{Annotation, Kind, Rect};
use std::sync::Arc;

fn number(value: &Object) -> Option<f64> {
    match value {
        Object::Integer(value) => Some(*value as f64),
        Object::Real(value) => Some(f64::from(*value)),
        _ => None,
    }
}

fn four(value: &Object) -> Option<[f64; 4]> {
    let values = value.as_array().ok()?;
    let [a, b, c, d] = values.as_slice() else {
        return None;
    };
    let result = [number(a)?, number(b)?, number(c)?, number(d)?];
    result.iter().all(|v| v.is_finite()).then_some(result)
}

fn name_is(dict: &Dictionary, key: &[u8], expected: &[u8]) -> bool {
    dict.get(key).ok().and_then(|v| v.as_name().ok()) == Some(expected)
}

fn inherited<'a>(
    doc: &'a Document,
    mut id: ObjectId,
    key: &[u8],
) -> Result<Option<&'a Object>, ()> {
    for _ in 0..32 {
        let dict = doc.get_dictionary(id).map_err(|_| ())?;
        if let Ok(value) = dict.get(key) {
            return doc
                .dereference(value)
                .map(|(_, value)| Some(value))
                .map_err(|_| ());
        }
        match dict.get(b"Parent") {
            Ok(parent) => id = parent.as_reference().map_err(|_| ())?,
            Err(_) => return Ok(None),
        }
    }
    Err(())
}

fn display_point(crop: [f64; 4], rotation: u16, x: f64, y: f64) -> [f64; 2] {
    match rotation {
        0 => [x - crop[0], crop[3] - y],
        90 => [y - crop[1], x - crop[0]],
        180 => [crop[2] - x, y - crop[1]],
        270 => [crop[3] - y, crop[2] - x],
        _ => unreachable!(),
    }
}

fn rgb_channel(value: f64, appearance: f64) -> Option<u8> {
    if !value.is_finite() || !(0.0..=1.0).contains(&value) || !appearance.is_finite() {
        return None;
    }
    let channel = (value * 255.0).round();
    if !(0.0..=255.0).contains(&channel) {
        return None;
    }
    let normalized = channel / 255.0;
    let printed = (normalized * 1000.0).round() / 1000.0;
    ((value - normalized).abs() <= 0.000_001 && (appearance - printed).abs() <= 0.000_001)
        .then_some(channel as u8)
}

/// Return a DynLex-composited page only for one exact, opaque 2pt Square Form
/// on a quarter-turned, integral page. Any uncertainty stays on Hayro.
pub fn render_single_square_appearance_direct(
    source: &[u8],
    page_number: usize,
) -> Result<Option<Bitmap>, String> {
    if source.len() > 2_000_000 || page_number == 0 {
        return Ok(None);
    }
    let mut doc = Document::load_mem(source).map_err(|e| e.to_string())?;
    if doc.is_encrypted() {
        return Ok(None);
    }
    let Ok(page_number_u32) = u32::try_from(page_number) else {
        return Ok(None);
    };
    let page_id = *doc
        .get_pages()
        .get(&page_number_u32)
        .ok_or("page missing")?;
    let page = doc.get_dictionary(page_id).map_err(|e| e.to_string())?;
    if page.has(b"Group") || !matches!(inherited(&doc, page_id, b"UserUnit"), Ok(None)) {
        return Ok(None);
    }
    let Ok(Some(media)) = inherited(&doc, page_id, b"MediaBox") else {
        return Ok(None);
    };
    let Some(media) = four(media) else {
        return Ok(None);
    };
    let crop = match inherited(&doc, page_id, b"CropBox") {
        Ok(Some(value)) => match four(value) {
            Some(box_) => box_,
            None => return Ok(None),
        },
        Ok(None) => media,
        Err(()) => return Ok(None),
    };
    let rotation = match inherited(&doc, page_id, b"Rotate") {
        Ok(Some(value)) => match number(value) {
            Some(value) if value.fract() == 0.0 && value.is_finite() => {
                value.rem_euclid(360.0) as u16
            }
            _ => return Ok(None),
        },
        Ok(None) => 0,
        Err(()) => return Ok(None),
    };
    if !matches!(rotation, 0 | 90 | 180 | 270) {
        return Ok(None);
    }
    let [x0, y0, x1, y1] = crop;
    let (width, height) = (x1 - x0, y1 - y0);
    if !media.iter().chain(crop.iter()).all(|v| v.is_finite())
        || media[0] > x0
        || media[1] > y0
        || x1 > media[2]
        || y1 > media[3]
        || width <= 0.0
        || height <= 0.0
        || width > 4096.0
        || height > 4096.0
        || crop.iter().any(|v| v.fract() != 0.0)
        || width * height > 6_000_000.0
    {
        return Ok(None);
    }
    let annotations = doc
        .get_page_annotations(page_id)
        .map_err(|e| e.to_string())?;
    let [annotation] = annotations.as_slice() else {
        return Ok(None);
    };
    if !name_is(annotation, b"Subtype", b"Square")
        || annotation.get(b"F").ok().and_then(number) != Some(4.0)
    {
        return Ok(None);
    }
    let Some(rect) = annotation.get(b"Rect").ok().and_then(four) else {
        return Ok(None);
    };
    if rect[0] < x0
        || rect[1] < y0
        || rect[2] > x1
        || rect[3] > y1
        || rect.iter().any(|v| v.fract() != 0.0)
        || rect[2] - rect[0] < 4.0
        || rect[3] - rect[1] < 4.0
    {
        return Ok(None);
    }
    let Some(color_values) = annotation.get(b"C").ok().and_then(|v| v.as_array().ok()) else {
        return Ok(None);
    };
    let [red, green, blue] = color_values.as_slice() else {
        return Ok(None);
    };
    let Ok(ap) = annotation.get(b"AP").and_then(Object::as_dict) else {
        return Ok(None);
    };
    let Ok(normal) = ap.get(b"N") else {
        return Ok(None);
    };
    let (_, normal) = doc.dereference(normal).map_err(|e| e.to_string())?;
    let Ok(form) = normal.as_stream() else {
        return Ok(None);
    };
    if !name_is(&form.dict, b"Type", b"XObject")
        || !name_is(&form.dict, b"Subtype", b"Form")
        || form.dict.has(b"Filter")
        || form.dict.has(b"Matrix")
        || form.dict.has(b"Group")
        || form.dict.has(b"Resources")
        || form.content.len() > 512
        || form.dict.get(b"BBox").ok().and_then(four)
            != Some([0.0, 0.0, rect[2] - rect[0], rect[3] - rect[1]])
    {
        return Ok(None);
    }
    let Ok(operations) = Content::decode(&form.content) else {
        return Ok(None);
    };
    let ops = operations.operations;
    let (ops, display_width, display_height) = if rotation == 0 {
        (ops.as_slice(), rect[2] - rect[0], rect[3] - rect[1])
    } else {
        if ops.len() != 9
            || ops[0].operator != "q"
            || ops[1].operator != "cm"
            || ops[8].operator != "Q"
            || !ops[0].operands.is_empty()
            || !ops[8].operands.is_empty()
            || ops[1].operands.len() != 6
        {
            return Ok(None);
        }
        let pdf_width = rect[2] - rect[0];
        let pdf_height = rect[3] - rect[1];
        let expected = match rotation {
            90 => [0.0, 1.0, -1.0, 0.0, pdf_width, 0.0],
            180 => [-1.0, 0.0, 0.0, -1.0, pdf_width, pdf_height],
            270 => [0.0, -1.0, 1.0, 0.0, 0.0, pdf_height],
            _ => unreachable!(),
        };
        if ops[1]
            .operands
            .iter()
            .map(number)
            .collect::<Option<Vec<_>>>()
            != Some(expected.to_vec())
        {
            return Ok(None);
        }
        let (dw, dh) = if matches!(rotation, 90 | 270) {
            (pdf_height, pdf_width)
        } else {
            (pdf_width, pdf_height)
        };
        (&ops[2..8], dw, dh)
    };
    if ops.len() != 6
        || ["q", "RG", "w", "re", "S", "Q"]
            .iter()
            .zip(ops)
            .any(|(expected, actual)| actual.operator != *expected)
        || !ops[0].operands.is_empty()
        || !ops[4].operands.is_empty()
        || !ops[5].operands.is_empty()
        || ops[1].operands.len() != 3
        || ops[2].operands.len() != 1
        || ops[3].operands.len() != 4
        || ops[2].operands.first().and_then(number) != Some(2.0)
    {
        return Ok(None);
    }
    let Some(path) = (|| {
        Some([
            number(&ops[3].operands[0])?,
            number(&ops[3].operands[1])?,
            number(&ops[3].operands[2])?,
            number(&ops[3].operands[3])?,
        ])
    })() else {
        return Ok(None);
    };
    if path != [1.0, 1.0, display_width - 2.0, display_height - 2.0] {
        return Ok(None);
    }
    let Some(color) = (|| {
        Some([
            rgb_channel(number(red)?, number(&ops[1].operands[0])?)?,
            rgb_channel(number(green)?, number(&ops[1].operands[1])?)?,
            rgb_channel(number(blue)?, number(&ops[1].operands[2])?)?,
        ])
    })() else {
        return Ok(None);
    };
    if color == [255, 226, 67] {
        // This native color represents Highlight with different transparency.
        return Ok(None);
    }
    let p0 = display_point(crop, rotation, rect[0], rect[1]);
    let p1 = display_point(crop, rotation, rect[2], rect[3]);
    let mark = Annotation {
        id: 1,
        page: page_number - 1,
        bounds: Rect {
            x: p0[0].min(p1[0]),
            y: p0[1].min(p1[1]),
            width: display_width,
            height: display_height,
        },
        kind: Kind::Rectangle { color_rgb: color },
    };
    doc.get_object_mut(page_id)
        .map_err(|e| e.to_string())?
        .as_dict_mut()
        .map_err(|e| e.to_string())?
        .remove(b"Annots");
    doc.trailer.remove(b"Prev");
    doc.trailer.remove(b"XRefStm");
    let mut bare_pdf = Vec::new();
    doc.save_to(&mut bare_pdf).map_err(|e| e.to_string())?;
    let bare = ParsedPdf::open(Arc::new(bare_pdf)).map_err(|e| e.to_string())?;
    let original = ParsedPdf::open(Arc::new(source.to_vec())).map_err(|e| e.to_string())?;
    let body = bare
        .render_page(page_number, 1.0)
        .map_err(|e| e.to_string())?;
    let expected = original
        .render_page(page_number, 1.0)
        .map_err(|e| e.to_string())?;
    let Some(actual) = render_native_annotation_overlay(&body, &[mark], page_number - 1)? else {
        return Ok(None);
    };
    Ok((actual.width == expected.width
        && actual.height == expected.height
        && actual.rgba == expected.rgba)
        .then_some(actual))
}
