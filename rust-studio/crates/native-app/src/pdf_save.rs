//! Pure-Rust, incremental PDF export for the native annotation overlay.
//! The original bytes remain intact; this first path deliberately accepts only
//! unrotated, origin-zero pages whose CropBox equals the MediaBox.
use lopdf::{Dictionary, Document, IncrementalDocument, Object, ObjectId, Stream, StringFormat};
use opds_native_core::annotations::{Annotation, Kind};
use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::path::Path;
use ttf_parser::{Face, GlyphId};

const FONT_BYTES: &[u8] = include_bytes!("../../../assets/fonts/LiberationSans-Regular.ttf");
const FONT_NAME: &[u8] = b"LiberationSans";

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

fn font_face() -> Result<Face<'static>, String> {
    Face::parse(FONT_BYTES, 0).map_err(|e| format!("Embedded font cannot be read: {e:?}"))
}

fn glyph_id(face: &Face<'_>, ch: char) -> Result<GlyphId, String> {
    let code = ch as u32;
    let simple_script = (0x20..=0x024F).contains(&code)
        || (0x0370..=0x052F).contains(&code)
        || (0x2000..=0x206F).contains(&code)
        || code == 0x20AC;
    if !simple_script || ch.is_control() {
        return Err(format!(
            "Text script U+{code:04X} is not supported by PDF Save As"
        ));
    }
    face.glyph_index(ch)
        .ok_or_else(|| format!("Embedded font has no glyph for U+{code:04X}"))
}

fn glyphs(value: &str) -> Result<String, String> {
    let face = font_face()?;
    let mut hex = String::new();
    for ch in value.chars() {
        let id = glyph_id(&face, ch)?;
        write!(hex, "{:04X}", id.0).map_err(|e| e.to_string())?;
    }
    Ok(hex)
}

fn unicode_string(value: &str) -> Object {
    let mut bytes = vec![0xFE, 0xFF];
    for unit in value.encode_utf16() {
        bytes.extend_from_slice(&unit.to_be_bytes());
    }
    Object::String(bytes, StringFormat::Hexadecimal)
}

