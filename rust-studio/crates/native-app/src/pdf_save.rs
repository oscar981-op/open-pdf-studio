//! Pure-Rust, incremental PDF export for the native annotation overlay.
//! The original bytes remain intact; this first path deliberately accepts only
//! unrotated, origin-zero pages whose CropBox equals the MediaBox.
use lopdf::{Dictionary, Document, IncrementalDocument, Object, ObjectId, Stream, StringFormat};
use opds_native_core::annotations::{Annotation, Kind};
use std::io::Write;
use std::path::Path;

fn number(value: f64) -> Object {
    Object::Real(value as f32)
}

fn array(values: &[f64]) -> Object {
    Object::Array(values.iter().copied().map(number).collect())
}

fn inherited<'a>(
    doc: &'a Document,
    mut page_id: ObjectId,
    key: &[u8],
) -> Result<Option<&'a Object>, String> {
    for _ in 0..32 {
        let page = doc.get_dictionary(page_id).map_err(|e| e.to_string())?;
        if let Ok(value) = page.get(key) {
            return doc
                .dereference(value)
                .map(|(_, value)| Some(value))
                .map_err(|e| e.to_string());
        }
        match page.get(b"Parent").and_then(Object::as_reference) {
            Ok(parent) => page_id = parent,
            Err(_) => return Ok(None),
        }
    }
    Err("PDF page tree is too deep".into())
}

fn numeric(object: &Object) -> Result<f64, String> {
    match object {
        Object::Integer(value) => Ok(*value as f64),
        Object::Real(value) => Ok(f64::from(*value)),
        _ => Err("Invalid PDF page coordinate".into()),
    }
}

fn box_values(object: &Object) -> Result<[f64; 4], String> {
    let values = object.as_array().map_err(|e| e.to_string())?;
    if values.len() != 4 {
        return Err("Invalid PDF page box".into());
    }
    Ok([
        numeric(&values[0])?,
        numeric(&values[1])?,
        numeric(&values[2])?,
        numeric(&values[3])?,
    ])
}

fn page_size(doc: &Document, page_id: ObjectId) -> Result<[f64; 2], String> {
    let media =
        box_values(inherited(doc, page_id, b"MediaBox")?.ok_or("PDF page has no MediaBox")?)?;
    let crop = inherited(doc, page_id, b"CropBox")?
        .map(box_values)
        .transpose()?
        .unwrap_or(media);
    let rotate = inherited(doc, page_id, b"Rotate")?
        .map(numeric)
        .transpose()?
        .unwrap_or(0.0);
    let user_unit = inherited(doc, page_id, b"UserUnit")?
        .map(numeric)
        .transpose()?
        .unwrap_or(1.0);
    if media != crop || media[0] != 0.0 || media[1] != 0.0 || rotate != 0.0 || user_unit != 1.0 {
        return Err(
            "Save As supports only unrotated pages with an origin-zero MediaBox/CropBox".into(),
        );
    }
    let size = [media[2], media[3]];
    if size.iter().all(|n| n.is_finite() && *n > 0.0) {
        Ok(size)
    } else {
        Err("Invalid PDF page size".into())
    }
}

fn escape_literal(value: &str) -> Result<String, String> {
    if !value.bytes().all(|c| (32..=126).contains(&c)) {
        return Err("PDF Save As currently requires printable ASCII text in text boxes".into());
    }
    Ok(value
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)"))
}

