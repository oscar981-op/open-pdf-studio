//! Narrow fixture adapter, not a general PDF interpreter.
#![forbid(unsafe_code)]

use lopdf::{content::Content, Document, Object, Stream};
use std::{env, error::Error, fs, path::Path};

const LIMIT: u32 = 1024;

fn fail(message: &str) -> Box<dyn Error> {
    message.to_owned().into()
}

fn number(object: &Object) -> Result<f64, Box<dyn Error>> {
    match object {
        Object::Integer(value) => Ok(*value as f64),
        Object::Real(value) => Ok(f64::from(*value)),
        _ => Err(fail("expected a PDF number")),
    }
}

fn exact_u32(value: f64) -> Result<u32, Box<dyn Error>> {
    if !value.is_finite() || value < 0.0 || value > f64::from(LIMIT) || value.fract() != 0.0 {
        return Err(fail(
            "only nonnegative integer fixture coordinates are supported",
        ));
    }
    Ok(value as u32)
}

fn operands<const N: usize>(items: &[Object]) -> Result<[f64; N], Box<dyn Error>> {
    if items.len() != N {
        return Err(fail("wrong PDF operator operand count"));
    }
    let mut result = [0.0; N];
    for (target, item) in result.iter_mut().zip(items) {
        *target = number(item)?;
    }
    Ok(result)
}

fn image_bytes<'a>(
    document: &'a Document,
    resources: &'a lopdf::Dictionary,
    name: &[u8],
    width: u32,
    height: u32,
) -> Result<&'a [u8], Box<dyn Error>> {
    if name != b"Im0" {
        return Err(fail("only /Im0 is supported"));
    }
    let xobjects = resources.get(b"XObject")?;
    let (_, xobjects) = document.dereference(xobjects)?;
    let xobjects = xobjects.as_dict()?;
    let reference = xobjects.get(name)?;
    let (_, image) = document.dereference(reference)?;
    let Stream { dict, content, .. } = image.as_stream()?;
    if dict.get(b"Subtype")?.as_name()? != b"Image"
        || dict.get(b"ColorSpace")?.as_name()? != b"DeviceRGB"
        || dict.get(b"BitsPerComponent")?.as_i64()? != 8
        || dict.get(b"Width")?.as_i64()? != i64::from(width)
        || dict.get(b"Height")?.as_i64()? != i64::from(height)
        || dict.has(b"Filter")
        || dict.has(b"SMask")
        || dict.has(b"Mask")
        || dict.has(b"Decode")
        || dict.has(b"Interpolate")
    {
        return Err(fail(
            "image must be an unfiltered, uninterpolated RGB8 full-page fixture",
        ));
    }
    if content.len() != width as usize * height as usize * 3 {
        return Err(fail("wrong RGB image byte length"));
    }
    Ok(content.as_slice())
}