fn embed_font(doc: &mut Document, annotations: &[Annotation]) -> Result<ObjectId, String> {
    let face = font_face()?;
    let mut mapping = BTreeMap::<u16, char>::new();
    for annotation in annotations {
        if let Kind::Text { value } = &annotation.kind {
            for ch in value.chars() {
                let gid = glyph_id(&face, ch)?;
                if let Some(previous) = mapping.insert(gid.0, ch) {
                    if previous != ch {
                        return Err("Two Unicode characters share one PDF glyph; cannot preserve text extraction".into());
                    }
                }
            }
        }
    }
    let units = f64::from(face.units_per_em());
    let scale = |value: i16| f64::from(value) * 1000.0 / units;
    let bbox = face.global_bounding_box();
    let mut font_file_dict = Dictionary::new();
    font_file_dict.set("Length1", FONT_BYTES.len() as i64);
    let font_file = doc.add_object(Stream::new(font_file_dict, FONT_BYTES.to_vec()));
    let mut descriptor = Dictionary::new();
    descriptor.set("Type", Object::Name(b"FontDescriptor".to_vec()));
    descriptor.set("FontName", Object::Name(FONT_NAME.to_vec()));
    descriptor.set("Flags", 32_i64);
    descriptor.set(
        "FontBBox",
        array(&[
            scale(bbox.x_min),
            scale(bbox.y_min),
            scale(bbox.x_max),
            scale(bbox.y_max),
        ]),
    );
    descriptor.set("ItalicAngle", 0_i64);
    descriptor.set("Ascent", number(scale(face.ascender())));
    descriptor.set("Descent", number(scale(face.descender())));
    descriptor.set(
        "CapHeight",
        number(scale(face.capital_height().unwrap_or(face.ascender()))),
    );
    descriptor.set("StemV", 80_i64);
    descriptor.set("FontFile2", Object::Reference(font_file));
    let descriptor_id = doc.add_object(descriptor);
    let mut widths = Vec::new();
    for gid in mapping.keys() {
        let advance = face
            .glyph_hor_advance(ttf_parser::GlyphId(*gid))
            .ok_or("Missing glyph width")?;
        widths.push(Object::Integer(i64::from(*gid)));
        widths.push(Object::Array(vec![number(
            f64::from(advance) * 1000.0 / units,
        )]));
    }
    let mut system = Dictionary::new();
    system.set("Registry", Object::string_literal("Adobe"));
    system.set("Ordering", Object::string_literal("Identity"));
    system.set("Supplement", 0_i64);
    let mut cid = Dictionary::new();
    cid.set("Type", Object::Name(b"Font".to_vec()));
    cid.set("Subtype", Object::Name(b"CIDFontType2".to_vec()));
    cid.set("BaseFont", Object::Name(FONT_NAME.to_vec()));
    cid.set("CIDSystemInfo", system);
    cid.set("FontDescriptor", Object::Reference(descriptor_id));
    cid.set("CIDToGIDMap", Object::Name(b"Identity".to_vec()));
    cid.set("DW", 1000_i64);
    cid.set("W", Object::Array(widths));
    let cid_id = doc.add_object(cid);
    let mut cmap = String::from("/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n/CMapName /OPDSToUnicode def\n/CMapType 2 def\n1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n");
    for chunk in mapping.iter().collect::<Vec<_>>().chunks(100) {
        writeln!(cmap, "{} beginbfchar", chunk.len()).map_err(|e| e.to_string())?;
        for (gid, ch) in chunk {
            let utf16 = ch
                .encode_utf16(&mut [0; 2])
                .iter()
                .map(|unit| format!("{unit:04X}"))
                .collect::<String>();
            writeln!(cmap, "<{gid:04X}> <{utf16}>").map_err(|e| e.to_string())?;
        }
        cmap.push_str("endbfchar\n");
    }
    cmap.push_str("endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n");
    let unicode_id = doc.add_object(Stream::new(Dictionary::new(), cmap.into_bytes()));
    let mut font = Dictionary::new();
    font.set("Type", Object::Name(b"Font".to_vec()));
    font.set("Subtype", Object::Name(b"Type0".to_vec()));
    font.set("BaseFont", Object::Name(FONT_NAME.to_vec()));
    font.set("Encoding", Object::Name(b"Identity-H".to_vec()));
    font.set(
        "DescendantFonts",
        Object::Array(vec![Object::Reference(cid_id)]),
    );
    font.set("ToUnicode", Object::Reference(unicode_id));
    Ok(doc.add_object(font))
}

