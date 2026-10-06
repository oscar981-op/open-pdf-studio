//! Bounded DynLex paint of validated imported annotation appearances.
//! Hayro still renders the page body and is an independent whole-page oracle.
//! The opt-in route does not interpret arbitrary PDF Form graphics operators.
use crate::{
    annotation_overlay::{
        native_imported_commands, paint_imported_commands_owned, paint_opaque_imported_rectangles,
        render_native_annotation_overlay_pixels_over_premultiplied, ImportedCommand,
    },
    glyph_scene::{
        render_validated_glyph_scene_over_premultiplied_at_origin_owned,
        render_validated_polygon_over_premultiplied_at_origin_owned, validate_polygon_outline,
    },
    imported_text::{stage_saved_native_freetext, StagedImportedText},
    Bitmap,
};
use lopdf::{content::Content, Dictionary, Document, Object, ObjectId};
use opds_engine_probe::{ParsedPdf, Raster};
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

pub(crate) fn display_point(crop: [f64; 4], rotation: u16, x: f64, y: f64) -> [f64; 2] {
    match rotation {
        0 => [x - crop[0], crop[3] - y],
        90 => [y - crop[1], x - crop[0]],
        180 => [crop[2] - x, y - crop[1]],
        270 => [crop[3] - y, crop[2] - x],
        _ => unreachable!(),
    }
}

fn pdf_point(crop: [f64; 4], rotation: u16, u: f64, v: f64) -> [f64; 2] {
    match rotation {
        0 => [crop[0] + u, crop[3] - v],
        90 => [crop[0] + v, crop[1] + u],
        180 => [crop[2] - u, crop[1] + v],
        270 => [crop[2] - v, crop[3] - u],
        _ => unreachable!(),
    }
}

fn native_highlight_resources(form: &lopdf::Stream) -> bool {
    let Ok(resources) = form.dict.get(b"Resources").and_then(Object::as_dict) else {
        return false;
    };
    if resources.len() != 1 {
        return false;
    }
    let Ok(states) = resources.get(b"ExtGState").and_then(Object::as_dict) else {
        return false;
    };
    if states.len() != 1 {
        return false;
    }
    let Ok(state) = states.get(b"GS0").and_then(Object::as_dict) else {
        return false;
    };
    state.len() == 2
        && name_is(state, b"Type", b"ExtGState")
        && state.get(b"ca").ok().and_then(number) == Some(f64::from(0.35_f32))
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

fn parse_shape_annotation(
    doc: &Document,
    annotation: &Dictionary,
    crop: [f64; 4],
    rotation: u16,
    page: usize,
    id: u64,
) -> Option<Annotation> {
    let [x0, y0, x1, y1] = crop;
    let highlight = name_is(annotation, b"Subtype", b"Highlight");
    if (!highlight && !name_is(annotation, b"Subtype", b"Square"))
        || annotation.get(b"F").ok().and_then(number) != Some(4.0)
        || annotation.has(b"OC")
        || annotation.has(b"AS")
    {
        return None;
    }
    let Some(rect) = annotation.get(b"Rect").ok().and_then(four) else {
        return None;
    };
    if rect[0] < x0
        || rect[1] < y0
        || rect[2] > x1
        || rect[3] > y1
        || rect.iter().any(|v| v.fract() != 0.0)
        || rect[2] - rect[0] < 4.0
        || rect[3] - rect[1] < 4.0
    {
        return None;
    }
    let Some(color_values) = annotation.get(b"C").ok().and_then(|v| v.as_array().ok()) else {
        return None;
    };
    let [red, green, blue] = color_values.as_slice() else {
        return None;
    };
    let Ok(ap) = annotation.get(b"AP").and_then(Object::as_dict) else {
        return None;
    };
    let Ok(normal) = ap.get(b"N") else {
        return None;
    };
    let (_, normal) = doc.dereference(normal).ok()?;
    let Ok(form) = normal.as_stream() else {
        return None;
    };
    if !name_is(&form.dict, b"Type", b"XObject")
        || !name_is(&form.dict, b"Subtype", b"Form")
        || form.dict.has(b"Filter")
        || form.dict.has(b"Matrix")
        || form.dict.has(b"Group")
        || (highlight && !native_highlight_resources(form))
        || (!highlight && form.dict.has(b"Resources"))
        || form.content.len() > 512
        || form.dict.get(b"BBox").ok().and_then(four)
            != Some([0.0, 0.0, rect[2] - rect[0], rect[3] - rect[1]])
    {
        return None;
    }
    let Ok(operations) = Content::decode(&form.content) else {
        return None;
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
            return None;
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
            return None;
        }
        let (dw, dh) = if matches!(rotation, 90 | 270) {
            (pdf_height, pdf_width)
        } else {
            (pdf_width, pdf_height)
        };
        (&ops[2..8], dw, dh)
    };
    let expected_ops = if highlight {
        ["q", "gs", "rg", "re", "f", "Q"]
    } else {
        ["q", "RG", "w", "re", "S", "Q"]
    };
    if ops.len() != 6
        || expected_ops
            .iter()
            .zip(ops)
            .any(|(expected, actual)| actual.operator != *expected)
        || !ops[0].operands.is_empty()
        || !ops[4].operands.is_empty()
        || !ops[5].operands.is_empty()
        || ops[1].operands.len() != if highlight { 1 } else { 3 }
        || ops[2].operands.len() != if highlight { 3 } else { 1 }
        || ops[3].operands.len() != 4
        || (highlight && ops[1].operands[0].as_name().ok() != Some(b"GS0"))
        || (!highlight && ops[2].operands.first().and_then(number) != Some(2.0))
    {
        return None;
    }
    let Some(path) = (|| {
        Some([
            number(&ops[3].operands[0])?,
            number(&ops[3].operands[1])?,
            number(&ops[3].operands[2])?,
            number(&ops[3].operands[3])?,
        ])
    })() else {
        return None;
    };
    if path
        != if highlight {
            [0.0, 0.0, display_width, display_height]
        } else {
            [1.0, 1.0, display_width - 2.0, display_height - 2.0]
        }
    {
        return None;
    }
    let color = if highlight {
        let expected_c = [1.0, 226.0 / 255.0, 67.0 / 255.0];
        let expected_rg = [1.0, 0.886, 0.263];
        let c = [number(red)?, number(green)?, number(blue)?];
        let rg = [
            number(&ops[2].operands[0])?,
            number(&ops[2].operands[1])?,
            number(&ops[2].operands[2])?,
        ];
        if c.iter()
            .zip(expected_c)
            .any(|(a, b)| (a - b).abs() > 0.000_001)
            || rg
                .iter()
                .zip(expected_rg)
                .any(|(a, b)| (a - b).abs() > 0.000_001)
        {
            return None;
        }
        [255, 226, 67]
    } else {
        let color = [
            rgb_channel(number(red)?, number(&ops[1].operands[0])?)?,
            rgb_channel(number(green)?, number(&ops[1].operands[1])?)?,
            rgb_channel(number(blue)?, number(&ops[1].operands[2])?)?,
        ];
        if color == [255, 226, 67] {
            return None;
        }
        color
    };
    let p0 = display_point(crop, rotation, rect[0], rect[1]);
    let p1 = display_point(crop, rotation, rect[2], rect[3]);
    if highlight {
        let corners = [
            pdf_point(crop, rotation, p0[0].min(p1[0]), p0[1].min(p1[1])),
            pdf_point(crop, rotation, p0[0].max(p1[0]), p0[1].min(p1[1])),
            pdf_point(crop, rotation, p0[0].min(p1[0]), p0[1].max(p1[1])),
            pdf_point(crop, rotation, p0[0].max(p1[0]), p0[1].max(p1[1])),
        ];
        let quad = annotation.get(b"QuadPoints").ok()?.as_array().ok()?;
        if quad.len() != 8
            || quad
                .iter()
                .zip(corners.iter().flat_map(|p| p.iter()))
                .any(|(actual, expected)| number(actual) != Some(*expected))
        {
            return None;
        }
    }
    let mark = Annotation {
        id,
        page,
        bounds: Rect {
            x: p0[0].min(p1[0]),
            y: p0[1].min(p1[1]),
            width: display_width,
            height: display_height,
        },
        kind: Kind::Rectangle { color_rgb: color },
    };
    Some(mark)
}

struct StagedPolygon {
    outline: Vec<u8>,
    color: [u8; 3],
    clip: [u32; 4],
    origin: [u32; 2],
}

fn clip_polygon_axis(
    polygon: &[[f64; 2]],
    axis: usize,
    boundary: f64,
    greater: bool,
) -> Vec<[f64; 2]> {
    if polygon.is_empty() {
        return Vec::new();
    }
    let mut output = Vec::with_capacity(polygon.len() + 2);
    let mut previous = *polygon.last().unwrap();
    let mut was_inside = if greater {
        previous[axis] >= boundary
    } else {
        previous[axis] <= boundary
    };
    for &point in polygon {
        let inside = if greater {
            point[axis] >= boundary
        } else {
            point[axis] <= boundary
        };
        if inside != was_inside {
            let fraction = (boundary - previous[axis]) / (point[axis] - previous[axis]);
            output.push([
                previous[0] + fraction * (point[0] - previous[0]),
                previous[1] + fraction * (point[1] - previous[1]),
            ]);
        }
        if inside {
            output.push(point);
        }
        previous = point;
        was_inside = inside;
    }
    output
}

