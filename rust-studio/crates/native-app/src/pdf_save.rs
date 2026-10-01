//! Pure-Rust, incremental PDF export for the native annotation overlay.
//! The original bytes remain intact. New annotation appearances are mapped
//! through the visible CropBox and quarter-turn page rotation.
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
fn print_page_geometry(doc: &Document, page_id: ObjectId) -> Result<([f64; 4], u16), String> {
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
