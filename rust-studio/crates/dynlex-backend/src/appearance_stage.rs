//! Read-only staging of one native FreeText /AP as ordinary page content.
//! This is a research bridge to the bounded Type0 glyph path, not a renderer.
use lopdf::{content::Content, Dictionary, Document, Object, Stream};
use std::collections::HashSet;

fn finite_number(value: &Object) -> Result<f64, String> {
    let number = match value {
        Object::Integer(value) => *value as f64,
        Object::Real(value) => f64::from(*value),
        _ => return Err("non-numeric annotation appearance box".into()),
    };
    (number.is_finite() && number.abs() <= 4096.0)
        .then_some(number)
        .ok_or_else(|| "annotation appearance coordinate outside limit".into())
}

fn box4(value: &Object) -> Result<[f64; 4], String> {
    let values = value
        .as_array()
        .map_err(|_| "annotation appearance box is not an array")?;
    if values.len() != 4 {
        return Err("annotation appearance box needs four coordinates".into());
    }
    Ok([
        finite_number(&values[0])?,
        finite_number(&values[1])?,
        finite_number(&values[2])?,
        finite_number(&values[3])?,
    ])
}

fn name_eq(dict: &Dictionary, key: &[u8], expected: &[u8]) -> bool {
    dict.get(key).ok().and_then(|value| value.as_name().ok()) == Some(expected)
}

