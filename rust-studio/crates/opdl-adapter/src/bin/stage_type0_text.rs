//! Read-only, bounded Type0/Identity-H glyph mapping and positioning probe.
//! This does not render PDF text or change the normal DynLex adapter.
#![forbid(unsafe_code)]

mod quad_glyph;

use lopdf::{content::Content, Dictionary, Document, Object, Stream};
use std::{
    collections::{BTreeMap, HashSet},
    env,
    error::Error,
    fs::File,
    io::{self, Read},
    path::Path,
};

const MAX_PDF_BYTES: usize = 16_000_000;
const MAX_FONT_BYTES: usize = 8_000_000;
const MAX_TOTAL_FONT_BYTES: usize = 16_000_000;
const MAX_FONTS: usize = 32;
const MAX_GLYPHS: usize = 65_536;
const FONT_NAME_BYTES: usize = 32;
const GLYPH_RECORD_BYTES: usize = FONT_NAME_BYTES + 4 + 6 * 8;
const HEADER_BYTES: usize = 16;

fn fail(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(fail("input exceeds byte limit").into());
    }
    Ok(bytes)
}

fn number(value: &Object) -> Result<f64, Box<dyn Error>> {
    let value = match value {
        Object::Integer(value) => *value as f64,
        Object::Real(value) => f64::from(*value),
        _ => return Err(fail("expected PDF number").into()),
    };
    if !value.is_finite() || value.abs() > 1_000_000.0 {
        return Err(fail("unbounded PDF number").into());
    }
    Ok(value)
}

fn endpoint_channel(value: &Object) -> Option<u8> {
    match number(value).ok()? {
        0.0 => Some(0),
        1.0 => Some(255),
        _ => None,
    }
}

fn six(operands: &[Object]) -> Result<[f64; 6], Box<dyn Error>> {
    let [a, b, c, d, e, f] = operands else {
        return Err(fail("matrix needs six numbers").into());
    };
    Ok([
        number(a)?,
        number(b)?,
        number(c)?,
        number(d)?,
        number(e)?,
        number(f)?,
    ])
}

#[derive(Clone, Copy)]
struct Matrix([f64; 6]);

impl Matrix {
    const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    fn concat(self, next: Self) -> Self {
        let [a, b, c, d, e, f] = self.0;
        let [g, h, i, j, k, l] = next.0;
        Self([
            a * g + c * h,
            b * g + d * h,
            a * i + c * j,
            b * i + d * j,
            a * k + c * l + e,
            b * k + d * l + f,
        ])
    }

    fn point(self, x: f64, y: f64) -> [f64; 2] {
        let [a, b, c, d, e, f] = self.0;
        [a * x + c * y + e, b * x + d * y + f]
    }

    fn bounded(self) -> bool {
        self.0
            .iter()
            .all(|value| value.is_finite() && value.abs() <= 1_000_000.0)
    }
}

/// PDF user space to top-left display coordinates at one pixel per point.
/// This is deliberately narrower than the general PDF page transform:
/// UserUnit, malformed/inverted boxes and non-quarter-turn rotations fail.
fn page_display_matrix(
    document: &Document,
    page_id: lopdf::ObjectId,
) -> Result<Matrix, Box<dyn Error>> {
    let mut media = None;
    let mut crop = None;
    let mut rotation = None;
    let mut current = Some(page_id);
    let mut visited = HashSet::new();
    while let Some(id) = current {
        if !visited.insert(id) || visited.len() > 16 {
            return Err(fail("cyclic or deep page tree").into());
        }
        let node = document.get_dictionary(id)?;
        if node.has(b"UserUnit") {
            return Err(fail("text-run page UserUnit unsupported").into());
        }
        let parse_box = |key: &[u8]| -> Result<Option<[f64; 4]>, Box<dyn Error>> {
            let Ok(value) = node.get(key) else {
                return Ok(None);
            };
            let values = document.dereference(value)?.1.as_array()?;
            if values.len() != 4 {
                return Err(fail("page box needs four coordinates").into());
            }
            Ok(Some([
                number(&values[0])?,
                number(&values[1])?,
                number(&values[2])?,
                number(&values[3])?,
            ]))
        };
        if media.is_none() {
            media = parse_box(b"MediaBox")?;
        }
        if crop.is_none() {
            crop = parse_box(b"CropBox")?;
        }
        if rotation.is_none() {
            rotation = node.get(b"Rotate").ok().map(number).transpose()?;
        }
        current = node
            .get(b"Parent")
            .ok()
            .map(Object::as_reference)
            .transpose()?;
    }
    let media = media.ok_or_else(|| fail("text-run page lacks MediaBox"))?;
    let crop = crop.unwrap_or(media);
    if media[0] >= media[2]
        || media[1] >= media[3]
        || crop[0] < media[0]
        || crop[1] < media[1]
        || crop[0] >= crop[2]
        || crop[1] >= crop[3]
        || crop[2] > media[2]
        || crop[3] > media[3]
    {
        return Err(fail("invalid text-run MediaBox or CropBox").into());
    }
    let matrix = match rotation.unwrap_or(0.0) {
        0.0 => [1.0, 0.0, 0.0, -1.0, -crop[0], crop[3]],
        90.0 => [0.0, 1.0, 1.0, 0.0, -crop[1], -crop[0]],
        180.0 => [-1.0, 0.0, 0.0, 1.0, crop[2], -crop[1]],
        270.0 => [0.0, -1.0, -1.0, 0.0, crop[3], crop[2]],
        _ => return Err(fail("text-run page rotation unsupported").into()),
    };
    Ok(Matrix(matrix))
}

