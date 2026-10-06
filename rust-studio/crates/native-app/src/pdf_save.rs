//! Pure-Rust, incremental PDF export for the native annotation overlay.
//! The original bytes remain intact. New annotation appearances are mapped
//! through the visible CropBox and quarter-turn page rotation.
use super::text_layout;
use lopdf::{Dictionary, Document, IncrementalDocument, Object, ObjectId, Stream, StringFormat};
use opds_native_core::annotations::{Annotation, Kind, TextAlign, TEXT_FONT_SIZES};
use std::collections::BTreeMap;
use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::path::Path;
use ttf_parser::{Face, GlyphId};

const FONT_BYTES: &[u8] = text_layout::FONT_BYTES;
const FONT_NAME: &[u8] = b"LiberationSans";

fn number(value: f64) -> Object {
    Object::Real(value as f32)
}

fn array(values: &[f64]) -> Object {
    Object::Array(values.iter().copied().map(number).collect())
}

fn transform_pairs(
    values: &[Object],
    crop: [f64; 4],
    scale: f64,
    tx: f64,
    ty: f64,
) -> Result<Vec<Object>, String> {
    if values.len() % 2 != 0 {
        return Err("Invalid PDF annotation point array".into());
    }
    values
        .iter()
        .enumerate()
        .map(|(index, value)| {
            let (origin, offset) = if index % 2 == 0 {
                (crop[0], tx)
            } else {
                (crop[1], ty)
            };
            Ok(number((numeric(value)? - origin) * scale + offset))
        })
        .collect()
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
    let (crop, rotation) = print_page_geometry(doc, page_id)?;
    let size = [crop[2] - crop[0], crop[3] - crop[1]];
    if matches!(rotation, 90 | 270) {
        Ok([size[1], size[0]])
    } else {
        Ok(size)
    }
}

fn display_to_pdf(crop: [f64; 4], rotation: u16, u: f64, v: f64) -> [f64; 2] {
    match rotation {
        0 => [crop[0] + u, crop[3] - v],
        90 => [crop[0] + v, crop[1] + u],
        180 => [crop[2] - u, crop[1] + v],
        270 => [crop[2] - v, crop[3] - u],
        _ => unreachable!("validated quarter-turn rotation"),
    }
}

fn appearance_matrix(rotation: u16, width: f64, height: f64) -> Option<String> {
    match rotation {
        0 => None,
        90 => Some(format!("0 1 -1 0 {height:.8} 0")),
        180 => Some(format!("-1 0 0 -1 {width:.8} {height:.8}")),
        270 => Some(format!("0 -1 1 0 0 {width:.8}")),
        _ => unreachable!("validated quarter-turn rotation"),
    }
}

/// Visible page region in default user space, before /Rotate is applied.
pub(super) fn print_page_geometry(
    doc: &Document,
    page_id: ObjectId,
) -> Result<([f64; 4], u16), String> {
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
    let unit = inherited(doc, page_id, b"UserUnit")?
        .map(numeric)
        .transpose()?
        .unwrap_or(1.0);
    if !media.iter().chain(crop.iter()).all(|n| n.is_finite())
        || crop[0] >= crop[2]
        || crop[1] >= crop[3]
        || crop[0] < media[0]
        || crop[1] < media[1]
        || crop[2] > media[2]
        || crop[3] > media[3]
        || unit != 1.0
        || !rotate.is_finite()
        || rotate.fract() != 0.0
    {
        return Err("Unsupported PDF page geometry".into());
    }
    let turn = rotate.rem_euclid(360.0) as u16;
    if !matches!(turn, 0 | 90 | 180 | 270) {
        return Err("Only quarter-turn PDF rotation is supported".into());
    }
    Ok((crop, turn))
}