fn page_to_opdl(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    let bytes = fs::read(path)?;
    if bytes.len() > 16_000_000 {
        return Err(fail("fixture PDF is too large"));
    }
    let document = Document::load_mem(&bytes)?;
    let pages = document.get_pages();
    if pages.len() != 1 {
        return Err(fail("fixture must have exactly one page"));
    }
    let page_id = *pages.values().next().ok_or_else(|| fail("missing page"))?;
    let page = document.get_dictionary(page_id)?;
    if page.has(b"Rotate") || page.has(b"CropBox") || page.has(b"Annots") {
        return Err(fail(
            "rotation, crop and annotations are not in this subset",
        ));
    }
    let media_box = page.get(b"MediaBox")?.as_array()?;
    let [x0, y0, x1, y1] = operands::<4>(media_box)?;
    if x0 != 0.0 || y0 != 0.0 {
        return Err(fail("fixture page must have zero origin"));
    }
    let width = exact_u32(x1)?;
    let height = exact_u32(y1)?;
    if width == 0 || height == 0 {
        return Err(fail("empty page"));
    }
    let (resources, inherited) = document.get_page_resources(page_id)?;
    if !inherited.is_empty() {
        return Err(fail("inherited resources are not in this subset"));
    }
    let resources = resources.ok_or_else(|| fail("missing resources"))?;
    let content = Content::decode(&document.get_page_content(page_id)?)?;
    let mut depth = 0_u8;
    let mut image_matrix = false;
    let mut image = None;
    let mut color = [0_u8; 3];
    let mut stroke_color = [0_u8; 3];
    let mut one_pixel_stroke = false;
    let mut pending_rect = None;
    let mut path_start = None;
    let mut pending_line = None;
    let mut commands: Vec<(u32, [u32; 4], [u8; 4])> = Vec::new();
    for op in &content.operations {
        match op.operator.as_str() {
            "q" if op.operands.is_empty()
                && depth == 0
                && commands.is_empty()
                && pending_rect.is_none()
                && path_start.is_none()
                && pending_line.is_none() =>
            {
                depth = 1
            }
            "Q" if op.operands.is_empty() && depth == 1 && image.is_some() => depth = 0,
            "cm" if depth == 1 && image.is_none() => {
                let [a, b, c, d, e, f] = operands::<6>(&op.operands)?;
                if [a, b, c, d, e, f] != [f64::from(width), 0.0, 0.0, f64::from(height), 0.0, 0.0] {
                    return Err(fail("only a full-page image transform is supported"));
                }
                image_matrix = true;
            }
            "Do" if depth == 1 && image_matrix && image.is_none() => {
                let [name] = op.operands.as_slice() else {
                    return Err(fail("Do requires one name"));
                };
                image = Some(image_bytes(
                    &document,
                    resources,
                    name.as_name()?,
                    width,
                    height,
                )?);
            }
            "rg" if depth == 0
                && pending_rect.is_none()
                && path_start.is_none()
                && pending_line.is_none() =>
            {
                let rgb = operands::<3>(&op.operands)?;
                for (target, channel) in color.iter_mut().zip(rgb) {
                    if !channel.is_finite() || !(0.0..=1.0).contains(&channel) {
                        return Err(fail("invalid RGB channel"));
                    }
                    *target = (channel * 255.0).round() as u8;
                }
            }
            "re" if depth == 0
                && pending_rect.is_none()
                && path_start.is_none()
                && pending_line.is_none() =>
            {
                let [x, y, w, h] = operands::<4>(&op.operands)?;
                let [x, y, w, h] = [exact_u32(x)?, exact_u32(y)?, exact_u32(w)?, exact_u32(h)?];
                if x + w > width || y + h > height {
                    return Err(fail("rectangle outside page"));
                }
                pending_rect = Some([x, height - y - h, x + w, height - y]);
            }
            "f" if depth == 0 && op.operands.is_empty() => {
                let rect = pending_rect
                    .take()
                    .ok_or_else(|| fail("fill without rectangle"))?;
                commands.push((1, rect, [color[0], color[1], color[2], 255]));
                if commands.len() > 64 {
                    return Err(fail("too many commands"));
                }
            }
            "w" if depth == 0
                && pending_rect.is_none()
                && path_start.is_none()
                && pending_line.is_none() =>
            {
                let [weight] = operands::<1>(&op.operands)?;
                if weight != 1.0 {
                    return Err(fail("only one-pixel butt-cap strokes are supported"));
                }
                one_pixel_stroke = true;
            }
            "RG" if depth == 0
                && pending_rect.is_none()
                && path_start.is_none()
                && pending_line.is_none() =>
            {
                let rgb = operands::<3>(&op.operands)?;
                for (target, channel) in stroke_color.iter_mut().zip(rgb) {
                    if !channel.is_finite() || !(0.0..=1.0).contains(&channel) {
                        return Err(fail("invalid stroke RGB channel"));
                    }
                    *target = (channel * 255.0).round() as u8;
                }
            }
            "m" if depth == 0
                && pending_rect.is_none()
                && path_start.is_none()
                && pending_line.is_none() =>
            {
                path_start = Some(operands::<2>(&op.operands)?);
            }
            "l" if depth == 0 && pending_line.is_none() => {
                let start = path_start.take().ok_or_else(|| fail("line without move"))?;
                let end = operands::<2>(&op.operands)?;
                pending_line = Some((start, end));
            }
            "S" if depth == 0 && op.operands.is_empty() && one_pixel_stroke => {
                let (start, end) = pending_line
                    .take()
                    .ok_or_else(|| fail("stroke without line"))?;
                let rect = if start[1] == end[1] {
                    let left = exact_u32(start[0].min(end[0]))?;
                    let right = exact_u32(start[0].max(end[0]))?;
                    let top = exact_u32(f64::from(height) - start[1] - 0.5)?;
                    [left, top, right, top + 1]
                } else if start[0] == end[0] {
                    let left = exact_u32(start[0] - 0.5)?;
                    let top = height - exact_u32(start[1].max(end[1]))?;
                    let bottom = height - exact_u32(start[1].min(end[1]))?;
                    [left, top, left + 1, bottom]
                } else {
                    return Err(fail("only horizontal or vertical strokes are supported"));
                };
                if rect[0] >= rect[2] || rect[1] >= rect[3] || rect[2] > width || rect[3] > height {
                    return Err(fail("stroke outside page or empty"));
                }
                commands.push((
                    3,
                    rect,
                    [stroke_color[0], stroke_color[1], stroke_color[2], 255],
                ));
                if commands.len() > 64 {
                    return Err(fail("too many commands"));
                }
            }
            _ => return Err(fail("PDF operator is outside the supported fixture subset")),
        }
    }
    if depth != 0
        || pending_rect.is_some()
        || path_start.is_some()
        || pending_line.is_some()
        || (image_matrix && image.is_none())
    {
        return Err(fail("incomplete fixture content stream"));
    }
    let image = image.unwrap_or_default();
    let mut output = Vec::with_capacity(40 + image.len() + commands.len() * 24);
    output.extend_from_slice(b"OPDL");
    for value in [2, width, height, 0, 0, width, height, image.len() as u32] {
        output.extend_from_slice(&value.to_le_bytes());
    }
    output.extend_from_slice(&image);
    output.extend_from_slice(&(commands.len() as u32).to_le_bytes());
    for (kind, rect, rgba) in commands {
        output.extend_from_slice(&kind.to_le_bytes());
        for value in rect {
            output.extend_from_slice(&value.to_le_bytes());
        }
        output.extend_from_slice(&rgba);
    }
    Ok(output)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or_else(|| fail("usage: opds-opdl-adapter INPUT.pdf OUTPUT.opdl"))?;
    let output = args.next().ok_or_else(|| fail("missing output path"))?;
    if args.next().is_some() {
        return Err(fail("extra argument"));
    }
    let converted = page_to_opdl(Path::new(&input))?;
    fs::write(output, converted)?;
    Ok(())
}