fn opaque_gstates(
    document: &Document,
    page_id: lopdf::ObjectId,
) -> Result<BTreeMap<Vec<u8>, bool>, Box<dyn Error>> {
    let (direct, inherited) = document.get_page_resources(page_id)?;
    let mut result = BTreeMap::new();
    for resources in direct.into_iter().chain(
        inherited
            .iter()
            .filter_map(|id| document.get_dictionary(*id).ok()),
    ) {
        let Ok(states) = resources.get(b"ExtGState") else {
            continue;
        };
        let states = document.dereference(states)?.1.as_dict()?;
        for (name, value) in states.iter() {
            if result.contains_key(name) {
                continue;
            }
            let safe = document.dereference(value)?.1.as_dict().is_ok_and(|dict| {
                dict.iter().all(|(key, value)| match key.as_slice() {
                    b"Type" => value.as_name().ok() == Some(b"ExtGState"),
                    b"CA" | b"ca" => number(value).ok() == Some(1.0),
                    b"BM" => value.as_name().ok() == Some(b"Normal"),
                    _ => false,
                })
            });
            result.insert(name.clone(), safe);
        }
    }
    Ok(result)
}

struct Font {
    glyph_count: u16,
    units_per_em: u16,
    widths: Vec<f64>,
    bytes: usize,
    data: Vec<u8>,
}

fn cid(value: &Object) -> Result<usize, Box<dyn Error>> {
    let value = value.as_i64()?;
    if !(0..=65535).contains(&value) {
        return Err(fail("CID outside 16-bit range").into());
    }
    Ok(value as usize)
}

fn widths(document: &Document, font: &Dictionary) -> Result<Vec<f64>, Box<dyn Error>> {
    let default = match font.get(b"DW") {
        Ok(value) => number(document.dereference(value)?.1)?,
        Err(_) => 1000.0,
    };
    if !(0.0..=10_000.0).contains(&default) {
        return Err(fail("invalid default CID width").into());
    }
    let mut widths = vec![default; MAX_GLYPHS];
    let Ok(entries) = font.get(b"W") else {
        return Ok(widths);
    };
    let entries = document.dereference(entries)?.1.as_array()?;
    let mut index = 0;
    while index < entries.len() {
        let first = cid(&entries[index])?;
        index += 1;
        let next = entries
            .get(index)
            .ok_or_else(|| fail("incomplete CID widths"))?;
        if let Ok(values) = next.as_array() {
            if values.is_empty() || first + values.len() > MAX_GLYPHS {
                return Err(fail("invalid CID width array").into());
            }
            for (offset, value) in values.iter().enumerate() {
                widths[first + offset] = number(value)?;
            }
            index += 1;
        } else {
            let last = cid(next)?;
            let value = number(
                entries
                    .get(index + 1)
                    .ok_or_else(|| fail("missing CID width"))?,
            )?;
            if last < first {
                return Err(fail("reversed CID width range").into());
            }
            widths[first..=last].fill(value);
            index += 2;
        }
    }
    if widths.iter().any(|width| !(0.0..=10_000.0).contains(width)) {
        return Err(fail("invalid CID width").into());
    }
    Ok(widths)
}

fn load_font(document: &Document, font: &Dictionary) -> Result<Font, Box<dyn Error>> {
    if font.get(b"Subtype")?.as_name()? != b"Type0"
        || font.get(b"Encoding")?.as_name()? != b"Identity-H"
    {
        return Err(fail("only Type0 Identity-H is staged").into());
    }
    let descendants = document
        .dereference(font.get(b"DescendantFonts")?)?
        .1
        .as_array()?;
    let [descendant] = descendants.as_slice() else {
        return Err(fail("expected one CIDFont descendant").into());
    };
    let descendant = document.dereference(descendant)?.1.as_dict()?;
    if descendant.get(b"Subtype")?.as_name()? != b"CIDFontType2" {
        return Err(fail("only CIDFontType2 is staged").into());
    }
    if let Ok(map) = descendant.get(b"CIDToGIDMap") {
        if document.dereference(map)?.1.as_name()? != b"Identity" {
            return Err(fail("non-Identity CIDToGIDMap is not staged").into());
        }
    }
    let descriptor = document
        .dereference(descendant.get(b"FontDescriptor")?)?
        .1
        .as_dict()?;
    let file = document
        .dereference(descriptor.get(b"FontFile2")?)?
        .1
        .as_stream()?;
    if file.content.len() > MAX_FONT_BYTES {
        return Err(fail("font stream exceeds limit").into());
    }
    if let Ok(parameters) = file.dict.get(b"DecodeParms") {
        let parameters = document.dereference(parameters)?.1.as_dict()?;
        if parameters.len() != 1 || parameters.get(b"Predictor")?.as_i64()? != 1 {
            return Err(fail("unsupported font decode parameters").into());
        }
    }
    let expected = usize::try_from(file.dict.get(b"Length1")?.as_i64()?)?;
    if expected == 0 || expected > MAX_FONT_BYTES {
        return Err(fail("invalid embedded TrueType length").into());
    }
    let bytes = if let Ok(filter) = file.dict.get(b"Filter") {
        if filter.as_name()? != b"FlateDecode" {
            return Err(fail("unsupported embedded font filter").into());
        }
        miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&file.content, expected)
            .map_err(|_| fail("invalid or oversized embedded TrueType"))?
    } else {
        file.content.clone()
    };
    if bytes.len() != expected {
        return Err(fail("wrong embedded TrueType length").into());
    }
    let face = ttf_parser::Face::parse(&bytes, 0)?;
    if face.number_of_glyphs() == 0 || face.units_per_em() == 0 {
        return Err(fail("embedded font has invalid glyph metrics").into());
    }
    Ok(Font {
        glyph_count: face.number_of_glyphs(),
        units_per_em: face.units_per_em(),
        widths: widths(document, descendant)?,
        bytes: bytes.len(),
        data: bytes,
    })
}