fn annotation_object(
    annotation: &Annotation,
    page_height: f64,
    text_font: Option<ObjectId>,
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
            let encoded = glyphs(value)?;
            let font_id = text_font.ok_or("Missing embedded text font")?;
            dictionary.set("Subtype", Object::Name(b"FreeText".to_vec()));
            dictionary.set("Contents", unicode_string(value));
            dictionary.set(
                "DA",
                Object::String(b"/OPDS0 14 Tf 0 0 0 rg".to_vec(), StringFormat::Literal),
            );
            let mut fonts = Dictionary::new();
            fonts.set("OPDS0", Object::Reference(font_id));
            let mut resources = Dictionary::new();
            resources.set("Font", fonts);
            appearance.set("Resources", resources);
            format!(
                "q BT /OPDS0 14 Tf 0 0 0 rg 0 {:.3} Td <{encoded}> Tj ET Q",
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
    let text_font = if annotations
        .iter()
        .any(|a| matches!(a.kind, Kind::Text { .. }))
    {
        Some(embed_font(&mut output.new_document, annotations)?)
    } else {
        None
    };
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
            let (mut dictionary, stream) = annotation_object(item, size[1], text_font)?;
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

/// Produce a printable PDF containing the selected original pages, in their
/// original order. The annotation appearances are added before page removal.
pub fn export_print_pages(
    source: &[u8],
    annotations: &[Annotation],
    selected: &[usize],
) -> Result<Vec<u8>, String> {
    let annotated = export_annotations(source, annotations)?;
    let mut document = Document::load_mem(&annotated).map_err(|e| e.to_string())?;
    let count = document.get_pages().len();
    if selected.is_empty()
        || selected.iter().any(|&page| page >= count)
        || selected.windows(2).any(|pair| pair[0] >= pair[1])
    {
        return Err("Choose at least one valid page in ascending order".into());
    }
    if selected.len() == count {
        return Ok(annotated);
    }
    let omitted: Vec<u32> = (0..count)
        .filter(|page| selected.binary_search(page).is_err())
        .map(|page| page as u32 + 1)
        .collect();
    document.delete_pages(&omitted);
    document.prune_objects();
    // A full rewrite must not retain offsets into the preceding incremental
    // revision: they no longer exist in the new file.
    document.trailer.remove(b"Prev");
    document.trailer.remove(b"XRefStm");
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).map_err(|e| e.to_string())?;
    let check = Document::load_mem(&bytes).map_err(|e| format!("Print PDF did not reopen: {e}"))?;
    if check.get_pages().len() != selected.len() {
        return Err("Print PDF page count differs from selection".into());
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
                    value: "Café Ω Ж".into(),
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
    fn unsupported_geometry_and_missing_glyph_fail_before_write() {
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
            kind: Kind::Text {
                value: "漢".into()
            },
        };
        assert!(export_annotations(&source, &[item.clone()])
            .unwrap_err()
            .contains("not supported"));
        item.bounds.x = 190.0;
        item.kind = Kind::Rectangle {
            color_rgb: [0, 0, 0],
        };
        assert!(export_annotations(&source, &[item])
            .unwrap_err()
            .contains("outside"));
    }

    #[test]
    fn print_pages_rejects_invalid_selection() {
        let source = source_pdf();
        assert!(export_print_pages(&source, &[], &[]).is_err());
        assert!(export_print_pages(&source, &[], &[1]).is_err());
        let all = export_print_pages(&source, &[], &[0]).unwrap();
        assert!(all.starts_with(&source));
    }

    #[test]
    fn print_pages_keeps_only_selected_original_pages_and_annotations() {
        let mut doc = Document::load_mem(&source_pdf()).unwrap();
        let root = doc.trailer.get(b"Root").unwrap().as_reference().unwrap();
        let pages_id = doc
            .get_dictionary(root)
            .unwrap()
            .get(b"Pages")
            .unwrap()
            .as_reference()
            .unwrap();
        let first = *doc.get_pages().values().next().unwrap();
        for _ in 0..2 {
            let page = doc.get_object(first).unwrap().clone();
            let id = doc.add_object(page);
            doc.get_dictionary_mut(pages_id)
                .unwrap()
                .get_mut(b"Kids")
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(Object::Reference(id));
        }
        doc.get_dictionary_mut(pages_id)
            .unwrap()
            .set("Count", 3_i64);
        let mut source = Vec::new();
        doc.save_to(&mut source).unwrap();
        let item = Annotation {
            id: 9,
            page: 1,
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 40.0,
                height: 30.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [255, 226, 67],
            },
        };
        let bytes = export_print_pages(&source, &[item], &[1]).unwrap();
        let output = Document::load_mem(&bytes).unwrap();
        assert_eq!(output.get_pages().len(), 1);
        let page = *output.get_pages().values().next().unwrap();
        let annots = output.get_page_annotations(page).unwrap();
        assert_eq!(annots.len(), 2);
        assert_eq!(
            annots[1].get(b"Subtype").unwrap().as_name().unwrap(),
            b"Highlight"
        );
        let parsed = opds_engine_probe::ParsedPdf::open(std::sync::Arc::new(bytes)).unwrap();
        assert_eq!(parsed.page_count(), 1);
    }
}