/// The DynLex kernel rounds exact polygon area to a byte; Hayro/Vello uses
/// f32 edge strips. Near a half-byte threshold the two can choose opposite
/// bytes. Reject such paths before any appearance in the page is painted.
fn unambiguous_polygon_coverage(points: &[[f64; 2]], bounds: [u32; 4]) -> bool {
    for y in bounds[1]..bounds[3] {
        for x in bounds[0]..bounds[2] {
            let mut clipped = points.to_vec();
            for (axis, boundary, greater) in [
                (0, f64::from(x), true),
                (0, f64::from(x + 1), false),
                (1, f64::from(y), true),
                (1, f64::from(y + 1), false),
            ] {
                clipped = clip_polygon_axis(&clipped, axis, boundary, greater);
                if clipped.is_empty() {
                    break;
                }
            }
            if clipped.is_empty() {
                continue;
            }
            let mut signed = 0.0;
            for index in 0..clipped.len() {
                let a = clipped[index];
                let b = clipped[(index + 1) % clipped.len()];
                signed += a[0] * b[1] - b[0] * a[1];
            }
            let scaled = (signed.abs() * 0.5).min(1.0) * 255.0;
            if (scaled.fract() - 0.5).abs() < 0.002 {
                return false;
            }
        }
    }
    true
}

/// Stage one closed, line-only Form path for the existing validated DynLex
/// area kernel. The tile may start anywhere on a larger page; the Form box
/// remains its appearance clip. All points are on a quarter-pixel grid and
/// fully inside it.
fn stage_form_polygon(
    points: &[[f64; 2]],
    color: [u8; 3],
    rect: [f64; 4],
    crop: [f64; 4],
    rotation: u16,
) -> Option<StagedPolygon> {
    if !(3..=64).contains(&points.len())
        || points.iter().any(|point| {
            !point[0].is_finite()
                || !point[1].is_finite()
                || (point[0] * 4.0).fract() != 0.0
                || (point[1] * 4.0).fract() != 0.0
                || point[0] < 0.0
                || point[1] < 0.0
                || point[0] > rect[2] - rect[0]
                || point[1] > rect[3] - rect[1]
        })
    {
        return None;
    }
    let clip = pixel_box(crop, rotation, &[[rect[0], rect[1]], [rect[2], rect[3]]])?;
    if clip[2] > 4096 || clip[3] > 4096 {
        return None;
    }
    let mapped: Vec<_> = points
        .iter()
        .map(|point| display_point(crop, rotation, rect[0] + point[0], rect[1] + point[1]))
        .collect();
    let left = mapped.iter().map(|p| p[0]).reduce(f64::min)?;
    let top = mapped.iter().map(|p| p[1]).reduce(f64::min)?;
    let right = mapped.iter().map(|p| p[0]).reduce(f64::max)?;
    let bottom = mapped.iter().map(|p| p[1]).reduce(f64::max)?;
    if left < f64::from(clip[0])
        || top < f64::from(clip[1])
        || right > f64::from(clip[2])
        || bottom > f64::from(clip[3])
        || right <= left
        || bottom <= top
    {
        return None;
    }
    let origin = [left.floor() as u32, top.floor() as u32];
    if right - f64::from(origin[0]) > 256.0 || bottom - f64::from(origin[1]) > 192.0 {
        return None;
    }
    let local: Vec<_> = mapped
        .iter()
        .map(|point| {
            [
                point[0] - f64::from(origin[0]),
                point[1] - f64::from(origin[1]),
            ]
        })
        .collect();
    if !unambiguous_polygon_coverage(
        &local,
        [
            0,
            0,
            (right - f64::from(origin[0])).ceil() as u32,
            (bottom - f64::from(origin[1])).ceil() as u32,
        ],
    ) {
        return None;
    }
    let mut outline = Vec::with_capacity(24 + (points.len() + 1) * 16);
    outline.extend_from_slice(b"OPM2");
    for word in [1_u32, 256, 192, 1, (points.len() + 1) as u32] {
        outline.extend_from_slice(&word.to_le_bytes());
    }
    for point in local.iter().chain(local.first()) {
        outline.extend_from_slice(&point[0].to_le_bytes());
        outline.extend_from_slice(&point[1].to_le_bytes());
    }
    validate_polygon_outline(&outline).then_some(StagedPolygon {
        outline,
        color,
        clip,
        origin,
    })
}

