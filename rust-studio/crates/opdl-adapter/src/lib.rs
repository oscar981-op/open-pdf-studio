//! Narrow fixture adapter, not a general PDF interpreter.
#![forbid(unsafe_code)]

use lopdf::{content::Content, Document, Object, Stream};
use std::{error::Error, fs, path::Path};

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

fn intersect_rect(rect: [u32; 4], clip: Option<[u32; 4]>) -> Option<[u32; 4]> {
    let [left, top, right, bottom] = rect;
    let [clip_left, clip_top, clip_right, clip_bottom] = clip.unwrap_or([0, 0, LIMIT, LIMIT]);
    let result = [
        left.max(clip_left),
        top.max(clip_top),
        right.min(clip_right),
        bottom.min(clip_bottom),
    ];
    (result[0] < result[2] && result[1] < result[3]).then_some(result)
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

pub fn page_to_opdl(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
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
    let mut translation: Option<[f64; 2]> = None;
    let mut image = None;
    let mut color = [0_u8; 3];
    let mut stroke_color = [0_u8; 3];
    let mut one_pixel_stroke = false;
    let mut pending_rect = None;
    let mut pending_clip = false;
    let mut clip = None;
    let mut saved_clips: Vec<Option<[u32; 4]>> = Vec::new();
    let mut path_start = None;
    let mut pending_line = None;
    let mut commands: Vec<(u32, [u32; 4], [u8; 4])> = Vec::new();
    for op in &content.operations {
        match op.operator.as_str() {
            "q" if op.operands.is_empty()
                && depth < 2
                && !image_matrix
                && translation.is_none()
                && pending_rect.is_none()
                && !pending_clip
                && path_start.is_none()
                && pending_line.is_none() =>
            {
                saved_clips.push(clip);
                depth += 1;
            }
            "Q" if op.operands.is_empty()
                && depth > 0
                && pending_rect.is_none()
                && !pending_clip
                && path_start.is_none()
                && pending_line.is_none()
                && (!image_matrix || image.is_some()) =>
            {
                clip = saved_clips
                    .pop()
                    .ok_or_else(|| fail("restore without save"))?;
                depth -= 1;
                translation = None;
                image_matrix = false;
            }
            "cm" if depth == 1 && !image_matrix && translation.is_none() => {
                let [a, b, c, d, e, f] = operands::<6>(&op.operands)?;
                if [a, b, c, d, e, f] == [f64::from(width), 0.0, 0.0, f64::from(height), 0.0, 0.0]
                    && image.is_none()
                    && commands.is_empty()
                    && clip.is_none()
                {
                    image_matrix = true;
                } else if [a, b, c, d] == [1.0, 0.0, 0.0, 1.0]
                    && e.fract() == 0.0
                    && f.fract() == 0.0
                    && e.abs() <= f64::from(LIMIT)
                    && f.abs() <= f64::from(LIMIT)
                {
                    translation = Some([e, f]);
                } else {
                    return Err(fail(
                        "only integer translation of vector rectangles and axis-aligned strokes is supported",
                    ));
                }
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
            "re" if !image_matrix
                && pending_rect.is_none()
                && path_start.is_none()
                && pending_line.is_none() =>
            {
                let [x, y, w, h] = operands::<4>(&op.operands)?;
                let [tx, ty] = translation.unwrap_or([0.0, 0.0]);
                let [x, y, w, h] = [
                    exact_u32(x + tx)?,
                    exact_u32(y + ty)?,
                    exact_u32(w)?,
                    exact_u32(h)?,
                ];
                if x + w > width || y + h > height {
                    return Err(fail("rectangle outside page"));
                }
                pending_rect = Some([x, height - y - h, x + w, height - y]);
            }
            "W" if !image_matrix
                && op.operands.is_empty()
                && pending_rect.is_some()
                && !pending_clip =>
            {
                pending_clip = true;
            }
            "n" if !image_matrix && op.operands.is_empty() && pending_clip => {
                let rect = pending_rect
                    .take()
                    .ok_or_else(|| fail("clip without rectangle"))?;
                clip = Some(intersect_rect(rect, clip).unwrap_or([0, 0, 0, 0]));
                pending_clip = false;
            }
            "f" if !image_matrix && op.operands.is_empty() && !pending_clip => {
                let rect = pending_rect
                    .take()
                    .ok_or_else(|| fail("fill without rectangle"))?;
                if let Some(rect) = intersect_rect(rect, clip) {
                    commands.push((1, rect, [color[0], color[1], color[2], 255]));
                }
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
            "m" if !image_matrix
                && pending_rect.is_none()
                && path_start.is_none()
                && pending_line.is_none() =>
            {
                let [x, y] = operands::<2>(&op.operands)?;
                let [tx, ty] = translation.unwrap_or([0.0, 0.0]);
                path_start = Some([x + tx, y + ty]);
            }
            "l" if !image_matrix && pending_line.is_none() => {
                let start = path_start.take().ok_or_else(|| fail("line without move"))?;
                let [x, y] = operands::<2>(&op.operands)?;
                let [tx, ty] = translation.unwrap_or([0.0, 0.0]);
                pending_line = Some((start, [x + tx, y + ty]));
            }
            "S" if !image_matrix && op.operands.is_empty() && one_pixel_stroke => {
                let (start, end) = pending_line
                    .take()
                    .ok_or_else(|| fail("stroke without line"))?;
                let (kind, rect) = if start[1] == end[1] {
                    let left = exact_u32(start[0].min(end[0]))?;
                    let right = exact_u32(start[0].max(end[0]))?;
                    let top = exact_u32(f64::from(height) - start[1] - 0.5)?;
                    (3, [left, top, right, top + 1])
                } else if start[0] == end[0] {
                    let left = exact_u32(start[0] - 0.5)?;
                    let top = height - exact_u32(start[1].max(end[1]))?;
                    let bottom = height - exact_u32(start[1].min(end[1]))?;
                    (3, [left, top, left + 1, bottom])
                } else if clip.is_none()
                    && end[0] > start[0]
                    && end[0] - start[0] == start[1] - end[1]
                    && start[0].fract() == 0.5
                    && start[1].fract() == 0.5
                    && end[0].fract() == 0.5
                    && end[1].fract() == 0.5
                {
                    let left = exact_u32(start[0] - 0.5)?;
                    let top = exact_u32(f64::from(height) - start[1] - 0.5)?;
                    let right = exact_u32(end[0] + 0.5)?;
                    let bottom = exact_u32(f64::from(height) - end[1] + 0.5)?;
                    (4, [left, top, right, bottom])
                } else if clip.is_none()
                    && end[0] > start[0]
                    && end[0] - start[0] == end[1] - start[1]
                    && start[0].fract() == 0.5
                    && start[1].fract() == 0.5
                    && end[0].fract() == 0.5
                    && end[1].fract() == 0.5
                {
                    let left = exact_u32(start[0] - 0.5)?;
                    let top = exact_u32(f64::from(height) - end[1] - 0.5)?;
                    let right = exact_u32(end[0] + 0.5)?;
                    let bottom = exact_u32(f64::from(height) - start[1] + 0.5)?;
                    (5, [left, top, right, bottom])
                } else {
                    return Err(fail("stroke outside supported axis or 45-degree subset"));
                };
                if rect[0] >= rect[2] || rect[1] >= rect[3] || rect[2] > width || rect[3] > height {
                    return Err(fail("stroke outside page or empty"));
                }
                if (kind == 4 || kind == 5)
                    && (rect[2] - rect[0] != rect[3] - rect[1] || rect[2] - rect[0] < 2)
                {
                    return Err(fail("invalid 45-degree stroke bounds"));
                }
                if let Some(rect) = intersect_rect(rect, clip) {
                    commands.push((
                        kind,
                        rect,
                        [stroke_color[0], stroke_color[1], stroke_color[2], 255],
                    ));
                }
                if commands.len() > 64 {
                    return Err(fail("too many commands"));
                }
            }
            _ => return Err(fail("PDF operator is outside the supported fixture subset")),
        }
    }
    if depth != 0
        || pending_rect.is_some()
        || pending_clip
        || path_start.is_some()
        || pending_line.is_some()
        || image_matrix
        || translation.is_some()
        || !saved_clips.is_empty()
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
