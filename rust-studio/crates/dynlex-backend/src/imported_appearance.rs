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
    paint_rgb_appearances, Bitmap,
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
    paint_clip: Option<[u32; 4]>,
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
fn signed_polygon_area(points: &[[f64; 2]]) -> f64 {
    points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
        .map(|(a, b)| a[0] * b[1] - b[0] * a[1])
        .sum::<f64>()
        * 0.5
}

fn point_inside_polygon(point: [f64; 2], polygon: &[[f64; 2]]) -> bool {
    let mut inside = false;
    for (a, b) in polygon
        .iter()
        .zip(polygon.iter().cycle().skip(1))
        .take(polygon.len())
    {
        if (a[1] > point[1]) != (b[1] > point[1])
            && point[0] < (b[0] - a[0]) * (point[1] - a[1]) / (b[1] - a[1]) + a[0]
        {
            inside = !inside;
        }
    }
    inside
}

fn unambiguous_polygon_coverage(contours: &[Vec<[f64; 2]>], bounds: [u32; 4]) -> bool {
    let contour_bounds: Vec<_> = contours
        .iter()
        .map(|path| {
            path.iter().fold(
                [
                    f64::INFINITY,
                    f64::INFINITY,
                    f64::NEG_INFINITY,
                    f64::NEG_INFINITY,
                ],
                |mut box_rect, point| {
                    box_rect[0] = box_rect[0].min(point[0]);
                    box_rect[1] = box_rect[1].min(point[1]);
                    box_rect[2] = box_rect[2].max(point[0]);
                    box_rect[3] = box_rect[3].max(point[1]);
                    box_rect
                },
            )
        })
        .collect();
    for y in bounds[1]..bounds[3] {
        for x in bounds[0]..bounds[2] {
            let mut area = 0.0;
            for (points, box_rect) in contours.iter().zip(&contour_bounds) {
                if f64::from(x + 1) <= box_rect[0]
                    || f64::from(y + 1) <= box_rect[1]
                    || f64::from(x) >= box_rect[2]
                    || f64::from(y) >= box_rect[3]
                {
                    continue;
                }
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
                area += signed_polygon_area(&clipped);
            }
            let scaled = area.clamp(0.0, 1.0) * 255.0;
            if (scaled.fract() - 0.5).abs() < 0.002 {
                return false;
            }
        }
    }
    true
}

/// PDF nonzero and even-odd filling depend on the winding of all enclosing
/// subpaths. Once OPM2 has rejected touching/crossing contours, a vertex is
/// sufficient to identify each immediate parent. Normalize only visible
/// boundaries into alternating filled/hole outlines for the DynLex kernel.
fn visible_form_contours(
    contours: Vec<Vec<[f64; 2]>>,
    even_odd: bool,
) -> Option<Vec<Vec<[f64; 2]>>> {
    let areas: Vec<_> = contours
        .iter()
        .map(|path| signed_polygon_area(path))
        .collect();
    if areas.iter().any(|area| !area.is_finite() || *area == 0.0) {
        return None;
    }
    let mut parents = vec![None; contours.len()];
    for index in 0..contours.len() {
        for candidate in 0..contours.len() {
            if index != candidate && point_inside_polygon(contours[index][0], &contours[candidate])
            {
                if areas[candidate].abs() <= areas[index].abs() {
                    return None;
                }
                if parents[index]
                    .is_none_or(|parent: usize| areas[candidate].abs() < areas[parent].abs())
                {
                    parents[index] = Some(candidate);
                }
            }
        }
    }
    let mut visible = Vec::new();
    for (index, mut path) in contours.into_iter().enumerate() {
        let mut depth = 0;
        let mut outside_winding = 0_i32;
        let mut parent = parents[index];
        while let Some(ancestor) = parent {
            depth += 1;
            if depth >= parents.len() {
                return None;
            }
            outside_winding += if areas[ancestor] > 0.0 { 1 } else { -1 };
            parent = parents[ancestor];
        }
        let inside_winding = outside_winding + if areas[index] > 0.0 { 1 } else { -1 };
        let (is_visible, enters_fill) = if even_odd {
            (true, depth % 2 == 0)
        } else {
            (
                (outside_winding == 0) != (inside_winding == 0),
                outside_winding == 0,
            )
        };
        if is_visible {
            if (areas[index] > 0.0) != enters_fill {
                path.reverse();
            }
            visible.push(path);
        }
    }
    (!visible.is_empty()).then_some(visible)
}

fn encode_opm_contours(contours: &[Vec<[f64; 2]>]) -> Vec<u8> {
    let mut outline = Vec::with_capacity(
        24 + (contours.iter().map(Vec::len).sum::<usize>() + contours.len()) * 16,
    );
    outline.extend_from_slice(b"OPM2");
    for word in [1_u32, 256, 192, contours.len() as u32] {
        outline.extend_from_slice(&word.to_le_bytes());
    }
    for points in contours {
        outline.extend_from_slice(&((points.len() + 1) as u32).to_le_bytes());
        for point in points.iter().chain(points.first()) {
            outline.extend_from_slice(&point[0].to_le_bytes());
            outline.extend_from_slice(&point[1].to_le_bytes());
        }
    }
    outline
}