/// A bounded Form appearance with opaque DeviceRGB rectangle fills, 2pt
/// outlines and small line-only filled paths. The paint order, rather than
/// `/Subtype`, determines the pixels. Other Form operators remain on Hayro.
fn parse_opaque_rect_form(
    doc: &Document,
    annotation: &Dictionary,
    crop: [f64; 4],
    rotation: u16,
) -> Option<Vec<ImportedAppearance>> {
    let subtype = annotation.get(b"Subtype").ok()?.as_name().ok()?;
    if subtype.is_empty()
        || subtype.len() > 32
        || annotation.get(b"F").ok().and_then(number) != Some(4.0)
        || annotation.has(b"OC")
        || annotation.has(b"AS")
    {
        return None;
    }
    let rect = four(annotation.get(b"Rect").ok()?)?;
    pixel_box(crop, rotation, &[[rect[0], rect[1]], [rect[2], rect[3]]])?;
    let ap = annotation.get(b"AP").ok()?.as_dict().ok()?;
    if ap.len() != 1 {
        return None;
    }
    let (_, normal) = doc.dereference(ap.get(b"N").ok()?).ok()?;
    let form = normal.as_stream().ok()?;
    let empty_resources = match form.dict.get(b"Resources") {
        Ok(value) => value.as_dict().ok()?.len() == 0,
        Err(_) => true,
    };
    let identity_matrix = match form.dict.get(b"Matrix") {
        Ok(value) => {
            value
                .as_array()
                .ok()?
                .iter()
                .map(number)
                .collect::<Option<Vec<_>>>()?
                == [1.0, 0.0, 0.0, 1.0, 0.0, 0.0]
        }
        Err(_) => true,
    };
    if !name_is(&form.dict, b"Type", b"XObject")
        || !name_is(&form.dict, b"Subtype", b"Form")
        || form.dict.has(b"Filter")
        || form.dict.has(b"DecodeParms")
        || form.dict.has(b"Group")
        || !identity_matrix
        || !empty_resources
        || form
            .dict
            .get(b"FormType")
            .ok()
            .is_some_and(|v| number(v) != Some(1.0))
        || form.dict.iter().any(|(key, _)| {
            !matches!(
                key.as_slice(),
                b"Type" | b"Subtype" | b"BBox" | b"Length" | b"Resources" | b"Matrix" | b"FormType"
            )
        })
        || form.content.len() > 4096
        || four(form.dict.get(b"BBox").ok()?)? != [0.0, 0.0, rect[2] - rect[0], rect[3] - rect[1]]
    {
        return None;
    }
    // lopdf's content decoder can accept an incomplete trailing operand.
    // lopdf may ignore trailing operands. Require an exact flat numeric
    // token stream before interpreting its operations; comments, strings,
    // compound operands and unknown graphics operators stay on Hayro.
    let tokens: Vec<_> = form
        .content
        .split(|byte| byte.is_ascii_whitespace())
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.len() < 12 || tokens[0] != b"q" || *tokens.last()? != b"Q" {
        return None;
    }
    let numeric = |token: &[u8]| {
        !token.is_empty()
            && token
                .iter()
                .all(|byte| byte.is_ascii_digit() || matches!(byte, b'.' | b'-' | b'+'))
    };
    let operations = Content::decode(&form.content).ok()?.operations;
    if operations.len() < 5 || operations.len() > 128 {
        return None;
    }
    let mut token_index = 0;
    for operation in &operations {
        let numeric_count = match operation.operator.as_str() {
            "q" | "Q" | "h" | "f" | "F" | "f*" | "S" | "s" | "B" | "B*" | "b" | "b*" => 0,
            "w" => 1,
            "m" | "l" => 2,
            "rg" | "RG" => 3,
            "re" => 4,
            _ => return None,
        };
        let Some(record) = tokens.get(token_index..token_index + numeric_count + 1) else {
            return None;
        };
        if operation.operands.len() != numeric_count
            || !record[..numeric_count].iter().all(|token| numeric(token))
            || record[numeric_count] != operation.operator.as_bytes()
        {
            return None;
        }
        token_index += numeric_count + 1;
    }
    if token_index != tokens.len()
        || operations.first()?.operator != "q"
        || operations.last()?.operator != "Q"
    {
        return None;
    }
    #[derive(Clone, Copy, Default)]
    struct FormState {
        fill: Option<[u8; 3]>,
        stroke: Option<[u8; 3]>,
        line_width: Option<f64>,
    }
    enum FormPath {
        Rect([f64; 4]),
        Polygon { points: Vec<[f64; 2]>, closed: bool },
    }
    let parse_color = |values: &[Object]| -> Option<[u8; 3]> {
        values
            .iter()
            .map(|value| {
                let value = number(value)?;
                let channel = (value * 255.0).round();
                let printed = (channel / 255.0 * 1000.0).round() / 1000.0;
                (value.is_finite()
                    && (0.0..=1.0).contains(&value)
                    && (value - printed).abs() <= 0.000_001)
                    .then_some(channel as u8)
            })
            .collect::<Option<Vec<_>>>()?
            .try_into()
            .ok()
    };
    let rect_command = |path: [f64; 4], stroke: bool, color: [u8; 3]| {
        let [x, y, width, height] = path;
        let inset = if stroke { 1.0 } else { 0.0 };
        if [x, y, width, height]
            .iter()
            .any(|v: &f64| !v.is_finite() || v.fract() != 0.0)
            || x < inset
            || y < inset
            || width < if stroke { 2.0 } else { 1.0 }
            || height < if stroke { 2.0 } else { 1.0 }
            || x + width + inset > rect[2] - rect[0]
            || y + height + inset > rect[3] - rect[1]
        {
            return None;
        }
        let bounds = pixel_box(
            crop,
            rotation,
            &[
                [rect[0] + x - inset, rect[1] + y - inset],
                [rect[0] + x + width + inset, rect[1] + y + height + inset],
            ],
        )?;
        Some(ImportedCommand {
            kind: if stroke { 2 } else { 1 },
            rect: bounds,
            tail: [color[0], color[1], color[2], 255],
        })
    };
    let mut appearances = Vec::new();
    let mut state = FormState::default();
    let mut stack = Vec::new();
    let mut pending_path = None;
    for (index, operation) in operations.iter().enumerate() {
        match operation.operator.as_str() {
            "q" => {
                if pending_path.is_some() || stack.len() >= 4 {
                    return None;
                }
                stack.push(state);
            }
            "Q" => {
                if pending_path.is_some() || (stack.len() == 1 && index + 1 != operations.len()) {
                    return None;
                }
                state = stack.pop()?;
            }
            "rg" | "RG" => {
                if stack.is_empty() || pending_path.is_some() {
                    return None;
                }
                let color = parse_color(&operation.operands)?;
                if operation.operator == "rg" {
                    state.fill = Some(color);
                } else {
                    state.stroke = Some(color);
                }
            }
            "w" => {
                if stack.is_empty()
                    || pending_path.is_some()
                    || operation.operands.first().and_then(number) != Some(2.0)
                {
                    return None;
                }
                state.line_width = Some(2.0);
            }
            "re" => {
                if stack.is_empty() || pending_path.is_some() {
                    return None;
                }
                pending_path = Some(FormPath::Rect(
                    operation
                        .operands
                        .iter()
                        .map(number)
                        .collect::<Option<Vec<_>>>()?
                        .try_into()
                        .ok()?,
                ));
            }
            "m" => {
                if stack.is_empty() || pending_path.is_some() {
                    return None;
                }
                let point = operation
                    .operands
                    .iter()
                    .map(number)
                    .collect::<Option<Vec<_>>>()?
                    .try_into()
                    .ok()?;
                pending_path = Some(FormPath::Polygon {
                    points: vec![point],
                    closed: false,
                });
            }
            "l" => {
                let Some(FormPath::Polygon {
                    points,
                    closed: false,
                }) = &mut pending_path
                else {
                    return None;
                };
                if points.len() >= 64 {
                    return None;
                }
                points.push(
                    operation
                        .operands
                        .iter()
                        .map(number)
                        .collect::<Option<Vec<_>>>()?
                        .try_into()
                        .ok()?,
                );
            }
            "h" => {
                let Some(FormPath::Polygon { closed, .. }) = &mut pending_path else {
                    return None;
                };
                if *closed {
                    return None;
                }
                *closed = true;
            }
            "f" | "F" | "f*" | "S" | "s" | "B" | "B*" | "b" | "b*" => {
                if stack.is_empty() {
                    return None;
                }
                let path = pending_path.take()?;
                match path {
                    FormPath::Rect(path) => {
                        if matches!(
                            operation.operator.as_str(),
                            "f" | "F" | "f*" | "B" | "B*" | "b" | "b*"
                        ) {
                            appearances.push(ImportedAppearance::Shapes(vec![rect_command(
                                path,
                                false,
                                state.fill?,
                            )?]));
                        }
                        if matches!(
                            operation.operator.as_str(),
                            "S" | "s" | "B" | "B*" | "b" | "b*"
                        ) {
                            if state.line_width != Some(2.0) {
                                return None;
                            }
                            appearances.push(ImportedAppearance::Shapes(vec![rect_command(
                                path,
                                true,
                                state.stroke?,
                            )?]));
                        }
                    }
                    FormPath::Polygon { points, .. } => {
                        if !matches!(operation.operator.as_str(), "f" | "F" | "f*") {
                            return None;
                        }
                        appearances.push(ImportedAppearance::Polygon(stage_form_polygon(
                            &points,
                            state.fill?,
                            rect,
                            crop,
                            rotation,
                        )?));
                    }
                }
                if appearances.len() > 32 {
                    return None;
                }
            }
            _ => return None,
        }
    }
    (stack.is_empty() && pending_path.is_none() && !appearances.is_empty()).then_some(appearances)
}

/// Return a DynLex-composited page only for one exact, opaque 2pt Square Form
/// on a quarter-turned, integral page. Any uncertainty stays on Hayro.
pub fn render_single_square_appearance_direct(
    source: &[u8],
    page_number: usize,
) -> Result<Option<Bitmap>, String> {
    render_shape_appearances_inner(source, page_number, 1, false)
}

/// Paint up to 64 validated imported Square Forms in PDF annotation order.
pub fn render_square_appearances_direct(
    source: &[u8],
    page_number: usize,
) -> Result<Option<Bitmap>, String> {
    render_shape_appearances_inner(source, page_number, 64, false)
}

/// Paint validated native-shaped Square and Highlight Forms in PDF order.
/// This is a read-only parity probe, not the app's general AP renderer.
pub fn render_native_shape_appearances_direct(
    source: &[u8],
    page_number: usize,
) -> Result<Option<Bitmap>, String> {
    render_shape_appearances_inner(source, page_number, 64, true)
}

/// App candidate: render the bare premultiplied PDF content once, then paint
/// fully validated imported forms in DynLex before adding white paper.
/// The read-only `*_direct` oracle remains separate for parity tests.
pub fn render_native_shape_appearances_for_app(
    pdf: &ParsedPdf,
    source: &[u8],
    page_number: usize,
) -> Result<Option<Raster>, String> {
    let Some(marks) = stage_shape_appearances(source, page_number, 64, true)? else {
        return Ok(None);
    };
    paint_staged_shapes(pdf, &marks, page_number)
}

fn paint_staged_shapes(
    pdf: &ParsedPdf,
    marks: &[Annotation],
    page_number: usize,
) -> Result<Option<Raster>, String> {
    let mut layer = pdf
        .render_page_content_without_annotations(page_number, 1.0)
        .map_err(|e| e.to_string())?;
    let started = std::time::Instant::now();
    let Some(painted) = render_native_annotation_overlay_pixels_over_premultiplied(
        &layer.rgba,
        layer.width,
        layer.height,
        (layer.page_width_pt, layer.page_height_pt),
        marks,
        page_number - 1,
    )?
    else {
        return Ok(None);
    };
    layer.rgba = painted.rgba;
    add_white_paper(&mut layer.rgba);
    layer.render_ms += started.elapsed().as_millis();
    Ok(Some(layer))
}

fn add_white_paper(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let paper = 255 - pixel[3];
        for channel in &mut pixel[..3] {
            *channel = channel.saturating_add(paper);
        }
        pixel[3] = 255;
    }
}

pub(crate) fn page_geometry(
    doc: &Document,
    page_number: usize,
) -> Option<(ObjectId, [f64; 4], u16)> {
    let page_number_u32 = u32::try_from(page_number).ok()?;
    let page_id = *doc.get_pages().get(&page_number_u32)?;
    let page = doc.get_dictionary(page_id).ok()?;
    if page.has(b"Group") || !matches!(inherited(doc, page_id, b"UserUnit"), Ok(None)) {
        return None;
    }
    let media = four(inherited(doc, page_id, b"MediaBox").ok()??)?;
    let crop = match inherited(doc, page_id, b"CropBox").ok()? {
        Some(value) => four(value)?,
        None => media,
    };
    let rotation = match inherited(doc, page_id, b"Rotate").ok()? {
        Some(value) => {
            let value = number(value)?;
            if !value.is_finite() || value.fract() != 0.0 {
                return None;
            }
            value.rem_euclid(360.0) as u16
        }
        None => 0,
    };
    if !matches!(rotation, 0 | 90 | 180 | 270) {
        return None;
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
        return None;
    }
    Some((page_id, crop, rotation))
}