fn annotation_object(
    annotation: &Annotation,
    page_height: f64,
) -> Result<(Dictionary, Stream), String> {
    let b = annotation.bounds;
    if ![b.x, b.y, b.width, b.height].iter().all(|n| n.is_finite())
        || b.width <= 0.0
        || b.height <= 0.0
        || b.x < 0.0
        || b.y < 0.0
        || b.x + b.width > 1.0e7
        || b.y + b.height > page_height + 0.1
    {
        return Err("Annotation is outside the PDF page".into());
    }
    let bottom = page_height - b.y - b.height;
    let rect = [b.x, bottom, b.x + b.width, bottom + b.height];
    let mut dictionary = Dictionary::new();
    dictionary.set("Type", Object::Name(b"Annot".to_vec()));
    dictionary.set("Rect", array(&rect));
    dictionary.set("F", 4_i64); // Print flag
    dictionary.set(
        "NM",
        Object::String(
            format!("opds-rust-{}", annotation.id).into_bytes(),
            StringFormat::Literal,
        ),
    );
    let mut appearance = Dictionary::new();
    appearance.set("Type", Object::Name(b"XObject".to_vec()));
    appearance.set("Subtype", Object::Name(b"Form".to_vec()));
    appearance.set("BBox", array(&[0.0, 0.0, b.width, b.height]));
    let content = match &annotation.kind {
        Kind::Rectangle { color_rgb } if *color_rgb == [255, 226, 67] => {
            dictionary.set("Subtype", Object::Name(b"Highlight".to_vec()));
            dictionary.set(
                "QuadPoints",
                array(&[b.x, rect[3], rect[2], rect[3], b.x, bottom, rect[2], bottom]),
            );
            dictionary.set("C", array(&[1.0, 226.0 / 255.0, 67.0 / 255.0]));
            let mut state = Dictionary::new();
            state.set("Type", Object::Name(b"ExtGState".to_vec()));
            state.set("ca", number(0.35));
            let mut states = Dictionary::new();
            states.set("GS0", state);
            let mut resources = Dictionary::new();
            resources.set("ExtGState", states);
            appearance.set("Resources", resources);
            format!(
                "q /GS0 gs 1 0.886 0.263 rg 0 0 {:.3} {:.3} re f Q",
                b.width, b.height
            )
            .into_bytes()
        }
        Kind::Rectangle { color_rgb } => {
            dictionary.set("Subtype", Object::Name(b"Square".to_vec()));
            let rgb = color_rgb.map(|v| f64::from(v) / 255.0);
            dictionary.set("C", array(&rgb));
            format!(
                "q {:.3} {:.3} {:.3} RG 2 w 1 1 {:.3} {:.3} re S Q",
                rgb[0],
                rgb[1],
                rgb[2],
                (b.width - 2.0).max(0.0),
                (b.height - 2.0).max(0.0)
            )
            .into_bytes()
        }
        Kind::Text { value } => {
            let escaped = escape_literal(value)?;
            dictionary.set("Subtype", Object::Name(b"FreeText".to_vec()));
            dictionary.set(
                "Contents",
                Object::String(value.as_bytes().to_vec(), StringFormat::Literal),
            );
            dictionary.set(
                "DA",
                Object::String(b"/Helv 14 Tf 0 0 0 rg".to_vec(), StringFormat::Literal),
            );
            let mut font = Dictionary::new();
            font.set("Type", Object::Name(b"Font".to_vec()));
            font.set("Subtype", Object::Name(b"Type1".to_vec()));
            font.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
            let mut fonts = Dictionary::new();
            fonts.set("Helv", font);
            let mut resources = Dictionary::new();
            resources.set("Font", fonts);
            appearance.set("Resources", resources);
            format!(
                "q BT /Helv 14 Tf 0 0 0 rg 0 {:.3} Td ({escaped}) Tj ET Q",
                (b.height - 14.0).max(0.0)
            )
            .into_bytes()
        }
    };
    Ok((dictionary, Stream::new(appearance, content)))
}

pub fn export_annotations(source: &[u8], annotations: &[Annotation]) -> Result<Vec<u8>, String> {
    let previous = Document::load_mem(source).map_err(|e| e.to_string())?;
    if previous.is_encrypted() {
        return Err("Encrypted PDFs are not supported for Save As".into());
    }
    if previous.objects.values().any(|o| {
        o.as_dict().is_ok_and(|d| {
            d.get(b"FT")
                .is_ok_and(|ft| ft.as_name().is_ok_and(|n| n == b"Sig"))
        })
    }) {
        return Err("Signed PDFs are not supported for Save As".into());
    }
    let pages = previous.get_pages();
    let mut output = IncrementalDocument::create_from(source.to_vec(), previous);
    for (index, page_id) in pages.values().copied().enumerate() {
        let items: Vec<_> = annotations.iter().filter(|a| a.page == index).collect();
        if items.is_empty() {
            continue;
        }
        let size = page_size(output.get_prev_documents(), page_id)?;
        output
            .opt_clone_object_to_new_document(page_id)
            .map_err(|e| e.to_string())?;
        let existing = output
            .get_prev_documents()
            .get_dictionary(page_id)
            .ok()
            .and_then(|d| d.get(b"Annots").ok())
            .cloned();
        let mut entries = match existing {
            Some(Object::Reference(id)) => output
                .get_prev_documents()
                .get_object(id)
                .and_then(Object::as_array)
                .map_err(|e| e.to_string())?
                .clone(),
            Some(Object::Array(values)) => values,
            Some(_) => return Err("Unsupported PDF annotation array".into()),
            None => Vec::new(),
        };
        for item in items {
            if item.bounds.x + item.bounds.width > size[0] + 0.1 {
                return Err("Annotation is outside the PDF page".into());
            }
            let (mut dictionary, stream) = annotation_object(item, size[1])?;
            let appearance_id = output.new_document.add_object(stream);
            let mut ap = Dictionary::new();
            ap.set("N", Object::Reference(appearance_id));
            dictionary.set("AP", ap);
            let annotation_id = output.new_document.add_object(dictionary);
            entries.push(Object::Reference(annotation_id));
        }
        output
            .new_document
            .get_dictionary_mut(page_id)
            .map_err(|e| e.to_string())?
            .set("Annots", Object::Array(entries));
    }
    if annotations.iter().any(|a| a.page >= pages.len()) {
        return Err("Annotation has an invalid PDF page".into());
    }
    let mut bytes = Vec::new();
    output.save_to(&mut bytes).map_err(|e| e.to_string())?;
    let check = Document::load_mem(&bytes).map_err(|e| format!("Saved PDF did not reopen: {e}"))?;
    if check.get_pages().len() != pages.len() {
        return Err("Saved PDF page count changed".into());
    }
    Ok(bytes)
}