#[derive(Default)]
struct OutlineInventory {
    moves: usize,
    lines: usize,
    quads: usize,
    cubics: usize,
    closes: usize,
    points: Vec<[f64; 2]>,
    contours: Vec<Vec<[f64; 2]>>,
}

impl ttf_parser::OutlineBuilder for OutlineInventory {
    fn move_to(&mut self, x: f32, y: f32) {
        self.moves += 1;
        self.points.push([f64::from(x), f64::from(y)]);
        self.contours.push(vec![[f64::from(x), f64::from(y)]]);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.lines += 1;
        self.points.push([f64::from(x), f64::from(y)]);
        if let Some(contour) = self.contours.last_mut() {
            contour.push([f64::from(x), f64::from(y)]);
        }
    }
    fn quad_to(&mut self, _x1: f32, _y1: f32, _x: f32, _y: f32) {
        self.quads += 1;
    }
    fn curve_to(&mut self, _x1: f32, _y1: f32, _x2: f32, _y2: f32, _x: f32, _y: f32) {
        self.cubics += 1;
    }
    fn close(&mut self) {
        self.closes += 1;
    }
}

fn multi_glyph_scene(
    font: &Font,
    record: &GlyphRecord,
) -> Result<(Vec<u8>, [f64; 2]), Box<dyn Error>> {
    let face = ttf_parser::Face::parse(&font.data, 0)?;
    let mut outline = OutlineInventory::default();
    face.outline_glyph(ttf_parser::GlyphId(record.gid), &mut outline)
        .ok_or_else(|| fail("glyph has no outline"))?;
    if !(2..=8).contains(&outline.moves)
        || outline.closes != outline.moves
        || outline.quads != 0
        || outline.cubics != 0
        || outline.lines + 2 * outline.moves > 256
        || outline.contours.iter().any(|c| c.len() < 3)
    {
        return Err(fail("glyph is not bounded line-only multi-contour").into());
    }
    let [a, b, c, d, e, f] = record.transform;
    for contour in &mut outline.contours {
        for point in contour {
            let [x, y] = *point;
            *point = [a * x + c * y + e, b * x + d * y + f];
        }
    }
    let (min_x, max_x, min_y, max_y) = outline.contours.iter().flatten().fold(
        (
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
        ),
        |(x0, x1, y0, y1), p| (x0.min(p[0]), x1.max(p[0]), y0.min(p[1]), y1.max(p[1])),
    );
    if max_x - min_x > 240.0 || max_y - min_y > 176.0 {
        return Err(fail("glyph exceeds experimental tile").into());
    }
    let shift = [128.0 - (min_x + max_x) / 2.0, 96.0 - (min_y + max_y) / 2.0];
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"OPM2");
    for value in [1_u32, 256, 192, u32::try_from(outline.contours.len())?] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for mut contour in outline.contours {
        for point in &mut contour {
            point[0] += shift[0];
            point[1] += shift[1];
        }
        if contour.last() != Some(&contour[0]) {
            contour.push(contour[0]);
        }
        bytes.extend_from_slice(&u32::try_from(contour.len())?.to_le_bytes());
        for point in contour {
            for value in point {
                if !value.is_finite() || value.abs() > 4096.0 {
                    return Err(fail("glyph point outside limit").into());
                }
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    println!(
        "multi_outline gid={} contours={} lines={}",
        record.gid, outline.moves, outline.lines
    );
    Ok((bytes, shift))
}

// OPLN is a local 256x192 experiment, not a PDF page or a production glyph scene.
fn simple_glyph_opln(
    font: &Font,
    record: &GlyphRecord,
) -> Result<(Vec<u8>, [f64; 2]), Box<dyn Error>> {
    let face = ttf_parser::Face::parse(&font.data, 0)?;
    let mut outline = OutlineInventory::default();
    face.outline_glyph(ttf_parser::GlyphId(record.gid), &mut outline)
        .ok_or_else(|| fail("glyph has no outline"))?;
    if outline.moves != 1
        || outline.closes != 1
        || outline.quads != 0
        || outline.cubics != 0
        || !(3..=4096).contains(&outline.lines)
        || outline.points.len() != outline.lines + 1
    {
        return Err(fail("glyph is not one bounded line-only contour").into());
    }
    let [a, b, c, d, e, f] = record.transform;
    let mut points: Vec<[f64; 2]> = outline
        .points
        .iter()
        .map(|[x, y]| [a * x + c * y + e, b * x + d * y + f])
        .collect();
    let (min_x, max_x) = points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p[0]), hi.max(p[0]))
        });
    let (min_y, max_y) = points
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| {
            (lo.min(p[1]), hi.max(p[1]))
        });
    if max_x - min_x > 240.0 || max_y - min_y > 176.0 {
        return Err(fail("glyph exceeds experimental tile").into());
    }
    let dx = 128.0 - (min_x + max_x) / 2.0;
    let dy = 96.0 - (min_y + max_y) / 2.0;
    for point in &mut points {
        point[0] += dx;
        point[1] += dy;
    }
    points.push(points[0]);
    if points
        .iter()
        .flatten()
        .any(|v| !v.is_finite() || v.abs() > 4096.0)
    {
        return Err(fail("glyph point outside OPLN limit").into());
    }
    let mut encoded = Vec::with_capacity(20 + points.len() * 16);
    encoded.extend_from_slice(b"OPLN");
    for value in [1_u32, 256, 192, u32::try_from(points.len())?] {
        encoded.extend_from_slice(&value.to_le_bytes());
    }
    for point in points {
        for value in point {
            encoded.extend_from_slice(&value.to_le_bytes());
        }
    }
    println!(
        "outline gid={} line_segments={} tile_shift={dx:.6},{dy:.6}",
        record.gid, outline.lines
    );
    Ok((encoded, [dx, dy]))
}