fn font_face() -> Result<&'static Face<'static>, String> {
    text_layout::font_face().ok_or_else(|| "Embedded font cannot be read".into())
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

fn encoded_line(face: &Face<'_>, value: &str, size: f64) -> Result<(String, f64), String> {
    let mut hex = String::new();
    let mut advance = 0_u64;
    for ch in value.chars() {
        let id = glyph_id(face, ch)?;
        write!(hex, "{:04X}", id.0).map_err(|e| e.to_string())?;
        advance += u64::from(face.glyph_hor_advance(id).ok_or("Missing glyph width")?);
    }
    Ok((hex, advance as f64 * size / f64::from(face.units_per_em())))
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
        if let Kind::Text { value, .. } = &annotation.kind {
            for ch in value.chars() {
                if ch == '\n' {
                    continue;
                }
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
    crop: [f64; 4],
    rotation: u16,
    text_font: Option<ObjectId>,
) -> Result<(Dictionary, Stream), String> {
    let b = annotation.bounds;
    let raw_size = [crop[2] - crop[0], crop[3] - crop[1]];
    let display_size = if matches!(rotation, 90 | 270) {
        [raw_size[1], raw_size[0]]
    } else {
        raw_size
    };
    if ![b.x, b.y, b.width, b.height].iter().all(|n| n.is_finite())
        || b.width <= 0.0
        || b.height <= 0.0
        || b.x < 0.0
        || b.y < 0.0
        || b.x + b.width > display_size[0] + 0.1
        || b.y + b.height > display_size[1] + 0.1
    {
        return Err("Annotation is outside the PDF page".into());
    }
    let corners = [
        display_to_pdf(crop, rotation, b.x, b.y),
        display_to_pdf(crop, rotation, b.x + b.width, b.y),
        display_to_pdf(crop, rotation, b.x, b.y + b.height),
        display_to_pdf(crop, rotation, b.x + b.width, b.y + b.height),
    ];
    let rect = [
        corners
            .iter()
            .map(|point| point[0])
            .fold(f64::INFINITY, f64::min),
        corners
            .iter()
            .map(|point| point[1])
            .fold(f64::INFINITY, f64::min),
        corners
            .iter()
            .map(|point| point[0])
            .fold(f64::NEG_INFINITY, f64::max),
        corners
            .iter()
            .map(|point| point[1])
            .fold(f64::NEG_INFINITY, f64::max),
    ];
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
    appearance.set(
        "BBox",
        array(&[0.0, 0.0, rect[2] - rect[0], rect[3] - rect[1]]),
    );
    let content = match &annotation.kind {
        Kind::Rectangle { color_rgb } if *color_rgb == [255, 226, 67] => {
            dictionary.set("Subtype", Object::Name(b"Highlight".to_vec()));
            dictionary.set(
                "QuadPoints",
                array(&[
                    corners[0][0],
                    corners[0][1],
                    corners[1][0],
                    corners[1][1],
                    corners[2][0],
                    corners[2][1],
                    corners[3][0],
                    corners[3][1],
                ]),
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
        Kind::Text {
            value,
            color_rgb,
            font_size,
            align,
        } => {
            if !TEXT_FONT_SIZES.contains(font_size) {
                return Err(format!("Unsupported text size: {font_size} pt"));
            }
            let font_id = text_font.ok_or("Missing embedded text font")?;
            let size = f64::from(*font_size);
            let rgb = color_rgb.map(|v| f64::from(v) / 255.0);
            let color = format!("{:.3} {:.3} {:.3} rg", rgb[0], rgb[1], rgb[2]);
            dictionary.set("Subtype", Object::Name(b"FreeText".to_vec()));
            dictionary.set("Contents", unicode_string(value));
            dictionary.set("C", array(&rgb));
            dictionary.set(
                "Q",
                match align {
                    TextAlign::Left => 0_i64,
                    TextAlign::Center => 1_i64,
                    TextAlign::Right => 2_i64,
                },
            );
            dictionary.set(
                "DA",
                Object::String(
                    format!("/OPDS0 {font_size} Tf {color}").into_bytes(),
                    StringFormat::Literal,
                ),
            );
            let mut fonts = Dictionary::new();
            fonts.set("OPDS0", Object::Reference(font_id));
            let mut resources = Dictionary::new();
            resources.set("Font", fonts);
            appearance.set("Resources", resources);
            let mut content = format!(
                "q 0 0 {:.3} {:.3} re W n BT /OPDS0 {font_size} Tf {color}\n",
                b.width, b.height,
            );
            let face = font_face()?;
            for (index, line) in text_layout::wrap_text(value, b.width, size, &face)
                .iter()
                .enumerate()
            {
                let (encoded, line_width) = encoded_line(&face, line.text, size)?;
                let baseline = b.height - size - index as f64 * (size + 4.0);
                let x = match align {
                    TextAlign::Left => 0.0,
                    TextAlign::Center => (b.width - line_width) / 2.0,
                    TextAlign::Right => b.width - line_width,
                };
                writeln!(content, "1 0 0 1 {x:.3} {baseline:.3} Tm <{encoded}> Tj")
                    .map_err(|e| e.to_string())?;
            }
            content.push_str("ET Q");
            content.into_bytes()
        }
    };
    let content = if let Some(matrix) = appearance_matrix(rotation, b.width, b.height) {
        let mut wrapped = format!("q {matrix} cm\n").into_bytes();
        wrapped.extend_from_slice(&content);
        wrapped.extend_from_slice(b"\nQ\n");
        wrapped
    } else {
        content
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
        let (crop, rotation) = print_page_geometry(output.get_prev_documents(), page_id)?;
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
            let (mut dictionary, stream) = annotation_object(item, crop, rotation, text_font)?;
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

/// Produce a printable PDF containing the selected original pages in the
/// requested order. Annotation appearances are added before page removal.
pub fn export_print_pages(
    source: &[u8],
    annotations: &[Annotation],
    selected: &[usize],
) -> Result<Vec<u8>, String> {
    let annotated = export_annotations(source, annotations)?;
    let mut document = Document::load_mem(&annotated).map_err(|e| e.to_string())?;
    let original_pages: Vec<_> = document.get_pages().into_values().collect();
    let count = original_pages.len();
    let selected_set: std::collections::BTreeSet<_> = selected.iter().copied().collect();
    if selected.is_empty()
        || selected_set.len() != selected.len()
        || selected_set.iter().any(|&page| page >= count)
    {
        return Err("Choose valid PDF pages without duplicates".into());
    }
    if selected.iter().copied().eq(0..count) {
        return Ok(annotated);
    }
    let omitted: Vec<u32> = (0..count)
        .filter(|page| !selected_set.contains(page))
        .map(|page| page as u32 + 1)
        .collect();
    document.delete_pages(&omitted);

    // Flatten the selected pages under a new root. Before changing Parent,
    // materialize attributes inherited from the original (possibly nested)
    // page tree so rotation, page boxes and resources remain intact.
    let selected_ids: Vec<_> = selected.iter().map(|&page| original_pages[page]).collect();
    for &page_id in &selected_ids {
        for key in [b"Resources".as_slice(), b"MediaBox", b"CropBox", b"Rotate"] {
            if document
                .get_dictionary(page_id)
                .map_err(|e| e.to_string())?
                .get(key)
                .is_err()
            {
                if let Some(value) = inherited(&document, page_id, key)?.cloned() {
                    document
                        .get_dictionary_mut(page_id)
                        .map_err(|e| e.to_string())?
                        .set(key, value);
                }
            }
        }
    }
    let mut page_root = Dictionary::new();
    page_root.set("Type", Object::Name(b"Pages".to_vec()));
    page_root.set("Count", selected_ids.len() as i64);
    page_root.set(
        "Kids",
        Object::Array(
            selected_ids
                .iter()
                .copied()
                .map(Object::Reference)
                .collect(),
        ),
    );
    let page_root_id = document.add_object(page_root);
    for &page_id in &selected_ids {
        document
            .get_dictionary_mut(page_id)
            .map_err(|e| e.to_string())?
            .set("Parent", Object::Reference(page_root_id));
    }
    document
        .catalog_mut()
        .map_err(|e| e.to_string())?
        .set("Pages", Object::Reference(page_root_id));
    document.prune_objects();
    // A full rewrite must not retain offsets into the preceding incremental
    // revision: they no longer exist in the new file.
    document.trailer.remove(b"Prev");
    document.trailer.remove(b"XRefStm");
    let mut bytes = Vec::new();
    document.save_to(&mut bytes).map_err(|e| e.to_string())?;
    let check = Document::load_mem(&bytes).map_err(|e| format!("Print PDF did not reopen: {e}"))?;
    let check_pages: Vec<_> = check.get_pages().into_values().collect();
    if check_pages != selected_ids {
        return Err("Print PDF page order differs from selection".into());
    }
    Ok(bytes)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PrintPaper {
    #[default]
    Original,
    A4,
    Letter,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PrintScale {
    #[default]
    Fit,
    Actual,
    Shrink,
    Custom,
}

/// Put each source page on its output sheet. A PDF destination has no known
/// unprintable margin, so Fit and Shrink use the full sheet. Preview and output
/// both parse these exact transformed bytes.
pub fn apply_print_placement(
    bytes: &[u8],
    paper: PrintPaper,
    mode: PrintScale,
    percent: u16,
) -> Result<Vec<u8>, String> {
    if !(10..=400).contains(&percent) {
        return Err("Page Zoom must be between 10% and 400%".into());
    }
    if paper == PrintPaper::Original && (mode != PrintScale::Custom || percent == 100) {
        return Ok(bytes.to_vec());
    }
    let mut doc = Document::load_mem(bytes).map_err(|e| e.to_string())?;
    let pages: Vec<_> = doc.get_pages().into_values().collect();
    for page_id in pages {
        let (crop, rotation) = print_page_geometry(&doc, page_id)?;
        let [width, height] = [crop[2] - crop[0], crop[3] - crop[1]];
        let display_landscape = if matches!(rotation, 90 | 270) {
            height > width
        } else {
            width > height
        };
        let [display_w, display_h] = match paper {
            PrintPaper::Original if matches!(rotation, 90 | 270) => [height, width],
            PrintPaper::Original => [width, height],
            PrintPaper::A4 if display_landscape => [841.8898, 595.2756],
            PrintPaper::A4 => [595.2756, 841.8898],
            PrintPaper::Letter if display_landscape => [792.0, 612.0],
            PrintPaper::Letter => [612.0, 792.0],
        };
        let [sheet_w, sheet_h] = if matches!(rotation, 90 | 270) {
            [display_h, display_w]
        } else {
            [display_w, display_h]
        };
        let fit = (sheet_w / width).min(sheet_h / height);
        let scale = match mode {
            PrintScale::Fit => fit,
            PrintScale::Actual => 1.0,
            PrintScale::Shrink => fit.min(1.0),
            PrintScale::Custom => f64::from(percent) / 100.0,
        };
        let tx = (sheet_w - width * scale) / 2.0;
        let ty = (sheet_h - height * scale) / 2.0;
        let sheet = array(&[0.0, 0.0, sheet_w, sheet_h]);
        let mut auxiliary_boxes = Vec::new();
        for key in [b"BleedBox".as_slice(), b"TrimBox", b"ArtBox"] {
            if inherited(&doc, page_id, key)?.is_some() {
                auxiliary_boxes.push(key);
            }
        }
        let page = doc.get_dictionary_mut(page_id).map_err(|e| e.to_string())?;
        page.set("MediaBox", sheet.clone());
        page.set("CropBox", sheet.clone());
        for key in auxiliary_boxes {
            page.set(key, sheet.clone());
        }
        let contents = doc
            .get_dictionary(page_id)
            .map_err(|e| e.to_string())?
            .get(b"Contents")
            .ok()
            .cloned();
        if let Some(contents) = contents {
            let mut streams = match contents {
                Object::Array(values) => values,
                Object::Reference(id) => match doc.get_object(id).map_err(|e| e.to_string())? {
                    Object::Array(values) => values.clone(),
                    Object::Stream(_) => vec![Object::Reference(id)],
                    _ => return Err("Unsupported PDF page contents".into()),
                },
                Object::Stream(stream) => vec![Object::Reference(doc.add_object(stream))],
                _ => return Err("Unsupported PDF page contents".into()),
            };
            let prefix = doc.add_object(Stream::new(
                Dictionary::new(),
                format!(
                    "q {scale:.8} 0 0 {scale:.8} {:.8} {:.8} cm\n",
                    tx - crop[0] * scale,
                    ty - crop[1] * scale
                )
                .into_bytes(),
            ));
            let suffix = doc.add_object(Stream::new(Dictionary::new(), b"\nQ\n".to_vec()));
            streams.insert(0, Object::Reference(prefix));
            streams.push(Object::Reference(suffix));
            doc.get_dictionary_mut(page_id)
                .map_err(|e| e.to_string())?
                .set("Contents", Object::Array(streams));
        }
        let annots = doc
            .get_dictionary(page_id)
            .map_err(|e| e.to_string())?
            .get(b"Annots")
            .ok()
            .cloned();
        let annot_entries = match annots {
            None => Vec::new(),
            Some(Object::Array(values)) => values,
            Some(Object::Reference(id)) => doc
                .get_object(id)
                .and_then(Object::as_array)
                .map_err(|e| e.to_string())?
                .clone(),
            Some(_) => return Err("Unsupported PDF annotation array for print placement".into()),
        };
        for entry in annot_entries {
            let id = entry
                .as_reference()
                .map_err(|_| "Print placement requires indirect PDF annotations")?;
            let dict = doc.get_dictionary_mut(id).map_err(|e| e.to_string())?;
            if let Ok(rect) = dict.get(b"Rect") {
                let [x0, y0, x1, y1] = box_values(rect)?;
                dict.set(
                    "Rect",
                    array(&[
                        (x0 - crop[0]) * scale + tx,
                        (y0 - crop[1]) * scale + ty,
                        (x1 - crop[0]) * scale + tx,
                        (y1 - crop[1]) * scale + ty,
                    ]),
                );
            }
            if let Ok(quads) = dict.get(b"QuadPoints") {
                let values = quads.as_array().map_err(|e| e.to_string())?;
                if values.len() % 8 != 0 {
                    return Err("Invalid PDF highlight quadrilateral".into());
                }
                dict.set(
                    "QuadPoints",
                    Object::Array(transform_pairs(values, crop, scale, tx, ty)?),
                );
            }
            for key in [b"Vertices".as_slice(), b"L", b"CL"] {
                if let Ok(points) = dict.get(key) {
                    let values = points.as_array().map_err(|e| e.to_string())?;
                    if (key == b"L" && values.len() != 4)
                        || (key == b"CL" && !matches!(values.len(), 4 | 6))
                    {
                        return Err("Invalid PDF annotation point array".into());
                    }
                    dict.set(
                        key,
                        Object::Array(transform_pairs(values, crop, scale, tx, ty)?),
                    );
                }
            }
            if let Ok(ink) = dict.get(b"InkList") {
                let strokes = ink.as_array().map_err(|e| e.to_string())?;
                let mut transformed = Vec::with_capacity(strokes.len());
                for stroke in strokes {
                    let values = stroke.as_array().map_err(|e| e.to_string())?;
                    transformed.push(Object::Array(transform_pairs(values, crop, scale, tx, ty)?));
                }
                dict.set("InkList", Object::Array(transformed));
            }
        }
    }
    doc.trailer.remove(b"Prev");
    doc.trailer.remove(b"XRefStm");
    doc.prune_objects();
    let mut output = Vec::new();
    doc.save_to(&mut output).map_err(|e| e.to_string())?;
    let check =
        Document::load_mem(&output).map_err(|e| format!("Placed print PDF did not reopen: {e}"))?;
    if check.get_pages().len() != doc.get_pages().len() {
        return Err("Print placement changed the PDF page count".into());
    }
    Ok(output)
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
    #[cfg(feature = "dynlex-experimental")]
    use std::sync::Arc;

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn reimported_native_freetext_appearance_uses_dynlex_glyphs() {
        use opds_dynlex_backend::{render_with_report, Backend};
        use opds_engine_probe::ParsedPdf;

        for (rotation, geometry) in [(0, 0), (90, 0), (180, 0), (270, 0), (0, 1), (90, 2)] {
            let (media, crop, position) = match geometry {
                1 => ([0_i64, 0, 595, 842], [0_i64, 0, 595, 842], [300.0, 500.0]),
                2 => (
                    [0_i64, 0, 1684, 2384],
                    [0_i64, 0, 1684, 2384],
                    [1100.0, 900.0],
                ),
                _ => ([20_i64, 30, 220, 330], [40_i64, 60, 200, 300], [20.0, 30.0]),
            };
            let mut doc = Document::with_version("1.4");
            let pages = doc.new_object_id();
            let content = doc.add_object(Stream::new(
                Dictionary::new(),
                if rotation == 90 {
                    b"0 0 1 rg 50 70 90 60 re f".to_vec()
                } else {
                    Vec::new()
                },
            ));
            let page = doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages, "Contents" => content,
                "MediaBox" => media.map(Object::from).to_vec(),
                "CropBox" => crop.map(Object::from).to_vec(),
                "Rotate" => rotation,
            });
            doc.objects.insert(
                pages,
                Object::Dictionary(dictionary! {
                    "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1,
                }),
            );
            let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
            doc.trailer.set("Root", Object::Reference(catalog));
            let mut source = Vec::new();
            doc.save_to(&mut source).unwrap();
            for (text, align) in [
                ("ΩΩΩ", TextAlign::Center),
                ("Hello Ω", TextAlign::Left),
                ("Line one\nNext Ω", TextAlign::Right),
            ] {
                if geometry == 2 && text != "ΩΩΩ" {
                    continue;
                }
                let annotation = Annotation {
                    id: 501,
                    page: 0,
                    bounds: Rect {
                        x: position[0],
                        y: position[1],
                        width: 110.0,
                        height: 58.0,
                    },
                    kind: Kind::Text {
                        value: text.into(),
                        color_rgb: [0, 80, 240],
                        font_size: 20,
                        align,
                    },
                };
                let saved = export_annotations(&source, &[annotation.clone()]).unwrap();
                let pdf = ParsedPdf::open(Arc::new(saved.clone())).unwrap();
                let expected = pdf.render_page(1, 1.0).unwrap();
                let actual = render_with_report(&pdf, &saved, 1, 1.0, Backend::Dynlex).unwrap();
                assert!(actual.imported_annotations_dynlex, "{text}");
                assert_eq!(actual.raster.rgba, expected.rgba, "{text}");
                if text == "ΩΩΩ" && matches!(rotation, 0 | 90) && geometry == 0 {
                    let mut second = annotation.clone();
                    second.id = 502;
                    second.bounds.x += 10.0;
                    let multiple =
                        export_annotations(&source, &[annotation.clone(), second.clone()]).unwrap();
                    let pdf = ParsedPdf::open(Arc::new(multiple.clone())).unwrap();
                    let expected = pdf.render_page(1, 1.0).unwrap();
                    let report =
                        render_with_report(&pdf, &multiple, 1, 1.0, Backend::Dynlex).unwrap();
                    assert!(report.imported_annotations_dynlex);
                    assert_eq!(report.raster.rgba, expected.rgba);

                    let square = Annotation {
                        id: 503,
                        page: 0,
                        bounds: Rect {
                            x: 45.0,
                            y: 60.0,
                            width: 42.0,
                            height: 32.0,
                        },
                        kind: Kind::Rectangle {
                            color_rgb: [231, 76, 60],
                        },
                    };
                    let highlight = Annotation {
                        id: 504,
                        page: 0,
                        bounds: Rect {
                            x: 30.0,
                            y: 40.0,
                            width: 60.0,
                            height: 24.0,
                        },
                        kind: Kind::Rectangle {
                            color_rgb: [255, 226, 67],
                        },
                    };
                    for marks in [
                        vec![annotation.clone(), square.clone(), highlight.clone()],
                        vec![highlight.clone(), annotation.clone(), square.clone()],
                        vec![
                            square.clone(),
                            annotation.clone(),
                            highlight.clone(),
                            second.clone(),
                        ],
                    ] {
                        let mixed = export_annotations(&source, &marks).unwrap();
                        let pdf = ParsedPdf::open(Arc::new(mixed.clone())).unwrap();
                        let expected = pdf.render_page(1, 1.0).unwrap();
                        let report =
                            render_with_report(&pdf, &mixed, 1, 1.0, Backend::Dynlex).unwrap();
                        assert!(report.imported_annotations_dynlex);
                        assert_eq!(report.raster.rgba, expected.rgba);
                        if rotation == 90
                            && marks.len() == 4
                            && std::env::var_os("OPDS_BENCH_IMPORTED_ANNOTATIONS").is_some()
                        {
                            let mut dynlex_us = Vec::new();
                            let mut hayro_us = Vec::new();
                            for round in 0..20 {
                                let mut measure_dynlex = || {
                                    let started = std::time::Instant::now();
                                    let actual =
                                        render_with_report(&pdf, &mixed, 1, 1.0, Backend::Dynlex)
                                            .unwrap();
                                    assert!(actual.imported_annotations_dynlex);
                                    assert_eq!(actual.raster.rgba, expected.rgba);
                                    dynlex_us.push(started.elapsed().as_micros());
                                };
                                let mut measure_hayro = || {
                                    let started = std::time::Instant::now();
                                    let actual = pdf.render_page(1, 1.0).unwrap();
                                    assert_eq!(actual.rgba, expected.rgba);
                                    hayro_us.push(started.elapsed().as_micros());
                                };
                                if round % 2 == 0 {
                                    measure_dynlex();
                                    measure_hayro();
                                } else {
                                    measure_hayro();
                                    measure_dynlex();
                                }
                            }
                            dynlex_us.sort_unstable();
                            hayro_us.sort_unstable();
                            println!(
                                "mixed imported annotations, 20 warm paired renders: DynLex+Hayro {} µs, Hayro {} µs",
                                dynlex_us[10], hayro_us[10]
                            );
                        }
                        if rotation == 0 && marks.len() == 3 {
                            let mut doc = Document::load_mem(&mixed).unwrap();
                            let page_id = *doc.get_pages().get(&1).unwrap();
                            let form = doc.add_object(Stream::new(
                                dictionary! {
                                    "Type" => "XObject", "Subtype" => "Form",
                                    "BBox" => vec![0.into(),0.into(),40.into(),30.into()],
                                },
                                b"q 1 0 0 rg 0 0 40 30 re f Q".to_vec(),
                            ));
                            let stamp = doc.add_object(dictionary! {
                                "Type" => "Annot", "Subtype" => "Stamp", "F" => 4,
                                "Rect" => vec![70.into(),90.into(),110.into(),120.into()],
                                "AP" => dictionary! { "N" => form },
                            });
                            let annots = doc
                                .get_dictionary(page_id)
                                .unwrap()
                                .get(b"Annots")
                                .unwrap()
                                .clone();
                            let (_, annots) = doc.dereference(&annots).unwrap();
                            let mut ordered = annots.as_array().unwrap().clone();
                            ordered.push(Object::Reference(stamp));
                            doc.get_dictionary_mut(page_id)
                                .unwrap()
                                .set("Annots", ordered);
                            doc.trailer.remove(b"Prev");
                            let mut combined = Vec::new();
                            doc.save_to(&mut combined).unwrap();
                            let pdf = ParsedPdf::open(Arc::new(combined.clone())).unwrap();
                            let expected = pdf.render_page(1, 1.0).unwrap();
                            let report =
                                render_with_report(&pdf, &combined, 1, 1.0, Backend::Dynlex)
                                    .unwrap();
                            assert!(report.imported_annotations_dynlex);
                            assert_eq!(report.raster.rgba, expected.rgba);
                        } else if rotation == 90 && marks.len() == 3 {
                            let mut doc = Document::load_mem(&mixed).unwrap();
                            let page_id = *doc.get_pages().get(&1).unwrap();
                            let missing_square = doc.add_object(dictionary! {
                                "Type" => "Annot", "Subtype" => "Square", "F" => 4,
                                "Rect" => vec![90.into(),100.into(),130.into(),140.into()],
                                "C" => vec![1.into(),0.into(),0.into()],
                                "BS" => dictionary! { "W" => 3 },
                            });
                            let annots = doc
                                .get_dictionary(page_id)
                                .unwrap()
                                .get(b"Annots")
                                .unwrap()
                                .clone();
                            let (_, annots) = doc.dereference(&annots).unwrap();
                            let mut ordered = annots.as_array().unwrap().clone();
                            ordered.push(Object::Reference(missing_square));
                            doc.get_dictionary_mut(page_id)
                                .unwrap()
                                .set("Annots", ordered);
                            doc.trailer.remove(b"Prev");
                            let mut combined = Vec::new();
                            doc.save_to(&mut combined).unwrap();
                            let pdf = ParsedPdf::open(Arc::new(combined.clone())).unwrap();
                            let expected = pdf.render_page(1, 1.0).unwrap();
                            let report =
                                render_with_report(&pdf, &combined, 1, 1.0, Backend::Dynlex)
                                    .unwrap();
                            assert!(report.imported_annotations_dynlex);
                            assert_eq!(report.raster.rgba, expected.rgba);
                        }
                    }

                    let mut modified = Document::load_mem(&saved).unwrap();
                    let page_id = *modified.get_pages().get(&1).unwrap();
                    let annotation_id = modified
                        .get_dictionary(page_id)
                        .unwrap()
                        .get(b"Annots")
                        .unwrap()
                        .as_array()
                        .unwrap()[0]
                        .as_reference()
                        .unwrap();
                    modified
                        .get_dictionary_mut(annotation_id)
                        .unwrap()
                        .set("OC", Object::Null);
                    modified.trailer.remove(b"Prev");
                    let mut unsupported = Vec::new();
                    modified.save_to(&mut unsupported).unwrap();
                    let pdf = ParsedPdf::open(Arc::new(unsupported.clone())).unwrap();
                    let report =
                        render_with_report(&pdf, &unsupported, 1, 1.0, Backend::Dynlex).unwrap();
                    assert!(!report.imported_annotations_dynlex);

                    modified
                        .get_dictionary_mut(annotation_id)
                        .unwrap()
                        .remove(b"OC");
                    let form_id = modified
                        .get_dictionary(annotation_id)
                        .unwrap()
                        .get(b"AP")
                        .unwrap()
                        .as_dict()
                        .unwrap()
                        .get(b"N")
                        .unwrap()
                        .as_reference()
                        .unwrap();
                    modified
                        .get_object_mut(form_id)
                        .unwrap()
                        .as_stream_mut()
                        .unwrap()
                        .content
                        .extend_from_slice(b"\n0 0 m");
                    unsupported.clear();
                    modified.save_to(&mut unsupported).unwrap();
                    let pdf = ParsedPdf::open(Arc::new(unsupported.clone())).unwrap();
                    let report =
                        render_with_report(&pdf, &unsupported, 1, 1.0, Backend::Dynlex).unwrap();
                    assert!(!report.imported_annotations_dynlex);
                }
            }
        }
    }

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn reimported_large_text_box_uses_ordered_dynlex_tiles() {
        use opds_dynlex_backend::{render_with_report, Backend};
        use opds_engine_probe::ParsedPdf;

        for rotation in [0, 90] {
            let mut doc = Document::with_version("1.4");
            let pages = doc.new_object_id();
            let content = doc.add_object(Stream::new(Dictionary::new(), Vec::new()));
            let page = doc.add_object(dictionary! {
                "Type" => "Page", "Parent" => pages, "Contents" => content,
                "MediaBox" => vec![0.into(),0.into(),640.into(),880.into()],
                "CropBox" => vec![20.into(),30.into(),620.into(),880.into()],
                "Rotate" => rotation,
            });
            doc.objects.insert(
                pages,
                Object::Dictionary(dictionary! {
                    "Type" => "Pages", "Kids" => vec![Object::Reference(page)], "Count" => 1,
                }),
            );
            let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages });
            doc.trailer.set("Root", Object::Reference(catalog));
            let mut source = Vec::new();
            doc.save_to(&mut source).unwrap();

            for (value, width, height) in [
                ("Hello Ω ".repeat(9), 500.0, 100.0),
                ("Hello Ω\n".repeat(9), 130.0, 300.0),
            ] {
                let mark = Annotation {
                    id: 700,
                    page: 0,
                    bounds: Rect {
                        x: 45.0,
                        y: 70.0,
                        width,
                        height,
                    },
                    kind: Kind::Text {
                        value,
                        color_rgb: [0, 80, 240],
                        font_size: 20,
                        align: TextAlign::Left,
                    },
                };
                let saved = export_annotations(&source, &[mark.clone()]).unwrap();
                let pdf = ParsedPdf::open(Arc::new(saved.clone())).unwrap();
                let expected = pdf.render_page(1, 1.0).unwrap();
                let actual = render_with_report(&pdf, &saved, 1, 1.0, Backend::Dynlex).unwrap();
                assert!(
                    actual.imported_annotations_dynlex,
                    "rotation={rotation} width={width}"
                );
                assert_eq!(
                    actual.raster.rgba, expected.rgba,
                    "rotation={rotation} width={width}"
                );
                if width == 500.0 {
                    let square = Annotation {
                        id: 702,
                        page: 0,
                        bounds: Rect {
                            x: 70.0,
                            y: 80.0,
                            width: 42.0,
                            height: 32.0,
                        },
                        kind: Kind::Rectangle {
                            color_rgb: [231, 76, 60],
                        },
                    };
                    for marks in [
                        vec![square.clone(), mark.clone()],
                        vec![mark.clone(), square.clone()],
                    ] {
                        let saved = export_annotations(&source, &marks).unwrap();
                        let pdf = ParsedPdf::open(Arc::new(saved.clone())).unwrap();
                        let expected = pdf.render_page(1, 1.0).unwrap();
                        let actual =
                            render_with_report(&pdf, &saved, 1, 1.0, Backend::Dynlex).unwrap();
                        assert!(actual.imported_annotations_dynlex);
                        assert_eq!(actual.raster.rgba, expected.rgba);
                    }
                }
            }
            let too_many = Annotation {
                id: 701,
                page: 0,
                bounds: Rect {
                    x: 45.0,
                    y: 70.0,
                    width: 500.0,
                    height: 300.0,
                },
                kind: Kind::Text {
                    value: "W".repeat(150),
                    color_rgb: [0, 80, 240],
                    font_size: 20,
                    align: TextAlign::Left,
                },
            };
            let saved = export_annotations(&source, &[too_many]).unwrap();
            let pdf = ParsedPdf::open(Arc::new(saved.clone())).unwrap();
            let expected = pdf.render_page(1, 1.0).unwrap();
            let actual = render_with_report(&pdf, &saved, 1, 1.0, Backend::Dynlex).unwrap();
            assert!(!actual.imported_annotations_dynlex);
            assert_eq!(actual.raster.rgba, expected.rgba);
        }
    }

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

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn dynlex_native_rectangle_and_highlight_match_saved_pdf_appearances() {
        use opds_dynlex_backend::render_native_annotation_overlay;
        use opds_engine_probe::ParsedPdf;

        let source = source_pdf();
        let base = ParsedPdf::open(Arc::new(source.clone()))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        for color in [[231, 76, 60], [255, 226, 67]] {
            let mark = Annotation {
                id: 77,
                page: 0,
                bounds: Rect {
                    x: 20.0,
                    y: 30.0,
                    width: 48.0,
                    height: 36.0,
                },
                kind: Kind::Rectangle { color_rgb: color },
            };
            let dynlex = render_native_annotation_overlay(&base, &[mark.clone()], 0)
                .unwrap()
                .expect("bounded mark should use DynLex");
            let saved = export_annotations(&source, &[mark]).unwrap();
            let hayro = ParsedPdf::open(Arc::new(saved))
                .unwrap()
                .render_page(1, 1.0)
                .unwrap();
            assert_eq!(dynlex.rgba.len(), hayro.rgba.len());
            let wrong: Vec<_> = dynlex
                .rgba
                .iter()
                .zip(&hayro.rgba)
                .enumerate()
                .filter_map(|(index, (left, right))| {
                    (left != right).then_some((index, *left, *right))
                })
                .collect();
            assert!(
                wrong.is_empty(),
                "mark color {color:?}: {} differing bytes, first {:?}",
                wrong.len(),
                wrong.first()
            );
        }
        let mut marks = [
            Annotation {
                id: 78,
                page: 0,
                bounds: Rect {
                    x: 20.0,
                    y: 30.0,
                    width: 48.0,
                    height: 36.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [231, 76, 60],
                },
            },
            Annotation {
                id: 79,
                page: 0,
                bounds: Rect {
                    x: 90.0,
                    y: 80.0,
                    width: 48.0,
                    height: 36.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [255, 226, 67],
                },
            },
        ];
        let dynlex = render_native_annotation_overlay(&base, &marks, 0)
            .unwrap()
            .expect("ordered marks should use DynLex");
        let saved = export_annotations(&source, &marks).unwrap();
        let hayro = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let wrong: Vec<_> = dynlex
            .rgba
            .iter()
            .zip(&hayro.rgba)
            .enumerate()
            .filter_map(|(index, (left, right))| (left != right).then_some((index, *left, *right)))
            .collect();
        assert!(
            wrong.is_empty(),
            "disjoint order: {} differing bytes, first {:?}",
            wrong.len(),
            &wrong[..wrong.len().min(20)]
        );
        marks[1].bounds.x = 42.0;
        marks[1].bounds.y = 46.0;
        for order in 0..2 {
            let candidate = render_native_annotation_overlay(&base, &marks, 0)
                .unwrap()
                .expect("bounded overlap");
            let saved = export_annotations(&source, &marks).unwrap();
            let expected = ParsedPdf::open(Arc::new(saved))
                .unwrap()
                .render_page(1, 1.0)
                .unwrap();
            let wrong: Vec<_> = candidate
                .rgba
                .iter()
                .zip(&expected.rgba)
                .enumerate()
                .filter_map(|(at, (actual, expected))| {
                    (actual != expected).then_some((at, *actual, *expected))
                })
                .collect();
            assert!(
                wrong.is_empty(),
                "overlap order {order}: {:?}",
                &wrong[..wrong.len().min(20)]
            );
            marks.swap(0, 1);
        }
        marks[1].kind = Kind::Rectangle {
            color_rgb: [0, 80, 240],
        };
        let overlap = render_native_annotation_overlay(&base, &marks, 0)
            .unwrap()
            .expect("overlapping opaque Square appearances should reach DynLex");
        let saved = export_annotations(&source, &marks).unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        assert_eq!(overlap.rgba, expected.rgba);
        marks[1].kind = Kind::Rectangle {
            color_rgb: [255, 226, 67],
        };
        marks[1].bounds.x = 90.5;
        marks[1].bounds.y = 80.0;
        assert!(
            render_native_annotation_overlay(&base, &marks, 0)
                .unwrap()
                .is_none(),
            "subpixel annotation must fall back"
        );
        marks[1].bounds.x = 90.0;
        marks[1].bounds.y = 80.0;
        marks[1].kind = Kind::Text {
            value: "Ω".into(),
            color_rgb: [0, 0, 0],
            font_size: 16,
            align: TextAlign::Left,
        };
        assert!(
            render_native_annotation_overlay(&base, &marks, 0)
                .unwrap()
                .is_none(),
            "unproven annotation text must fall back"
        );
    }

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn dynlex_overlapping_opaque_squares_match_colored_and_rotated_pages() {
        use opds_dynlex_backend::render_native_annotation_overlay;
        use opds_engine_probe::ParsedPdf;

        let colored =
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec();
        let mut rotated = Document::load_mem(&source_pdf()).unwrap();
        let page_id = *rotated.get_pages().get(&1).unwrap();
        let page = rotated
            .get_object_mut(page_id)
            .unwrap()
            .as_dict_mut()
            .unwrap();
        page.set(
            "CropBox",
            vec![20.into(), 20.into(), 180.into(), 260.into()],
        );
        page.set("Rotate", 90);
        let mut rotated_bytes = Vec::new();
        rotated.save_to(&mut rotated_bytes).unwrap();

        for (source, top) in [(colored, 140.0), (rotated_bytes, 30.0)] {
            let parsed = ParsedPdf::open(Arc::new(source.clone())).unwrap();
            let base = parsed.render_page(1, 1.0).unwrap();
            for colors in [([231, 76, 60], [0, 80, 240]), ([0, 80, 240], [231, 76, 60])] {
                let marks = [
                    Annotation {
                        id: 901,
                        page: 0,
                        bounds: Rect {
                            x: 20.0,
                            y: top,
                            width: 48.0,
                            height: 36.0,
                        },
                        kind: Kind::Rectangle {
                            color_rgb: colors.0,
                        },
                    },
                    Annotation {
                        id: 902,
                        page: 0,
                        bounds: Rect {
                            x: 42.0,
                            y: top + 16.0,
                            width: 48.0,
                            height: 36.0,
                        },
                        kind: Kind::Rectangle {
                            color_rgb: colors.1,
                        },
                    },
                ];
                let actual = render_native_annotation_overlay(&base, &marks, 0)
                    .unwrap()
                    .expect("pixel-aligned opaque overlap");
                let saved = export_annotations(&source, &marks).unwrap();
                let expected = ParsedPdf::open(Arc::new(saved))
                    .unwrap()
                    .render_page(1, 1.0)
                    .unwrap();
                assert_eq!(actual.rgba, expected.rgba, "colors={colors:?}, top={top}");
            }
        }

        let source = source_pdf();
        let parsed = ParsedPdf::open(Arc::new(source.clone())).unwrap();
        let base = parsed.render_page(1, 1.0).unwrap();
        let mut seed = 0x4da7_36b1_u32;
        for case in 0..32 {
            let mut next_color = || {
                let mut color = [0; 3];
                for channel in &mut color {
                    seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                    *channel = (seed >> 24) as u8;
                }
                color
            };
            let mut colors = [next_color(), next_color()];
            for color in &mut colors {
                if *color == [255, 226, 67] {
                    color[0] = 254;
                }
            }
            let marks = [
                Annotation {
                    id: 910,
                    page: 0,
                    bounds: Rect {
                        x: 20.0,
                        y: 30.0,
                        width: 48.0,
                        height: 36.0,
                    },
                    kind: Kind::Rectangle {
                        color_rgb: colors[0],
                    },
                },
                Annotation {
                    id: 911,
                    page: 0,
                    bounds: Rect {
                        x: 42.0,
                        y: 46.0,
                        width: 48.0,
                        height: 36.0,
                    },
                    kind: Kind::Rectangle {
                        color_rgb: colors[1],
                    },
                },
            ];
            let actual = render_native_annotation_overlay(&base, &marks, 0)
                .unwrap()
                .expect("opaque overlap");
            let saved = export_annotations(&source, &marks).unwrap();
            let expected = ParsedPdf::open(Arc::new(saved))
                .unwrap()
                .render_page(1, 1.0)
                .unwrap();
            assert_eq!(actual.rgba, expected.rgba, "case {case}, colors={colors:?}");
        }
    }

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn highlight_appearance_exposes_premultiplied_alpha_and_overlap_edge() {
        use opds_engine_probe::ParsedPdf;
        let source = source_pdf();
        let square = Annotation {
            id: 991,
            page: 0,
            bounds: Rect {
                x: 20.0,
                y: 30.0,
                width: 48.0,
                height: 36.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [231, 76, 60],
            },
        };
        let highlight = Annotation {
            id: 992,
            page: 0,
            bounds: Rect {
                x: 42.0,
                y: 46.0,
                width: 48.0,
                height: 36.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [255, 226, 67],
            },
        };
        let pixel = |raster: &opds_engine_probe::Raster, x: u32, y: u32| -> [u8; 4] {
            let at = ((y * raster.width + x) * 4) as usize;
            raster.rgba[at..at + 4].try_into().unwrap()
        };
        for (name, marks) in [
            ("square", vec![square.clone()]),
            ("highlight", vec![highlight.clone()]),
            ("both", vec![square.clone(), highlight.clone()]),
        ] {
            let saved = export_annotations(&source, &marks).unwrap();
            let pdf = ParsedPdf::open(Arc::new(saved)).unwrap();
            let raster = pdf.render_page_content_layer(1, 1.0).unwrap();
            match name {
                "square" => {
                    assert_eq!(pixel(&raster, 66, 46), [231, 76, 60, 255]);
                    assert_eq!(pixel(&raster, 66, 48), [231, 76, 60, 255]);
                }
                "highlight" => {
                    assert_eq!(pixel(&raster, 66, 46), [89, 79, 23, 89]);
                    assert_eq!(pixel(&raster, 66, 48), [89, 79, 23, 89]);
                }
                "both" => {
                    assert_eq!(pixel(&raster, 66, 46), [239, 128, 62, 255]);
                    assert_eq!(pixel(&raster, 66, 48), [239, 129, 62, 255]);
                }
                _ => unreachable!(),
            }
        }
    }

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn dynlex_highlight_over_partially_covered_page_falls_back() {
        use opds_dynlex_backend::{
            render_native_annotation_overlay, render_native_annotation_overlay_pixels_with_content,
        };
        use opds_engine_probe::ParsedPdf;

        let source =
            include_bytes!("../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf").to_vec();
        let mark = Annotation {
            id: 78,
            page: 0,
            bounds: Rect {
                x: 20.0,
                y: 140.0,
                width: 50.0,
                height: 36.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [255, 226, 67],
            },
        };
        let base = ParsedPdf::open(Arc::new(source.clone()))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let source_layer = ParsedPdf::open(Arc::new(source.clone()))
            .unwrap()
            .render_page_content_layer(1, 1.0)
            .unwrap();
        assert!(render_native_annotation_overlay(&base, &[mark.clone()], 0)
            .unwrap()
            .is_none());
        let highlight = render_native_annotation_overlay_pixels_with_content(
            &base.rgba,
            &source_layer.rgba,
            base.width,
            base.height,
            (base.page_width_pt, base.page_height_pt),
            &[mark.clone()],
            0,
        )
        .unwrap();
        assert!(
            highlight.is_none(),
            "the source layer is not opaque throughout the Highlight"
        );
        let opaque_square = Annotation {
            id: 79,
            page: 0,
            bounds: Rect {
                x: 20.0,
                y: 140.0,
                width: 50.0,
                height: 36.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [0, 80, 240],
            },
        };
        let dynlex = render_native_annotation_overlay(&base, &[opaque_square.clone()], 0)
            .unwrap()
            .expect("opaque square over colored page");
        let saved = export_annotations(&source, &[opaque_square]).unwrap();
        let hayro = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        assert!(
            dynlex.rgba == hayro.rgba,
            "opaque Square over colored page differs"
        );
    }

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn dynlex_highlight_premultiplied_composition_matches_colored_and_layered_pages() {
        use opds_dynlex_backend::render_native_annotation_overlay_pixels_with_content;
        use opds_engine_probe::ParsedPdf;

        let mut seed = 0x5ac3_1947_u32;
        for case in 0..128 {
            let mut source = Document::load_mem(&source_pdf()).unwrap();
            let page_id = *source.get_pages().get(&1).unwrap();
            let contents_id = source
                .get_dictionary(page_id)
                .unwrap()
                .get(b"Contents")
                .unwrap()
                .as_reference()
                .unwrap();
            let mut color = [0_u8; 3];
            for channel in &mut color {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *channel = (seed >> 24) as u8;
            }
            let mut content = format!(
                "{:.3} {:.3} {:.3} rg 0 0 200 300 re f",
                color[0] as f64 / 255.0,
                color[1] as f64 / 255.0,
                color[2] as f64 / 255.0,
            );
            if case % 2 == 1 {
                // A second opaque source shape crosses the Highlight. The
                // compositor must use each pixel's actual rendered underlay.
                content.push_str(" q 0.13 0.61 0.92 rg 35 225 42 44 re f Q");
            }
            source
                .get_object_mut(contents_id)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_plain_content(content.into_bytes());
            if case % 8 == 0 {
                let page = source
                    .get_object_mut(page_id)
                    .unwrap()
                    .as_dict_mut()
                    .unwrap();
                page.set(
                    "CropBox",
                    vec![20.into(), 20.into(), 180.into(), 260.into()],
                );
                page.set("Rotate", 90);
            }
            let mut bytes = Vec::new();
            source.save_to(&mut bytes).unwrap();
            let mark = Annotation {
                id: 994,
                page: 0,
                bounds: Rect {
                    x: 20.0 + (case % 11) as f64,
                    y: 30.0 + (case % 9) as f64,
                    width: 40.0 + (case % 7) as f64,
                    height: 28.0 + (case % 13) as f64,
                },
                kind: Kind::Rectangle {
                    color_rgb: [255, 226, 67],
                },
            };
            let mut marks = vec![mark];
            if case % 3 == 0 {
                marks.push(Annotation {
                    id: 995,
                    page: 0,
                    bounds: Rect {
                        x: 105.0,
                        y: 110.0,
                        width: 40.0,
                        height: 28.0,
                    },
                    kind: Kind::Rectangle {
                        color_rgb: [255, 226, 67],
                    },
                });
            }
            let parsed = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
            let base = parsed.render_page(1, 1.0).unwrap();
            let content_layer = parsed.render_page_content_layer(1, 1.0).unwrap();
            if case == 0 {
                let mut stale = content_layer.rgba.clone();
                let at = ((30 * base.width + 20) * 4) as usize;
                stale[at] ^= 1;
                assert!(render_native_annotation_overlay_pixels_with_content(
                    &base.rgba,
                    &stale,
                    base.width,
                    base.height,
                    (base.page_width_pt, base.page_height_pt),
                    &marks,
                    0,
                )
                .unwrap()
                .is_none());
            }
            let actual = render_native_annotation_overlay_pixels_with_content(
                &base.rgba,
                &content_layer.rgba,
                base.width,
                base.height,
                (base.page_width_pt, base.page_height_pt),
                &marks,
                0,
            )
            .unwrap()
            .expect("opaque page and pixel-aligned Highlight");
            let saved = export_annotations(&bytes, &marks).unwrap();
            let expected = ParsedPdf::open(Arc::new(saved))
                .unwrap()
                .render_page(1, 1.0)
                .unwrap();
            let wrong: Vec<_> = actual
                .rgba
                .iter()
                .zip(&expected.rgba)
                .enumerate()
                .filter_map(|(index, (actual, expected))| {
                    (actual != expected).then_some((index, *actual, *expected))
                })
                .collect();
            assert!(
                wrong.is_empty(),
                "case {case}, input {color:?}, bounds {:?}, {} mismatches: {:?}",
                marks[0].bounds,
                wrong.len(),
                &wrong[..wrong.len().min(12)]
            );
        }
    }

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn dynlex_overlapping_highlights_and_squares_match_saved_appearances() {
        use opds_dynlex_backend::render_native_annotation_overlay_pixels_with_content;
        use opds_engine_probe::ParsedPdf;

        let mut seed = 0x2981_64ea_u32;
        for case in 0..64 {
            let mut color = [0_u8; 3];
            let mut square_color = [0_u8; 3];
            for channel in color.iter_mut().chain(square_color.iter_mut()) {
                seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                *channel = (seed >> 24) as u8;
            }
            if square_color == [255, 226, 67] {
                square_color[0] = 254;
            }
            let mut source = Document::load_mem(&source_pdf()).unwrap();
            let page_id = *source.get_pages().get(&1).unwrap();
            let contents_id = source
                .get_dictionary(page_id)
                .unwrap()
                .get(b"Contents")
                .unwrap()
                .as_reference()
                .unwrap();
            source
                .get_object_mut(contents_id)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_plain_content(
                    format!(
                        "{:.3} {:.3} {:.3} rg 0 0 200 300 re f",
                        color[0] as f64 / 255.0,
                        color[1] as f64 / 255.0,
                        color[2] as f64 / 255.0,
                    )
                    .into_bytes(),
                );
            if case % 8 == 0 {
                let page = source
                    .get_object_mut(page_id)
                    .unwrap()
                    .as_dict_mut()
                    .unwrap();
                page.set(
                    "CropBox",
                    vec![20.into(), 20.into(), 180.into(), 260.into()],
                );
                page.set("Rotate", 90);
            }
            let mut bytes = Vec::new();
            source.save_to(&mut bytes).unwrap();
            let first_color = if case % 4 == 1 || case % 4 == 2 {
                [255, 226, 67]
            } else {
                square_color
            };
            let second_color = [255, 226, 67];
            let first = Annotation {
                id: 998,
                page: 0,
                bounds: Rect {
                    x: 20.0 + (case % 7) as f64,
                    y: 30.0 + (case % 5) as f64,
                    width: 48.0 + (case % 5) as f64,
                    height: 36.0 + (case % 7) as f64,
                },
                kind: Kind::Rectangle {
                    color_rgb: first_color,
                },
            };
            let second = Annotation {
                id: 999,
                page: 0,
                bounds: Rect {
                    x: first.bounds.x + 20.0,
                    y: first.bounds.y + 12.0,
                    width: 48.0,
                    height: 36.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: second_color,
                },
            };
            let mut marks = if case % 4 == 3 {
                vec![second, first]
            } else {
                vec![first, second]
            };
            if case % 5 == 0 {
                marks.push(Annotation {
                    id: 1000,
                    page: 0,
                    bounds: Rect {
                        x: marks[0].bounds.x + 10.0,
                        y: marks[0].bounds.y + 6.0,
                        width: 40.0,
                        height: 30.0,
                    },
                    kind: Kind::Rectangle {
                        color_rgb: if case % 2 == 0 {
                            [255, 226, 67]
                        } else {
                            square_color
                        },
                    },
                });
            }
            if case == 0 {
                while marks.len() < 64 {
                    let index = marks.len();
                    marks.push(Annotation {
                        id: 1_001 + index as u64,
                        page: 0,
                        bounds: Rect {
                            x: 24.0 + (index % 6) as f64,
                            y: 34.0 + (index % 5) as f64,
                            width: 44.0,
                            height: 32.0,
                        },
                        kind: Kind::Rectangle {
                            color_rgb: if index % 2 == 0 {
                                [255, 226, 67]
                            } else {
                                square_color
                            },
                        },
                    });
                }
            }
            let parsed = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
            let base = parsed.render_page(1, 1.0).unwrap();
            let layer = parsed.render_page_content_layer(1, 1.0).unwrap();
            let actual = render_native_annotation_overlay_pixels_with_content(
                &base.rgba,
                &layer.rgba,
                base.width,
                base.height,
                (base.page_width_pt, base.page_height_pt),
                &marks,
                0,
            )
            .unwrap()
            .expect("opaque source and pixel-aligned annotations");
            let saved = export_annotations(&bytes, &marks).unwrap();
            let expected = ParsedPdf::open(Arc::new(saved))
                .unwrap()
                .render_page(1, 1.0)
                .unwrap();
            let wrong: Vec<_> = actual
                .rgba
                .iter()
                .zip(&expected.rgba)
                .enumerate()
                .filter_map(|(at, (actual, expected))| {
                    (actual != expected).then_some((at, *actual, *expected))
                })
                .collect();
            assert!(
                wrong.is_empty(),
                "case {case}, source {color:?}, square {square_color:?}, {} mismatches: {:?}",
                wrong.len(),
                &wrong[..wrong.len().min(12)]
            );
        }
    }

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn dynlex_glyph_and_shape_order_matches_saved_appearances() {
        use opds_dynlex_backend::{
            render_native_annotation_overlay_pixels,
            render_native_annotation_overlay_pixels_on_opaque_source,
            render_native_annotation_overlay_pixels_with_content,
            render_validated_glyph_scene_over_opaque,
        };
        use opds_engine_probe::ParsedPdf;

        let mut source = Document::load_mem(include_bytes!(
            "../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf"
        ))
        .unwrap();
        let page_id = *source.get_pages().get(&1).unwrap();
        let contents_id = source
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .as_reference()
            .unwrap();
        source
            .get_object_mut(contents_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_plain_content(b"0.2 0.4 0.7 rg 0 0 256 192 re f".to_vec());
        let mut bytes = Vec::new();
        source.save_to(&mut bytes).unwrap();
        let parsed = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        let base = parsed.render_page(1, 1.0).unwrap();
        let content = parsed.render_page_content_layer(1, 1.0).unwrap();
        let text = Annotation {
            id: 1001,
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
        let mut shape = Annotation {
            id: 1002,
            page: 0,
            bounds: Rect {
                x: 55.0,
                y: 40.0,
                width: 40.0,
                height: 30.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [231, 76, 60],
            },
        };
        let scene = crate::native_text_scene::stage_native_text_box_scene(&text).unwrap();
        let clip = [20, 30, 130, 88];
        let points = (base.page_width_pt, base.page_height_pt);
        for shape_color in [[231, 76, 60], [255, 226, 67]] {
            shape.kind = Kind::Rectangle {
                color_rgb: shape_color,
            };
            for order in 0..2 {
                let marks = if order == 0 {
                    vec![text.clone(), shape.clone()]
                } else {
                    vec![shape.clone(), text.clone()]
                };
                let actual = if order == 0 {
                    let text_bitmap = render_validated_glyph_scene_over_opaque(
                        &scene,
                        &base.rgba,
                        base.width,
                        base.height,
                        clip,
                    )
                    .unwrap();
                    if shape_color == [255, 226, 67] {
                        render_native_annotation_overlay_pixels_on_opaque_source(
                            &text_bitmap.rgba,
                            &base.rgba,
                            &content.rgba,
                            base.width,
                            base.height,
                            points,
                            &[shape.clone()],
                            0,
                        )
                    } else {
                        render_native_annotation_overlay_pixels(
                            &text_bitmap.rgba,
                            base.width,
                            base.height,
                            points,
                            &[shape.clone()],
                            0,
                        )
                    }
                    .unwrap()
                    .unwrap()
                } else {
                    let painted = if shape_color == [255, 226, 67] {
                        render_native_annotation_overlay_pixels_with_content(
                            &base.rgba,
                            &content.rgba,
                            base.width,
                            base.height,
                            points,
                            &[shape.clone()],
                            0,
                        )
                    } else {
                        render_native_annotation_overlay_pixels(
                            &base.rgba,
                            base.width,
                            base.height,
                            points,
                            &[shape.clone()],
                            0,
                        )
                    }
                    .unwrap()
                    .unwrap();
                    render_validated_glyph_scene_over_opaque(
                        &scene,
                        &painted.rgba,
                        base.width,
                        base.height,
                        clip,
                    )
                    .unwrap()
                };
                let saved = export_annotations(&bytes, &marks).unwrap();
                let expected = ParsedPdf::open(Arc::new(saved))
                    .unwrap()
                    .render_page(1, 1.0)
                    .unwrap();
                let wrong: Vec<_> = actual
                    .rgba
                    .iter()
                    .zip(&expected.rgba)
                    .enumerate()
                    .filter_map(|(at, (actual, expected))| {
                        (actual != expected).then_some((at, *actual, *expected))
                    })
                    .collect();
                assert!(
                    wrong.is_empty(),
                    "glyph/{shape_color:?} order {order}: {} mismatches, first {:?}",
                    wrong.len(),
                    &wrong[..wrong.len().min(12)]
                );
            }
        }
    }

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn dynlex_premultiplied_text_and_shape_order_matches_saved_appearances() {
        use opds_dynlex_backend::{
            render_native_annotation_overlay_pixels_over_premultiplied,
            render_validated_glyph_scene_over_premultiplied,
        };
        use opds_engine_probe::ParsedPdf;

        let mut source = Document::load_mem(include_bytes!(
            "../../dynlex-backend/tests/fixtures/simple-fill-stroke.pdf"
        ))
        .unwrap();
        let page_id = *source.get_pages().get(&1).unwrap();
        let contents_id = source
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .as_reference()
            .unwrap();
        let text = Annotation {
            id: 1001,
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
        let mut shape = Annotation {
            id: 1002,
            page: 0,
            bounds: Rect {
                x: 55.0,
                y: 40.0,
                width: 40.0,
                height: 30.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [231, 76, 60],
            },
        };
        let scene = crate::native_text_scene::stage_native_text_box_scene(&text).unwrap();
        for content in [
            b"".as_slice(),
            b"0.2 0.4 0.7 rg 40 55 55 45 re f".as_slice(),
        ] {
            source
                .get_object_mut(contents_id)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_plain_content(content.to_vec());
            let mut bytes = Vec::new();
            source.save_to(&mut bytes).unwrap();
            let parsed = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
            let base = parsed.render_page(1, 1.0).unwrap();
            let layer = parsed.render_page_content_layer(1, 1.0).unwrap();
            let points = (base.page_width_pt, base.page_height_pt);
            for shape_color in [[231, 76, 60], [255, 226, 67]] {
                shape.kind = Kind::Rectangle {
                    color_rgb: shape_color,
                };
                for order in 0..2 {
                    let marks = if order == 0 {
                        vec![text.clone(), shape.clone()]
                    } else {
                        vec![shape.clone(), text.clone()]
                    };
                    let painted = if order == 0 {
                        let glyphs = render_validated_glyph_scene_over_premultiplied(
                            &scene,
                            &layer.rgba,
                            layer.width,
                            layer.height,
                            [20, 30, 130, 88],
                        )
                        .unwrap();
                        render_native_annotation_overlay_pixels_over_premultiplied(
                            &glyphs.rgba,
                            layer.width,
                            layer.height,
                            points,
                            &[shape.clone()],
                            0,
                        )
                        .unwrap()
                        .unwrap()
                    } else {
                        let shape_layer =
                            render_native_annotation_overlay_pixels_over_premultiplied(
                                &layer.rgba,
                                layer.width,
                                layer.height,
                                points,
                                &[shape.clone()],
                                0,
                            )
                            .unwrap()
                            .unwrap();
                        render_validated_glyph_scene_over_premultiplied(
                            &scene,
                            &shape_layer.rgba,
                            layer.width,
                            layer.height,
                            [20, 30, 130, 88],
                        )
                        .unwrap()
                    };
                    let mut on_paper = painted.rgba;
                    for pixel in on_paper.chunks_exact_mut(4) {
                        let paper = 255 - pixel[3];
                        for channel in &mut pixel[..3] {
                            *channel = channel.saturating_add(paper);
                        }
                        pixel[3] = 255;
                    }
                    let saved = export_annotations(&bytes, &marks).unwrap();
                    let expected = ParsedPdf::open(Arc::new(saved))
                        .unwrap()
                        .render_page(1, 1.0)
                        .unwrap();
                    let wrong: Vec<_> = on_paper
                        .iter()
                        .zip(&expected.rgba)
                        .enumerate()
                        .filter_map(|(at, (actual, expected))| {
                            (actual != expected).then_some((at, *actual, *expected))
                        })
                        .collect();
                    assert!(
                        wrong.is_empty(),
                        "content={content:?}, shape={shape_color:?}, order={order}: {} mismatches, first {:?}",
                        wrong.len(),
                        &wrong[..wrong.len().min(12)]
                    );
                }
            }
        }
    }

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn dynlex_highlight_over_translucent_source_content_falls_back() {
        use opds_dynlex_backend::render_native_annotation_overlay_pixels_with_content;
        use opds_engine_probe::ParsedPdf;

        let mut source = Document::load_mem(&source_pdf()).unwrap();
        let page_id = *source.get_pages().get(&1).unwrap();
        let contents_id = source
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .as_reference()
            .unwrap();
        let gs_id = source.add_object(dictionary! {
            "Type" => "ExtGState", "ca" => 0.5,
        });
        let resources_id = source.add_object(dictionary! {
            "ExtGState" => dictionary! { "GS1" => gs_id },
        });
        source
            .get_object_mut(page_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Resources", resources_id);
        source
            .get_object_mut(contents_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_plain_content(b"0.2 0.4 0.7 rg /GS1 gs 0 0 200 300 re f".to_vec());
        let mut bytes = Vec::new();
        source.save_to(&mut bytes).unwrap();
        let mark = Annotation {
            id: 997,
            page: 0,
            bounds: Rect {
                x: 20.0,
                y: 30.0,
                width: 48.0,
                height: 36.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [255, 226, 67],
            },
        };
        let parsed = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        let base = parsed.render_page(1, 1.0).unwrap();
        let content_layer = parsed.render_page_content_layer(1, 1.0).unwrap();
        assert!(content_layer
            .rgba
            .chunks_exact(4)
            .any(|pixel| pixel[3] != 255));
        let actual = render_native_annotation_overlay_pixels_with_content(
            &base.rgba,
            &content_layer.rgba,
            base.width,
            base.height,
            (base.page_width_pt, base.page_height_pt),
            &[mark.clone()],
            0,
        )
        .unwrap()
        .is_none();
        let saved = export_annotations(&bytes, &[mark]).unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        assert!(actual, "translucent underlay must use the fallback");
        assert_ne!(base.rgba, expected.rgba);
    }

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn dynlex_native_annotations_match_on_large_rotated_crop_page() {
        use opds_dynlex_backend::render_native_annotation_overlay;
        use opds_engine_probe::ParsedPdf;

        let mut doc = Document::load_mem(&source_pdf()).unwrap();
        let page_id = *doc.get_pages().get(&1).unwrap();
        let parent_id = doc
            .get_object(page_id)
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Parent")
            .unwrap()
            .as_reference()
            .unwrap();
        doc.get_object_mut(parent_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set(
                "MediaBox",
                vec![0.into(), 0.into(), 1200.into(), 1400.into()],
            );
        let page = doc.get_object_mut(page_id).unwrap().as_dict_mut().unwrap();
        page.set(
            "CropBox",
            vec![100.into(), 200.into(), 1100.into(), 1300.into()],
        );
        page.set("Rotate", 90);
        let mut source = Vec::new();
        doc.save_to(&mut source).unwrap();
        let base = ParsedPdf::open(Arc::new(source.clone()))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        assert_eq!((base.width, base.height), (1100, 1000));
        let marks = [
            Annotation {
                id: 201,
                page: 0,
                bounds: Rect {
                    x: 75.0,
                    y: 80.0,
                    width: 92.0,
                    height: 64.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [0, 80, 240],
                },
            },
            Annotation {
                id: 202,
                page: 0,
                bounds: Rect {
                    x: 600.0,
                    y: 700.0,
                    width: 140.0,
                    height: 90.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [255, 226, 67],
                },
            },
        ];
        let dynlex = render_native_annotation_overlay(&base, &marks, 0)
            .unwrap()
            .expect("large rotated page is within the bounded native route");
        let saved = export_annotations(&source, &marks).unwrap();
        let hayro = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        assert_eq!(dynlex.rgba, hayro.rgba);
    }

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn native_text_appearance_can_be_staged_as_page_content() {
        use opds_dynlex_backend::flatten_native_free_text_appearance;
        use opds_engine_probe::ParsedPdf;

        let mut blank = Document::load_mem(&source_pdf()).unwrap();
        let page_id = *blank.get_pages().get(&1).unwrap();
        blank
            .get_object_mut(page_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .remove(b"Annots");
        let mut source = Vec::new();
        blank.save_to(&mut source).unwrap();
        assert!(flatten_native_free_text_appearance(&source, 1).is_err());
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
        let saved = export_annotations(&source, &[mark.clone()]).unwrap();
        let flattened = flatten_native_free_text_appearance(&saved, 1).unwrap();
        if let Some(directory) = std::env::var_os("OPDS_AP_FIXTURE_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            std::fs::write(directory.join("saved-freetext.pdf"), &saved).unwrap();
            std::fs::write(directory.join("flattened-freetext.pdf"), &flattened).unwrap();
        }
        let original = ParsedPdf::open(Arc::new(saved.clone()))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let base = ParsedPdf::open(Arc::new(source.clone()))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let live = crate::native_text_scene::render_opaque_page_text_box(
            &source,
            0,
            &base.rgba,
            base.width,
            base.height,
            (base.page_width_pt, base.page_height_pt),
            &mark,
        )
        .unwrap()
        .expect("the bounded Text Box should paint with DynLex");
        let mismatches = live
            .iter()
            .zip(&original.rgba)
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(mismatches, 0, "blank-page glyph composition differs");
        assert!(crate::native_text_scene::render_opaque_page_text_box(
            &saved,
            0,
            &base.rgba,
            base.width,
            base.height,
            (base.page_width_pt, base.page_height_pt),
            &mark,
        )
        .unwrap()
        .is_none());
        let mut transparent = base.rgba.clone();
        transparent[3] = 254;
        assert!(crate::native_text_scene::render_opaque_page_text_box(
            &source,
            0,
            &transparent,
            base.width,
            base.height,
            (base.page_width_pt, base.page_height_pt),
            &mark,
        )
        .unwrap()
        .is_none());
        let staged = ParsedPdf::open(Arc::new(flattened.clone()))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        assert_eq!(original.rgba, staged.rgba);
        let flattened_doc = Document::load_mem(&flattened).unwrap();
        let content = flattened_doc.get_page_content(page_id).unwrap();
        assert!(content.windows(2).any(|window| window == b"Tj"));
        assert!(flatten_native_free_text_appearance(&flattened, 1).is_err());
        let mut inherited_rotation = Document::load_mem(&saved).unwrap();
        let page_id = *inherited_rotation.get_pages().get(&1).unwrap();
        let parent_id = inherited_rotation
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Parent")
            .unwrap()
            .as_reference()
            .unwrap();
        inherited_rotation
            .get_object_mut(parent_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .set("Rotate", 90);
        inherited_rotation.trailer.remove(b"Prev");
        let mut rotated = Vec::new();
        inherited_rotation.save_to(&mut rotated).unwrap();
        assert!(flatten_native_free_text_appearance(&rotated, 1).is_err());
        assert!(crate::native_text_scene::render_opaque_page_text_box(
            &rotated,
            0,
            &base.rgba,
            base.width,
            base.height,
            (base.page_width_pt, base.page_height_pt),
            &mark,
        )
        .unwrap()
        .is_none());
        let mut unsupported_appearance = Document::load_mem(&saved).unwrap();
        let page_id = *unsupported_appearance.get_pages().get(&1).unwrap();
        let annotation_id = unsupported_appearance
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .as_reference()
            .unwrap();
        let form_id = unsupported_appearance
            .get_dictionary(annotation_id)
            .unwrap()
            .get(b"AP")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"N")
            .unwrap()
            .as_reference()
            .unwrap();
        unsupported_appearance
            .get_object_mut(form_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .content
            .extend_from_slice(b"\n0 0 m\n");
        unsupported_appearance.trailer.remove(b"Prev");
        let mut unsupported = Vec::new();
        unsupported_appearance.save_to(&mut unsupported).unwrap();
        assert!(flatten_native_free_text_appearance(&unsupported, 1).is_err());
    }

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn native_model_text_samples_match_printable_pdf_when_accepted() {
        use opds_engine_probe::ParsedPdf;

        let mut blank = Document::load_mem(&source_pdf()).unwrap();
        let page_id = *blank.get_pages().get(&1).unwrap();
        blank
            .get_object_mut(page_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .remove(b"Annots");
        let mut source = Vec::new();
        blank.save_to(&mut source).unwrap();
        let base = ParsedPdf::open(Arc::new(source.clone()))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let cases = [
            ("Hello", 16, TextAlign::Left, [0, 0, 0]),
            ("Café Ω", 16, TextAlign::Center, [0, 80, 240]),
            ("PDF Test", 12, TextAlign::Right, [255, 0, 0]),
            ("ЖЖЖ", 20, TextAlign::Center, [0, 128, 0]),
            ("123456", 24, TextAlign::Left, [75, 0, 130]),
            ("Open", 32, TextAlign::Right, [0, 0, 0]),
            ("μμμ", 20, TextAlign::Center, [255, 0, 0]),
            ("www", 24, TextAlign::Left, [0, 102, 204]),
            ("ABC", 16, TextAlign::Center, [0, 0, 0]),
            ("MMMM", 12, TextAlign::Right, [20, 80, 160]),
            ("iiii", 20, TextAlign::Left, [200, 20, 20]),
            ("789", 32, TextAlign::Center, [0, 0, 0]),
            ("ΩΣ", 24, TextAlign::Right, [30, 60, 90]),
            ("éé", 16, TextAlign::Left, [80, 30, 140]),
            ("ΩΩ\nЖЖ", 16, TextAlign::Center, [0, 80, 240]),
            ("A B\nC D", 16, TextAlign::Right, [40, 100, 20]),
            ("Hi\n\nΩΩ", 16, TextAlign::Left, [150, 20, 90]),
            ("Hello World", 16, TextAlign::Center, [20, 50, 90]),
        ];
        let mut accepted = 0;
        let mut rejected = Vec::new();
        for (index, (value, size, align, color_rgb)) in cases.into_iter().enumerate() {
            let mark = Annotation {
                id: index as u64 + 1,
                page: 0,
                bounds: Rect {
                    x: 10.0,
                    y: 28.0,
                    width: if value == "Hello World" { 60.0 } else { 180.0 },
                    height: 64.0,
                },
                kind: Kind::Text {
                    value: value.into(),
                    color_rgb,
                    font_size: size,
                    align,
                },
            };
            let saved = export_annotations(&source, &[mark.clone()]).unwrap();
            let hayro = ParsedPdf::open(Arc::new(saved))
                .unwrap()
                .render_page(1, 1.0)
                .unwrap();
            if let Err(error) = crate::native_text_scene::stage_native_text_box_scene(&mark) {
                rejected.push((value, error));
            }
            let rendered = crate::native_text_scene::render_opaque_page_text_box(
                &source,
                0,
                &base.rgba,
                base.width,
                base.height,
                (base.page_width_pt, base.page_height_pt),
                &mark,
            )
            .unwrap();
            if value.contains('\n') || value == "Hello World" {
                assert!(
                    rendered.is_some(),
                    "multi-line or wrapped case {value:?} fell back"
                );
            }
            if let Some(dynlex) = rendered {
                let differences: Vec<_> = dynlex
                    .iter()
                    .zip(&hayro.rgba)
                    .enumerate()
                    .filter_map(|(at, (actual, expected))| {
                        (actual != expected).then_some((at, *actual, *expected))
                    })
                    .collect();
                assert!(
                    differences.is_empty(),
                    "model text differs for {value:?}: {} bytes, first {:?}",
                    differences.len(),
                    &differences[..differences.len().min(16)]
                );
                accepted += 1;
            }
        }
        assert!(
            accepted >= 8,
            "too few text cases reached DynLex: {accepted}, rejected {rejected:?}"
        );
    }

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn page_content_alpha_distinguishes_blank_full_and_partial_paints() {
        use opds_engine_probe::ParsedPdf;

        let mut source = Document::load_mem(&source_pdf()).unwrap();
        let page_id = *source.get_pages().get(&1).unwrap();
        let contents_id = source
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .as_reference()
            .unwrap();
        source
            .get_object_mut(page_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .remove(b"Annots");
        let mark = Annotation {
            id: 145,
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
        let scene = crate::native_text_scene::stage_native_text_box_scene(&mark).unwrap();
        for (content, inside_y, inside, outside) in [
            (b"".as_slice(), 250_usize, 0_u8, 0_u8),
            (b"0.2 0.5 0.8 rg 0 0 200 300 re f".as_slice(), 250, 255, 255),
            (b"0.2 0.5 0.8 rg 20 20 100 100 re f".as_slice(), 250, 255, 0),
            (b"0.2 0.5 0.8 rg 20 245 100 55 re f".as_slice(), 30, 255, 0),
            (
                b"0.2 0.5 0.8 rg 20 245 100 55 re f 1 0 0 rg 50 230 30 70 re f".as_slice(),
                30,
                255,
                0,
            ),
        ] {
            source
                .get_object_mut(contents_id)
                .unwrap()
                .as_stream_mut()
                .unwrap()
                .set_plain_content(content.to_vec());
            let mut bytes = Vec::new();
            source.save_to(&mut bytes).unwrap();
            let parsed = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
            let visible = parsed.render_page(1, 1.0).unwrap();
            let layer = parsed.render_page_content_layer(1, 1.0).unwrap();
            assert_eq!((visible.width, visible.height), (layer.width, layer.height));
            assert_eq!(visible.rgba[3], 255);
            assert_eq!(
                layer.rgba[(inside_y * 200 + 50) * 4 + 3],
                inside,
                "{content:?}"
            );
            assert_eq!(layer.rgba[(5 * 200 + 5) * 4 + 3], outside, "{content:?}");
            let reconstructed: Vec<_> = layer
                .rgba
                .chunks_exact(4)
                .flat_map(|pixel| {
                    let paper = 255 - pixel[3];
                    [
                        pixel[0].saturating_add(paper),
                        pixel[1].saturating_add(paper),
                        pixel[2].saturating_add(paper),
                        255,
                    ]
                })
                .collect();
            let base_differences = reconstructed
                .iter()
                .zip(&visible.rgba)
                .filter(|(actual, expected)| actual != expected)
                .count();
            assert_eq!(
                base_differences, 0,
                "source layer reconstruction for {content:?}"
            );
            let saved = export_annotations(&bytes, &[mark.clone()]).unwrap();
            let expected = ParsedPdf::open(Arc::new(saved))
                .unwrap()
                .render_page(1, 1.0)
                .unwrap();
            let combined = opds_dynlex_backend::render_validated_glyph_scene_over_premultiplied(
                &scene,
                &layer.rgba,
                layer.width,
                layer.height,
                [20, 30, 130, 88],
            )
            .unwrap();
            let mut paper = combined.rgba;
            for pixel in paper.chunks_exact_mut(4) {
                let remaining = 255 - pixel[3];
                for channel in &mut pixel[..3] {
                    *channel = channel.saturating_add(remaining);
                }
                pixel[3] = 255;
            }
            let mismatches = paper
                .iter()
                .zip(&expected.rgba)
                .filter(|(actual, expected)| actual != expected)
                .count();
            assert_eq!(mismatches, 0, "content-alpha composition for {content:?}");
            let live = crate::native_text_scene::render_mixed_page_text_box(
                &bytes,
                0,
                &visible.rgba,
                &layer.rgba,
                visible.width,
                visible.height,
                (visible.page_width_pt, visible.page_height_pt),
                &mark,
            )
            .unwrap();
            if outside == 0 && inside == 255 {
                let actual = live.expect("bounded partial paint must reach DynLex");
                let differences = actual
                    .iter()
                    .zip(&expected.rgba)
                    .filter(|(actual, expected)| actual != expected)
                    .count();
                assert_eq!(differences, 0, "mixed route for {content:?}");
            } else {
                assert!(live.is_none());
            }
        }
        source
            .get_object_mut(contents_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_plain_content(b"0 0 200 300 re W n".to_vec());
        let mut unsupported = Vec::new();
        source.save_to(&mut unsupported).unwrap();
        assert!(!crate::native_text_scene::eligible_mixed_page(
            &unsupported,
            0,
            200,
            300,
        ));
    }

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn mixed_page_text_on_rotated_crop_matches_saved_appearance() {
        use opds_engine_probe::ParsedPdf;

        let mut source = Document::load_mem(&source_pdf()).unwrap();
        let page_id = *source.get_pages().get(&1).unwrap();
        let contents_id = source
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .as_reference()
            .unwrap();
        let page = source
            .get_object_mut(page_id)
            .unwrap()
            .as_dict_mut()
            .unwrap();
        page.remove(b"Annots");
        page.set(
            "CropBox",
            vec![40.into(), 60.into(), 200.into(), 260.into()],
        );
        page.set("Rotate", 90);
        source
            .get_object_mut(contents_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_plain_content(b"0.2 0.5 0.8 rg 50 160 100 100 re f".to_vec());
        let mut source_bytes = Vec::new();
        source.save_to(&mut source_bytes).unwrap();
        let parsed = ParsedPdf::open(Arc::new(source_bytes.clone())).unwrap();
        let base = parsed.render_page(1, 1.0).unwrap();
        let content = parsed.render_page_content_layer(1, 1.0).unwrap();
        let (single_pass, single_pass_content) =
            crate::render_page_with_content_layer(&parsed, 1, 1.0).unwrap();
        assert_eq!(single_pass.raster.rgba, base.rgba);
        assert_eq!(single_pass_content.rgba, content.rgba);
        assert_eq!((base.width, base.height), (200, 160));
        let mark = Annotation {
            id: 146,
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
        let live = crate::native_text_scene::render_mixed_page_text_box(
            &source_bytes,
            0,
            &base.rgba,
            &content.rgba,
            base.width,
            base.height,
            (base.page_width_pt, base.page_height_pt),
            &mark,
        )
        .unwrap()
        .expect("rotated mixed page should reach DynLex");
        let saved = export_annotations(&source_bytes, &[mark.clone()]).unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let mismatches = live
            .iter()
            .zip(&expected.rgba)
            .filter(|(actual, expected)| actual != expected)
            .count();
        assert_eq!(mismatches, 0);

        let shape = Annotation {
            id: 147,
            page: 0,
            bounds: Rect {
                x: 55.0,
                y: 40.0,
                width: 40.0,
                height: 30.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [255, 226, 67],
            },
        };
        let later = Annotation {
            id: 148,
            page: 0,
            bounds: Rect {
                x: 45.0,
                y: 35.0,
                width: 110.0,
                height: 58.0,
            },
            kind: Kind::Text {
                value: "Hello Ω".into(),
                color_rgb: [231, 76, 60],
                font_size: 16,
                align: TextAlign::Right,
            },
        };
        let first = opds_dynlex_backend::render_validated_glyph_scene_over_premultiplied(
            &crate::native_text_scene::stage_native_text_box_scene(&mark).unwrap(),
            &content.rgba,
            base.width,
            base.height,
            [20, 30, 130, 88],
        )
        .unwrap();
        let middle =
            opds_dynlex_backend::render_native_annotation_overlay_pixels_over_premultiplied(
                &first.rgba,
                base.width,
                base.height,
                (base.page_width_pt, base.page_height_pt),
                &[shape.clone()],
                0,
            )
            .unwrap()
            .unwrap();
        let mut actual = opds_dynlex_backend::render_validated_glyph_scene_over_premultiplied(
            &crate::native_text_scene::stage_native_text_box_scene(&later).unwrap(),
            &middle.rgba,
            base.width,
            base.height,
            [45, 35, 155, 93],
        )
        .unwrap()
        .rgba;
        for pixel in actual.chunks_exact_mut(4) {
            let paper = 255 - pixel[3];
            for channel in &mut pixel[..3] {
                *channel = channel.saturating_add(paper);
            }
            pixel[3] = 255;
        }
        let saved = export_annotations(&source_bytes, &[mark, shape, later]).unwrap();
        let expected = ParsedPdf::open(Arc::new(saved))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        assert_eq!(actual, expected.rgba, "rotated two-text/shape order");
    }

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn colored_page_text_direct_dynlex_matches_saved_appearance() {
        use opds_engine_probe::ParsedPdf;

        let mut doc = Document::load_mem(&source_pdf()).unwrap();
        let page_id = *doc.get_pages().get(&1).unwrap();
        let page = doc.get_dictionary(page_id).unwrap();
        let contents_id = page.get(b"Contents").unwrap().as_reference().unwrap();
        doc.get_object_mut(page_id)
            .unwrap()
            .as_dict_mut()
            .unwrap()
            .remove(b"Annots");
        doc.get_object_mut(contents_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_plain_content(b"0.2 0.5 0.8 rg 0 0 200 300 re f".to_vec());
        let mut source = Vec::new();
        doc.save_to(&mut source).unwrap();
        let base = ParsedPdf::open(Arc::new(source.clone()))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let mark = Annotation {
            id: 11,
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
        let saved = export_annotations(&source, &[mark.clone()]).unwrap();
        let expected = ParsedPdf::open(Arc::new(saved.clone()))
            .unwrap()
            .render_page(1, 1.0)
            .unwrap();
        let scene = crate::native_text_scene::stage_native_text_box_scene(&mark).unwrap();
        let layer = opds_dynlex_backend::render_validated_glyph_layer(&scene).unwrap();
        if let Some(directory) = std::env::var_os("OPDS_COLORED_TEXT_FIXTURE_DIR") {
            let directory = std::path::PathBuf::from(directory);
            std::fs::create_dir_all(&directory).unwrap();
            for (name, bytes) in [
                ("source.pdf", source.as_slice()),
                ("saved.pdf", saved.as_slice()),
                ("scene.opgs", scene.as_slice()),
                ("base.rgba", base.rgba.as_slice()),
                ("hayro.rgba", expected.rgba.as_slice()),
                ("dynlex-layer.rgba", layer.rgba.as_slice()),
            ] {
                std::fs::write(directory.join(name), bytes).unwrap();
            }
        }
        let live = crate::native_text_scene::render_opaque_page_text_box(
            &source,
            0,
            &base.rgba,
            base.width,
            base.height,
            (base.page_width_pt, base.page_height_pt),
            &mark,
        )
        .unwrap()
        .expect("opaque colored page should use direct DynLex glyph composition");
        let mismatches = live
            .iter()
            .zip(&expected.rgba)
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(mismatches, 0, "opaque-page direct composition differs");
        let mut differing_channels = 0_usize;
        let mut direct_differences = 0_usize;
        for y in 0..192_usize {
            for x in 0..200_usize {
                let at = (y * 200 + x) * 4;
                let tile_at = (y * 256 + x) * 4;
                let alpha = u32::from(layer.rgba[tile_at + 3]);
                for channel in 0..3 {
                    let bg = u32::from(base.rgba[at + channel]);
                    let fg = u32::from(layer.rgba[tile_at + channel]);
                    let attenuation = bg * (255 - alpha);
                    // Reblending a precomposed glyph group produces a
                    // different 8-bit result than painting the PDF glyphs
                    // directly over the page in appearance order.
                    let candidate = (fg + (attenuation + 255) / 256).min(255) as u8;
                    if candidate != expected.rgba[at + channel] {
                        differing_channels += 1;
                    }
                    let direct = ((u32::from([0_u8, 80, 240][channel]) * alpha + attenuation + 255)
                        >> 8) as u8;
                    if direct != expected.rgba[at + channel] {
                        direct_differences += 1;
                    }
                }
            }
        }
        assert_eq!(differing_channels, 207);
        assert_eq!(direct_differences, 0);

        // A visible opaque bitmap does not prove that the PDF content itself
        // covers the page. Do not apply the direct path to partial paints or
        // extra graphics operations; the Iced annotation path handles them.
        let mut partial = Document::load_mem(&source).unwrap();
        partial
            .get_object_mut(contents_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .set_plain_content(b"0.2 0.5 0.8 rg 20 20 100 100 re f".to_vec());
        let mut partial_bytes = Vec::new();
        partial.save_to(&mut partial_bytes).unwrap();
        assert!(crate::native_text_scene::render_opaque_page_text_box(
            &partial_bytes,
            0,
            &base.rgba,
            base.width,
            base.height,
            (base.page_width_pt, base.page_height_pt),
            &mark,
        )
        .unwrap()
        .is_none());
    }

    #[cfg(feature = "dynlex-experimental")]
    #[test]
    fn native_text_on_rotated_cropped_pages_matches_saved_appearance() {
        use opds_engine_probe::ParsedPdf;

        for rotation in [0, 90, 180, 270] {
            let mut doc = Document::load_mem(&source_pdf()).unwrap();
            let page_id = *doc.get_pages().get(&1).unwrap();
            let parent_id = doc
                .get_dictionary(page_id)
                .unwrap()
                .get(b"Parent")
                .unwrap()
                .as_reference()
                .unwrap();
            let parent = doc
                .get_object_mut(parent_id)
                .unwrap()
                .as_dict_mut()
                .unwrap();
            parent.set(
                "MediaBox",
                vec![20.into(), 30.into(), 220.into(), 330.into()],
            );
            if rotation == 180 {
                parent.set(
                    "CropBox",
                    vec![40.into(), 60.into(), 200.into(), 260.into()],
                );
            }
            if rotation == 270 {
                parent.set("Rotate", rotation);
            }
            let page = doc.get_object_mut(page_id).unwrap().as_dict_mut().unwrap();
            page.remove(b"Annots");
            if rotation != 180 {
                page.set(
                    "CropBox",
                    vec![40.into(), 60.into(), 200.into(), 260.into()],
                );
            }
            if rotation != 270 {
                page.set("Rotate", rotation);
            }
            let mut source = Vec::new();
            doc.save_to(&mut source).unwrap();
            let base = ParsedPdf::open(Arc::new(source.clone()))
                .unwrap()
                .render_page(1, 1.0)
                .unwrap();
            let mark = Annotation {
                id: 100 + rotation as u64,
                page: 0,
                bounds: Rect {
                    x: 10.0,
                    y: 28.0,
                    width: 140.0,
                    height: 64.0,
                },
                kind: Kind::Text {
                    value: "ΩΩ\nЖЖ".into(),
                    color_rgb: [0, 80, 240],
                    font_size: 16,
                    align: TextAlign::Center,
                },
            };
            let saved = export_annotations(&source, &[mark.clone()]).unwrap();
            let expected = ParsedPdf::open(Arc::new(saved))
                .unwrap()
                .render_page(1, 1.0)
                .unwrap();
            let actual = crate::native_text_scene::render_opaque_page_text_box(
                &source,
                0,
                &base.rgba,
                base.width,
                base.height,
                (base.page_width_pt, base.page_height_pt),
                &mark,
            )
            .unwrap()
            .expect("white rotated/CropBox page must use DynLex");
            let mismatches = actual
                .iter()
                .zip(&expected.rgba)
                .filter(|(a, b)| a != b)
                .count();
            assert_eq!(mismatches, 0, "text differs at /Rotate {rotation}");
        }
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
                    color_rgb: [0, 0, 0],
                    font_size: 16,
                    align: TextAlign::Left,
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
                value: "漢".into(),
                color_rgb: [0, 0, 0],
                font_size: 16,
                align: TextAlign::Left,
            },
        };
        assert!(export_annotations(&source, &[item.clone()])
            .unwrap_err()
            .contains("not supported"));
        item.kind = Kind::Text {
            value: "A".into(),
            color_rgb: [0, 0, 0],
            font_size: 0,
            align: TextAlign::Left,
        };
        assert!(export_annotations(&source, &[item.clone()])
            .unwrap_err()
            .contains("Unsupported text size"));
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

    #[test]
    fn reverse_print_flattens_nested_page_tree_and_preserves_inheritance() {
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
        let second = doc.add_object(doc.get_object(first).unwrap().clone());
        let third = doc.add_object(doc.get_object(first).unwrap().clone());
        let nested = doc.add_object(dictionary! {
            "Type" => "Pages", "Parent" => pages_id,
            "Kids" => vec![Object::Reference(first), Object::Reference(second), Object::Reference(third)],
            "Count" => 3,
        });
        doc.get_dictionary_mut(pages_id)
            .unwrap()
            .set("Kids", vec![Object::Reference(nested)]);
        doc.get_dictionary_mut(pages_id)
            .unwrap()
            .set("Count", 3_i64);
        doc.get_dictionary_mut(pages_id)
            .unwrap()
            .set("Resources", Dictionary::new());
        for id in [first, second, third] {
            doc.get_dictionary_mut(id)
                .unwrap()
                .set("Parent", Object::Reference(nested));
        }
        let mut source = Vec::new();
        doc.save_to(&mut source).unwrap();
        let highlight = Annotation {
            id: 7,
            page: 2,
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 40.0,
                height: 20.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [255, 226, 67],
            },
        };
        let bytes = export_print_pages(&source, &[highlight], &[2, 0]).unwrap();
        let output = Document::load_mem(&bytes).unwrap();
        assert_eq!(
            output.get_pages().into_values().collect::<Vec<_>>(),
            vec![third, first]
        );
        for page in output.get_pages().into_values() {
            let dict = output.get_dictionary(page).unwrap();
            assert_eq!(dict.get(b"MediaBox").unwrap().as_array().unwrap().len(), 4);
            assert!(dict.get(b"Resources").unwrap().as_dict().is_ok());
        }
        let first_printed = *output.get_pages().values().next().unwrap();
        let annots = output.get_page_annotations(first_printed).unwrap();
        assert_eq!(annots.len(), 2);
        assert_eq!(
            annots[1].get(b"Subtype").unwrap().as_name().unwrap(),
            b"Highlight"
        );
        let parsed = opds_engine_probe::ParsedPdf::open(std::sync::Arc::new(bytes)).unwrap();
        assert_eq!(parsed.page_count(), 2);
    }

    #[test]
    fn page_zoom_centers_contents_and_highlight_without_changing_sheet_size() {
        let source = source_pdf();
        let highlight = Annotation {
            id: 4,
            page: 0,
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 40.0,
                height: 20.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [255, 226, 67],
            },
        };
        let annotated = export_annotations(&source, &[highlight]).unwrap();
        assert_eq!(
            apply_print_placement(&annotated, PrintPaper::Original, PrintScale::Custom, 100)
                .unwrap(),
            annotated
        );
        assert!(
            apply_print_placement(&annotated, PrintPaper::Original, PrintScale::Custom, 9).is_err()
        );
        let zoomed =
            apply_print_placement(&annotated, PrintPaper::Original, PrintScale::Custom, 80)
                .unwrap();
        let doc = Document::load_mem(&zoomed).unwrap();
        let page = *doc.get_pages().values().next().unwrap();
        assert_eq!(page_size(&doc, page).unwrap(), [200.0, 300.0]);
        let annotations = doc.get_page_annotations(page).unwrap();
        let rect = box_values(annotations[1].get(b"Rect").unwrap()).unwrap();
        for (actual, expected) in rect.into_iter().zip([28.0, 238.0, 60.0, 254.0]) {
            assert!((actual - expected).abs() < 0.001);
        }
        let quads = annotations[1]
            .get(b"QuadPoints")
            .unwrap()
            .as_array()
            .unwrap();
        assert!((numeric(&quads[0]).unwrap() - 28.0).abs() < 0.001);
        let parsed = opds_engine_probe::ParsedPdf::open(std::sync::Arc::new(zoomed)).unwrap();
        assert_eq!(parsed.page_count(), 1);
    }

    #[test]
    fn fit_and_shrink_place_pages_and_annotations_on_letter() {
        let source = source_pdf();
        let mark = Annotation {
            id: 4,
            page: 0,
            bounds: Rect {
                x: 10.0,
                y: 20.0,
                width: 40.0,
                height: 20.0,
            },
            kind: Kind::Rectangle {
                color_rgb: [255, 226, 67],
            },
        };
        let annotated = export_annotations(&source, &[mark]).unwrap();
        let fit =
            apply_print_placement(&annotated, PrintPaper::Letter, PrintScale::Fit, 100).unwrap();
        let shrink =
            apply_print_placement(&annotated, PrintPaper::Letter, PrintScale::Shrink, 100).unwrap();
        let fit_doc = Document::load_mem(&fit).unwrap();
        let shrink_doc = Document::load_mem(&shrink).unwrap();
        let fit_page = *fit_doc.get_pages().values().next().unwrap();
        let shrink_page = *shrink_doc.get_pages().values().next().unwrap();
        assert_eq!(page_size(&fit_doc, fit_page).unwrap(), [612.0, 792.0]);
        assert_eq!(page_size(&shrink_doc, shrink_page).unwrap(), [612.0, 792.0]);
        let fit_rect = box_values(
            fit_doc.get_page_annotations(fit_page).unwrap()[1]
                .get(b"Rect")
                .unwrap(),
        )
        .unwrap();
        let shrink_rect = box_values(
            shrink_doc.get_page_annotations(shrink_page).unwrap()[1]
                .get(b"Rect")
                .unwrap(),
        )
        .unwrap();
        // Fit enlarges 200×300 to 528×792, centered; Shrink leaves it 1:1.
        for (actual, expected) in fit_rect.into_iter().zip([68.4, 686.4, 174.0, 739.2]) {
            assert!((actual - expected).abs() < 0.01);
        }
        for (actual, expected) in shrink_rect.into_iter().zip([216.0, 506.0, 256.0, 526.0]) {
            assert!((actual - expected).abs() < 0.01);
        }
    }

    #[test]
    fn placement_preserves_quarter_turn_and_rebases_crop_coordinates() {
        let mut source = Document::load_mem(&source_pdf()).unwrap();
        let page_id = *source.get_pages().values().next().unwrap();
        let annot_id = source
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_reference()
            .and_then(|id| source.get_object(id))
            .unwrap()
            .as_array()
            .unwrap()[0]
            .as_reference()
            .unwrap();
        let page = source.get_dictionary_mut(page_id).unwrap();
        page.set("MediaBox", array(&[20.0, 30.0, 220.0, 330.0]));
        page.set("CropBox", array(&[40.0, 60.0, 200.0, 300.0]));
        page.set("Rotate", 90);
        let annot = source.get_dictionary_mut(annot_id).unwrap();
        annot.set("Rect", array(&[50.0, 70.0, 90.0, 110.0]));
        annot.set(
            "QuadPoints",
            array(&[50.0, 110.0, 90.0, 110.0, 50.0, 70.0, 90.0, 70.0]),
        );
        let mut bytes = Vec::new();
        source.save_to(&mut bytes).unwrap();
        let result = apply_print_placement(&bytes, PrintPaper::A4, PrintScale::Fit, 100).unwrap();
        let output = Document::load_mem(&result).unwrap();
        let page_id = *output.get_pages().values().next().unwrap();
        let (crop, rotation) = print_page_geometry(&output, page_id).unwrap();
        assert_eq!(rotation, 90);
        assert!((crop[2] - 595.2756).abs() < 0.01);
        assert!((crop[3] - 841.8898).abs() < 0.01);
        let fit = (595.2756_f64 / 160.0).min(841.8898 / 240.0);
        let tx = (595.2756 - 160.0 * fit) / 2.0;
        let ty = (841.8898 - 240.0 * fit) / 2.0;
        let annotation = output.get_page_annotations(page_id).unwrap()[0];
        let rect = box_values(annotation.get(b"Rect").unwrap()).unwrap();
        for (actual, expected) in rect.into_iter().zip([
            10.0 * fit + tx,
            10.0 * fit + ty,
            50.0 * fit + tx,
            50.0 * fit + ty,
        ]) {
            assert!((actual - expected).abs() < 0.01);
        }
        let quad = annotation.get(b"QuadPoints").unwrap().as_array().unwrap();
        assert!((numeric(&quad[0]).unwrap() - (10.0 * fit + tx)).abs() < 0.01);
        assert!((numeric(&quad[1]).unwrap() - (50.0 * fit + ty)).abs() < 0.01);
        assert_eq!(
            opds_engine_probe::ParsedPdf::open(std::sync::Arc::new(result))
                .unwrap()
                .page_count(),
            1
        );
    }

    #[test]
    fn placement_keeps_each_supported_page_rotation() {
        for rotation in [0, 90, 180, 270] {
            let mut source = Document::load_mem(&source_pdf()).unwrap();
            let page = *source.get_pages().values().next().unwrap();
            source
                .get_dictionary_mut(page)
                .unwrap()
                .set("Rotate", rotation);
            let mut bytes = Vec::new();
            source.save_to(&mut bytes).unwrap();
            let result =
                apply_print_placement(&bytes, PrintPaper::A4, PrintScale::Fit, 100).unwrap();
            let output = Document::load_mem(&result).unwrap();
            let page = *output.get_pages().values().next().unwrap();
            assert_eq!(print_page_geometry(&output, page).unwrap().1, rotation);
            assert_eq!(
                opds_engine_probe::ParsedPdf::open(std::sync::Arc::new(result))
                    .unwrap()
                    .page_count(),
                1
            );
        }
    }

    #[test]
    fn save_maps_native_highlight_and_text_on_rotated_crop() {
        let mut source = Document::load_mem(&source_pdf()).unwrap();
        let page = *source.get_pages().values().next().unwrap();
        let page_dict = source.get_dictionary_mut(page).unwrap();
        page_dict.set("MediaBox", array(&[20.0, 30.0, 220.0, 330.0]));
        page_dict.set("CropBox", array(&[40.0, 60.0, 200.0, 300.0]));
        page_dict.set("Rotate", 90);
        let mut source_bytes = Vec::new();
        source.save_to(&mut source_bytes).unwrap();
        let annotations = [
            Annotation {
                id: 11,
                page: 0,
                bounds: Rect {
                    x: 20.0,
                    y: 30.0,
                    width: 50.0,
                    height: 15.0,
                },
                kind: Kind::Rectangle {
                    color_rgb: [255, 226, 67],
                },
            },
            Annotation {
                id: 12,
                page: 0,
                bounds: Rect {
                    x: 60.0,
                    y: 40.0,
                    width: 100.0,
                    height: 20.0,
                },
                kind: Kind::Text {
                    value: "Rotated".into(),
                    color_rgb: [0, 0, 0],
                    font_size: 16,
                    align: TextAlign::Left,
                },
            },
        ];
        let saved = export_annotations(&source_bytes, &annotations).unwrap();
        assert!(saved.starts_with(&source_bytes));
        let output = Document::load_mem(&saved).unwrap();
        let page = *output.get_pages().values().next().unwrap();
        let annots = output.get_page_annotations(page).unwrap();
        assert_eq!(annots.len(), 3);
        let highlight_rect = box_values(annots[1].get(b"Rect").unwrap()).unwrap();
        assert_eq!(highlight_rect, [70.0, 80.0, 85.0, 130.0]);
        let text_rect = box_values(annots[2].get(b"Rect").unwrap()).unwrap();
        assert_eq!(text_rect, [80.0, 120.0, 100.0, 220.0]);
        let text_ap = annots[2]
            .get(b"AP")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"N")
            .unwrap()
            .as_reference()
            .unwrap();
        let stream = output.get_object(text_ap).unwrap().as_stream().unwrap();
        assert_eq!(
            box_values(stream.dict.get(b"BBox").unwrap()).unwrap(),
            [0.0, 0.0, 20.0, 100.0]
        );
        assert!(stream.content.starts_with(b"q 0 1 -1 0 20.00000000 0 cm"));
        assert_eq!(
            opds_engine_probe::ParsedPdf::open(std::sync::Arc::new(saved))
                .unwrap()
                .page_count(),
            1
        );
    }

    #[test]
    fn save_multiline_unicode_text_clips_to_box_on_rotated_page() {
        let mut source = Document::load_mem(&source_pdf()).unwrap();
        let page = *source.get_pages().values().next().unwrap();
        source.get_dictionary_mut(page).unwrap().set("Rotate", 90);
        let mut source_bytes = Vec::new();
        source.save_to(&mut source_bytes).unwrap();
        let value = "One\nCafé Ω\nThree";
        let saved = export_annotations(
            &source_bytes,
            &[Annotation {
                id: 13,
                page: 0,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 100.0,
                    height: 80.0,
                },
                kind: Kind::Text {
                    value: value.into(),
                    color_rgb: [0, 102, 204],
                    font_size: 24,
                    align: TextAlign::Center,
                },
            }],
        )
        .unwrap();
        let output = Document::load_mem(&saved).unwrap();
        let page = *output.get_pages().values().next().unwrap();
        let annots = output.get_page_annotations(page).unwrap();
        let contents = annots[1].get(b"Contents").unwrap().as_str().unwrap();
        assert!(contents.starts_with(&[0xFE, 0xFF]));
        assert_eq!(contents.len(), 2 + value.encode_utf16().count() * 2);
        assert_eq!(annots[1].get(b"Q").unwrap().as_i64().unwrap(), 1);
        assert_eq!(annots[1].get(b"C").unwrap().as_array().unwrap().len(), 3);
        assert!(
            std::str::from_utf8(annots[1].get(b"DA").unwrap().as_str().unwrap())
                .unwrap()
                .contains("0.000 0.400 0.800 rg")
        );
        let ap_id = annots[1]
            .get(b"AP")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"N")
            .unwrap()
            .as_reference()
            .unwrap();
        let content = std::str::from_utf8(
            &output
                .get_object(ap_id)
                .unwrap()
                .as_stream()
                .unwrap()
                .content,
        )
        .unwrap();
        assert!(content.starts_with("q 0 1 -1 0 80.00000000 0 cm\n"));
        assert!(content.contains("q 0 0 100.000 80.000 re W n"));
        assert!(content.contains("/OPDS0 24 Tf"));
        assert!(content.contains("0.000 0.400 0.800 rg"));
        assert_eq!(content.matches(" Tj").count(), 3);
        let face = font_face().unwrap();
        for (line, baseline) in value.split('\n').zip([56.0_f64, 28.0, 0.0]) {
            let (_, line_width) = encoded_line(&face, line, 24.0).unwrap();
            let x = (100.0 - line_width) / 2.0;
            assert!(content.contains(&format!("1 0 0 1 {x:.3} {baseline:.3} Tm")));
        }
        assert_eq!(
            opds_engine_probe::ParsedPdf::open(std::sync::Arc::new(saved))
                .unwrap()
                .page_count(),
            1
        );
    }

    #[test]
    fn wrapped_unicode_text_keeps_contents_and_printable_line_positions() {
        let mut source = Document::load_mem(&source_pdf()).unwrap();
        let page = *source.get_pages().values().next().unwrap();
        source.get_dictionary_mut(page).unwrap().set("Rotate", 90);
        let mut source_bytes = Vec::new();
        source.save_to(&mut source_bytes).unwrap();
        let value = "Café Ω longword voorbeeld en nog meer tekst";
        let saved = export_annotations(
            &source_bytes,
            &[Annotation {
                id: 14,
                page: 0,
                bounds: Rect {
                    x: 10.0,
                    y: 20.0,
                    width: 130.0,
                    height: 150.0,
                },
                kind: Kind::Text {
                    value: value.into(),
                    color_rgb: [0, 0, 0],
                    font_size: 24,
                    align: TextAlign::Right,
                },
            }],
        )
        .unwrap();
        let output = Document::load_mem(&saved).unwrap();
        let page = *output.get_pages().values().next().unwrap();
        let annotation = output.get_page_annotations(page).unwrap()[1];
        assert_eq!(annotation.get(b"Q").unwrap().as_i64().unwrap(), 2);
        let contents = annotation.get(b"Contents").unwrap().as_str().unwrap();
        assert_eq!(contents.len(), 2 + value.encode_utf16().count() * 2);
        let ap_id = annotation
            .get(b"AP")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"N")
            .unwrap()
            .as_reference()
            .unwrap();
        let appearance = std::str::from_utf8(
            &output
                .get_object(ap_id)
                .unwrap()
                .as_stream()
                .unwrap()
                .content,
        )
        .unwrap();
        let face = font_face().unwrap();
        let lines = text_layout::wrap_text(value, 130.0, 24.0, &face);
        assert_eq!(lines.len(), 5);
        assert_eq!(appearance.matches(" Tj").count(), 5);
        for (index, line) in lines.iter().enumerate() {
            let x = 130.0 - line.width;
            let baseline = 150.0 - 24.0 - index as f64 * 28.0;
            assert!(appearance.contains(&format!("1 0 0 1 {x:.3} {baseline:.3} Tm")));
        }
    }

    #[test]
    fn save_maps_all_quarter_turns_into_pdf_page_space() {
        for (rotation, expected) in [
            (0, [60.0, 255.0, 110.0, 270.0]),
            (90, [70.0, 80.0, 85.0, 130.0]),
            (180, [130.0, 90.0, 180.0, 105.0]),
            (270, [155.0, 230.0, 170.0, 280.0]),
        ] {
            let mut source = Document::load_mem(&source_pdf()).unwrap();
            let page = *source.get_pages().values().next().unwrap();
            let page_dict = source.get_dictionary_mut(page).unwrap();
            page_dict.set("MediaBox", array(&[20.0, 30.0, 220.0, 330.0]));
            page_dict.set("CropBox", array(&[40.0, 60.0, 200.0, 300.0]));
            page_dict.set("Rotate", rotation);
            let mut source_bytes = Vec::new();
            source.save_to(&mut source_bytes).unwrap();
            let saved = export_annotations(
                &source_bytes,
                &[Annotation {
                    id: 20,
                    page: 0,
                    bounds: Rect {
                        x: 20.0,
                        y: 30.0,
                        width: 50.0,
                        height: 15.0,
                    },
                    kind: Kind::Rectangle {
                        color_rgb: [255, 226, 67],
                    },
                }],
            )
            .unwrap();
            let output = Document::load_mem(&saved).unwrap();
            let page = *output.get_pages().values().next().unwrap();
            let annots = output.get_page_annotations(page).unwrap();
            assert_eq!(
                box_values(annots[1].get(b"Rect").unwrap()).unwrap(),
                expected
            );
        }
    }
}