/// Stage up to eight closed, line-only Form contours for the validated DynLex
/// area kernel. The tile may start anywhere on a larger page; the Form box
/// remains its appearance clip. All points are on a quarter-pixel grid and
/// fully inside it.
fn stage_form_polygon(
    contours: &[Vec<[f64; 2]>],
    even_odd: bool,
    color: [u8; 3],
    rect: [f64; 4],
    crop: [f64; 4],
    rotation: u16,
    active_clip: Option<[u32; 4]>,
) -> Option<StagedPolygon> {
    if !(1..=8).contains(&contours.len()) {
        return None;
    }
    let contours: Vec<Vec<[f64; 2]>> = contours
        .iter()
        .map(|points| {
            let mut points = points.clone();
            if points.len() > 3 && points.first() == points.last() {
                points.pop();
            }
            points
        })
        .collect();
    if contours
        .iter()
        .any(|points| !(3..=64).contains(&points.len()))
        || contours.iter().map(Vec::len).sum::<usize>() + contours.len() > 256
        || contours.iter().flatten().any(|point| {
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
    let form_bounds = pixel_box(crop, rotation, &[[rect[0], rect[1]], [rect[2], rect[3]]])?;
    let paint_clip = match active_clip {
        Some(active) => pixel_rect_intersection(form_bounds, active)?,
        None => form_bounds,
    };
    if form_bounds[2] > 4096 || form_bounds[3] > 4096 {
        return None;
    }
    let mapped: Vec<Vec<_>> = contours
        .iter()
        .map(|points| {
            points
                .iter()
                .map(|point| display_point(crop, rotation, rect[0] + point[0], rect[1] + point[1]))
                .collect()
        })
        .collect();
    let left = mapped.iter().flatten().map(|p| p[0]).reduce(f64::min)?;
    let top = mapped.iter().flatten().map(|p| p[1]).reduce(f64::min)?;
    let right = mapped.iter().flatten().map(|p| p[0]).reduce(f64::max)?;
    let bottom = mapped.iter().flatten().map(|p| p[1]).reduce(f64::max)?;
    if left < f64::from(form_bounds[0])
        || top < f64::from(form_bounds[1])
        || right > f64::from(form_bounds[2])
        || bottom > f64::from(form_bounds[3])
        || right <= left
        || bottom <= top
    {
        return None;
    }
    let origin = [left.floor() as u32, top.floor() as u32];
    if right - f64::from(origin[0]) > 256.0 || bottom - f64::from(origin[1]) > 192.0 {
        return None;
    }
    let local: Vec<Vec<[f64; 2]>> = mapped
        .iter()
        .map(|points| {
            points
                .iter()
                .map(|point| {
                    [
                        point[0] - f64::from(origin[0]),
                        point[1] - f64::from(origin[1]),
                    ]
                })
                .collect()
        })
        .collect();
    if !validate_polygon_outline(&encode_opm_contours(&local)) {
        return None;
    }
    let local = visible_form_contours(local, even_odd)?;
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
    let outline = encode_opm_contours(&local);
    validate_polygon_outline(&outline).then_some(StagedPolygon {
        outline,
        color,
        clip: form_bounds,
        paint_clip: (paint_clip != form_bounds).then_some(paint_clip),
        origin,
    })
}

/// A bounded Form appearance with opaque DeviceRGB rectangle fills, 2pt
/// outlines and small line-only filled paths. The paint order, rather than
/// `/Subtype`, determines the pixels. Other Form operators remain on Hayro.
fn parse_bounded_opaque_form(
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
    let image_resources = match form.dict.get(b"Resources") {
        Ok(value) => {
            let resources = value.as_dict().ok()?;
            if resources.is_empty() {
                None
            } else {
                if resources.len() != 1 {
                    return None;
                }
                let xobjects = resources.get(b"XObject").ok()?.as_dict().ok()?;
                if !(1..=8).contains(&xobjects.len()) {
                    return None;
                }
                Some(xobjects)
            }
        }
        Err(_) => None,
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
            "q" | "Q" | "h" | "f" | "F" | "f*" | "S" | "s" | "B" | "B*" | "b" | "b*" | "W"
            | "W*" | "n" => 0,
            "w" | "Do" => 1,
            "m" | "l" => 2,
            "rg" | "RG" => 3,
            "re" => 4,
            "cm" => 6,
            _ => return None,
        };
        let Some(record) = tokens.get(token_index..token_index + numeric_count + 1) else {
            return None;
        };
        let operands_match = if operation.operator == "Do" {
            let name = operation.operands.first()?.as_name().ok()?;
            !name.is_empty()
                && name.len() <= 32
                && name.iter().all(u8::is_ascii_alphanumeric)
                && record[0] == [b"/".as_slice(), name].concat()
        } else {
            record[..numeric_count].iter().all(|token| numeric(token))
        };
        if operation.operands.len() != numeric_count
            || !operands_match
            || record[numeric_count] != operation.operator.as_bytes()
        {
            return None;
        }
        // lopdf stores PDF real operands as f32. Reject coordinates whose
        // original decimal has already moved before page/annotation geometry
        // is calculated; Hayro reads the original PDF precision.
        if matches!(operation.operator.as_str(), "cm" | "re" | "m" | "l") {
            for (token, operand) in record[..numeric_count].iter().zip(&operation.operands) {
                let precise = std::str::from_utf8(token).ok()?.parse::<f64>().ok()?;
                if !precise.is_finite() || precise != number(operand)? {
                    return None;
                }
            }
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
        image_matrix: Option<[f64; 4]>,
        vector_translation: Option<[f64; 2]>,
        image_painted: bool,
        painted_in_group: bool,
        clip: Option<[u32; 4]>,
    }
    enum FormPath {
        Rect([f64; 4]),
        Polygon {
            contours: Vec<Vec<[f64; 2]>>,
            closed: bool,
        },
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
    let rect_command = |path: [f64; 4], stroke: bool, color: [u8; 3], clip| {
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
        let bounds = if let Some(clip) = clip {
            let intersection = pixel_rect_intersection(bounds, clip)?;
            if stroke && intersection != bounds {
                return None;
            }
            intersection
        } else {
            bounds
        };
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
    let mut pending_clip = false;
    let mut image_count = 0_usize;
    let mut image_bytes = 0_usize;
    for (index, operation) in operations.iter().enumerate() {
        if (state.image_matrix.is_some() && state.vector_translation.is_none()
            || state.image_painted)
            && !matches!(operation.operator.as_str(), "Do" | "Q")
        {
            return None;
        }
        match operation.operator.as_str() {
            "q" => {
                if pending_path.is_some() || pending_clip || stack.len() >= 4 {
                    return None;
                }
                stack.push(state);
                state.painted_in_group = false;
            }
            "Q" => {
                if pending_path.is_some()
                    || pending_clip
                    || (state.image_matrix.is_some()
                        && state.vector_translation.is_none()
                        && !state.image_painted)
                    || (stack.len() == 1 && index + 1 != operations.len())
                {
                    return None;
                }
                state = stack.pop()?;
            }
            "cm" => {
                if !(1..=4).contains(&stack.len())
                    || pending_path.is_some()
                    || pending_clip
                    || state.image_matrix.is_some()
                    || state.painted_in_group
                {
                    return None;
                }
                let [w, b, c, h, tx, ty]: [f64; 6] = operation
                    .operands
                    .iter()
                    .map(number)
                    .collect::<Option<Vec<_>>>()?
                    .try_into()
                    .ok()?;
                if b != 0.0 || c != 0.0 {
                    return None;
                }
                let translation = (w == 1.0
                    && h == 1.0
                    && tx.is_finite()
                    && ty.is_finite()
                    && tx.fract() == 0.0
                    && ty.fract() == 0.0
                    && tx.abs() <= 4096.0
                    && ty.abs() <= 4096.0)
                    .then_some([tx, ty]);
                if stack.len() == 1 && translation.is_none() {
                    return None;
                }
                state.vector_translation = translation;
                state.image_matrix = Some([w, h, tx, ty]);
                state.image_painted = false;
            }
            "Do" => {
                if !(2..=4).contains(&stack.len()) || pending_path.is_some() || state.image_painted
                {
                    return None;
                }
                let [w, h, tx, ty] = state.image_matrix?;
                let name = operation.operands.first()?.as_name().ok()?;
                let image = stage_form_image(
                    doc,
                    image_resources?,
                    name,
                    rect,
                    crop,
                    rotation,
                    [w, h, tx, ty],
                )?;
                let image = if let Some(clip) = state.clip {
                    clip_staged_image(image, clip)?
                } else {
                    image
                };
                image_count += 1;
                image_bytes += image.rgb.len();
                if image_count > 8 || image_bytes > 1_000_000 {
                    return None;
                }
                if let Some(ImportedAppearance::Images(images)) = appearances.last_mut() {
                    images.push(image);
                } else {
                    appearances.push(ImportedAppearance::Images(vec![image]));
                }
                state.image_painted = true;
                state.painted_in_group = true;
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
                if stack.is_empty() || pending_path.is_some() || pending_clip {
                    return None;
                }
                let mut path: [f64; 4] = operation
                    .operands
                    .iter()
                    .map(number)
                    .collect::<Option<Vec<_>>>()?
                    .try_into()
                    .ok()?;
                if let Some([tx, ty]) = state.vector_translation {
                    path[0] += tx;
                    path[1] += ty;
                }
                if path.iter().any(|value| !value.is_finite()) {
                    return None;
                }
                pending_path = Some(FormPath::Rect(path));
            }
            "m" => {
                if stack.is_empty() {
                    return None;
                }
                let mut point: [f64; 2] = operation
                    .operands
                    .iter()
                    .map(number)
                    .collect::<Option<Vec<_>>>()?
                    .try_into()
                    .ok()?;
                if let Some([tx, ty]) = state.vector_translation {
                    point[0] += tx;
                    point[1] += ty;
                }
                if point.iter().any(|value| !value.is_finite()) {
                    return None;
                }
                match &mut pending_path {
                    None => {
                        pending_path = Some(FormPath::Polygon {
                            contours: vec![vec![point]],
                            closed: false,
                        });
                    }
                    Some(FormPath::Polygon { contours, closed })
                        if contours.len() < 8
                            && contours.last().is_some_and(|path| path.len() >= 3) =>
                    {
                        contours.push(vec![point]);
                        *closed = false;
                    }
                    _ => return None,
                }
            }
            "l" => {
                let Some(FormPath::Polygon {
                    contours,
                    closed: false,
                }) = &mut pending_path
                else {
                    return None;
                };
                let points = contours.last_mut()?;
                if points.len() >= 64 {
                    return None;
                }
                let mut point: [f64; 2] = operation
                    .operands
                    .iter()
                    .map(number)
                    .collect::<Option<Vec<_>>>()?
                    .try_into()
                    .ok()?;
                if let Some([tx, ty]) = state.vector_translation {
                    point[0] += tx;
                    point[1] += ty;
                }
                if point.iter().any(|value| !value.is_finite()) {
                    return None;
                }
                points.push(point);
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
            "W" | "W*" => {
                if stack.is_empty()
                    || pending_clip
                    || !matches!(pending_path, Some(FormPath::Rect(_)))
                {
                    return None;
                }
                pending_clip = true;
            }
            "n" => {
                if !pending_clip {
                    return None;
                }
                let Some(FormPath::Rect([x, y, w, h])) = pending_path.take() else {
                    return None;
                };
                let clip = pixel_box(
                    crop,
                    rotation,
                    &[
                        [rect[0] + x, rect[1] + y],
                        [rect[0] + x + w, rect[1] + y + h],
                    ],
                )?;
                state.clip = Some(match state.clip {
                    Some(existing) => pixel_rect_intersection(existing, clip)?,
                    None => clip,
                });
                pending_clip = false;
            }
            "f" | "F" | "f*" | "S" | "s" | "B" | "B*" | "b" | "b*" => {
                if stack.is_empty() || pending_clip {
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
                                state.clip,
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
                                state.clip,
                            )?]));
                        }
                    }
                    FormPath::Polygon { contours, .. } => {
                        if !matches!(operation.operator.as_str(), "f" | "F" | "f*") {
                            return None;
                        }
                        appearances.push(ImportedAppearance::Polygon(stage_form_polygon(
                            &contours,
                            operation.operator == "f*",
                            state.fill?,
                            rect,
                            crop,
                            rotation,
                            state.clip,
                        )?));
                    }
                }
                if appearances.len() > 32 {
                    return None;
                }
                state.painted_in_group = true;
            }
            _ => return None,
        }
    }
    (stack.is_empty() && pending_path.is_none() && !pending_clip && !appearances.is_empty())
        .then_some(appearances)
}