fn fallback_color(annotation: &Dictionary, default: [u8; 3]) -> Option<[u8; 3]> {
    let Ok(value) = annotation.get(b"C") else {
        return Some(default);
    };
    let [r, g, b] = value.as_array().ok()?.as_slice() else {
        return None;
    };
    [r, g, b]
        .map(|channel| {
            let value = number(channel)?;
            (value.is_finite() && (0.0..=1.0).contains(&value))
                .then_some((value * 255.0).round() as u8)
        })
        .into_iter()
        .collect::<Option<Vec<_>>>()?
        .try_into()
        .ok()
}

fn pixel_box(crop: [f64; 4], rotation: u16, points: &[[f64; 2]]) -> Option<[u32; 4]> {
    let mapped: Vec<_> = points
        .iter()
        .map(|point| display_point(crop, rotation, point[0], point[1]))
        .collect();
    let min_x = mapped.iter().map(|p| p[0]).reduce(f64::min)?;
    let max_x = mapped.iter().map(|p| p[0]).reduce(f64::max)?;
    let min_y = mapped.iter().map(|p| p[1]).reduce(f64::min)?;
    let max_y = mapped.iter().map(|p| p[1]).reduce(f64::max)?;
    let (width, height) = if matches!(rotation, 90 | 270) {
        (crop[3] - crop[1], crop[2] - crop[0])
    } else {
        (crop[2] - crop[0], crop[3] - crop[1])
    };
    if min_x < 0.0
        || min_y < 0.0
        || max_x > width
        || max_y > height
        || min_x >= max_x
        || min_y >= max_y
        || [min_x, min_y, max_x, max_y]
            .iter()
            .any(|value| !value.is_finite() || value.fract() != 0.0)
    {
        return None;
    }
    Some([min_x as u32, min_y as u32, max_x as u32, max_y as u32])
}

fn fallback_stroke_width(annotation: &Dictionary) -> Option<u32> {
    let width = annotation
        .get(b"BS")
        .ok()
        .and_then(|value| value.as_dict().ok())
        .and_then(|dict| dict.get(b"W").ok())
        .and_then(number)
        .or_else(|| {
            annotation
                .get(b"Border")
                .ok()?
                .as_array()
                .ok()?
                .get(2)
                .and_then(number)
        })
        .unwrap_or(1.0);
    (width.is_finite() && (1.0..=20.0).contains(&width) && width.fract() == 0.0)
        .then_some(width as u32)
}

fn parse_missing_shape_annotation(
    annotation: &Dictionary,
    crop: [f64; 4],
    rotation: u16,
) -> Option<Vec<([u32; 4], [u8; 3])>> {
    let mut rectangles = Vec::new();
    let flags = annotation.get(b"F").ok().and_then(number).unwrap_or(0.0);
    if !flags.is_finite() || flags.fract() != 0.0 || flags < 0.0 || flags > i32::MAX as f64 {
        return None;
    }
    if (flags as i32) & (1 | 2 | 32) != 0 {
        return Some(Vec::new());
    }
    let Some(rect) = annotation.get(b"Rect").ok().and_then(four) else {
        return None;
    };
    if rect[0] >= rect[2] || rect[1] >= rect[3] {
        return None;
    }
    let Some(box_) = pixel_box(crop, rotation, &[[rect[0], rect[1]], [rect[2], rect[3]]]) else {
        return None;
    };
    if name_is(annotation, b"Subtype", b"Square") {
        let Some(stroke) = fallback_stroke_width(annotation) else {
            return None;
        };
        let Some(color) = fallback_color(annotation, [0, 0, 0]) else {
            return None;
        };
        let [x0, y0, x1, y1] = box_;
        for bounds in [
            [x0, y0, x1, (y0 + stroke).min(y1)],
            [x0, y1.saturating_sub(stroke).max(y0), x1, y1],
            [x0, y0, (x0 + stroke).min(x1), y1],
            [x1.saturating_sub(stroke).max(x0), y0, x1, y1],
        ] {
            rectangles.push((bounds, color));
        }
    } else if name_is(annotation, b"Subtype", b"Highlight") {
        let Some(color) = fallback_color(annotation, [255, 255, 0]) else {
            return None;
        };
        let quad = match annotation.get(b"QuadPoints") {
            Ok(value) => {
                let Ok(values) = value.as_array() else {
                    return None;
                };
                if values.len() != 8 {
                    return None;
                }
                let Some(points) = values
                    .iter()
                    .map(number)
                    .collect::<Option<Vec<_>>>()
                    .and_then(|v| {
                        (v.iter().all(|n| n.is_finite())).then_some([
                            [v[0], v[1]],
                            [v[2], v[3]],
                            [v[6], v[7]],
                            [v[4], v[5]],
                        ])
                    })
                else {
                    return None;
                };
                points
            }
            Err(_) => [
                [rect[0], rect[3]],
                [rect[2], rect[3]],
                [rect[2], rect[1]],
                [rect[0], rect[1]],
            ],
        };
        let Some(quad_box) = pixel_box(crop, rotation, &quad) else {
            return None;
        };
        let [qx0, qy0, qx1, qy1] = quad_box;
        let corners = [[qx0, qy0], [qx1, qy0], [qx1, qy1], [qx0, qy1]];
        let transformed: Vec<_> = quad
            .iter()
            .map(|p| display_point(crop, rotation, p[0], p[1]))
            .collect();
        if transformed.iter().any(|p| {
            !corners
                .iter()
                .any(|corner| p[0] == f64::from(corner[0]) && p[1] == f64::from(corner[1]))
        }) || corners.iter().any(|corner| {
            transformed
                .iter()
                .filter(|p| p[0] == f64::from(corner[0]) && p[1] == f64::from(corner[1]))
                .count()
                != 1
        }) || {
            let crosses: Vec<_> = (0..4)
                .map(|i| {
                    let a = transformed[i];
                    let b = transformed[(i + 1) % 4];
                    let c = transformed[(i + 2) % 4];
                    (b[0] - a[0]) * (c[1] - b[1]) - (b[1] - a[1]) * (c[0] - b[0])
                })
                .collect();
            crosses
                .iter()
                .any(|cross| *cross == 0.0 || cross.signum() != crosses[0].signum())
        } || qx0 < box_[0]
            || qy0 < box_[1]
            || qx1 > box_[2]
            || qy1 > box_[3]
        {
            return None;
        }
        rectangles.push((quad_box, color));
    } else {
        return None;
    }
    Some(rectangles)
}

fn stage_missing_shape_appearances(
    source: &[u8],
    page_number: usize,
) -> Result<Option<Vec<([u32; 4], [u8; 3])>>, String> {
    if source.len() > 2_000_000 || page_number == 0 {
        return Ok(None);
    }
    let doc = Document::load_mem(source).map_err(|e| e.to_string())?;
    if doc.is_encrypted() {
        return Ok(None);
    }
    let Some((page_id, crop, rotation)) = page_geometry(&doc, page_number) else {
        return Ok(None);
    };
    let annotations = doc
        .get_page_annotations(page_id)
        .map_err(|e| e.to_string())?;
    if annotations.is_empty() || annotations.len() > 64 {
        return Ok(None);
    }
    let mut painted = Vec::new();
    for annotation in annotations {
        if annotation.has(b"AP") {
            return Ok(None);
        }
        let Some(rectangles) = parse_missing_shape_annotation(annotation, crop, rotation) else {
            return Ok(None);
        };
        painted.extend(rectangles);
    }
    Ok((!painted.is_empty() && painted.len() <= 256).then_some(painted))
}

/// Preview-only imported Square/Highlight annotations without `/AP`. The
/// constrained OPAN fill list reproduces the existing Hayro-preview fallback.
pub fn render_missing_shape_appearances_for_app(
    pdf: &ParsedPdf,
    source: &[u8],
    page_number: usize,
) -> Result<Option<Raster>, String> {
    let Some(rectangles) = stage_missing_shape_appearances(source, page_number)? else {
        return Ok(None);
    };
    let mut layer = pdf
        .render_page_content_without_annotations(page_number, 1.0)
        .map_err(|e| e.to_string())?;
    let started = std::time::Instant::now();
    let Some(painted) = paint_opaque_imported_rectangles(
        &layer.rgba,
        layer.width,
        layer.height,
        (layer.page_width_pt, layer.page_height_pt),
        &rectangles,
    )?
    else {
        return Ok(None);
    };
    layer.rgba = painted.rgba;
    add_white_paper(&mut layer.rgba);
    layer.render_ms += started.elapsed().as_millis();
    Ok(Some(layer))
}

enum ImportedAppearance {
    Shapes(Vec<ImportedCommand>),
    Polygon(StagedPolygon),
    Text(StagedImportedText),
}

fn flush_shapes(layer: &mut Raster, commands: &mut Vec<ImportedCommand>) -> Result<bool, String> {
    if commands.is_empty() {
        return Ok(true);
    }
    let Some(painted) = paint_imported_commands_owned(
        std::mem::take(&mut layer.rgba),
        layer.width,
        layer.height,
        (layer.page_width_pt, layer.page_height_pt),
        commands,
    )?
    else {
        return Ok(false);
    };
    layer.rgba = painted.rgba;
    commands.clear();
    Ok(true)
}