#[derive(Clone)]
struct GlyphRecord {
    font: Vec<u8>,
    cid: u16,
    gid: u16,
    transform: [f64; 6],
    // Not part of TGR1; only used by the bounded page-color experiment.
    color: Option<[u8; 3]>,
}

fn encode_records(page: u32, records: &[GlyphRecord]) -> Result<Vec<u8>, Box<dyn Error>> {
    if page == 0 || records.is_empty() || records.len() > MAX_GLYPHS {
        return Err(fail("glyph record count or page outside limit").into());
    }
    let mut encoded = Vec::with_capacity(HEADER_BYTES + records.len() * GLYPH_RECORD_BYTES);
    encoded.extend_from_slice(b"TGR1");
    encoded.extend_from_slice(&1_u16.to_le_bytes());
    encoded.extend_from_slice(&(GLYPH_RECORD_BYTES as u16).to_le_bytes());
    encoded.extend_from_slice(&page.to_le_bytes());
    encoded.extend_from_slice(&(records.len() as u32).to_le_bytes());
    for record in records {
        if record.font.is_empty() || record.font.len() >= FONT_NAME_BYTES {
            return Err(fail("font resource name outside limit").into());
        }
        encoded.push(record.font.len() as u8);
        encoded.extend_from_slice(&record.font);
        encoded.resize(encoded.len() + FONT_NAME_BYTES - 1 - record.font.len(), 0);
        encoded.extend_from_slice(&record.cid.to_le_bytes());
        encoded.extend_from_slice(&record.gid.to_le_bytes());
        for value in record.transform {
            if !value.is_finite() || value.abs() > 1_000_000.0 {
                return Err(fail("glyph transform outside limit").into());
            }
            encoded.extend_from_slice(&value.to_le_bytes());
        }
    }
    validate_records(&encoded)?;
    Ok(encoded)
}

fn validate_records(encoded: &[u8]) -> Result<(u32, usize), Box<dyn Error>> {
    if encoded.len() < HEADER_BYTES || &encoded[..4] != b"TGR1" {
        return Err(fail("wrong glyph-run header").into());
    }
    if u16::from_le_bytes([encoded[4], encoded[5]]) != 1
        || usize::from(u16::from_le_bytes([encoded[6], encoded[7]])) != GLYPH_RECORD_BYTES
    {
        return Err(fail("unsupported glyph-run version or record size").into());
    }
    let page = u32::from_le_bytes(encoded[8..12].try_into()?);
    let count = u32::from_le_bytes(encoded[12..16].try_into()?) as usize;
    if page == 0
        || count == 0
        || count > MAX_GLYPHS
        || encoded.len() != HEADER_BYTES + count * GLYPH_RECORD_BYTES
    {
        return Err(fail("glyph-run length or page outside limit").into());
    }
    for record in encoded[HEADER_BYTES..].chunks_exact(GLYPH_RECORD_BYTES) {
        let name_len = record[0] as usize;
        if name_len == 0
            || name_len >= FONT_NAME_BYTES
            || record[1 + name_len..FONT_NAME_BYTES]
                .iter()
                .any(|byte| *byte != 0)
        {
            return Err(fail("invalid glyph font name").into());
        }
        for coordinate in record[FONT_NAME_BYTES + 4..].chunks_exact(8) {
            let value = f64::from_le_bytes(coordinate.try_into()?);
            if !value.is_finite() || value.abs() > 1_000_000.0 {
                return Err(fail("invalid glyph transform").into());
            }
        }
    }
    Ok((page, count))
}