struct StagedImage {
    rect: [u32; 4],
    rgb: Vec<u8>,
}

fn pixel_rect_intersection(a: [u32; 4], b: [u32; 4]) -> Option<[u32; 4]> {
    let rect = [
        a[0].max(b[0]),
        a[1].max(b[1]),
        a[2].min(b[2]),
        a[3].min(b[3]),
    ];
    (rect[0] < rect[2] && rect[1] < rect[3]).then_some(rect)
}

fn clip_staged_image(mut image: StagedImage, clip: [u32; 4]) -> Option<StagedImage> {
    let visible = pixel_rect_intersection(image.rect, clip)?;
    if visible == image.rect {
        return Some(image);
    }
    let source_width = (image.rect[2] - image.rect[0]) as usize;
    let visible_width = (visible[2] - visible[0]) as usize;
    let mut rgb = Vec::with_capacity(visible_width * (visible[3] - visible[1]) as usize * 3);
    for y in visible[1]..visible[3] {
        let start = ((y - image.rect[1]) as usize * source_width
            + (visible[0] - image.rect[0]) as usize)
            * 3;
        rgb.extend_from_slice(&image.rgb[start..start + visible_width * 3]);
    }
    image.rect = visible;
    image.rgb = rgb;
    Some(image)
}

fn decode_form_rgb_image(
    doc: &Document,
    xobjects: &Dictionary,
    name: &[u8],
    width: usize,
    height: usize,
) -> Option<Vec<u8>> {
    if name.is_empty() || name.len() > 32 || !name.iter().all(u8::is_ascii_alphanumeric) {
        return None;
    }
    let (_, object) = doc.dereference(xobjects.get(name).ok()?).ok()?;
    let image = object.as_stream().ok()?;
    if image.dict.iter().any(|(key, _)| {
        !matches!(
            key.as_slice(),
            b"Type"
                | b"Subtype"
                | b"Width"
                | b"Height"
                | b"ColorSpace"
                | b"BitsPerComponent"
                | b"Length"
                | b"Filter"
                | b"Interpolate"
        )
    }) || !name_is(&image.dict, b"Type", b"XObject")
        || !name_is(&image.dict, b"Subtype", b"Image")
        || !name_is(&image.dict, b"ColorSpace", b"DeviceRGB")
        || image.dict.get(b"BitsPerComponent").ok().and_then(number) != Some(8.0)
        || image.dict.get(b"Width").ok().and_then(number) != Some(width as f64)
        || image.dict.get(b"Height").ok().and_then(number) != Some(height as f64)
        || image
            .dict
            .get(b"Interpolate")
            .ok()
            .is_some_and(|value| value.as_bool().ok() != Some(false))
        || image.content.len() > 1_000_000
    {
        return None;
    }
    let decoded_len = width * height * 3;
    let pixels = match image.dict.get(b"Filter") {
        Ok(value) if value.as_name().ok() == Some(b"FlateDecode") => {
            miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&image.content, decoded_len)
                .ok()?
        }
        Ok(_) => return None,
        Err(_) => image.content.clone(),
    };
    (pixels.len() == decoded_len).then_some(pixels)
}

fn stage_form_image(
    doc: &Document,
    xobjects: &Dictionary,
    name: &[u8],
    form_rect: [f64; 4],
    crop: [f64; 4],
    rotation: u16,
    placement: [f64; 4], // width, height, x, y in Form coordinates
) -> Option<StagedImage> {
    let [w, h, tx, ty] = placement;
    if w < 1.0
        || h < 1.0
        || [w, h, tx, ty]
            .iter()
            .any(|v| !v.is_finite() || *v < 0.0 || v.fract() != 0.0)
        || tx + w > form_rect[2] - form_rect[0]
        || ty + h > form_rect[3] - form_rect[1]
        || w * h > 65_536.0
    {
        return None;
    }
    let (source_width, source_height) = (w as usize, h as usize);
    let pixels = decode_form_rgb_image(doc, xobjects, name, source_width, source_height)?;
    let bounds = pixel_box(
        crop,
        rotation,
        &[
            [form_rect[0] + tx, form_rect[1] + ty],
            [form_rect[0] + tx + w, form_rect[1] + ty + h],
        ],
    )?;
    let output_width = (bounds[2] - bounds[0]) as usize;
    let output_height = (bounds[3] - bounds[1]) as usize;
    if output_width * output_height != source_width * source_height {
        return None;
    }
    let mut rgb = vec![0; pixels.len()];
    for y in 0..source_height {
        for x in 0..source_width {
            let (out_x, out_y) = match rotation {
                0 => (x, y),
                90 => (source_height - 1 - y, x),
                180 => (source_width - 1 - x, source_height - 1 - y),
                270 => (y, source_width - 1 - x),
                _ => return None,
            };
            let source = (y * source_width + x) * 3;
            let target = (out_y * output_width + out_x) * 3;
            rgb[target..target + 3].copy_from_slice(&pixels[source..source + 3]);
        }
    }
    Some(StagedImage { rect: bounds, rgb })
}