/// Move one *native* `/FreeText` appearance into an isolated page-content PDF.
/// The input is unchanged. Only the exact, uncompressed OPDS Form shape is
/// accepted; this probe deliberately rejects existing page art and rotation.
pub fn flatten_native_free_text_appearance(
    source: &[u8],
    page_number: u32,
) -> Result<Vec<u8>, String> {
    if source.len() > 16_000_000 || page_number == 0 {
        return Err("appearance source outside limit".into());
    }
    let mut doc = Document::load_mem(source).map_err(|e| e.to_string())?;
    if doc.is_encrypted() {
        return Err("encrypted appearance source".into());
    }
    let page_id = *doc
        .get_pages()
        .get(&page_number)
        .ok_or("annotation page missing")?;
    let page = doc.get_dictionary(page_id).map_err(|e| e.to_string())?;
    let (_, original_content) = doc
        .dereference(page.get(b"Contents").map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let original_content = original_content.as_stream().map_err(|e| e.to_string())?;
    if original_content.dict.has(b"Filter") || !original_content.content.is_empty() {
        return Err("nonempty or filtered page content is outside appearance probe".into());
    }
    if page.has(b"Rotate") || page.has(b"CropBox") || page.has(b"UserUnit") {
        return Err("page transform is outside appearance probe".into());
    }
    let mut parent = page
        .get(b"Parent")
        .ok()
        .map(|value| value.as_reference())
        .transpose()
        .map_err(|e| e.to_string())?;
    let mut seen = HashSet::new();
    while let Some(parent_id) = parent {
        if !seen.insert(parent_id) || seen.len() > 32 {
            return Err("cyclic or deep page parent tree".into());
        }
        let node = doc.get_dictionary(parent_id).map_err(|e| e.to_string())?;
        if node.has(b"Rotate") || node.has(b"CropBox") || node.has(b"UserUnit") {
            return Err("inherited page transform is outside appearance probe".into());
        }
        parent = node
            .get(b"Parent")
            .ok()
            .map(|value| value.as_reference())
            .transpose()
            .map_err(|e| e.to_string())?;
    }
    let annotations = doc
        .get_page_annotations(page_id)
        .map_err(|e| e.to_string())?;
    if annotations.len() != 1 {
        return Err("appearance probe requires one annotation".into());
    }
    let annotation = annotations[0];
    if !name_eq(annotation, b"Subtype", b"FreeText") {
        return Err("annotation is not FreeText".into());
    }
    let name = annotation
        .get(b"NM")
        .map_err(|e| e.to_string())?
        .as_str()
        .map_err(|e| e.to_string())?;
    if !name.starts_with(b"opds-rust-") {
        return Err("appearance was not made by this app".into());
    }
    let [x0, y0, x1, y1] = box4(annotation.get(b"Rect").map_err(|e| e.to_string())?)?;
    if x0 < 0.0 || y0 < 0.0 || x1 <= x0 || y1 <= y0 {
        return Err("invalid annotation rectangle".into());
    }
    let ap = annotation
        .get(b"AP")
        .map_err(|e| e.to_string())?
        .as_dict()
        .map_err(|e| e.to_string())?;
    let normal = ap.get(b"N").map_err(|e| e.to_string())?;
    let (_, normal) = doc.dereference(normal).map_err(|e| e.to_string())?;
    let form = normal.as_stream().map_err(|e| e.to_string())?;
    if !name_eq(&form.dict, b"Type", b"XObject")
        || !name_eq(&form.dict, b"Subtype", b"Form")
        || form.dict.has(b"Filter")
        || form.dict.has(b"Matrix")
        || form.dict.has(b"Group")
        || form.content.len() > 1_000_000
    {
        return Err("unsupported FreeText Form appearance".into());
    }
    let [bx0, by0, bx1, by1] = box4(form.dict.get(b"BBox").map_err(|e| e.to_string())?)?;
    if bx0 != 0.0
        || by0 != 0.0
        || (bx1 - (x1 - x0)).abs() > 0.001
        || (by1 - (y1 - y0)).abs() > 0.001
    {
        return Err("FreeText Form BBox differs from annotation rectangle".into());
    }
    let resources = form
        .dict
        .get(b"Resources")
        .map_err(|e| e.to_string())?
        .as_dict()
        .map_err(|e| e.to_string())?
        .clone();
    if !resources.has(b"Font") || resources.len() != 1 {
        return Err("unsupported FreeText resources".into());
    }
    let fonts = resources
        .get(b"Font")
        .map_err(|e| e.to_string())?
        .as_dict()
        .map_err(|e| e.to_string())?;
    if fonts.len() != 1 {
        return Err("FreeText probe requires one font resource".into());
    }
    let (_, font) = doc
        .dereference(fonts.get(b"OPDS0").map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    let font = font.as_dict().map_err(|e| e.to_string())?;
    if !name_eq(font, b"Subtype", b"Type0")
        || font
            .get(b"Encoding")
            .ok()
            .and_then(|value| value.as_name().ok())
            != Some(&b"Identity-H"[..])
    {
        return Err("unsupported FreeText font".into());
    }
    let operations = Content::decode(&form.content).map_err(|e| e.to_string())?;
    let mut text_shows = 0_usize;
    for operation in operations.operations {
        if !matches!(
            operation.operator.as_str(),
            "q" | "Q" | "re" | "W" | "n" | "BT" | "ET" | "Tf" | "rg" | "Tm" | "Tj"
        ) {
            return Err("unsupported FreeText appearance operator".into());
        }
        if operation.operator == "Tj" {
            text_shows += 1;
            if text_shows > 256 {
                return Err("FreeText appearance has too many lines".into());
            }
        }
    }
    if text_shows == 0 {
        return Err("FreeText appearance has no text show".into());
    }
    let mut content = format!("q 1 0 0 1 {x0} {y0} cm\n").into_bytes();
    content.extend_from_slice(&form.content);
    content.extend_from_slice(b"\nQ\n");
    let stream_id = doc.add_object(Stream::new(Dictionary::new(), content));
    let page = doc
        .get_object_mut(page_id)
        .map_err(|e| e.to_string())?
        .as_dict_mut()
        .map_err(|e| e.to_string())?;
    page.set("Contents", Object::Reference(stream_id));
    page.set("Resources", Object::Dictionary(resources));
    page.remove(b"Annots");
    // The source may have been written by IncrementalDocument. A fresh full
    // save must not carry an old /Prev offset into its new xref trailer.
    doc.trailer.remove(b"Prev");
    doc.trailer.remove(b"XRefStm");
    let mut flattened = Vec::new();
    doc.save_to(&mut flattened).map_err(|e| e.to_string())?;
    Ok(flattened)
}