pub fn save_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    if let Err(error) = file.write_all(bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(path);
        return Err(error.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::dictionary;
    use opds_native_core::annotations::Rect;

    fn source_pdf() -> Vec<u8> {
        let mut doc = Document::with_version("1.4");
        let pages_id = doc.new_object_id();
        let contents_id = doc.add_object(Stream::new(Dictionary::new(), Vec::new()));
        let existing_id = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Text",
            "Rect" => vec![1.into(), 1.into(), 10.into(), 10.into()],
            "Contents" => Object::string_literal("existing"),
        });
        let annots_id = doc.add_object(vec![Object::Reference(existing_id)]);
        let page_id = doc.add_object(
            dictionary! { "Type" => "Page", "Parent" => pages_id, "Contents" => contents_id, "Annots" => annots_id },
        );
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![Object::Reference(page_id)], "Count" => 1,
                "MediaBox" => vec![0.into(),0.into(),200.into(),300.into()],
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", Object::Reference(catalog_id));
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn incremental_export_preserves_source_and_printable_appearances() {
        let source = source_pdf();
        let items = vec![
            Annotation {
                id: 1,
                page: 0,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 40.0,
                    height: 30.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [231, 76, 60],
                },
            },
            Annotation {
                id: 2,
                page: 0,
                bounds: Rect {
                    x: 60.0,
                    y: 70.0,
                    width: 40.0,
                    height: 20.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [255, 226, 67],
                },
            },
            Annotation {
                id: 3,
                page: 0,
                bounds: Rect {
                    x: 10.0,
                    y: 110.0,
                    width: 100.0,
                    height: 30.0,
                },
                kind: Kind::Text {
                    value: "Rust text".into(),
                },
            },
        ];
        let output = export_annotations(&source, &items).unwrap();
        assert!(output.starts_with(&source));
        let doc = Document::load_mem(&output).unwrap();
        let page = *doc.get_pages().values().next().unwrap();
        let annots = doc.get_page_annotations(page).unwrap();
        assert_eq!(annots.len(), 4);
        assert_eq!(
            annots[0].get(b"Contents").unwrap().as_str().unwrap(),
            b"existing"
        );
        for annot in annots.into_iter().skip(1) {
            assert_eq!(annot.get(b"F").unwrap().as_i64().unwrap(), 4);
            let ap_id = annot
                .get(b"AP")
                .unwrap()
                .as_dict()
                .unwrap()
                .get(b"N")
                .unwrap()
                .as_reference()
                .unwrap();
            assert!(!doc
                .get_object(ap_id)
                .unwrap()
                .as_stream()
                .unwrap()
                .content
                .is_empty());
        }
        let rendered = opds_engine_probe::ParsedPdf::open(std::sync::Arc::new(output)).unwrap();
        assert_eq!(rendered.page_count(), 1);
    }

    #[test]
    fn unsupported_geometry_and_non_ascii_text_fail_before_write() {
        let source = source_pdf();
        let mut item = Annotation {
            id: 1,
            page: 0,
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 40.0,
                height: 30.0,
            },
            kind: Kind::Text { value: "é".into() },
        };
        assert!(export_annotations(&source, &[item.clone()])
            .unwrap_err()
            .contains("ASCII"));
        item.bounds.x = 190.0;
        item.kind = Kind::Rectangle {
            color_rgb: [0, 0, 0],
        };
        assert!(export_annotations(&source, &[item])
            .unwrap_err()
            .contains("outside"));
    }
}