/// Ordered, bounded scene for validated native-shaped `/AP` Forms and the
/// native FreeText glyph Form. Preview-only Square/Highlight dictionaries
/// without `/AP` remain after all Forms to match Hayro's visible behavior.
/// Hayro's current preview paints fallback marks after all Forms. Consecutive
/// shape commands share one OPAN call; FreeText glyph tiles split those batches
/// while preserving their PDF appearance order.
pub fn render_imported_shape_appearances_for_app(
    pdf: &ParsedPdf,
    source: &[u8],
    page_number: usize,
) -> Result<Option<Raster>, String> {
    let started = std::time::Instant::now();
    if source.len() > 16_000_000 || page_number == 0 {
        return Ok(None);
    }
    let doc = Document::load_mem(source).map_err(|e| e.to_string())?;
    if doc.is_encrypted() {
        return Ok(None);
    }
    let Some((page_id, crop, rotation)) = page_geometry(&doc, page_number) else {
        return Ok(None);
    };
    let annotations = doc
        .get_page_annotations(page_id)
        .map_err(|e| e.to_string())?;
    if annotations.is_empty() || annotations.len() > 64 {
        return Ok(None);
    }
    let mut appearances = Vec::new();
    let mut fallback_rectangles = Vec::new();
    for (index, annotation) in annotations.iter().enumerate() {
        if annotation.has(b"AP") {
            if let Some(mark) = parse_shape_annotation(
                &doc,
                annotation,
                crop,
                rotation,
                page_number - 1,
                index as u64 + 1,
            ) {
                let Some(native) = native_imported_commands(&mark, 4096, 4096) else {
                    return Ok(None);
                };
                appearances.push(ImportedAppearance::Shapes(native));
            } else if let Some(commands) = parse_opaque_rect_form(&doc, annotation, crop, rotation)
            {
                appearances.extend(commands);
            } else if let Some(text) = stage_saved_native_freetext(&doc, annotation, crop, rotation)
            {
                appearances.push(ImportedAppearance::Text(text));
            } else {
                return Ok(None);
            }
        } else {
            let Some(rectangles) = parse_missing_shape_annotation(annotation, crop, rotation)
            else {
                return Ok(None);
            };
            fallback_rectangles.extend(rectangles);
        }
    }
    if fallback_rectangles.len() > 256 {
        return Ok(None);
    }
    if !fallback_rectangles.is_empty() {
        appearances.push(ImportedAppearance::Shapes(
            fallback_rectangles
                .into_iter()
                .map(|(rect, color)| ImportedCommand {
                    kind: 1,
                    rect,
                    tail: [color[0], color[1], color[2], 255],
                })
                .collect(),
        ));
    }
    let mut layer = pdf
        .render_page_content_without_annotations(page_number, 1.0)
        .map_err(|e| e.to_string())?;
    let mut pending_shapes = Vec::new();
    for appearance in appearances {
        match appearance {
            ImportedAppearance::Shapes(commands) => pending_shapes.extend(commands),
            ImportedAppearance::Polygon(polygon) => {
                if !flush_shapes(&mut layer, &mut pending_shapes)? {
                    return Ok(None);
                }
                let base = std::mem::take(&mut layer.rgba);
                layer.rgba = render_validated_polygon_over_premultiplied_at_origin_owned(
                    &polygon.outline,
                    polygon.color,
                    base,
                    layer.width,
                    layer.height,
                    polygon.clip,
                    polygon.origin,
                )?
                .rgba;
            }
            ImportedAppearance::Text(text) => {
                if !flush_shapes(&mut layer, &mut pending_shapes)?
                    || text.page_size != [layer.width, layer.height]
                {
                    return Ok(None);
                }
                for (scene, origin) in text.tiles {
                    let base = std::mem::take(&mut layer.rgba);
                    layer.rgba = render_validated_glyph_scene_over_premultiplied_at_origin_owned(
                        &scene,
                        base,
                        layer.width,
                        layer.height,
                        text.clip,
                        origin,
                    )?
                    .rgba;
                }
            }
        }
    }
    if !flush_shapes(&mut layer, &mut pending_shapes)? {
        return Ok(None);
    }
    add_white_paper(&mut layer.rgba);
    layer.render_ms = started.elapsed().as_millis();
    Ok(Some(layer))
}

fn stage_shape_appearances(
    source: &[u8],
    page_number: usize,
    max_annotations: usize,
    allow_highlights: bool,
) -> Result<Option<Vec<Annotation>>, String> {
    if source.len() > 2_000_000 || page_number == 0 {
        return Ok(None);
    }
    let doc = Document::load_mem(source).map_err(|e| e.to_string())?;
    if doc.is_encrypted() {
        return Ok(None);
    }
    let Some((page_id, crop, rotation)) = page_geometry(&doc, page_number) else {
        return Ok(None);
    };
    let annotations = doc
        .get_page_annotations(page_id)
        .map_err(|e| e.to_string())?;
    if annotations.is_empty() || annotations.len() > max_annotations {
        return Ok(None);
    }
    let mut marks = Vec::with_capacity(annotations.len());
    for (index, annotation) in annotations.iter().enumerate() {
        let Some(mark) = parse_shape_annotation(
            &doc,
            annotation,
            crop,
            rotation,
            page_number - 1,
            index as u64 + 1,
        ) else {
            return Ok(None);
        };
        if !allow_highlights
            && matches!(
                mark.kind,
                Kind::Rectangle {
                    color_rgb: [255, 226, 67]
                }
            )
        {
            return Ok(None);
        }
        marks.push(mark);
    }
    Ok(Some(marks))
}

fn render_shape_appearances_inner(
    source: &[u8],
    page_number: usize,
    max_annotations: usize,
    allow_highlights: bool,
) -> Result<Option<Bitmap>, String> {
    let Some(marks) =
        stage_shape_appearances(source, page_number, max_annotations, allow_highlights)?
    else {
        return Ok(None);
    };
    let original = ParsedPdf::open(Arc::new(source.to_vec())).map_err(|e| e.to_string())?;
    let Some(actual) = paint_staged_shapes(&original, &marks, page_number)? else {
        return Ok(None);
    };
    let expected = original
        .render_page(page_number, 1.0)
        .map_err(|e| e.to_string())?;
    Ok((actual.width == expected.width
        && actual.height == expected.height
        && actual.rgba == expected.rgba)
        .then_some(Bitmap {
            width: actual.width,
            height: actual.height,
            rgba: actual.rgba,
        }))
}

#[cfg(test)]
mod missing_appearance_tests {
    use super::*;
    use crate::{render_with_report, Backend};
    use lopdf::{dictionary, Stream};