#[derive(Default)]
struct Summary {
    glyphs: usize,
    shows: usize,
    first: Option<(u16, u16, [f64; 2], f64)>,
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: stage_type0_text INPUT.pdf PAGE [OUTPUT.tgr] | --validate INPUT.tgr | --isolate-quad INPUT.pdf PAGE RECORD_INDEX OUTPUT.opm OUTPUT.pdf [OUTPUT.opq [OUTPUT.ttf]]")?;
    if input == "--validate" {
        let path = args.next().ok_or("--validate requires a file")?;
        if args.next().is_some() {
            return Err("--validate takes one file".into());
        }
        let (page, count) = validate_records(&read_bounded(
            Path::new(&path),
            HEADER_BYTES + MAX_GLYPHS * GLYPH_RECORD_BYTES,
        )?)?;
        println!("TGR1 page={page} glyphs={count}");
        return Ok(());
    }
    let inventory_mode = input == "--outline-inventory";
    let multi_mode = input == "--isolate-multi";
    let quad_skrifa_mode = input == "--isolate-quad-skrifa";
    let quad_mode = input == "--isolate-quad" || quad_skrifa_mode;
    let run_mode = input == "--isolate-text-run";
    let outline_mode =
        input == "--outline" || input == "--isolate" || multi_mode || quad_mode || run_mode;
    let isolate_mode = input == "--isolate" || multi_mode || quad_mode || run_mode;
    let input = if outline_mode || inventory_mode {
        args.next().ok_or("--outline requires PDF")?
    } else {
        input
    };
    let page_number = args
        .next()
        .ok_or("usage: stage_type0_text INPUT.pdf PAGE [OUTPUT.tgr]")?
        .to_string_lossy()
        .parse::<u32>()?;
    let outline_index = if outline_mode {
        let index = args.next().ok_or("--outline requires record index")?;
        Some(if index == "auto" {
            usize::MAX
        } else {
            index.to_string_lossy().parse::<usize>()?
        })
    } else {
        None
    };
    let run_count = if run_mode {
        let count = args.next().ok_or("text run requires glyph count")?;
        let count = count.to_string_lossy().parse::<usize>()?;
        if !(2..=32).contains(&count) {
            return Err(fail("text run needs 2..32 glyphs").into());
        }
        Some(count)
    } else {
        None
    };
    let output = args.next();
    let pdf_output = if isolate_mode {
        Some(args.next().ok_or("--isolate requires output PDF path")?)
    } else {
        None
    };
    let (red_last, overlap_last, page_coords) = if run_mode {
        match args.next().as_deref() {
            None => (false, false, false),
            Some(value) if value == "red-last" => (true, false, false),
            Some(value) if value == "red-overlap-last" => (true, true, false),
            Some(value) if value == "page-coords" => (false, false, true),
            _ => return Err(fail("unsupported text-run color mode").into()),
        }
    } else {
        (false, false, false)
    };
    let raw_output = if quad_mode { args.next() } else { None };
    let font_output = if quad_mode { args.next() } else { None };
    if outline_mode && output.is_none() {
        return Err("--outline requires output OPLN path".into());
    }
    if args.next().is_some() {
        return Err("usage: stage_type0_text INPUT.pdf PAGE [OUTPUT.tgr]".into());
    }
    for target in output
        .iter()
        .chain(pdf_output.iter())
        .chain(raw_output.iter())
        .chain(font_output.iter())
    {
        if let Ok(source) = Path::new(&input).canonicalize() {
            if Path::new(target).canonicalize().ok() == Some(source) {
                return Err("output must not overwrite the source PDF".into());
            }
        }
    }
    let input = read_bounded(Path::new(&input), MAX_PDF_BYTES)?;
    let mut document = Document::load_mem(&input)?;
    let page_id = *document
        .get_pages()
        .get(&page_number)
        .ok_or_else(|| fail("page not found"))?;
    let opaque_states = if run_mode {
        opaque_gstates(&document, page_id)?
    } else {
        BTreeMap::new()
    };
    let mut fonts = BTreeMap::new();
    let mut total_font_bytes = 0_usize;
    for (name, font) in document.get_page_fonts(page_id)? {
        match load_font(&document, font) {
            Ok(staged) => {
                if fonts.len() >= MAX_FONTS
                    || total_font_bytes.saturating_add(staged.bytes) > MAX_TOTAL_FONT_BYTES
                {
                    return Err(fail("staged fonts exceed page limit").into());
                }
                total_font_bytes += staged.bytes;
                fonts.insert(name, staged);
            }
            Err(error) => {
                println!(
                    "unsupported_font={} reason={error}",
                    String::from_utf8_lossy(&name)
                );
            }
        }
    }
    let content = Content::decode(&document.get_page_content(page_id)?)?;
    let mut active_font = Vec::new();
    let mut font_size = 0.0;
    let mut text_matrix = Matrix::IDENTITY;
    let mut text_offset = 0.0;
    let mut char_spacing = 0.0;
    let mut word_spacing = 0.0;
    let mut horizontal_scale = 1.0;
    let mut rise = 0.0;
    let mut render_mode = 0.0;
    let mut in_text = false;
    let mut positioned = false;
    let mut ctm = Matrix::IDENTITY;
    let mut fill_ink = Some([0_u8; 3]); // PDF's default fill is black.
    let mut paint_safe = true;
    let mut saved = Vec::new();
    let mut summaries = BTreeMap::<Vec<u8>, Summary>::new();
    let mut records = Vec::<GlyphRecord>::new();
    let mut skipped = 0;
    for operation in content.operations {
        match (operation.operator.as_str(), operation.operands.as_slice()) {
            ("q", []) if !in_text => {
                if saved.len() >= 32 {
                    return Err(fail("graphics state nesting exceeds limit").into());
                }
                saved.push((
                    ctm,
                    active_font.clone(),
                    font_size,
                    char_spacing,
                    word_spacing,
                    horizontal_scale,
                    rise,
                    render_mode,
                    fill_ink,
                    paint_safe,
                ));
            }
            ("Q", []) if !in_text => {
                (
                    ctm,
                    active_font,
                    font_size,
                    char_spacing,
                    word_spacing,
                    horizontal_scale,
                    rise,
                    render_mode,
                    fill_ink,
                    paint_safe,
                ) = saved.pop().ok_or_else(|| fail("unbalanced Q"))?;
            }
            ("rg", [red, green, blue]) => {
                fill_ink = endpoint_channel(red)
                    .zip(endpoint_channel(green))
                    .zip(endpoint_channel(blue))
                    .map(|((red, green), blue)| [red, green, blue]);
            }
            ("g", [gray]) => {
                fill_ink = endpoint_channel(gray).map(|value| [value; 3]);
            }
            ("gs", [Object::Name(name)]) => {
                paint_safe &= opaque_states.get(name).copied().unwrap_or(false);
            }
            ("gs", _) => paint_safe = false,
            ("rg" | "g" | "k" | "cs" | "sc" | "scn", _) => fill_ink = None,
            ("cm", operands) if !in_text => {
                ctm = ctm.concat(Matrix(six(operands)?));
                if !ctm.bounded() {
                    return Err(fail("CTM exceeds coordinate limit").into());
                }
            }
            ("BT", []) if !in_text => {
                in_text = true;
                text_matrix = Matrix::IDENTITY;
                text_offset = 0.0;
                positioned = false;
            }
            ("ET", []) if in_text => in_text = false,
            ("Tf", [Object::Name(name), size]) => {
                active_font = name.clone();
                font_size = number(size)?;
                if !(0.0..=1000.0).contains(&font_size) {
                    return Err(fail("invalid text font size").into());
                }
            }
            ("Tm", operands) if in_text => {
                text_matrix = Matrix(six(operands)?);
                if !text_matrix.bounded() {
                    return Err(fail("text matrix exceeds coordinate limit").into());
                }
                text_offset = 0.0;
                positioned = true;
            }
            ("Tc", [value]) => char_spacing = number(value)?,
            ("Tw", [value]) => word_spacing = number(value)?,
            ("Tz", [value]) => horizontal_scale = number(value)? / 100.0,
            ("Ts", [value]) => rise = number(value)?,
            ("Tr", [value]) => render_mode = number(value)?,
            ("Tj", [Object::String(bytes, _)]) if in_text => {
                if !positioned
                    || render_mode != 0.0
                    || word_spacing != 0.0
                    || !(0.0..=4.0).contains(&horizontal_scale)
                {
                    skipped += 1;
                    continue;
                }
                let Some(font) = fonts.get(&active_font) else {
                    skipped += 1;
                    continue;
                };
                if bytes.len() % 2 != 0 || bytes.len() / 2 > MAX_GLYPHS || font_size == 0.0 {
                    return Err(fail("invalid Identity-H text string").into());
                }
                let summary = summaries.entry(active_font.clone()).or_default();
                summary.shows += 1;
                for pair in bytes.chunks_exact(2) {
                    let cid = u16::from_be_bytes([pair[0], pair[1]]);
                    let gid = cid; // /Identity or absent CIDToGIDMap in this read-only probe.
                    if gid >= font.glyph_count || records.len() >= MAX_GLYPHS {
                        return Err(fail("glyph outside embedded TrueType").into());
                    }
                    let text_position = text_matrix.point(text_offset, rise);
                    let position = ctm.point(text_position[0], text_position[1]);
                    if position
                        .iter()
                        .any(|value| !value.is_finite() || value.abs() > 1_000_000.0)
                    {
                        return Err(fail("glyph position exceeds coordinate limit").into());
                    }
                    let step_x = font_size * horizontal_scale / f64::from(font.units_per_em);
                    let step_y = font_size / f64::from(font.units_per_em);
                    let text_x = text_matrix.point(text_offset + step_x, rise);
                    let text_y = text_matrix.point(text_offset, rise + step_y);
                    let basis_x = ctm.point(text_x[0], text_x[1]);
                    let basis_y = ctm.point(text_y[0], text_y[1]);
                    records.push(GlyphRecord {
                        font: active_font.clone(),
                        cid,
                        gid,
                        color: if paint_safe { fill_ink } else { None },
                        transform: [
                            basis_x[0] - position[0],
                            basis_x[1] - position[1],
                            basis_y[0] - position[0],
                            basis_y[1] - position[1],
                            position[0],
                            position[1],
                        ],
                    });
                    summary.first.get_or_insert((cid, gid, position, font_size));
                    summary.glyphs += 1;
                    let space = if cid == 32 { word_spacing } else { 0.0 };
                    text_offset +=
                        (font.widths[cid as usize] * font_size / 1000.0 + char_spacing + space)
                            * horizontal_scale;
                }
            }
            ("Tj" | "TJ" | "'" | "\"", _) if in_text => skipped += 1,
            ("Td" | "TD" | "T*", _) if in_text => positioned = false,
            _ => {}
        }
    }
    if in_text || !saved.is_empty() {
        return Err(fail("unclosed text or graphics state").into());
    }
    for (name, summary) in summaries {
        let font = &fonts[&name];
        let (cid, gid, origin, size) = summary.first.ok_or_else(|| fail("empty staged run"))?;
        println!(
            "font={} ttf_bytes={} glyph_capacity={} shows={} staged_glyphs={} first_cid={} first_gid={} first_origin={:.4},{:.4} first_size={:.4}",
            String::from_utf8_lossy(&name), font.bytes, font.glyph_count,
            summary.shows, summary.glyphs, cid, gid, origin[0], origin[1], size
        );
    }
    println!("skipped_text_ops={skipped}");
    if inventory_mode {
        let mut seen = std::collections::BTreeSet::new();
        for (index, record) in records.iter().enumerate() {
            if !seen.insert((record.font.clone(), record.gid)) {
                continue;
            }
            let font = fonts
                .get(&record.font)
                .ok_or_else(|| fail("glyph font unavailable"))?;
            let face = ttf_parser::Face::parse(&font.data, 0)?;
            let mut outline = OutlineInventory::default();
            if face
                .outline_glyph(ttf_parser::GlyphId(record.gid), &mut outline)
                .is_some()
                && outline.moves >= 2
                && outline.quads == 0
                && outline.cubics == 0
            {
                println!(
                    "multi_line_record={index} font={} gid={} contours={} lines={}",
                    String::from_utf8_lossy(&record.font),
                    record.gid,
                    outline.moves,
                    outline.lines
                );
            }
            if outline.quads > 0 || outline.cubics > 0 {
                println!(
                    "curved_record={index} font={} gid={} contours={} lines={} quads={} cubics={}",
                    String::from_utf8_lossy(&record.font),
                    record.gid,
                    outline.moves,
                    outline.lines,
                    outline.quads,
                    outline.cubics
                );
            }
        }
        return Ok(());
    }
    if let Some(output) = output {
        if skipped != 0 {
            return Err(fail("cannot export a partial glyph run").into());
        }
        if let Some(count) = run_count {
            let first = outline_index.ok_or_else(|| fail("missing first text-run record"))?;
            if first == usize::MAX
                || first
                    .checked_add(count)
                    .is_none_or(|end| end > records.len())
            {
                return Err(fail("text-run records outside staged glyph run").into());
            }
            let mut run_records = records[first..first + count].to_vec();
            let display = page_display_matrix(&document, page_id)?;
            for record in &mut run_records {
                record.transform = display.concat(Matrix(record.transform)).0;
            }
            if overlap_last {
                // Deliberate controlled overlap fixture. Both the isolated
                // PDF and OPGS geometry receive the same 20pt displacement.
                run_records[count - 1].transform[5] -= 20.0;
            }
            let mut scenes = Vec::with_capacity(count);
            let mut bounds = [
                f64::INFINITY,
                f64::INFINITY,
                f64::NEG_INFINITY,
                f64::NEG_INFINITY,
            ];
            for record in &run_records {
                if record.font.is_empty()
                    || record.font.len() > 31
                    || !record
                        .font
                        .iter()
                        .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
                {
                    return Err(fail("text-run font resource name unsupported").into());
                }
                let font = fonts
                    .get(&record.font)
                    .ok_or_else(|| fail("glyph font unavailable"))?;
                let (scene, shift, _) = quad_glyph::stage(
                    &font.data,
                    record.gid,
                    record.transform,
                    quad_glyph::Source::SkrifaAtThousand,
                )?;
                let rect = quad_glyph::absolute_bounds(&scene, shift)?;
                bounds[0] = bounds[0].min(rect[0]);
                bounds[1] = bounds[1].min(rect[1]);
                bounds[2] = bounds[2].max(rect[2]);
                bounds[3] = bounds[3].max(rect[3]);
                scenes.push((scene, shift));
            }
            if bounds[2] - bounds[0] > 240.0 || bounds[3] - bounds[1] > 176.0 {
                return Err(fail("text run exceeds bounded output tile").into());
            }
            let shift = if page_coords {
                if bounds[0] < 0.0 || bounds[1] < 0.0 || bounds[2] > 256.0 || bounds[3] > 192.0 {
                    return Err(fail("text run exceeds visible page tile").into());
                }
                [0.0, 0.0]
            } else {
                [
                    128.0 - (bounds[0] + bounds[2]) / 2.0,
                    96.0 - (bounds[1] + bounds[3]) / 2.0,
                ]
            };
            let mut scene = Vec::new();
            scene.extend_from_slice(b"OPGS");
            for word in [1_u32, 256, 192, count as u32] {
                scene.extend_from_slice(&word.to_le_bytes());
            }
            let mut contents = Vec::new();
            for (position, (record, (mut opm, old_shift))) in
                run_records.iter().zip(scenes).enumerate()
            {
                quad_glyph::rebase(&mut opm, old_shift, shift)?;
                let font = &fonts[&record.font];
                let mut name = [0_u8; 32];
                name[..record.font.len()].copy_from_slice(&record.font);
                scene.extend_from_slice(&name);
                scene.extend_from_slice(&record.cid.to_le_bytes());
                scene.extend_from_slice(&record.gid.to_le_bytes());
                let source_color = record
                    .color
                    .ok_or_else(|| fail("text-run fill color unsupported"))?;
                let color = if red_last && position + 1 == count {
                    [255_u8, 0, 0]
                } else {
                    source_color
                };
                scene.extend_from_slice(&color);
                scene.push(0); // reserved flags
                scene.extend_from_slice(&u32::try_from(opm.len())?.to_le_bytes());
                scene.extend_from_slice(&opm);
                let [a, b, c, d, e, f] = record.transform;
                let text = format!(
                    "q {} {} {} rg BT /{} {} Tf {:.12} {:.12} {:.12} {:.12} {:.12} {:.12} Tm <{:04X}> Tj ET Q\n",
                    if color[0] == 255 { 1 } else { 0 }, 0, 0,
                    String::from_utf8_lossy(&record.font), font.units_per_em,
                    a, -b, c, -d, e + shift[0], 192.0 - f - shift[1], record.cid
                );
                contents.extend_from_slice(text.as_bytes());
            }
            let pdf_output = pdf_output.ok_or_else(|| fail("text run requires output PDF"))?;
            let stream_id = document.add_object(Stream::new(Dictionary::new(), contents));
            let page = document.get_object_mut(page_id)?.as_dict_mut()?;
            page.set("Contents", stream_id);
            page.set("MediaBox", vec![0.into(), 0.into(), 256.into(), 192.into()]);
            page.set("CropBox", vec![0.into(), 0.into(), 256.into(), 192.into()]);
            page.set("Rotate", 0);
            std::fs::write(output, scene)?;
            document.save(pdf_output)?;
            println!("OPGS glyphs={count} first_record={first}");
            return Ok(());
        }
        if let Some(index) = outline_index {
            let candidates: Box<dyn Iterator<Item = usize>> = if index == usize::MAX {
                Box::new(0..records.len())
            } else {
                Box::new(std::iter::once(index))
            };
            let mut selected = None;
            for candidate in candidates {
                let record = records
                    .get(candidate)
                    .ok_or_else(|| fail("glyph record index outside run"))?;
                let font = fonts
                    .get(&record.font)
                    .ok_or_else(|| fail("glyph font unavailable"))?;
                let result = if quad_mode {
                    let source = if quad_skrifa_mode {
                        quad_glyph::Source::SkrifaAtThousand
                    } else {
                        quad_glyph::Source::RawTtf
                    };
                    quad_glyph::stage(&font.data, record.gid, record.transform, source)
                        .map(|(opm, shift, raw)| (opm, shift, Some(raw)))
                } else if multi_mode {
                    multi_glyph_scene(font, record).map(|(opm, shift)| (opm, shift, None))
                } else {
                    simple_glyph_opln(font, record).map(|(opm, shift)| (opm, shift, None))
                };
                if let Ok((opln, shift, raw)) = result {
                    selected = Some((candidate, opln, shift, raw));
                    break;
                }
            }
            let (index, opln, shift, raw) =
                selected.ok_or_else(|| fail("no bounded glyph contour for requested mode"))?;
            std::fs::write(output, opln)?;
            if let Some(raw_output) = raw_output {
                std::fs::write(
                    raw_output,
                    raw.ok_or_else(|| fail("quadratic path missing"))?,
                )?;
            }
            if let Some(font_output) = font_output {
                let record = &records[index];
                let font = fonts
                    .get(&record.font)
                    .ok_or_else(|| fail("glyph font unavailable"))?;
                std::fs::write(font_output, &font.data)?;
            }
            println!("selected_record={index}");
            if quad_mode {
                println!("selected_gid={}", records[index].gid);
                let m = records[index].transform;
                println!(
                    "selected_transform={} {} {} {} {} {} shift={} {}",
                    m[0], m[1], m[2], m[3], m[4], m[5], shift[0], shift[1]
                );
            }
            if let Some(pdf_output) = pdf_output {
                let record = &records[index];
                let font = &fonts[&record.font];
                let [a, b, c, d, e, f] = record.transform;
                let content = format!(
                    "q 0 g BT /{} {} Tf {:.12} {:.12} {:.12} {:.12} {:.12} {:.12} Tm <{:04X}> Tj ET Q\n",
                    String::from_utf8_lossy(&record.font), font.units_per_em,
                    a, -b, c, -d, e + shift[0], 192.0 - f - shift[1], record.cid
                );
                let stream_id =
                    document.add_object(Stream::new(Dictionary::new(), content.into_bytes()));
                let page = document.get_object_mut(page_id)?.as_dict_mut()?;
                page.set("Contents", stream_id);
                page.set("MediaBox", vec![0.into(), 0.into(), 256.into(), 192.into()]);
                page.set("CropBox", vec![0.into(), 0.into(), 256.into(), 192.into()]);
                page.set("Rotate", 0);
                document.save(pdf_output)?;
            }
            return Ok(());
        }
        let encoded = encode_records(page_number, &records)?;
        std::fs::write(output, &encoded)?;
        println!("TGR1 bytes={} records={}", encoded.len(), records.len());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyph_run_rejects_truncation_and_corrupt_structure() {
        let record = GlyphRecord {
            font: b"F1".to_vec(),
            cid: 65,
            gid: 65,
            transform: [0.02, 0.0, 0.0, 0.02, 20.0, 30.0],
            color: Some([0; 3]),
        };
        let valid = encode_records(1, &[record]).unwrap();
        assert_eq!(validate_records(&valid).unwrap(), (1, 1));
        for len in 0..valid.len() {
            assert!(
                validate_records(&valid[..len]).is_err(),
                "truncation at {len}"
            );
        }
        let mut corrupt = valid.clone();
        corrupt[0] = b'X';
        assert!(validate_records(&corrupt).is_err());
        corrupt = valid.clone();
        corrupt[6] = 0;
        assert!(validate_records(&corrupt).is_err());
        corrupt = valid.clone();
        corrupt[HEADER_BYTES] = 255;
        assert!(validate_records(&corrupt).is_err());
        corrupt = valid;
        corrupt[HEADER_BYTES + FONT_NAME_BYTES + 4..HEADER_BYTES + FONT_NAME_BYTES + 12]
            .copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(validate_records(&corrupt).is_err());
    }

    #[test]
    fn matrix_concat_preserves_pdf_user_space_origin() {
        let ctm = Matrix::IDENTITY.concat(Matrix([0.1, 0.0, 0.0, 0.1, 0.0, 0.0]));
        let text = Matrix([0.0, 1.0, -1.0, 0.0, 4778.0, 979.0]);
        let glyph = text.point(0.0, 0.0);
        assert_eq!(ctm.point(glyph[0], glyph[1]), [477.8, 97.9]);
    }
}