/// A bounded ordered sequence of opaque RGB8 XObjects in an annotation Form.
/// Each image has its own axis-aligned, integral placement; unknown graphics
/// operators or states reject the whole Form before any page paint begins.
fn parse_bounded_image_form(
    doc: &Document,
    annotation: &Dictionary,
    crop: [f64; 4],
    rotation: u16,
) -> Option<Vec<StagedImage>> {
    if annotation.get(b"F").ok().and_then(number) != Some(4.0)
        || annotation.has(b"OC")
        || annotation.has(b"AS")
    {
        return None;
    }
    let rect = four(annotation.get(b"Rect").ok()?)?;
    pixel_box(crop, rotation, &[[rect[0], rect[1]], [rect[2], rect[3]]])?;
    let pdf_width = rect[2] - rect[0];
    let pdf_height = rect[3] - rect[1];
    if pdf_width <= 0.0
        || pdf_height <= 0.0
        || pdf_width.fract() != 0.0
        || pdf_height.fract() != 0.0
        || pdf_width * pdf_height > 1_000_000.0
    {
        return None;
    }
    let ap = annotation.get(b"AP").ok()?.as_dict().ok()?;
    if ap.len() != 1 {
        return None;
    }
    let (_, normal) = doc.dereference(ap.get(b"N").ok()?).ok()?;
    let form = normal.as_stream().ok()?;
    if !name_is(&form.dict, b"Type", b"XObject")
        || !name_is(&form.dict, b"Subtype", b"Form")
        || form.dict.has(b"Filter")
        || form.dict.has(b"DecodeParms")
        || form.dict.has(b"Group")
        || form.dict.get(b"BBox").ok().and_then(four) != Some([0.0, 0.0, pdf_width, pdf_height])
        || form.dict.get(b"Matrix").ok().is_some_and(|value| {
            value
                .as_array()
                .ok()
                .and_then(|array| array.iter().map(number).collect::<Option<Vec<_>>>())
                != Some(vec![1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
        })
        || form.dict.iter().any(|(key, _)| {
            !matches!(
                key.as_slice(),
                b"Type" | b"Subtype" | b"BBox" | b"Length" | b"Resources" | b"Matrix" | b"FormType"
            )
        })
        || form
            .dict
            .get(b"FormType")
            .ok()
            .is_some_and(|v| number(v) != Some(1.0))
        || form.content.len() > 1024
    {
        return None;
    }
    let resources = form.dict.get(b"Resources").ok()?.as_dict().ok()?;
    if resources.len() != 1 {
        return None;
    }
    let xobjects = resources.get(b"XObject").ok()?.as_dict().ok()?;
    if !(1..=8).contains(&xobjects.len()) {
        return None;
    }
    let tokens: Vec<_> = form
        .content
        .split(|byte| byte.is_ascii_whitespace())
        .filter(|token| !token.is_empty())
        .collect();
    if tokens.is_empty() || tokens.len() % 11 != 0 || tokens.len() / 11 > 8 {
        return None;
    }
    let mut images = Vec::with_capacity(tokens.len() / 11);
    let mut used_names = std::collections::HashSet::new();
    let mut total_bytes = 0_usize;
    for record in tokens.chunks_exact(11) {
        if record[0] != b"q"
            || record[7] != b"cm"
            || record[9] != b"Do"
            || record[10] != b"Q"
            || !record[8].starts_with(b"/")
        {
            return None;
        }
        let name = &record[8][1..];
        let values = record[1..7]
            .iter()
            .map(|token| {
                if token.is_empty()
                    || !token
                        .iter()
                        .all(|byte| byte.is_ascii_digit() || *byte == b'.')
                {
                    return None;
                }
                std::str::from_utf8(token)
                    .ok()
                    .and_then(|text| text.parse::<f64>().ok())
            })
            .collect::<Option<Vec<_>>>()?;
        let [w, b, c, h, tx, ty] = values.as_slice() else {
            return None;
        };
        if *b != 0.0 || *c != 0.0 {
            return None;
        }
        let image = stage_form_image(
            doc,
            xobjects,
            name,
            rect,
            crop,
            rotation,
            [*w, *h, *tx, *ty],
        )?;
        total_bytes += image.rgb.len();
        if total_bytes > 1_000_000 {
            return None;
        }
        used_names.insert(name);
        images.push(image);
    }
    (used_names.len() == xobjects.len()).then_some(images)
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
    let pixels = rgba.len() / 4;
    let workers = std::thread::available_parallelism()
        .map(|count| count.get().min(4))
        .unwrap_or(1);
    if pixels < 500_000 || workers < 2 {
        add_white_paper_chunk(rgba);
        return;
    }
    let chunk_bytes = pixels.div_ceil(workers) * 4;
    std::thread::scope(|scope| {
        let mut chunks = rgba.chunks_mut(chunk_bytes);
        if let Some(first) = chunks.next() {
            for chunk in chunks {
                scope.spawn(move || add_white_paper_chunk(chunk));
            }
            add_white_paper_chunk(first);
        }
    });
}

fn add_white_paper_chunk(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        let paper = 255 - pixel[3];
        pixel[0] = pixel[0].saturating_add(paper);
        pixel[1] = pixel[1].saturating_add(paper);
        pixel[2] = pixel[2].saturating_add(paper);
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
    Images(Vec<StagedImage>),
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
    let load_done = std::time::Instant::now();
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
    let mut image_count = 0_usize;
    let mut image_bytes = 0_usize;
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
            } else if let Some(commands) =
                parse_bounded_opaque_form(&doc, annotation, crop, rotation)
            {
                for appearance in &commands {
                    if let ImportedAppearance::Images(images) = appearance {
                        let Some(next_count) = image_count.checked_add(images.len()) else {
                            return Ok(None);
                        };
                        let Some(next_bytes) = image_bytes
                            .checked_add(images.iter().map(|image| image.rgb.len()).sum::<usize>())
                        else {
                            return Ok(None);
                        };
                        image_count = next_count;
                        image_bytes = next_bytes;
                    }
                }
                if image_count > 8 || image_bytes > 1_000_000 {
                    return Ok(None);
                }
                appearances.extend(commands);
            } else if let Some(images) = parse_bounded_image_form(&doc, annotation, crop, rotation)
            {
                image_count += images.len();
                image_bytes += images.iter().map(|image| image.rgb.len()).sum::<usize>();
                if image_count > 8 || image_bytes > 1_000_000 {
                    return Ok(None);
                }
                appearances.push(ImportedAppearance::Images(images));
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
        if appearances.len() > 128 {
            return Ok(None);
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
    let stage_done = std::time::Instant::now();
    let mut layer = pdf
        .render_page_content_without_annotations(page_number, 1.0)
        .map_err(|e| e.to_string())?;
    let base_done = std::time::Instant::now();
    let mut pending_shapes = Vec::new();
    let mut paper_added = false;
    let mut shapes_time = std::time::Duration::ZERO;
    let mut polygons_time = std::time::Duration::ZERO;
    let mut text_time = std::time::Duration::ZERO;
    let mut images_time = std::time::Duration::ZERO;
    let mut paper_time = std::time::Duration::ZERO;
    for appearance in appearances {
        match appearance {
            ImportedAppearance::Shapes(commands) => pending_shapes.extend(commands),
            ImportedAppearance::Polygon(polygon) => {
                let shape_start = std::time::Instant::now();
                if !flush_shapes(&mut layer, &mut pending_shapes)? {
                    return Ok(None);
                }
                shapes_time += shape_start.elapsed();
                let polygon_start = std::time::Instant::now();
                let Some(right) = polygon.origin[0].checked_add(256) else {
                    return Ok(None);
                };
                let Some(bottom) = polygon.origin[1].checked_add(192) else {
                    return Ok(None);
                };
                let tile_bounds = [
                    polygon.origin[0],
                    polygon.origin[1],
                    right.min(layer.width),
                    bottom.min(layer.height),
                ];
                if tile_bounds[0] >= tile_bounds[2]
                    || tile_bounds[1] >= tile_bounds[3]
                    || (layer.width as usize)
                        .checked_mul(layer.height as usize)
                        .and_then(|pixels| pixels.checked_mul(4))
                        != Some(layer.rgba.len())
                {
                    return Ok(None);
                }
                let tile_width = (tile_bounds[2] - tile_bounds[0]) as usize;
                let tile_height = (tile_bounds[3] - tile_bounds[1]) as usize;
                let mut tile = Vec::with_capacity(tile_width * tile_height * 4);
                for y in tile_bounds[1]..tile_bounds[3] {
                    let start = (y as usize * layer.width as usize + tile_bounds[0] as usize) * 4;
                    tile.extend_from_slice(&layer.rgba[start..start + tile_width * 4]);
                }
                let Some(render_clip) = pixel_rect_intersection(polygon.clip, tile_bounds) else {
                    return Ok(None);
                };
                let local_clip = [
                    render_clip[0] - tile_bounds[0],
                    render_clip[1] - tile_bounds[1],
                    render_clip[2] - tile_bounds[0],
                    render_clip[3] - tile_bounds[1],
                ];
                // Keep the Form BBox as the kernel's validated outline clip.
                // The narrower PDF graphics clip is applied when copying the
                // painted tile back to the page, so covered pixels outside it
                // retain the exact prior image/vector layer.
                let painted = render_validated_polygon_over_premultiplied_at_origin_owned(
                    &polygon.outline,
                    polygon.color,
                    tile,
                    tile_width as u32,
                    tile_height as u32,
                    local_clip,
                    [0, 0],
                )?
                .rgba;
                let paint_bounds = polygon.paint_clip.unwrap_or(polygon.clip);
                if let Some(paint_bounds) = pixel_rect_intersection(paint_bounds, tile_bounds) {
                    let span = (paint_bounds[2] - paint_bounds[0]) as usize * 4;
                    for y in paint_bounds[1]..paint_bounds[3] {
                        let source = ((y - tile_bounds[1]) as usize * tile_width
                            + (paint_bounds[0] - tile_bounds[0]) as usize)
                            * 4;
                        let target =
                            (y as usize * layer.width as usize + paint_bounds[0] as usize) * 4;
                        layer.rgba[target..target + span]
                            .copy_from_slice(&painted[source..source + span]);
                    }
                }
                polygons_time += polygon_start.elapsed();
            }
            ImportedAppearance::Text(text) => {
                let shape_start = std::time::Instant::now();
                if !flush_shapes(&mut layer, &mut pending_shapes)?
                    || text.page_size != [layer.width, layer.height]
                {
                    return Ok(None);
                }
                shapes_time += shape_start.elapsed();
                let text_start = std::time::Instant::now();
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
                text_time += text_start.elapsed();
            }
            ImportedAppearance::Images(images) => {
                let shape_start = std::time::Instant::now();
                if !flush_shapes(&mut layer, &mut pending_shapes)? {
                    return Ok(None);
                }
                shapes_time += shape_start.elapsed();
                if !paper_added {
                    let paper_start = std::time::Instant::now();
                    add_white_paper(&mut layer.rgba);
                    paper_time += paper_start.elapsed();
                    paper_added = true;
                }
                let image_start = std::time::Instant::now();
                let image_refs: Vec<_> = images
                    .iter()
                    .map(|image| (image.rect, image.rgb.as_slice()))
                    .collect();
                let Some(painted) = paint_rgb_appearances(
                    std::mem::take(&mut layer.rgba),
                    layer.width,
                    layer.height,
                    &image_refs,
                )?
                else {
                    return Ok(None);
                };
                layer.rgba = painted;
                images_time += image_start.elapsed();
            }
        }
    }
    let shape_start = std::time::Instant::now();
    if !flush_shapes(&mut layer, &mut pending_shapes)? {
        return Ok(None);
    }
    shapes_time += shape_start.elapsed();
    if !paper_added {
        let paper_start = std::time::Instant::now();
        add_white_paper(&mut layer.rgba);
        paper_time += paper_start.elapsed();
    }
    layer.render_ms = started.elapsed().as_millis();
    if std::env::var_os("OPDS_AP_PROFILE").is_some() {
        let ms = |duration: std::time::Duration| duration.as_secs_f64() * 1000.0;
        eprintln!(
            "OPDS_AP_PHASES load={:.3} stage={:.3} base={:.3} shapes={:.3} polygons={:.3} text={:.3} images={:.3} paper={:.3} total={:.3}",
            ms(load_done - started),
            ms(stage_done - load_done),
            ms(base_done - stage_done),
            ms(shapes_time),
            ms(polygons_time),
            ms(text_time),
            ms(images_time),
            ms(paper_time),
            ms(started.elapsed()),
        );
    }
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

    fn stamp_with_form_content(rotation: i64, content: &[u8]) -> Vec<u8> {
        let mut doc = Document::load_mem(&opaque_stamp_fixture(rotation, false)).unwrap();
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
        source
    }

    fn image_stamp_fixture(
        rotation: i64,
        compressed: bool,
        unsupported_image_key: bool,
        image_first: bool,
    ) -> Vec<u8> {
        let mut doc = Document::with_version("1.4");
        let pages_id = doc.new_object_id();
        let content_id = doc.add_object(Stream::new(
            Dictionary::new(),
            b"0 0 1 rg 40 60 160 240 re f".to_vec(),
        ));
        let mut pixels = Vec::with_capacity(40 * 30 * 3);
        for y in 0..30 {
            for x in 0..40 {
                pixels.extend_from_slice(if x < 20 && y < 15 {
                    &[255, 0, 0]
                } else if x >= 20 && y < 15 {
                    &[0, 255, 0]
                } else if x < 20 {
                    &[255, 255, 0]
                } else {
                    &[0, 255, 255]
                });
            }
        }
        let mut image_dict = dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => 40,
            "Height" => 30, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
        };
        if unsupported_image_key {
            image_dict.set("Interpolate", true);
        } else if compressed {
            image_dict.set("Interpolate", false);
        }
        if compressed {
            image_dict.set("Filter", "FlateDecode");
            pixels = miniz_oxide::deflate::compress_to_vec_zlib(&pixels, 6);
        }
        let image_id = doc.add_object(Stream::new(image_dict, pixels));
        let form_id = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Form",
                "BBox" => vec![0.into(), 0.into(), 40.into(), 30.into()],
                "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => image_id } },
            },
            b"q 40 0 0 30 0 0 cm /Im0 Do Q".to_vec(),
        ));
        let stamp_id = doc.add_object(dictionary! {
            "Type" => "Annot", "Subtype" => "Stamp", "F" => 4,
            "Rect" => vec![70.into(), 90.into(), 110.into(), 120.into()],
            "AP" => dictionary! { "N" => form_id },
        });
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
        let annotations = if image_first {
            vec![Object::Reference(stamp_id), Object::Reference(square_id)]
        } else {
            vec![Object::Reference(square_id), Object::Reference(stamp_id)]
        };
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
    fn image_stamp_preserves_rgb_pixels_rotation_and_annotation_order() {
        for rotation in [0, 90, 180, 270] {
            for compressed in [false, true] {
                for image_first in [false, true] {
                    let bytes = image_stamp_fixture(rotation, compressed, false, image_first);
                    if rotation == 90 && compressed && image_first {
                        if let Ok(path) = std::env::var("OPDS_IMAGE_AP_TEST_PDF") {
                            std::fs::write(path, &bytes).unwrap();
                        }
                    }
                    let doc = Document::load_mem(&bytes).unwrap();
                    let page = *doc.get_pages().get(&1).unwrap();
                    let (_, crop, turn) = page_geometry(&doc, 1).unwrap();
                    let image_index = if image_first { 0 } else { 1 };
                    assert!(parse_bounded_image_form(
                        &doc,
                        doc.get_page_annotations(page).unwrap()[image_index],
                        crop,
                        turn
                    )
                    .is_some());
                    let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
                    let expected = pdf.render_page(1, 1.0).unwrap();
                    let selected =
                        render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
                    assert!(selected.imported_annotations_dynlex, "rotation={rotation}");
                    assert_eq!(
                        selected.raster.rgba, expected.rgba,
                        "rotation={rotation} image_first={image_first}"
                    );
                }
            }
        }
    }

    fn layered_image_stamp_fixture(rotation: i64, compressed: bool, small_first: bool) -> Vec<u8> {
        let bytes = image_stamp_fixture(rotation, compressed, false, true);
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
        let first_image_id = doc
            .get_object(form_id)
            .unwrap()
            .as_stream()
            .unwrap()
            .dict
            .get(b"Resources")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"XObject")
            .unwrap()
            .as_dict()
            .unwrap()
            .get(b"Im0")
            .unwrap()
            .as_reference()
            .unwrap();
        let second_image_id = doc.add_object(Stream::new(
            dictionary! {
                "Type" => "XObject", "Subtype" => "Image", "Width" => 20,
                "Height" => 15, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8,
            },
            [70, 40, 230].repeat(20 * 15),
        ));
        let form = doc
            .get_object_mut(form_id)
            .unwrap()
            .as_stream_mut()
            .unwrap();
        form.dict.set(
            "Resources",
            dictionary! {
                "XObject" => dictionary! { "Im0" => first_image_id, "Im1" => second_image_id }
            },
        );
        let large = b"q 40 0 0 30 0 0 cm /Im0 Do Q".as_slice();
        let small = b"q 20 0 0 15 10 8 cm /Im1 Do Q".as_slice();
        form.content = if small_first {
            [small, b" ", large].concat()
        } else {
            [large, b" ", small].concat()
        };
        form.dict.set("Length", form.content.len() as i64);
        let mut result = Vec::new();
        doc.save_to(&mut result).unwrap();
        result
    }

    #[test]
    fn two_images_inside_one_form_preserve_overlap_and_rotation() {
        for rotation in [0, 90, 180, 270] {
            for compressed in [false, true] {
                for small_first in [false, true] {
                    let bytes = layered_image_stamp_fixture(rotation, compressed, small_first);
                    if rotation == 90 && compressed && !small_first {
                        if let Ok(path) = std::env::var("OPDS_LAYERED_IMAGE_AP_TEST_PDF") {
                            std::fs::write(path, &bytes).unwrap();
                        }
                    }
                    let doc = Document::load_mem(&bytes).unwrap();
                    let page_id = *doc.get_pages().get(&1).unwrap();
                    let (_, crop, turn) = page_geometry(&doc, 1).unwrap();
                    assert_eq!(
                        parse_bounded_image_form(
                            &doc,
                            doc.get_page_annotations(page_id).unwrap()[0],
                            crop,
                            turn,
                        )
                        .unwrap()
                        .len(),
                        2
                    );
                    let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
                    let selected =
                        render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
                    assert!(selected.imported_annotations_dynlex);
                    assert_eq!(
                        selected.raster.rgba,
                        pdf.render_page(1, 1.0).unwrap().rgba,
                        "rotation={rotation}, compressed={compressed}, small_first={small_first}"
                    );
                }
            }
        }
    }

    fn mixed_image_vector_form_fixture(rotation: i64, compressed: bool) -> Vec<u8> {
        let original = layered_image_stamp_fixture(rotation, compressed, false);
        let mut doc = Document::load_mem(&original).unwrap();
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
        form.content = b"q 0 1 0 rg 0 0 40 30 re f \
            q 40 0 0 30 0 0 cm /Im0 Do Q \
            1 0 0 rg 5 5 15 10 re f \
            q 20 0 0 15 10 8 cm /Im1 Do Q \
            0 0 0 RG 2 w 1 1 38 28 re S Q"
            .to_vec();
        form.dict.set("Length", form.content.len() as i64);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn mixed_image_and_vector_form_preserves_pdf_paint_order() {
        for rotation in [0, 90, 180, 270] {
            for compressed in [false, true] {
                let bytes = mixed_image_vector_form_fixture(rotation, compressed);
                if rotation == 90 && compressed {
                    if let Ok(path) = std::env::var("OPDS_MIXED_AP_TEST_PDF") {
                        std::fs::write(path, &bytes).unwrap();
                    }
                }
                let doc = Document::load_mem(&bytes).unwrap();
                let page_id = *doc.get_pages().get(&1).unwrap();
                let (_, crop, turn) = page_geometry(&doc, 1).unwrap();
                let staged = parse_bounded_opaque_form(
                    &doc,
                    doc.get_page_annotations(page_id).unwrap()[0],
                    crop,
                    turn,
                )
                .unwrap();
                assert_eq!(staged.len(), 5);
                let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
                let selected = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
                assert!(selected.imported_annotations_dynlex);
                assert_eq!(
                    selected.raster.rgba,
                    pdf.render_page(1, 1.0).unwrap().rgba,
                    "rotation={rotation}, compressed={compressed}"
                );
            }
        }
    }

    #[test]
    fn mixed_form_rejects_vector_paint_under_unmodeled_image_ctm() {
        let original = mixed_image_vector_form_fixture(90, true);
        let mut doc = Document::load_mem(&original).unwrap();
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
        form.content = b"q 0 1 0 rg 0 0 40 30 re f \
            q 40 0 0 30 0 0 cm /Im0 Do 1 0 0 rg 5 5 15 10 re f Q Q"
            .to_vec();
        form.dict.set("Length", form.content.len() as i64);
        let (_, crop, turn) = page_geometry(&doc, 1).unwrap();
        assert!(parse_bounded_opaque_form(
            &doc,
            doc.get_page_annotations(page_id).unwrap()[0],
            crop,
            turn,
        )
        .is_none());
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        let selected = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
        assert!(!selected.imported_annotations_dynlex);
        assert_eq!(selected.raster.rgba, pdf.render_page(1, 1.0).unwrap().rgba);
    }

    fn clipped_mixed_form_fixture(rotation: i64, compressed: bool, even_odd: bool) -> Vec<u8> {
        let original = mixed_image_vector_form_fixture(rotation, compressed);
        let mut doc = Document::load_mem(&original).unwrap();
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
        let rule = if even_odd { "W*" } else { "W" };
        form.content = format!(
            "q 0 1 0 rg 0 0 40 30 re f \
             q 10 5 20 20 re {rule} n \
             q 40 0 0 30 0 0 cm /Im0 Do Q \
             1 0 0 rg 5 5 15 10 re f \
             q 20 0 0 15 10 8 cm /Im1 Do Q Q \
             0 0 0 RG 2 w 1 1 38 28 re S Q"
        )
        .into_bytes();
        form.dict.set("Length", form.content.len() as i64);
        let mut result = Vec::new();
        doc.save_to(&mut result).unwrap();
        result
    }

    #[test]
    fn rectangular_form_clip_applies_to_images_and_fills_then_restores() {
        for rotation in [0, 90, 180, 270] {
            for compressed in [false, true] {
                for even_odd in [false, true] {
                    let bytes = clipped_mixed_form_fixture(rotation, compressed, even_odd);
                    if rotation == 90 && compressed && even_odd {
                        if let Ok(path) = std::env::var("OPDS_CLIPPED_MIXED_AP_TEST_PDF") {
                            std::fs::write(path, &bytes).unwrap();
                        }
                    }
                    let doc = Document::load_mem(&bytes).unwrap();
                    let page_id = *doc.get_pages().get(&1).unwrap();
                    let (_, crop, turn) = page_geometry(&doc, 1).unwrap();
                    assert!(parse_bounded_opaque_form(
                        &doc,
                        doc.get_page_annotations(page_id).unwrap()[0],
                        crop,
                        turn,
                    )
                    .is_some());
                    let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
                    let selected =
                        render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
                    assert!(selected.imported_annotations_dynlex);
                    assert_eq!(
                        selected.raster.rgba,
                        pdf.render_page(1, 1.0).unwrap().rgba,
                        "rotation={rotation}, compressed={compressed}, even_odd={even_odd}"
                    );
                }
            }
        }
    }

    #[test]
    fn nested_form_clips_intersect_and_restore() {
        for rotation in [0, 90, 180, 270] {
            let original = clipped_mixed_form_fixture(rotation, true, false);
            let mut doc = Document::load_mem(&original).unwrap();
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
            let content = String::from_utf8(form.content.clone()).unwrap();
            let content = content
                .replace(
                    "q 10 5 20 20 re W n",
                    "q 10 5 20 20 re W n q 16 10 8 10 re W* n",
                )
                .replace("/Im1 Do Q Q", "/Im1 Do Q Q Q");
            form.content = content.into_bytes();
            form.dict.set("Length", form.content.len() as i64);
            let mut bytes = Vec::new();
            doc.save_to(&mut bytes).unwrap();
            let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
            let selected = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
            assert!(selected.imported_annotations_dynlex);
            assert_eq!(
                selected.raster.rgba,
                pdf.render_page(1, 1.0).unwrap().rgba,
                "rotation={rotation}"
            );
        }
    }

    #[test]
    fn clipped_multi_contour_fill_keeps_image_and_stroke_order() {
        for rotation in [0, 90, 180, 270] {
            for compressed in [false, true] {
                let original = clipped_mixed_form_fixture(rotation, compressed, true);
                let mut doc = Document::load_mem(&original).unwrap();
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
                let content = String::from_utf8(form.content.clone()).unwrap();
                let content = content.replace(
                    "1 0 0 rg 5 5 15 10 re f",
                    "1 0 0 rg 3 3 m 37 3 l 37 27 l 3 27 l h \
                     12 9 m 28 9 l 28 21 l 12 21 l h f*",
                );
                form.content = content.into_bytes();
                form.dict.set("Length", form.content.len() as i64);
                let mut bytes = Vec::new();
                doc.save_to(&mut bytes).unwrap();
                if rotation == 90 && compressed {
                    if let Ok(path) = std::env::var("OPDS_CLIPPED_POLYGON_AP_TEST_PDF") {
                        std::fs::write(path, &bytes).unwrap();
                    }
                }
                let (_, crop, turn) = page_geometry(&doc, 1).unwrap();
                assert!(
                    parse_bounded_opaque_form(
                        &doc,
                        doc.get_page_annotations(page_id).unwrap()[0],
                        crop,
                        turn,
                    )
                    .is_some(),
                    "stage rotation={rotation}, compressed={compressed}"
                );
                let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
                let selected = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
                assert!(
                    selected.imported_annotations_dynlex,
                    "rotation={rotation}, compressed={compressed}"
                );
                assert_eq!(
                    selected.raster.rgba,
                    pdf.render_page(1, 1.0).unwrap().rgba,
                    "rotation={rotation}, compressed={compressed}"
                );
            }
        }
    }

    #[test]
    fn clipped_diagonal_forms_match_hayro_or_fall_back() {
        let mut accepted = 0;
        for index in 0..64 {
            let rotation = [0, 90, 180, 270][index % 4];
            let original = clipped_mixed_form_fixture(rotation, index % 2 == 0, true);
            let mut doc = Document::load_mem(&original).unwrap();
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
            let content = String::from_utf8(form.content.clone()).unwrap();
            let x0 = 2.0 + (index % 5) as f64 * 0.25;
            let y0 = 2.0 + (index % 7) as f64 * 0.25;
            let x1 = 32.0 + (index % 4) as f64 * 0.25;
            let y1 = 3.0 + (index % 3) as f64 * 0.25;
            let x2 = 17.0 + (index % 9) as f64 * 0.25;
            let y2 = 26.0 - (index % 6) as f64 * 0.25;
            form.content = content
                .replace(
                    "1 0 0 rg 5 5 15 10 re f",
                    &format!("1 0 0 rg {x0} {y0} m {x1} {y1} l {x2} {y2} l h f"),
                )
                .into_bytes();
            form.dict.set("Length", form.content.len() as i64);
            let mut bytes = Vec::new();
            doc.save_to(&mut bytes).unwrap();
            let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
            let selected = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
            accepted += usize::from(selected.imported_annotations_dynlex);
            assert_eq!(
                selected.raster.rgba,
                pdf.render_page(1, 1.0).unwrap().rgba,
                "triangle {index}, accepted={}",
                selected.imported_annotations_dynlex
            );
        }
        assert!(
            accepted >= 20,
            "only {accepted}/64 clipped polygons accepted"
        );
        eprintln!("accepted clipped polygons: {accepted}/64");
    }

    fn large_clipped_mixed_form_fixture() -> Vec<u8> {
        let original = clipped_mixed_form_fixture(90, true, true);
        let mut doc = Document::load_mem(&original).unwrap();
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
        let content = String::from_utf8(form.content.clone()).unwrap();
        form.content = content
            .replace(
                "1 0 0 rg 5 5 15 10 re f",
                "1 0 0 rg 3 3 m 37 3 l 37 27 l 3 27 l h \
                 12 9 m 28 9 l 28 21 l 12 21 l h f*",
            )
            .into_bytes();
        form.dict.set("Length", form.content.len() as i64);
        let mut annotations = vec![Object::Reference(annotation_id)];
        for (x, y) in [(500, 500), (1000, 800), (1500, 1500)] {
            let mut annotation = doc.get_dictionary(annotation_id).unwrap().clone();
            annotation.set(
                "Rect",
                vec![x.into(), y.into(), (x + 40).into(), (y + 30).into()],
            );
            annotations.push(Object::Reference(doc.add_object(annotation)));
        }
        let page = doc.get_dictionary_mut(page_id).unwrap();
        page.set(
            "MediaBox",
            vec![20.into(), 30.into(), 1724.into(), 2454.into()],
        );
        page.set(
            "CropBox",
            vec![40.into(), 60.into(), 1724.into(), 2444.into()],
        );
        page.set("Annots", annotations);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn large_rotated_page_with_four_clipped_mixed_forms_is_exact() {
        let bytes = large_clipped_mixed_form_fixture();
        if let Ok(path) = std::env::var("OPDS_LARGE_CLIPPED_AP_TEST_PDF") {
            std::fs::write(path, &bytes).unwrap();
        }
        let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        let selected = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
        assert!(selected.imported_annotations_dynlex);
        assert_eq!(
            (selected.raster.width, selected.raster.height),
            (2384, 1684)
        );
        assert_eq!(selected.raster.rgba, pdf.render_page(1, 1.0).unwrap().rgba);
    }

    #[test]
    fn clipped_form_rejects_incomplete_clip_and_partially_clipped_stroke() {
        let original = clipped_mixed_form_fixture(90, true, false);
        for content in [
            b"q 10 5 20 20 re W Q".as_slice(),
            b"q 10 5 20 20 re W n 0 0 40 30 re W Q".as_slice(),
            b"q 10 5 20 20 re W n 0 0 0 RG 2 w 1 1 38 28 re S Q".as_slice(),
            b"q 10 5 20 20 re W n 0 1 0 rg 0 0 40 30 re f".as_slice(),
        ] {
            let mut doc = Document::load_mem(&original).unwrap();
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
            let (_, crop, turn) = page_geometry(&doc, 1).unwrap();
            assert!(parse_bounded_opaque_form(
                &doc,
                doc.get_page_annotations(page_id).unwrap()[0],
                crop,
                turn,
            )
            .is_none());
        }
    }

    #[test]
    fn layered_image_form_rejects_unproven_graphics_before_painting() {
        let original = layered_image_stamp_fixture(90, true, false);
        for (name, content) in [
            (
                "missing resource",
                b"q 40 0 0 30 0 0 cm /Im0 Do Q q 20 0 0 15 10 8 cm /Ghost Do Q".as_slice(),
            ),
            (
                "outside bbox",
                b"q 40 0 0 30 0 0 cm /Im0 Do Q q 20 0 0 15 30 20 cm /Im1 Do Q".as_slice(),
            ),
            (
                "extra color state",
                b"q 40 0 0 30 0 0 cm /Im0 Do Q 1 0 0 rg q 20 0 0 15 10 8 cm /Im1 Do Q".as_slice(),
            ),
            (
                "incomplete group",
                b"q 40 0 0 30 0 0 cm /Im0 Do Q q 20 0 0 15 10 8 cm /Im1 Do".as_slice(),
            ),
        ] {
            let mut doc = Document::load_mem(&original).unwrap();
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
            form.dict.set("Length", content.len() as i64);
            let (_, crop, turn) = page_geometry(&doc, 1).unwrap();
            assert!(
                parse_bounded_image_form(
                    &doc,
                    doc.get_page_annotations(page_id).unwrap()[0],
                    crop,
                    turn
                )
                .is_none(),
                "{name}"
            );
            if name == "extra color state" {
                let mut bytes = Vec::new();
                doc.save_to(&mut bytes).unwrap();
                let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
                let selected = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
                assert!(!selected.imported_annotations_dynlex);
                assert_eq!(selected.raster.rgba, pdf.render_page(1, 1.0).unwrap().rgba);
            }
        }
    }

    #[test]
    fn unsupported_image_appearance_falls_back_without_partial_paint() {
        let bytes = image_stamp_fixture(90, false, true, true);
        let pdf = ParsedPdf::open(Arc::new(bytes.clone())).unwrap();
        let selected = render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex).unwrap();
        assert!(!selected.imported_annotations_dynlex);
        assert_eq!(selected.raster.rgba, pdf.render_page(1, 1.0).unwrap().rgba);
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
                    parse_bounded_opaque_form(&source_doc, source_stamp, crop, page_rotation)
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
    fn translated_form_paths_and_clip_match_hayro_on_rotated_crop() {
        for rotation in [0, 90, 180, 270] {
            for content in [
                &b"q 1 0 0 1 6 4 cm 1 0 0 rg 2 2 10 8 re f Q"[..],
                &b"q q 1 0 0 1 6 4 cm 1 0 0 rg 2 2 10 8 re f Q 0 0 1 rg 3 4 6 5 re f Q"[..],
                &b"q q 1 0 0 1 4 3 cm 0 1 0 rg 0 0 32 24 re W n 2 2 m 18 2 l 18 10 l 2 10 l h f Q 1 0 0 rg 3 3 5 5 re f Q"[..],
                &b"q q 1 0 0 1 4 3 cm 1 0 0 rg 0 1 0 RG 2 w 2 2 20 14 re B Q Q"[..],
            ] {
                let source = stamp_with_form_content(rotation, content);
                let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
                let expected = pdf.render_page(1, 1.0).unwrap();
                let actual = render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
                assert!(actual.imported_annotations_dynlex, "rotation {rotation}");
                assert_eq!(actual.raster.rgba, expected.rgba, "rotation {rotation}");
            }
        }
    }

    #[test]
    fn bounded_form_translations_are_pixel_exact_across_quarter_turns() {
        for index in 0..64 {
            let tx = (index * 7) % 15;
            let ty = (index * 11) % 12;
            let rotation = [0, 90, 180, 270][index % 4];
            let content =
                format!("q 1 0 0 1 {tx} {ty} cm 1 0 0 rg 2 2 10 8 re f 0 1 0 rg 16 3 7 6 re f Q");
            let source = stamp_with_form_content(rotation, content.as_bytes());
            let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
            let expected = pdf.render_page(1, 1.0).unwrap();
            let actual = render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
            assert!(actual.imported_annotations_dynlex, "case {index}");
            assert_eq!(actual.raster.rgba, expected.rgba, "case {index}");
        }
    }

    #[test]
    fn unproven_form_transforms_fall_back_without_partial_paint() {
        for content in [
            &b"q q 1 0 0 1 0.5 4 cm 1 0 0 rg 2 2 10 8 re f Q Q"[..],
            &b"q q 1 0 0 1 0.99999999 4 cm 1 0 0 rg 2 2 10 8 re f Q Q"[..],
            &b"q q 2 0 0 2 4 3 cm 1 0 0 rg 2 2 10 8 re f Q Q"[..],
            &b"q q 1 0 0 1 4 3 cm 1 0 0 1 1 1 cm 1 0 0 rg 2 2 10 8 re f Q Q"[..],
            &b"q q 1 0 0 1 30 25 cm 0 1 0 RG 2 w 2 2 20 14 re S Q Q"[..],
            &b"q 1 0 0 rg 2.00000001 2 10 8 re f Q"[..],
        ] {
            let source = stamp_with_form_content(90, content);
            let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
            let expected = pdf.render_page(1, 1.0).unwrap();
            let actual = render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
            assert!(!actual.imported_annotations_dynlex);
            assert_eq!(actual.raster.rgba, expected.rgba);
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
    fn imported_form_multi_contours_match_hayro_on_rotated_colored_page() {
        let mut paths = vec![
            "3 3 m 14 3 l 14 13 l 3 13 l h 22 15 m 36 15 l 36 26 l 22 26 l h f",
            "3 3 m 14 3 l 14 13 l 3 13 l h 22 15 m 22 26 l 36 26 l 36 15 l h f",
            "3 3 m 37 3 l 37 27 l 3 27 l h 12 9 m 28 9 l 28 21 l 12 21 l h f*",
            "12 9 m 28 9 l 28 21 l 12 21 l h 3 3 m 37 3 l 37 27 l 3 27 l h f*",
            "3 3 m 37 3 l 37 27 l 3 27 l h 12 9 m 12 21 l 28 21 l 28 9 l h f",
            "3 3 m 37 3 l 37 27 l 3 27 l h 12 9 m 28 9 l 28 21 l 12 21 l h f",
            "3 3 m 10 3 l 10 10 l 3 10 l h 15 3 m 22 3 l 22 10 l 15 10 l h 27 15 m 35 15 l 35 25 l 27 25 l h f*",
            "3 3 m 37 3 l 37 27 l 3 27 l h 7 8 m 14 8 l 14 17 l 7 17 l h 23 8 m 32 8 l 32 17 l 23 17 l h f*",
            "3 3 m 37 3 l 37 12 l 20 12 l 20 27 l 3 27 l h 8 8 m 14 8 l 14 11 l 8 11 l h 8 18 m 14 18 l 14 24 l 8 24 l h f*",
            "3 3 m 37 3 l 37 27 l 3 27 l h 8 7 m 32 7 l 32 23 l 8 23 l h 14 11 m 26 11 l 26 19 l 14 19 l h 18 13 m 22 13 l 22 17 l 18 17 l h f*",
            "3 3 m 37 3 l 37 27 l 3 27 l h 8 7 m 8 23 l 32 23 l 32 7 l h 14 11 m 26 11 l 26 19 l 14 19 l h 18 13 m 18 17 l 22 17 l 22 13 l h f",
            "3 3 m 37 3 l 37 27 l 3 27 l h 8 7 m 32 7 l 32 23 l 8 23 l h 14 11 m 14 19 l 26 19 l 26 11 l h 18 13 m 18 17 l 22 17 l 22 13 l h f",
        ];
        let mut eight = String::new();
        for row in 0..2 {
            for column in 0..4 {
                let x = 3 + 9 * column;
                let y = 3 + 12 * row;
                eight.push_str(&format!(
                    "{x} {y} m {} {y} l {} {} l {x} {} l h ",
                    x + 4,
                    x + 4,
                    y + 4,
                    y + 4,
                ));
            }
        }
        eight.push_str("f*");
        paths.push(&eight);
        for rotation in [0, 90, 180, 270] {
            for path in &paths {
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
                form.content = format!("q 1 0 0 rg {path} Q").into_bytes();
                form.dict.set("Length", form.content.len() as i64);
                let mut source = Vec::new();
                doc.save_to(&mut source).unwrap();
                let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
                let expected = pdf.render_page(1, 1.0).unwrap();
                let actual = render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
                assert!(actual.imported_annotations_dynlex, "{rotation} {path}");
                assert_eq!(actual.raster.rgba, expected.rgba, "{rotation} {path}");
            }
        }
    }

    #[test]
    fn bounded_even_odd_holes_are_pixel_exact_when_accepted() {
        let mut accepted = 0;
        for index in 0_u32..96 {
            let x0 = f64::from(5 + index * 7 % 18) + 0.25;
            let y0 = f64::from(5 + index * 11 % 12) + 0.25;
            let x1 = f64::from(6 + (index * 13 + 5) % 28) + 0.5;
            let y1 = f64::from(5 + (index * 17 + 3) % 18);
            let x2 = f64::from(5 + (index * 19 + 7) % 28) + 0.75;
            let y2 = f64::from(5 + (index * 23 + 9) % 18) + 0.5;
            let bytes = opaque_stamp_fixture(if index % 4 == 0 { 90 } else { 0 }, false);
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
            form.content = format!(
                "q 0.906 0.298 0.235 rg 3 3 m 37 3 l 37 27 l 3 27 l h {x0} {y0} m {x1} {y1} l {x2} {y2} l h f* Q",
            )
            .into_bytes();
            form.dict.set("Length", form.content.len() as i64);
            let mut source = Vec::new();
            doc.save_to(&mut source).unwrap();
            let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
            let expected = pdf.render_page(1, 1.0).unwrap();
            let actual = render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
            if actual.imported_annotations_dynlex {
                accepted += 1;
            }
            assert_eq!(actual.raster.rgba, expected.rgba, "hole {index}");
        }
        assert!(
            accepted >= 80,
            "too many safe hole fallbacks: {accepted}/96"
        );
        eprintln!("accepted even-odd holes: {accepted}/96");
    }

    #[test]
    fn two_disjoint_holes_keep_one_rounding_step() {
        let mut accepted = 0;
        for index in 0_u32..64 {
            let x0 = f64::from(5 + index * 3 % 4) + 0.25;
            let x1 = f64::from(10 + index * 5 % 4) + 0.5;
            let x2 = f64::from(7 + index * 7 % 5) + 0.75;
            let y0 = f64::from(6 + index * 11 % 6) + 0.25;
            let y1 = f64::from(6 + index * 13 % 6);
            let y2 = f64::from(11 + index * 17 % 8) + 0.5;
            let bytes = opaque_stamp_fixture(if index % 4 == 0 { 90 } else { 0 }, false);
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
            form.content = format!(
                "q 0.906 0.298 0.235 rg 3 3 m 37 3 l 37 27 l 3 27 l h {x0} {y0} m {x1} {y1} l {x2} {y2} l h {} {y0} m {} {y1} l {} {y2} l h f* Q",
                x0 + 19.0,
                x1 + 19.0,
                x2 + 19.0,
            )
            .into_bytes();
            form.dict.set("Length", form.content.len() as i64);
            let mut source = Vec::new();
            doc.save_to(&mut source).unwrap();
            let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
            let expected = pdf.render_page(1, 1.0).unwrap();
            let actual = render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
            if actual.imported_annotations_dynlex {
                accepted += 1;
            }
            assert_eq!(actual.raster.rgba, expected.rgba, "two holes {index}");
        }
        assert!(
            accepted >= 40,
            "too many safe multi-hole fallbacks: {accepted}/64"
        );
        eprintln!("accepted two-hole forms: {accepted}/64");
    }

    #[test]
    fn imported_form_hole_on_large_rotated_page_uses_bounded_tile() {
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
        doc.get_dictionary_mut(page_id).unwrap().set(
            "MediaBox",
            vec![20.into(), 30.into(), 1724.into(), 2454.into()],
        );
        doc.get_dictionary_mut(page_id).unwrap().set(
            "CropBox",
            vec![40.into(), 60.into(), 1724.into(), 2444.into()],
        );
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
            b"q 1 0 0 rg 3 3 m 37 3 l 37 27 l 3 27 l h 12 9 m 28 9 l 28 21 l 12 21 l h f* Q"
                .to_vec();
        form.dict.set("Length", form.content.len() as i64);
        let mut source = Vec::new();
        doc.save_to(&mut source).unwrap();
        let pdf = ParsedPdf::open(Arc::new(source.clone())).unwrap();
        let expected = pdf.render_page(1, 1.0).unwrap();
        assert_eq!((expected.width, expected.height), (2384, 1684));
        let actual = render_with_report(&pdf, &source, 1, 1.0, Backend::Dynlex).unwrap();
        assert!(actual.imported_annotations_dynlex);
        assert_eq!(actual.raster.rgba, expected.rgba);
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
        let mut cases: Vec<String> = [
            // The first fill is supported. A later invalid path must not
            // leave that fill on a partially DynLex-painted page.
            "q 0 1 0 rg 0 0 40 30 re f 1 0 0 rg 2 2 m 12 12 l S Q",
            "q 1 0 0 rg 2 2 m 20 20 l 2 20 l 20 2 l h f Q",
            "q 1 0 0 rg 2 2 m 20 20 l 2 20 l 20 2 l c f Q",
            "q 1 0 0 rg 2 2 m 20 2 l 20 20 l h 1 1 m 2 1 l 2 2 l h f Q",
            "q 1 0 0 rg 3 3 m 24 3 l 24 24 l 3 24 l h 16 8 m 36 8 l 36 27 l 16 27 l h f* Q",
            "q 1 0 0 rg 2 2 m 20 2 l 20 20 l h 3 3 l f Q",
            "q 1 0 0 rg 2.125 2 m 20 2 l 20 20 l h f Q",
            "q 1 0 0 rg 2 2 m 42 2 l 20 20 l h f Q",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        let mut ninth_contour = String::from("q 1 0 0 rg ");
        for row in 0..3 {
            for column in 0..3 {
                let x = 3 + column * 12;
                let y = 3 + row * 8;
                ninth_contour.push_str(&format!(
                    "{x} {y} m {} {y} l {} {} l {x} {} l h ",
                    x + 4,
                    x + 4,
                    y + 4,
                    y + 4,
                ));
            }
        }
        ninth_contour.push_str("f* Q");
        cases.push(ninth_contour);
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
        assert!(parse_bounded_opaque_form(&doc, stamp, crop, rotation).is_none());
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
        assert!(parse_bounded_opaque_form(
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
        assert!(parse_bounded_opaque_form(
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
        assert!(parse_bounded_opaque_form(
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
            assert!(parse_bounded_opaque_form(
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