    fn opaque_stamp_fixture(rotation: i64, include_square: bool) -> Vec<u8> {
        let mut doc = Document::with_version("1.4");
        let pages_id = doc.new_object_id();
        let content_id = doc.add_object(Stream::new(
            Dictionary::new(),
            b"0 0 1 rg 40 60 160 240 re f".to_vec(),
        ));
        let form_id = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 40.into(), 30.into()],
            },
            b"q 1 0 0 rg 0 0 40 30 re f Q".to_vec(),
        ));
        let stamp_id = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Stamp", "F" => 4,
            "Rect" => vec![70.into(), 90.into(), 110.into(), 120.into()],
            "AP" => dictionary! { "N" => form_id },
        });
        let mut annotations = vec![Object::Reference(stamp_id)];
        if include_square {
            let square_form = doc.add_object(Stream::new(
                dictionary! {
                    "Type" => "XObject", "Subtype" => "Form",
                    "BBox" => vec![0.into(), 0.into(), 40.into(), 30.into()],
                },
                b"q 0 1 0 RG 2 w 1 1 38 28 re S Q".to_vec(),
            ));
            let square_id = doc.add_object(dictionary! {
                "Type" => "Annot", "Subtype" => "Square", "F" => 4,
                "Rect" => vec![80.into(), 100.into(), 120.into(), 130.into()],
                "C" => vec![0.into(), 1.into(), 0.into()],
                "AP" => dictionary! { "N" => square_form },
            });
            annotations.push(Object::Reference(square_id));
        }
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            "Annots" => annotations,
            "MediaBox" => vec![20.into(),30.into(),220.into(),330.into()],
            "CropBox" => vec![40.into(),60.into(),200.into(),300.into()],
            "Rotate" => rotation,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![Object::Reference(page_id)], "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", Object::Reference(catalog_id));
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn opaque_stamp_form_matches_hayro_with_rotated_crop_and_square_order() {
        for rotation in [0, 90, 180, 270] {
            for include_square in if rotation == 0 {
                &[false, true][..]
            } else {
                &[false][..]
            } {
                let bytes = opaque_stamp_fixture(rotation, *include_square);
                let source_doc = Document::load_mem(&bytes).unwrap();
                let source_page = *source_doc.get_pages().get(&1).unwrap();
                let source_stamp = source_doc.get_page_annotations(source_page).unwrap()[0];
                let (_, crop, page_rotation) = page_geometry(&source_doc, 1).unwrap();
                assert!(
                    parse_opaque_rect_form(&source_doc, source_stamp, crop, page_rotation)
                        .is_some(),
                    "stage rotation={rotation}"
                );
                let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
                let expected = pdf.render_page(1, 1.0).unwrap();
                let selected = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
                assert!(
                    selected.imported_annotations_dynlex,
                    "rotation={rotation}, square={include_square}"
                );
                assert_eq!(
                    selected.raster.rgba, expected.rgba,
                    "rotation={rotation}, square={include_square}"
                );
            }
        }
    }

    #[test]
    fn ordered_opaque_form_fills_are_subtype_independent() {
        for rotation in [0, 90, 180, 270] {
            for subtype in ["Stamp", "Circle", "Ink", "Widget"] {
                let bytes = opaque_stamp_fixture(rotation, false);
                let mut doc = Document::load_mem(&bytes).unwrap();
                let page_id = *doc.get_pages().get(&1).unwrap();
                let first_id = doc
                    .get_dictionary(page_id)
                    .unwrap()
                    .get(b"Annots")
                    .unwrap()
                    .as_array()
                    .unwrap()[0]
                    .as_reference()
                    .unwrap();
                let form_id = doc
                    .get_dictionary(first_id)
                    .unwrap()
                    .get(b"AP")
                    .unwrap()
                    .as_dict()
                    .unwrap()
                    .get(b"N")
                    .unwrap()
                    .as_reference()
                    .unwrap();
                doc.get_dictionary_mut(first_id)
                    .unwrap()
                    .set("Subtype", Object::Name(subtype.as_bytes().to_vec()));
                let form = doc
                    .get_object_mut(form_id)
                    .unwrap()
                    .as_stream_mut()
                    .unwrap();
                form.content =
                    b"q 1 0 0 rg 0 0 40 30 re f 0.906 0.298 0.235 rg 10 5 20 10 re f Q".to_vec();
                form.dict.set("Length", form.content.len() as i64);
                let second_form = doc.add_object(Stream::new(
                    dictionary! {
                        "Type" => "XObject", "Subtype" => "Form",
                        "BBox" => vec![0.into(), 0.into(), 32.into(), 20.into()],
                        "FormType" => 1,
                        "Matrix" => vec![1.into(),0.into(),0.into(),1.into(),0.into(),0.into()],
                        "Resources" => dictionary! {},
                    },
                    b"q 0 0 1 rg 0 0 32 20 re f Q".to_vec(),
                ));
                let second_id = doc.add_object(dictionary! {
                    "Type" => "Annot", "Subtype" => "Underline", "F" => 4,
                    "Rect" => vec![80.into(), 100.into(), 112.into(), 120.into()],
                    "AP" => dictionary! { "N" => second_form },
                });
                for order in [
                    vec![Object::Reference(first_id), Object::Reference(second_id)],
                    vec![Object::Reference(second_id), Object::Reference(first_id)],
                ] {
                    doc.get_dictionary_mut(page_id)
                        .unwrap()
                        .set("Annots", order);
                    let mut source = Vec::new();
                    doc.save_to(&mut source).unwrap();
                    let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
                    let expected = pdf.render_page(1, 1.0).unwrap();
                    let actual =
                        render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
                    assert!(actual.imported_annotations_dynlex, "{subtype} {rotation}");
                    assert_eq!(actual.raster.rgba, expected.rgba, "{subtype} {rotation}");
                }
            }
        }
    }

    #[test]
    fn mixed_form_fill_and_stroke_preserve_paint_order() {
        for rotation in [0, 90, 180, 270] {
            for (content, subtype) in [
                (
                    &b"q 1 0 0 rg 0 0 40 30 re f 0 1 0 RG 2 w 5 5 30 20 re S Q"[..],
                    "Ink",
                ),
                (
                    &b"q 0 1 0 RG 2 w 5 5 30 20 re S 1 0 0 rg 10 5 20 15 re f Q"[..],
                    "Widget",
                ),
            ] {
                let bytes = opaque_stamp_fixture(rotation, false);
                let mut doc = Document::load_mem(&bytes).unwrap();
                let page_id = *doc.get_pages().get(&1).unwrap();
                let annotation_id = doc
                    .get_dictionary(page_id)
                    .unwrap()
                    .get(b"Annots")
                    .unwrap()
                    .as_array()
                    .unwrap()[0]
                    .as_reference()
                    .unwrap();
                let form_id = doc
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
                doc.get_dictionary_mut(annotation_id)
                    .unwrap()
                    .set("Subtype", Object::Name(subtype.as_bytes().to_vec()));
                let form = doc
                    .get_object_mut(form_id)
                    .unwrap()
                    .as_stream_mut()
                    .unwrap();
                form.content = content.to_vec();
                form.dict.set("Length", form.content.len() as i64);
                let mut source = Vec::new();
                doc.save_to(&mut source).unwrap();
                let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
                let expected = pdf.render_page(1, 1.0).unwrap();
                let actual = render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
                assert!(actual.imported_annotations_dynlex, "{subtype} {rotation}");
                assert_eq!(actual.raster.rgba, expected.rgba, "{subtype} {rotation}");
            }
        }
    }

    #[test]
    fn form_graphics_state_reuse_nested_restore_and_combined_paint_match_hayro() {
        for rotation in [0, 90, 180, 270] {
            for content in [
                &b"q 0 1 0 rg 1 0 0 RG 2 w 0 0 40 30 re f 2 2 36 26 re S 6 6 8 8 re B Q"[..],
                &b"q 1 0 0 rg 0 1 0 RG 2 w 0 0 40 30 re f q 0 0 1 rg 4 4 16 10 re f Q 6 6 20 14 re B Q"[..],
            ] {
                let bytes = opaque_stamp_fixture(rotation, false);
                let mut doc = Document::load_mem(&bytes).unwrap();
                let page_id = *doc.get_pages().get(&1).unwrap();
                let annotation_id = doc
                    .get_dictionary(page_id)
                    .unwrap()
                    .get(b"Annots")
                    .unwrap()
                    .as_array()
                    .unwrap()[0]
                    .as_reference()
                    .unwrap();
                let form_id = doc
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
                let form = doc
                    .get_object_mut(form_id)
                    .unwrap()
                    .as_stream_mut()
                    .unwrap();
                form.content = content.to_vec();
                form.dict.set("Length", form.content.len() as i64);
                let mut source = Vec::new();
                doc.save_to(&mut source).unwrap();
                let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
                let expected = pdf.render_page(1, 1.0).unwrap();
                let actual = render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
                assert!(actual.imported_annotations_dynlex, "{rotation}");
                assert_eq!(actual.raster.rgba, expected.rgba, "{rotation}");
            }
        }
    }

    #[test]
    fn closed_rectangle_and_even_odd_paint_operators_match_hayro() {
        for rotation in [0, 90, 180, 270] {
            for operator in ["F", "f*", "s", "B*", "b", "b*"] {
                let content = match operator {
                    "F" | "f*" => format!("q 1 0 0 rg 0 0 40 30 re {operator} Q"),
                    "s" => format!("q 0 1 0 RG 2 w 1 1 38 28 re {operator} Q"),
                    _ => format!("q 1 0 0 rg 0 1 0 RG 2 w 1 1 38 28 re {operator} Q"),
                };
                let bytes = opaque_stamp_fixture(rotation, false);
                let mut doc = Document::load_mem(&bytes).unwrap();
                let page_id = *doc.get_pages().get(&1).unwrap();
                let annotation_id = doc
                    .get_dictionary(page_id)
                    .unwrap()
                    .get(b"Annots")
                    .unwrap()
                    .as_array()
                    .unwrap()[0]
                    .as_reference()
                    .unwrap();
                let form_id = doc
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
                let form = doc
                    .get_object_mut(form_id)
                    .unwrap()
                    .as_stream_mut()
                    .unwrap();
                form.content = content.into_bytes();
                form.dict.set("Length", form.content.len() as i64);
                let mut source = Vec::new();
                doc.save_to(&mut source).unwrap();
                let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
                let expected = pdf.render_page(1, 1.0).unwrap();
                let actual = render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
                assert!(actual.imported_annotations_dynlex, "{operator} {rotation}");
                assert_eq!(actual.raster.rgba, expected.rgba, "{operator} {rotation}");
            }
        }
    }

    #[test]
    fn bounded_rectangle_strokes_match_hayro_at_multiple_sizes() {
        for rotation in [0, 90, 180, 270] {
            for path in ["1 1 2 2", "1 1 3 4", "10 7 12 10", "1 1 38 28"] {
                let bytes = opaque_stamp_fixture(rotation, false);
                let mut doc = Document::load_mem(&bytes).unwrap();
                let page_id = *doc.get_pages().get(&1).unwrap();
                let annotation_id = doc
                    .get_dictionary(page_id)
                    .unwrap()
                    .get(b"Annots")
                    .unwrap()
                    .as_array()
                    .unwrap()[0]
                    .as_reference()
                    .unwrap();
                let form_id = doc
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
                let form = doc
                    .get_object_mut(form_id)
                    .unwrap()
                    .as_stream_mut()
                    .unwrap();
                form.content = format!("q 0 1 0 RG 2 w {path} re S Q").into_bytes();
                form.dict.set("Length", form.content.len() as i64);
                let mut source = Vec::new();
                doc.save_to(&mut source).unwrap();
                let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
                let expected = pdf.render_page(1, 1.0).unwrap();
                let actual = render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
                assert!(actual.imported_annotations_dynlex, "{path} {rotation}");
                assert_eq!(actual.raster.rgba, expected.rgba, "{path} {rotation}");
            }
        }
    }

    #[test]
    fn imported_form_triangle_uses_dynlex_area_kernel() {
        for rotation in [0, 90, 180, 270] {
            for path in [
                "22 4 m 36 25 l 29 16 l h",
                "25 12 m 20 18 l 3 8 l h",
                "22 4 m 36 25 l 29 16 l",
            ] {
                for layered in [false, true] {
                    let bytes = opaque_stamp_fixture(rotation, false);
                    let mut doc = Document::load_mem(&bytes).unwrap();
                    let page_id = *doc.get_pages().get(&1).unwrap();
                    let annotation_id = doc
                        .get_dictionary(page_id)
                        .unwrap()
                        .get(b"Annots")
                        .unwrap()
                        .as_array()
                        .unwrap()[0]
                        .as_reference()
                        .unwrap();
                    let form_id = doc
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
                    let form = doc
                        .get_object_mut(form_id)
                        .unwrap()
                        .as_stream_mut()
                        .unwrap();
                    form.content = if layered {
                    format!("q 0 1 0 rg 0 0 40 30 re f 1 0 0 rg {path} f 0 0 1 rg 15 8 12 10 re f Q")
                } else {
                    format!("q 1 0 0 rg {path} f Q")
                }
                .into_bytes();
                    form.dict.set("Length", form.content.len() as i64);
                    let mut source = Vec::new();
                    doc.save_to(&mut source).unwrap();
                    let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
                    let expected = pdf.render_page(1, 1.0).unwrap();
                    let actual =
                        render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
                    assert!(
                        actual.imported_annotations_dynlex,
                        "{path} {rotation} layered={layered}"
                    );
                    assert_eq!(
                        actual.raster.rgba, expected.rgba,
                        "{path} {rotation} layered={layered}"
                    );
                }
            }
        }
    }

    #[test]
    fn bounded_imported_triangles_are_pixel_exact_when_accepted() {
        let mut accepted = [0; 2];
        for quarter in [false, true] {
            for index in 0_u32..256 {
                let offsets = if quarter {
                    [0.25, 0.25, 0.5, 0.0, 0.75, 0.5]
                } else {
                    [0.0; 6]
                };
                let x0 = f64::from(2 + (index * 7) % 25) + offsets[0];
                let y0 = f64::from(2 + (index * 11) % 18) + offsets[1];
                let x1 = f64::from(3 + (index * 13 + 5) % 34) + offsets[2];
                let y1 = f64::from(2 + (index * 17 + 3) % 25) + offsets[3];
                let x2 = f64::from(2 + (index * 19 + 7) % 34) + offsets[4];
                let y2 = f64::from(2 + (index * 23 + 9) % 25) + offsets[5];
                let bytes = opaque_stamp_fixture(0, false);
                let mut doc = Document::load_mem(&bytes).unwrap();
                let page_id = *doc.get_pages().get(&1).unwrap();
                let annotation_id = doc
                    .get_dictionary(page_id)
                    .unwrap()
                    .get(b"Annots")
                    .unwrap()
                    .as_array()
                    .unwrap()[0]
                    .as_reference()
                    .unwrap();
                let form_id = doc
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
                let form = doc
                    .get_object_mut(form_id)
                    .unwrap()
                    .as_stream_mut()
                    .unwrap();
                form.content =
                    format!("q 0.906 0.298 0.235 rg {x0} {y0} m {x1} {y1} l {x2} {y2} l h f Q")
                        .into_bytes();
                form.dict.set("Length", form.content.len() as i64);
                let mut source = Vec::new();
                doc.save_to(&mut source).unwrap();
                let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
                let expected = pdf.render_page(1, 1.0).unwrap();
                let actual = render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
                if actual.imported_annotations_dynlex {
                    accepted[usize::from(quarter)] += 1;
                    let differences: Vec<_> = actual
                        .raster
                        .rgba
                        .iter()
                        .zip(&expected.rgba)
                        .enumerate()
                        .filter(|(_, (a, b))| a != b)
                        .take(6)
                        .map(|(at, (a, b))| {
                            (
                                at / 4 % actual.raster.width as usize,
                                at / 4 / actual.raster.width as usize,
                                at % 4,
                                *a,
                                *b,
                            )
                        })
                        .collect();
                    assert!(
                        differences.is_empty(),
                        "triangle {index} quarter={quarter}: {differences:?}"
                    );
                }
            }
        }
        assert!(
            accepted[0] >= 30,
            "no useful integer triangle staging: {accepted:?}"
        );
        assert!(
            accepted[1] >= 200,
            "no useful quarter triangle staging: {accepted:?}"
        );
        eprintln!("accepted triangles, integer/quarter: {accepted:?}");
    }

    #[test]
    fn unsupported_form_paths_fall_back_without_partial_paint() {
        let cases = [
            // The first fill is supported. A later invalid path must not
            // leave that fill on a partially DynLex-painted page.
            "q 0 1 0 rg 0 0 40 30 re f 1 0 0 rg 2 2 m 12 12 l S Q",
            "q 1 0 0 rg 2 2 m 20 20 l 2 20 l 20 2 l h f Q",
            "q 1 0 0 rg 2 2 m 20 20 l 2 20 l 20 2 l c f Q",
            "q 1 0 0 rg 2 2 m 20 2 l 20 20 l h 1 1 m 2 1 l 2 2 l h f Q",
            "q 1 0 0 rg 2 2 m 20 2 l 20 20 l h 3 3 l f Q",
            "q 1 0 0 rg 2.125 2 m 20 2 l 20 20 l h f Q",
            "q 1 0 0 rg 2 2 m 42 2 l 20 20 l h f Q",
        ];
        for (index, content) in cases.iter().enumerate() {
            let bytes = opaque_stamp_fixture(90, false);
            let mut doc = Document::load_mem(&bytes).unwrap();
            let page_id = *doc.get_pages().get(&1).unwrap();
            let annotation_id = doc
                .get_dictionary(page_id)
                .unwrap()
                .get(b"Annots")
                .unwrap()
                .as_array()
                .unwrap()[0]
                .as_reference()
                .unwrap();
            let form_id = doc
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
            let form = doc
                .get_object_mut(form_id)
                .unwrap()
                .as_stream_mut()
                .unwrap();
            form.content = content.as_bytes().to_vec();
            form.dict.set("Length", form.content.len() as i64);
            let mut source = Vec::new();
            doc.save_to(&mut source).unwrap();
            let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
            let expected = pdf.render_page(1, 1.0).unwrap();
            let actual = render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
            assert!(!actual.imported_annotations_dynlex, "case {index}");
            assert_eq!(actual.raster.rgba, expected.rgba, "case {index}");
        }
    }

    #[test]
    fn form_stroke_does_not_inherit_page_dash_state() {
        let bytes = opaque_stamp_fixture(0, false);
        let mut doc = Document::load_mem(&bytes).unwrap();
        let page_id = *doc.get_pages().get(&1).unwrap();
        let content_id = doc
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Contents")
            .unwrap()
            .as_reference()
            .unwrap();
        let annotation_id = doc
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .as_reference()
            .unwrap();
        let form_id = doc
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
        for (id, content) in [
            (content_id, &b"0 0 1 rg 40 60 160 240 re f [3 2] 0 d"[..]),
            (form_id, &b"q 0 1 0 RG 2 w 1 1 38 28 re S Q"[..]),
        ] {
            let stream = doc.get_object_mut(id).unwrap().as_stream_mut().unwrap();
            stream.content = content.to_vec();
            stream.dict.set("Length", stream.content.len() as i64);
        }
        let mut source = Vec::new();
        doc.save_to(&mut source).unwrap();
        let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
        let expected = pdf.render_page(1, 1.0).unwrap();
        let actual = render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
        assert!(actual.imported_annotations_dynlex);
        assert_eq!(actual.raster.rgba, expected.rgba);
    }

    #[test]
    fn unsupported_stamp_form_falls_back_without_partial_paint() {
        let bytes = opaque_stamp_fixture(90, false);
        let mut doc = Document::load_mem(&bytes).unwrap();
        let page_id = *doc.get_pages().get(&1).unwrap();
        let stamp_id = doc
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .as_reference()
            .unwrap();
        let form_id = doc
            .get_dictionary(stamp_id)
            .unwrap()
            .get(b"AP")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"N")
            .unwrap()
            .as_reference()
            .unwrap();
        let form = doc
            .get_object_mut(form_id)
            .unwrap()
            .as_stream_mut()
            .unwrap();
        form.content = b"q 1 0 0 rg 0 0 40 30 re f 0 0 m 10 10 l S Q".to_vec();
        form.dict.set("Length", form.content.len() as i64);
        let mut modified = Vec::new();
        doc.save_to(&mut modified).unwrap();
        assert!(Document::load_mem(&modified)
            .unwrap()
            .get_object(form_id)
            .unwrap()
            .as_stream()
            .is_ok());
        let pdf = ParsedPdf::open(Arc::new(modified.clone())).unwrap();
        let expected = pdf.render_page(1, 1.0).unwrap();
        let selected = render_with_report(&pdf, &modified, 1, 1.0, Backend::Dynlex).unwrap();
        assert!(!selected.imported_annotations_dynlex);
        assert_eq!(selected.raster.rgba, expected.rgba);
    }

    #[test]
    fn stamp_form_rejects_ignored_trailing_pdf_syntax() {
        let bytes = opaque_stamp_fixture(0, false);
        let mut doc = Document::load_mem(&bytes).unwrap();
        let page_id = *doc.get_pages().get(&1).unwrap();
        let stamp_id = doc
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .as_reference()
            .unwrap();
        let form_id = doc
            .get_dictionary(stamp_id)
            .unwrap()
            .get(b"AP")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"N")
            .unwrap()
            .as_reference()
            .unwrap();
        doc.get_object_mut(form_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .content
            .extend_from_slice(b" [");
        let stamp = doc.get_dictionary(stamp_id).unwrap();
        let (_, crop, rotation) = page_geometry(&doc, 1).unwrap();
        assert!(parse_opaque_rect_form(&doc, stamp, crop, rotation).is_none());
    }

    #[test]
    fn generic_form_rejects_hidden_out_of_box_and_excessive_fills() {
        let bytes = opaque_stamp_fixture(0, false);
        let mut doc = Document::load_mem(&bytes).unwrap();
        let page_id = *doc.get_pages().get(&1).unwrap();
        let annotation_id = doc
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap()[0]
            .as_reference()
            .unwrap();
        let form_id = doc
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
        let (_, crop, rotation) = page_geometry(&doc, 1).unwrap();
        doc.get_dictionary_mut(annotation_id)
            .unwrap()
            .set("OC", Object::Null);
        assert!(parse_opaque_rect_form(
            &doc,
            doc.get_dictionary(annotation_id).unwrap(),
            crop,
            rotation
        )
        .is_none());
        doc.get_dictionary_mut(annotation_id).unwrap().remove(b"OC");
        doc.get_object_mut(form_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .dict
            .set(
                "Matrix",
                vec![1.into(), 0.into(), 0.into(), 1.into(), 1.into(), 0.into()],
            );
        assert!(parse_opaque_rect_form(
            &doc,
            doc.get_dictionary(annotation_id).unwrap(),
            crop,
            rotation
        )
        .is_none());
        doc.get_object_mut(form_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .dict
            .remove(b"Matrix");
        doc.get_object_mut(form_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .dict
            .set("Resources", dictionary! { "XObject" => dictionary! {} });
        assert!(parse_opaque_rect_form(
            &doc,
            doc.get_dictionary(annotation_id).unwrap(),
            crop,
            rotation
        )
        .is_none());
        doc.get_object_mut(form_id)
            .unwrap()
            .as_stream_mut()
            .unwrap()
            .dict
            .remove(b"Resources");
        for content in [
            b"q 1 0 0 rg 39 0 2 30 re f Q".to_vec(),
            format!("q {}Q", "1 0 0 rg 0 0 40 30 re f ".repeat(33)).into_bytes(),
            b"q 1 0 0 RG 1 w 1 1 38 28 re S Q".to_vec(),
            b"q 1 0 0 RG 3 w 1 1 38 28 re S Q".to_vec(),
            b"q 1 0 0 RG 2 w 0 1 38 28 re S Q".to_vec(),
            b"q 1 0 0 RG 2 w 1 1 0 28 re S Q".to_vec(),
            b"q 1 0 0 RG 2 w 1 1 38 28 re S Q 99".to_vec(),
            b"q 1 0 0 rg 0 0 40 30 re B Q".to_vec(),
            b"q 1 0 0 rg 0 0 40 30 re Q".to_vec(),
            b"q q 1 0 0 rg 0 0 40 30 re f Q".to_vec(),
            b"q 1 0 0 rg 0 0 40 30 re f 1 0 0 1 0 0 cm Q".to_vec(),
        ] {
            let form = doc
                .get_object_mut(form_id)
                .unwrap()
                .as_stream_mut()
                .unwrap();
            form.content = content;
            form.dict.set("Length", form.content.len() as i64);
            assert!(parse_opaque_rect_form(
                &doc,
                doc.get_dictionary(annotation_id).unwrap(),
                crop,
                rotation
            )
            .is_none());
        }
    }

    #[test]
    fn native_square_with_optional_content_falls_back() {
        let bytes = opaque_stamp_fixture(0, true);
        let mut doc = Document::load_mem(&bytes).unwrap();
        let page_id = *doc.get_pages().get(&1).unwrap();
        let square_id = doc
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap()[1]
            .as_reference()
            .unwrap();
        doc.get_dictionary_mut(square_id)
            .unwrap()
            .set("OC", Object::Null);
        let mut source = Vec::new();
        doc.save_to(&mut source).unwrap();
        let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
        let expected = pdf.render_page(1, 1.0).unwrap();
        let actual = render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
        assert!(!actual.imported_annotations_dynlex);
        assert_eq!(actual.raster.rgba, expected.rgba);
    }

    fn fixture(rotation: i64, stroke: i64, slanted_quad: bool) -> Vec<u8> {
        let mut doc = Document::with_version("1.4");
        let pages_id = doc.new_object_id();
        let content_id = doc.add_object(Stream::new(
            Dictionary::new(),
            b"0 0 1 rg 40 60 160 240 re f".to_vec(),
        ));
        let square_id = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Square", "F" => 4,
            "Rect" => vec![60.into(),80.into(),120.into(),140.into()],
            "C" => vec![0.into(),1.into(),0.into()],
            "BS" => dictionary! { "W" => stroke },
        });
        let highlight_id = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Highlight", "F" => 4,
            "Rect" => vec![90.into(),100.into(),150.into(),125.into()],
            "QuadPoints" => vec![
                90.into(),125.into(),150.into(),125.into(),
                (if slanted_quad { 91 } else { 90 }).into(),100.into(),150.into(),100.into(),
            ],
            "C" => vec![1.into(),1.into(),0.into()],
        });
        let page_id = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content_id,
            "Annots" => vec![Object::Reference(square_id), Object::Reference(highlight_id)],
            "MediaBox" => vec![20.into(),30.into(),220.into(),330.into()],
            "CropBox" => vec![40.into(),60.into(),200.into(),300.into()],
            "Rotate" => rotation,
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![Object::Reference(page_id)], "Count" => 1,
            }),
        );
        let catalog_id = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", Object::Reference(catalog_id));
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn missing_ap_square_and_highlight_match_preview_on_rotated_crop() {
        for rotation in [0, 90, 180, 270] {
            for stroke in [1, 3, 20] {
                let bytes = fixture(rotation, stroke, false);
                if rotation == 90 && stroke == 3 {
                    if let Ok(directory) = std::env::var("OPDS_MISSING_AP_FIXTURE_DIR") {
                        std::fs::create_dir_all(&directory).unwrap();
                        std::fs::write(
                            std::path::Path::new(&directory).join("missing-square-highlight.pdf"),
                            &bytes,
                        )
                        .unwrap();
                    }
                }
                let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
                let expected = pdf.render_page(1, 1.0).unwrap();
                let selected = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
                assert_eq!(selected.used, Backend::Hayro);
                assert!(selected.imported_annotations_dynlex);
                assert_eq!(
                    selected.raster.rgba, expected.rgba,
                    "rotation={rotation}, stroke={stroke}"
                );
            }
        }
    }

    #[test]
    fn slanted_missing_ap_highlight_falls_back_without_partial_paint() {
        let bytes = fixture(90, 3, true);
        let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        let expected = pdf.render_page(1, 1.0).unwrap();
        let selected = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
        assert!(!selected.imported_annotations_dynlex);
        assert_eq!(selected.raster.rgba, expected.rgba);
    }

    #[test]
    fn missing_ap_defaults_and_hidden_flags_keep_preview_semantics() {
        let mut doc = Document::load_mem(&fixture(270, 1, false)).unwrap();
        let page_id = *doc.get_pages().get(&1).unwrap();
        let ids: Vec<_> = doc
            .get_dictionary(page_id)
            .unwrap()
            .get(b"Annots")
            .unwrap()
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry.as_reference().unwrap())
            .collect();
        doc.get_dictionary_mut(ids[0]).unwrap().remove(b"C");
        let highlight = doc.get_dictionary_mut(ids[1]).unwrap();
        highlight.remove(b"C");
        highlight.remove(b"QuadPoints");
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        let expected = pdf.render_page(1, 1.0).unwrap();
        let selected = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
        assert!(selected.imported_annotations_dynlex);
        assert_eq!(selected.raster.rgba, expected.rgba);

        doc.get_dictionary_mut(ids[0]).unwrap().set("F", 2);
        bytes.clear();
        doc.save_to(&mut bytes).unwrap();
        let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        let expected = pdf.render_page(1, 1.0).unwrap();
        let selected = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
        assert!(selected.imported_annotations_dynlex);
        assert_eq!(selected.raster.rgba, expected.rgba);
    }
}
