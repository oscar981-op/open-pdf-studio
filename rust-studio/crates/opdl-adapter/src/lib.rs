//! Narrow fixture adapter, not a general PDF interpreter.
#![forbid(unsafe_code)]

pub mod geometry_v1;
pub mod geometry_v2;
pub mod geometry_v3;
pub mod paint_v4;
pub mod path_v3;

fn load_path_probe(
    path: &std::path::Path,
) -> Result<(u32, u32, Content), Box<dyn std::error::Error>> {
    let bytes = std::fs::read(path)?;
    if bytes.len() > 16_000_000 {
        return Err(fail("path probe PDF is too large"));
    }
    let document = Document::load_mem(&bytes)?;
    let pages = document.get_pages();
    if pages.len() != 1 {
        return Err(fail("path probe requires one page"));
    }
    let page_id = *pages.values().next().ok_or_else(|| fail("missing page"))?;
    let page = document.get_dictionary(page_id)?;
    if page.has(b"Rotate") || page.has(b"CropBox") || page.has(b"Annots") || page.has(b"UserUnit") {
        return Err(fail("rotation, crop and annotations are not in path probe"));
    }
    let mut parent = page
        .get(b"Parent")
        .ok()
        .map(|value| value.as_reference())
        .transpose()?;
    let mut seen = std::collections::HashSet::new();
    while let Some(parent_id) = parent {
        if !seen.insert(parent_id) {
            return Err(fail("cyclic page parent tree"));
        }
        let node = document.get_dictionary(parent_id)?;
        if node.has(b"Rotate") || node.has(b"CropBox") || node.has(b"UserUnit") {
            return Err(fail("inherited geometry is outside path probe"));
        }
        parent = node
            .get(b"Parent")
            .ok()
            .map(|value| value.as_reference())
            .transpose()?;
    }
    let [x0, y0, x1, y1] = operands::<4>(page.get(b"MediaBox")?.as_array()?)?;
    if x0 != 0.0
        || y0 != 0.0
        || !x1.is_finite()
        || !y1.is_finite()
        || x1.fract() != 0.0
        || y1.fract() != 0.0
        || !(1.0..=16_384.0).contains(&x1)
        || !(1.0..=16_384.0).contains(&y1)
    {
        return Err(fail("unsupported path probe page bounds"));
    }
    let content_bytes = document.get_page_content(page_id)?;
    if content_bytes.len() > 2_000_000 {
        return Err(fail("path probe content is too large"));
    }
    if has_compound_operand(&content_bytes) {
        return Err(fail("compound operands are outside path probe"));
    }
    let content = Content::decode(&content_bytes)?;
    Ok((x1 as u32, y1 as u32, content))
}

pub fn page_to_v3_path(path: &std::path::Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use path_v3::PathList;
    let (width, height, content) = load_path_probe(path)?;
    let mut staged: Option<PathStage> = None;
    let mut segments = None;
    let mut finished = false;
    for op in content.operations {
        if finished {
            return Err(fail("content after path stroke is unsupported"));
        }
        match op.operator.as_str() {
            "m" => {
                if staged.is_some() {
                    return Err(fail("second subpath is outside path probe"));
                }
                let [x, y] = operands::<2>(&op.operands)?;
                staged = Some(PathStage::start([x, y]));
            }
            "l" => {
                let [x, y] = operands::<2>(&op.operands)?;
                staged
                    .as_mut()
                    .ok_or_else(|| fail("line without move"))?
                    .line_to([x, y])?;
            }
            "c" => {
                let [a, b, c, d, e, f] = operands::<6>(&op.operands)?;
                staged
                    .as_mut()
                    .ok_or_else(|| fail("cubic without move"))?
                    .cubic_to([[a, b], [c, d], [e, f]])?;
            }
            "h" if op.operands.is_empty() => {
                staged
                    .as_mut()
                    .ok_or_else(|| fail("close without move"))?
                    .close()?;
            }
            "S" if op.operands.is_empty() => {
                let path = staged.take().ok_or_else(|| fail("stroke without move"))?;
                if !path.has_draw_segment() {
                    return Err(fail("stroke without line or curve"));
                }
                segments = Some(path.to_v3_segments());
                finished = true;
            }
            _ => return Err(fail("operator outside isolated path probe subset")),
        }
    }
    if !finished {
        return Err(fail("path probe requires a complete stroke"));
    }
    let list = PathList {
        width,
        height,
        segments: segments.ok_or_else(|| fail("missing path geometry"))?,
    };
    list.encode()
}

pub fn page_to_v4_strokes(path: &std::path::Path) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use paint_v4::{Command, DisplayList, MAX_RECORDS};
    let (width, height, content) = load_path_probe(path)?;
    let mut commands = Vec::new();
    let mut staged: Option<PathStage> = None;
    let mut rgb = None;
    let mut line_width = None;
    for op in content.operations {
        match op.operator.as_str() {
            "RG" if staged.is_none() => {
                let channels = operands::<3>(&op.operands)?;
                let mut color = [0_u8; 3];
                for (target, channel) in color.iter_mut().zip(channels) {
                    if !channel.is_finite() || !(0.0..=1.0).contains(&channel) {
                        return Err(fail("invalid PDF stroke color"));
                    }
                    *target = (channel * 255.0).round() as u8;
                }
                rgb = Some(color);
            }
            "w" if staged.is_none() => {
                let [value] = operands::<1>(&op.operands)?;
                if !value.is_finite() || value <= 0.0 || value > 1_000.0 {
                    return Err(fail("invalid PDF stroke width"));
                }
                line_width = Some(value);
            }
            "m" if staged.is_none() => {
                if rgb.is_none() || line_width.is_none() {
                    return Err(fail("v4 probe requires explicit RGB and width"));
                }
                let [x, y] = operands::<2>(&op.operands)?;
                staged = Some(PathStage::start([x, y]));
            }
            "l" => {
                let [x, y] = operands::<2>(&op.operands)?;
                staged
                    .as_mut()
                    .ok_or_else(|| fail("line without move"))?
                    .line_to([x, y])?;
            }
            "c" => {
                let [a, b, c, d, e, f] = operands::<6>(&op.operands)?;
                staged
                    .as_mut()
                    .ok_or_else(|| fail("cubic without move"))?
                    .cubic_to([[a, b], [c, d], [e, f]])?;
            }
            "h" if op.operands.is_empty() => {
                staged
                    .as_mut()
                    .ok_or_else(|| fail("close without move"))?
                    .close()?;
            }
            "S" if op.operands.is_empty() => {
                let path = staged.take().ok_or_else(|| fail("stroke without move"))?;
                if !path.has_draw_segment() {
                    return Err(fail("stroke without line or curve"));
                }
                let path_commands = path.to_v3_segments();
                if commands.len() + path_commands.len() + 2 > MAX_RECORDS {
                    return Err(fail("too many OPDL-v4 commands"));
                }
                commands.push(Command::StrokeStyle {
                    rgb: rgb.ok_or_else(|| fail("missing stroke RGB"))?,
                    width: line_width.ok_or_else(|| fail("missing stroke width"))?,
                });
                commands.extend(path_commands.into_iter().map(Command::Path));
                commands.push(Command::Stroke);
            }
            _ => return Err(fail("operator outside isolated v4 stroke probe subset")),
        }
    }
    if staged.is_some() || commands.is_empty() {
        return Err(fail("incomplete v4 stroke probe"));
    }
    DisplayList {
        width,
        height,
        commands,
    }
    .encode()
}

use lopdf::{
    content::{Content, Operation},
    Document, Object, Stream,
};
use std::{error::Error, fs, path::Path, time::Instant};

const LIMIT: u32 = 1024;
const MAX_COMMANDS: usize = 16_384;
const MAX_ORDERED_IMAGES: usize = 256;
const MAX_ORDERED_SCENE_BYTES: usize = 3_538_984;

// The fixture generator writes one canonical, whitespace-separated operation per
// line. Other PDF syntax stays on lopdf's general parser path.
fn decode_canonical_content(bytes: &[u8]) -> Option<Content> {
    if bytes.is_empty() || bytes.len() > 2_000_000 {
        return None;
    }
    let bytes = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    if bytes.is_empty() {
        return None;
    }
    let mut operations = Vec::new();
    for line in bytes.split(|byte| *byte == b'\n') {
        if line.is_empty() {
            return None;
        }
        let mut words = line.split(|byte| *byte == b' ');
        let mut operands = Vec::new();
        let mut operator = None;
        while let Some(word) = words.next() {
            if word.is_empty() || !word.is_ascii() {
                return None;
            }
            let first = word[0];
            if first.is_ascii_digit() || first == b'-' || first == b'+' || first == b'.' {
                if operator.is_some() {
                    return None;
                }
                let token = std::str::from_utf8(word).ok()?;
                let object = if let Ok(value) = token.parse::<i64>() {
                    Object::Integer(value)
                } else if token
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || byte == b'-' || byte == b'.')
                    && token.contains('.')
                {
                    Object::Real(token.parse::<f32>().ok()?)
                } else {
                    return None;
                };
                operands.push(object);
                if operands.len() > 6 {
                    return None;
                }
            } else if first == b'/'
                && word.len() > 1
                && word.len() <= 128
                && word[1..]
                    .iter()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'_' | b'.'))
                && operator.is_none()
            {
                operands.push(Object::Name(word[1..].to_vec()));
            } else {
                if words.next().is_some() {
                    return None;
                }
                let name = std::str::from_utf8(word).ok()?;
                if !matches!(
                    name,
                    "q" | "Q"
                        | "cm"
                        | "Do"
                        | "rg"
                        | "re"
                        | "W"
                        | "n"
                        | "f"
                        | "w"
                        | "RG"
                        | "m"
                        | "l"
                        | "c"
                        | "S"
                ) {
                    return None;
                }
                operator = Some(name.to_owned());
                break;
            }
        }
        let operator = operator?;
        let arity = match operator.as_str() {
            "q" | "Q" | "W" | "n" | "f" | "S" => 0,
            "Do" | "w" => 1,
            "m" | "l" => 2,
            "rg" | "RG" => 3,
            "re" => 4,
            "cm" | "c" => 6,
            _ => return None,
        };
        if operands.len() != arity || (operator == "Do" && operands[0].as_name().is_err()) {
            return None;
        }
        operations.push(Operation { operator, operands });
        if operations.len() > 40_000 {
            return None;
        }
    }
    let content = Content { operations };
    (content.encode().ok()?.as_slice() == bytes).then_some(content)
}

// This adapter cannot interpret compound PDF operands. Reject them before the
// general parser, which may otherwise ignore an unfinished trailing object.
fn has_compound_operand(bytes: &[u8]) -> bool {
    let mut comment = false;
    for &byte in bytes {
        if comment {
            if byte == b'\r' || byte == b'\n' {
                comment = false;
            }
        } else if byte == b'%' {
            comment = true;
        } else if matches!(byte, b'[' | b']' | b'(' | b')' | b'<' | b'>' | b'{' | b'}') {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod fast_decode_tests {
    use super::*;

    #[test]
    fn canonical_subset_matches_general_decoder() {
        let bytes = b"1 0 0 rg\n10 20 30 40 re\nf\n/Im0 Do";
        let fast = decode_canonical_content(bytes).expect("canonical input");
        let general = Content::decode(bytes).expect("general parser");
        assert_eq!(fast.encode().unwrap(), general.encode().unwrap());
        assert!(decode_canonical_content(b"1 0 0 rg\n").is_some());
        assert!(decode_canonical_content(b"/Im1 Do").is_some());
        assert!(decode_canonical_content(b"/Picture-42 Do").is_some());
    }

    #[test]
    fn noncanonical_syntax_falls_back() {
        for bytes in [
            b"1  0 0 rg".as_slice(),
            b"1 0 0 rg\n\n".as_slice(),
            b"%comment\n1 0 0 rg".as_slice(),
            b"1e3 0 0 rg".as_slice(),
            b"1 0 0 xx".as_slice(),
            b"/Im#31 Do".as_slice(),
            b"(text) Tj".as_slice(),
            b"[1 2] re".as_slice(),
            b"1\t0 0 rg".as_slice(),
            b"1 0 0 rg\r\n".as_slice(),
            b"1 0 0 rg 2 0 0 rg".as_slice(),
            b"1 0 rg".as_slice(),
        ] {
            assert!(decode_canonical_content(bytes).is_none(), "{bytes:?}");
        }
    }

    #[test]
    fn accepted_numeric_forms_match_lopdf_objects() {
        for token in [
            "0",
            "1",
            "-1",
            "+1",
            "01",
            "0.0",
            "1.25",
            "-1.25",
            ".5",
            "1.",
            "-0",
            "99999999999999999999",
            "1e3",
            "1.2.3",
            "NaN",
            "inf",
        ] {
            let bytes = format!("{token} w");
            if let Some(fast) = decode_canonical_content(bytes.as_bytes()) {
                let general = Content::decode(bytes.as_bytes())
                    .expect("fast decoder must not accept input rejected by lopdf");
                assert_eq!(fast.operations.len(), general.operations.len(), "{token}");
                for (left, right) in fast.operations.iter().zip(general.operations.iter()) {
                    assert_eq!(left.operator, right.operator, "{token}");
                    assert_eq!(left.operands, right.operands, "{token}");
                }
            }
        }
    }

    #[test]
    fn compound_syntax_is_detected_outside_comments() {
        assert!(has_compound_operand(b"[1 2 re\n"));
        assert!(has_compound_operand(b"(text) Tj\n"));
        assert!(!has_compound_operand(b"% [ignored]\n1 0 0 rg\n"));
    }
}

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
    exact_u32_with_limit(value, LIMIT)
}

fn exact_u32_with_limit(value: f64, limit: u32) -> Result<u32, Box<dyn Error>> {
    if !value.is_finite() || value < 0.0 || value > f64::from(limit) || value.fract() != 0.0 {
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
    let Some([clip_left, clip_top, clip_right, clip_bottom]) = clip else {
        return (left < right && top < bottom).then_some(rect);
    };
    let result = [
        left.max(clip_left),
        top.max(clip_top),
        right.min(clip_right),
        bottom.min(clip_bottom),
    ];
    (result[0] < result[2] && result[1] < result[3]).then_some(result)
}

#[derive(Default)]
struct PathStage {
    points: Vec<[f64; 2]>,
    // Position among line endpoints preserves the order of mixed l/c paths.
    cubics: Vec<(usize, [[f64; 2]; 3])>,
    closed: bool,
}

impl PathStage {
    fn start(point: [f64; 2]) -> Self {
        Self {
            points: vec![point],
            cubics: Vec::new(),
            closed: false,
        }
    }

    fn line_to(&mut self, point: [f64; 2]) -> Result<(), Box<dyn Error>> {
        if self.closed {
            return Err(fail("segment after close is unsupported"));
        }
        if self.points.len() + self.cubics.len() > MAX_COMMANDS {
            return Err(fail("too many staged path segments"));
        }
        self.points.push(point);
        Ok(())
    }

    fn cubic_to(&mut self, controls_and_end: [[f64; 2]; 3]) -> Result<(), Box<dyn Error>> {
        if self.closed {
            return Err(fail("segment after close is unsupported"));
        }
        if self.points.len() + self.cubics.len() > MAX_COMMANDS {
            return Err(fail("too many staged path segments"));
        }
        if controls_and_end
            .iter()
            .flatten()
            .any(|value| !value.is_finite())
        {
            return Err(fail("non-finite cubic coordinate"));
        }
        self.cubics.push((self.points.len(), controls_and_end));
        Ok(())
    }

    fn close(&mut self) -> Result<(), Box<dyn Error>> {
        if self.closed
            || !self.has_draw_segment()
            || self.points.len() + self.cubics.len() >= MAX_COMMANDS
        {
            return Err(fail("invalid or oversized closed path"));
        }
        self.closed = true;
        Ok(())
    }

    fn has_draw_segment(&self) -> bool {
        self.points.len() > 1 || !self.cubics.is_empty()
    }

    fn closed_axis_rectangle(&self) -> Result<[f64; 4], Box<dyn Error>> {
        if !self.closed || !self.cubics.is_empty() || self.points.len() != 4 {
            return Err(fail(
                "clip path is not a single closed four-corner rectangle",
            ));
        }
        let left = self
            .points
            .iter()
            .map(|point| point[0])
            .fold(f64::INFINITY, f64::min);
        let right = self
            .points
            .iter()
            .map(|point| point[0])
            .fold(f64::NEG_INFINITY, f64::max);
        let bottom = self
            .points
            .iter()
            .map(|point| point[1])
            .fold(f64::INFINITY, f64::min);
        let top = self
            .points
            .iter()
            .map(|point| point[1])
            .fold(f64::NEG_INFINITY, f64::max);
        if ![left, right, bottom, top]
            .iter()
            .all(|value| value.is_finite())
            || left >= right
            || bottom >= top
        {
            return Err(fail("invalid rectangular clip path bounds"));
        }
        for corner in [[left, bottom], [left, top], [right, bottom], [right, top]] {
            if self.points.iter().filter(|point| **point == corner).count() != 1 {
                return Err(fail("clip path has unverified rectangle geometry"));
            }
        }
        for index in 0..4 {
            let a = self.points[index];
            let b = self.points[(index + 1) % 4];
            if !matches!((a[0] == b[0], a[1] == b[1]), (true, false) | (false, true)) {
                return Err(fail("clip path has unverified rectangle geometry"));
            }
        }
        Ok([left, bottom, right - left, top - bottom])
    }

    fn to_v3_segments(self) -> Vec<path_v3::Segment> {
        use path_v3::Segment;
        let mut segments =
            Vec::with_capacity(self.points.len() + self.cubics.len() + usize::from(self.closed));
        segments.push(Segment::Move(self.points[0]));
        let mut curves = self.cubics.into_iter().peekable();
        for index in 1..=self.points.len() {
            while curves
                .peek()
                .is_some_and(|(position, _)| *position == index)
            {
                let (_, controls) = curves.next().unwrap();
                segments.push(Segment::Cubic(controls));
            }
            if let Some(point) = self.points.get(index) {
                segments.push(Segment::Line(*point));
            }
        }
        if self.closed {
            segments.push(Segment::Close);
        }
        segments
    }

    fn collapsed_line(&self) -> Result<([f64; 2], [f64; 2]), Box<dyn Error>> {
        let [start, first, ..] = self.points.as_slice() else {
            return Err(fail("stroke without line"));
        };
        if self.points.len() == 2 {
            return Ok((*start, *first));
        }
        let axis = self.points.windows(2).find(|pair| pair[0] != pair[1]);
        let Some(axis) = axis else {
            return Ok((*start, *start));
        };
        let horizontal = axis[0][1] == axis[1][1];
        let direction = if horizontal {
            axis[1][0] - axis[0][0]
        } else {
            axis[1][1] - axis[0][1]
        };
        for pair in self.points.windows(2) {
            let [a, b] = [pair[0], pair[1]];
            let valid = if horizontal {
                a[1] == start[1] && b[1] == start[1] && direction * (b[0] - a[0]) >= 0.0
            } else {
                a[0] == start[0] && b[0] == start[0] && direction * (b[1] - a[1]) >= 0.0
            };
            if !valid {
                return Err(fail(
                    "only monotonic collinear multi-segment strokes are supported",
                ));
            }
        }
        Ok((
            *start,
            *self.points.last().ok_or_else(|| fail("empty path"))?,
        ))
    }

    fn orthogonal_rects(&self, width: u32, height: u32) -> Result<Vec<[u32; 4]>, Box<dyn Error>> {
        if !(3..=4).contains(&self.points.len()) {
            return Err(fail(
                "only one or two verified orthogonal joins are supported",
            ));
        }
        let mut rectangles = Vec::with_capacity(self.points.len() - 1);
        let mut previous_horizontal = None;
        for (index, pair) in self.points.windows(2).enumerate() {
            let [a, b] = [pair[0], pair[1]];
            let horizontal = a[1] == b[1] && a[0] != b[0];
            let vertical = a[0] == b[0] && a[1] != b[1];
            if (!horizontal && !vertical) || previous_horizontal == Some(horizontal) {
                return Err(fail("unverified path join or segment"));
            }
            previous_horizontal = Some(horizontal);
            let first = index == 0;
            let last = index + 2 == self.points.len();
            let rect = if horizontal {
                if a[1].fract() != 0.5
                    || a[0].fract() != if first { 0.0 } else { 0.5 }
                    || b[0].fract() != if last { 0.0 } else { 0.5 }
                    || (b[0] - a[0]).abs() < 2.0
                {
                    return Err(fail("horizontal path segment is not pixel aligned"));
                }
                let left = exact_u32(a[0].min(b[0]).floor())?;
                let right = exact_u32(a[0].max(b[0]).ceil())?;
                let row = exact_u32(f64::from(height) - a[1] - 0.5)?;
                [left, row, right, row + 1]
            } else {
                if a[0].fract() != 0.5
                    || a[1].fract() != if first { 0.0 } else { 0.5 }
                    || b[1].fract() != if last { 0.0 } else { 0.5 }
                    || (b[1] - a[1]).abs() < 2.0
                {
                    return Err(fail("vertical path segment is not pixel aligned"));
                }
                let column = exact_u32(a[0] - 0.5)?;
                let y0 = f64::from(height) - a[1];
                let y1 = f64::from(height) - b[1];
                let top = exact_u32(y0.min(y1).floor())?;
                let bottom = exact_u32(y0.max(y1).ceil())?;
                [column, top, column + 1, bottom]
            };
            if rect[0] >= rect[2] || rect[1] >= rect[3] || rect[2] > width || rect[3] > height {
                return Err(fail("path segment outside page or empty"));
            }
            rectangles.push(rect);
        }
        Ok(rectangles)
    }
}

#[cfg(test)]
mod path_stage_tests {
    use super::*;

    #[test]
    fn clip_path_accepts_only_closed_axis_rectangles() {
        for points in [
            [[10.0, 20.0], [30.0, 20.0], [30.0, 40.0], [10.0, 40.0]],
            [[30.0, 40.0], [30.0, 20.0], [10.0, 20.0], [10.0, 40.0]],
        ] {
            let mut path = PathStage::start(points[0]);
            for point in &points[1..] {
                path.line_to(*point).unwrap();
            }
            assert!(path.closed_axis_rectangle().is_err());
            path.close().unwrap();
            assert_eq!(
                path.closed_axis_rectangle().unwrap(),
                [10.0, 20.0, 20.0, 20.0]
            );
        }
        for points in [
            [[10.0, 20.0], [30.0, 40.0], [30.0, 20.0], [10.0, 40.0]],
            [[10.0, 20.0], [30.0, 20.0], [30.0, 20.0], [10.0, 40.0]],
        ] {
            let mut path = PathStage::start(points[0]);
            for point in &points[1..] {
                path.line_to(*point).unwrap();
            }
            path.close().unwrap();
            assert!(path.closed_axis_rectangle().is_err());
        }
    }

    #[test]
    fn collapses_long_monotone_path_without_changing_endpoints() {
        let mut path = PathStage::start([20.0, 40.5]);
        for x in 21..=148 {
            path.line_to([f64::from(x), 40.5]).unwrap();
        }
        assert_eq!(
            path.collapsed_line().unwrap(),
            ([20.0, 40.5], [148.0, 40.5])
        );
    }

    #[test]
    fn collapsed_line_rejects_turn_and_reversal() {
        let mut bent = PathStage::start([20.0, 40.5]);
        bent.line_to([60.0, 40.5]).unwrap();
        bent.line_to([60.0, 80.5]).unwrap();
        assert!(bent.collapsed_line().is_err());
        let mut reversed = PathStage::start([20.0, 40.5]);
        reversed.line_to([60.0, 40.5]).unwrap();
        reversed.line_to([40.0, 40.5]).unwrap();
        assert!(reversed.collapsed_line().is_err());
    }

    #[test]
    fn limits_staged_segments() {
        let mut path = PathStage::start([0.0, 0.0]);
        for x in 0..MAX_COMMANDS {
            path.line_to([x as f64, 0.0]).unwrap();
        }
        assert!(path.line_to([0.0, 0.0]).is_err());
    }

    #[test]
    fn orthogonal_path_maps_two_joins_and_rejects_more() {
        let mut path = PathStage::start([20.0, 151.5]);
        path.line_to([60.5, 151.5]).unwrap();
        path.line_to([60.5, 111.5]).unwrap();
        path.line_to([100.0, 111.5]).unwrap();
        assert_eq!(
            path.orthogonal_rects(256, 192).unwrap(),
            [[20, 40, 61, 41], [60, 40, 61, 81], [60, 80, 100, 81]]
        );
        path.line_to([100.0, 72.0]).unwrap();
        assert!(path.orthogonal_rects(256, 192).is_err());
    }

    #[test]
    fn stages_mixed_cubics_in_order_but_never_classifies_them_as_lines() {
        let mut path = PathStage::start([10.0, 20.0]);
        path.line_to([20.0, 20.0]).unwrap();
        path.cubic_to([[25.0, 20.0], [30.0, 25.0], [30.0, 30.0]])
            .unwrap();
        path.line_to([40.0, 30.0]).unwrap();
        path.cubic_to([[45.0, 30.0], [50.0, 35.0], [50.0, 40.0]])
            .unwrap();
        assert_eq!(path.cubics[0].0, 2);
        assert_eq!(path.cubics[1].0, 3);
        assert_eq!(path.cubics[1].1[2], [50.0, 40.0]);
        assert!(path
            .cubic_to([[f64::NAN, 0.0], [0.0; 2], [0.0; 2]])
            .is_err());
    }

    #[test]
    fn limits_cubic_staging_without_creating_draw_commands() {
        let mut path = PathStage::start([0.0, 0.0]);
        let curve = [[1.0, 0.0], [2.0, 1.0], [3.0, 1.0]];
        for _ in 0..MAX_COMMANDS {
            path.cubic_to(curve).unwrap();
        }
        assert_eq!(path.cubics.len(), MAX_COMMANDS);
        assert!(path.cubic_to(curve).is_err());
        assert!(std::mem::size_of_val(&path.cubics[0]) <= 64);
    }
}

fn image_bytes<'a>(
    document: &'a Document,
    resources: &'a lopdf::Dictionary,
    name: &[u8],
    placed_width: u32,
    placed_height: u32,
    allow_scaling: bool,
    force_catmull: bool,
) -> Result<std::borrow::Cow<'a, [u8]>, Box<dyn Error>> {
    if name.is_empty() || name.len() > 127 {
        return Err(fail("image resource name is outside the bounded subset"));
    }
    let xobjects = resources.get(b"XObject")?;
    let (_, xobjects) = document.dereference(xobjects)?;
    let xobjects = xobjects.as_dict()?;
    let reference = xobjects.get(name)?;
    let (_, image) = document.dereference(reference)?;
    let Stream { dict, content, .. } = image.as_stream()?;
    const IMAGE_KEYS: &[&[u8]] = &[
        b"Type",
        b"Subtype",
        b"Width",
        b"Height",
        b"ColorSpace",
        b"BitsPerComponent",
        b"Length",
        b"Filter",
        b"DecodeParms",
    ];
    if dict
        .iter()
        .any(|(key, _)| !IMAGE_KEYS.contains(&key.as_slice()))
    {
        return Err(fail("unsupported image dictionary key"));
    }
    let source_width = u32::try_from(dict.get(b"Width")?.as_i64()?)?;
    let source_height = u32::try_from(dict.get(b"Height")?.as_i64()?)?;
    let (_, color_space) = document.dereference(dict.get(b"ColorSpace")?)?;
    let indexed_palette = match color_space {
        lopdf::Object::Name(name) if name == b"DeviceRGB" => None,
        lopdf::Object::Array(parts) if parts.len() == 4 => {
            if parts[0].as_name()? != b"Indexed" || parts[1].as_name()? != b"DeviceRGB" {
                return Err(fail("unsupported Indexed image base color space"));
            }
            let high = usize::try_from(parts[2].as_i64()?)?;
            if high > 255 {
                return Err(fail("Indexed image palette exceeds 8-bit range"));
            }
            let (_, lookup) = document.dereference(&parts[3])?;
            let lopdf::Object::String(bytes, _) = lookup else {
                return Err(fail("Indexed image requires a literal lookup string"));
            };
            if bytes.len() != (high + 1) * 3 {
                return Err(fail("Indexed image palette has the wrong byte length"));
            }
            Some(bytes.as_slice())
        }
        _ => return Err(fail("unsupported image color space")),
    };
    if dict.get(b"Subtype")?.as_name()? != b"Image"
        || dict.get(b"BitsPerComponent")?.as_i64()? != 8
        || source_width == 0
        || source_height == 0
        || source_width > 4096
        || source_height > 4096
        || source_width
            .checked_mul(source_height)
            .is_none_or(|pixels| pixels > 6_000_000)
        || (!allow_scaling && (source_width != placed_width || source_height != placed_height))
    {
        return Err(fail(
            "image is outside bounded 8-bit source/placement subset",
        ));
    }
    let sample_len = source_width as usize
        * source_height as usize
        * if indexed_palette.is_some() { 1 } else { 3 };
    let png_predictor = if dict.has(b"DecodeParms") {
        if indexed_palette.is_some() {
            return Err(fail("Indexed image predictors are not yet supported"));
        }
        if !dict.has(b"Filter") || dict.get(b"Filter")?.as_name()? != b"FlateDecode" {
            return Err(fail(
                "image DecodeParms require a single FlateDecode filter",
            ));
        }
        let (_, params) = document.dereference(dict.get(b"DecodeParms")?)?;
        let params = params.as_dict()?;
        if params.iter().any(|(key, _)| {
            ![
                b"Predictor".as_slice(),
                b"Columns",
                b"Colors",
                b"BitsPerComponent",
            ]
            .contains(&key.as_slice())
        }) || params.get(b"Predictor")?.as_i64()? != 15
            || params.get(b"Columns")?.as_i64()? != i64::from(source_width)
            || params.get(b"Colors")?.as_i64()? != 3
            || params
                .get(b"BitsPerComponent")
                .ok()
                .is_some_and(|value| value.as_i64().ok() != Some(8))
        {
            return Err(fail("unsupported PNG image predictor parameters"));
        }
        true
    } else {
        false
    };
    let pixels = if dict.has(b"Filter") {
        if dict.get(b"Filter")?.as_name()? != b"FlateDecode" {
            return Err(fail("only a single FlateDecode image filter is supported"));
        }
        let decoded_len = if png_predictor {
            source_height as usize * (source_width as usize * 3 + 1)
        } else {
            sample_len
        };
        let decoded = miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(content, decoded_len)
            .map_err(|_| fail("invalid or oversized FlateDecode image"))?;
        if decoded.len() != decoded_len {
            return Err(fail("wrong decoded image byte length"));
        }
        std::borrow::Cow::Owned(if png_predictor {
            decode_png_rgb8_rows(&decoded, source_width as usize, source_height as usize)?
        } else {
            decoded
        })
    } else {
        if content.len() != sample_len {
            return Err(fail("wrong image byte length"));
        }
        std::borrow::Cow::Borrowed(content.as_slice())
    };
    let pixels = if let Some(palette) = indexed_palette {
        let mut rgb = Vec::with_capacity(source_width as usize * source_height as usize * 3);
        for &index in pixels.iter() {
            let offset = usize::from(index) * 3;
            let color = palette
                .get(offset..offset + 3)
                .ok_or_else(|| fail("Indexed image sample exceeds palette"))?;
            rgb.extend_from_slice(color);
        }
        std::borrow::Cow::Owned(rgb)
    } else {
        pixels
    };
    if (source_width, source_height) == (placed_width, placed_height) {
        return Ok(pixels);
    }
    if placed_width
        .checked_mul(placed_height)
        .is_none_or(|count| count > 6_000_000)
    {
        return Err(fail("placed RGB image exceeds pixel bound"));
    }
    let target_len = placed_width as usize * placed_height as usize * 3;
    let mut output = vec![0; target_len];
    if force_catmull || source_width > placed_width || source_height > placed_height {
        use pic_scale::{ImageSize, ImageStore, ImageStoreMut, ResamplingFunction, Scaler};
        let source = ImageStore::<u8, 3>::from_slice(
            &pixels,
            source_width as usize,
            source_height as usize,
        )?;
        let mut target = ImageStoreMut::<u8, 3>::from_slice(
            &mut output,
            placed_width as usize,
            placed_height as usize,
        )?;
        let plan = Scaler::new(ResamplingFunction::CatmullRom).plan_rgb_resampling(
            ImageSize::new(source_width as usize, source_height as usize),
            ImageSize::new(placed_width as usize, placed_height as usize),
        )?;
        plan.resample(&source, &mut target)?;
    } else {
        // Non-interpolated PDF images use nearest sampling at pixel centres.
        for y in 0..placed_height as usize {
            let source_y = ((2 * y + 1) * source_height as usize / (2 * placed_height as usize))
                .min(source_height as usize - 1);
            for x in 0..placed_width as usize {
                let source_x = ((2 * x + 1) * source_width as usize / (2 * placed_width as usize))
                    .min(source_width as usize - 1);
                let from = (source_y * source_width as usize + source_x) * 3;
                let to = (y * placed_width as usize + x) * 3;
                output[to..to + 3].copy_from_slice(&pixels[from..from + 3]);
            }
        }
    }
    Ok(std::borrow::Cow::Owned(output))
}

fn decode_png_rgb8_rows(
    encoded: &[u8],
    width: usize,
    height: usize,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let row_bytes = width
        .checked_mul(3)
        .ok_or_else(|| fail("PNG row overflow"))?;
    if encoded.len()
        != height
            .checked_mul(row_bytes + 1)
            .ok_or_else(|| fail("PNG image overflow"))?
    {
        return Err(fail("wrong PNG predictor byte length"));
    }
    let mut output = Vec::with_capacity(height * row_bytes);
    for row in 0..height {
        let encoded_at = row * (row_bytes + 1);
        let filter = encoded[encoded_at];
        if filter > 4 {
            return Err(fail("unsupported PNG row filter"));
        }
        let row_at = output.len();
        for column in 0..row_bytes {
            let left = if column >= 3 {
                output[row_at + column - 3]
            } else {
                0
            };
            let up = if row > 0 {
                output[row_at - row_bytes + column]
            } else {
                0
            };
            let upper_left = if row > 0 && column >= 3 {
                output[row_at - row_bytes + column - 3]
            } else {
                0
            };
            let prediction = match filter {
                0 => 0,
                1 => left,
                2 => up,
                3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                4 => {
                    let p = i32::from(left) + i32::from(up) - i32::from(upper_left);
                    let a = (p - i32::from(left)).abs();
                    let b = (p - i32::from(up)).abs();
                    let c = (p - i32::from(upper_left)).abs();
                    if a <= b && a <= c {
                        left
                    } else if b <= c {
                        up
                    } else {
                        upper_left
                    }
                }
                _ => unreachable!(),
            };
            output.push(encoded[encoded_at + 1 + column].wrapping_add(prediction));
        }
    }
    Ok(output)
}

#[cfg(test)]
mod png_predictor_tests {
    use super::decode_png_rgb8_rows;

    #[test]
    fn all_png_row_filters_and_invalid_rows() {
        // First row uses None. Each following row exercises a different filter.
        let encoded = [
            0, 10, 20, 30, 40, 50, 60, 1, 10, 20, 30, 10, 20, 30, 2, 5, 5, 5, 5, 5, 5, 3, 5, 5, 5,
            5, 5, 5, 4, 5, 5, 5, 5, 5, 5,
        ];
        let decoded = decode_png_rgb8_rows(&encoded, 2, 5).unwrap();
        assert_eq!(decoded.len(), 30);
        assert_eq!(&decoded[..6], &[10, 20, 30, 40, 50, 60]);
        let mut invalid = encoded;
        invalid[7] = 5;
        assert!(decode_png_rgb8_rows(&invalid, 2, 5).is_err());
        assert!(decode_png_rgb8_rows(&encoded[..encoded.len() - 1], 2, 5).is_err());
        assert!(decode_png_rgb8_rows(&encoded, usize::MAX, 5).is_err());
    }
}

fn image_to_display(
    pixels: &[u8],
    [x, y, placed_width, placed_height]: [u32; 4],
    crop: [u32; 4],
    turn: u32,
) -> Result<([u32; 4], Vec<u8>), Box<dyn Error>> {
    image_to_display_channels(pixels, [x, y, placed_width, placed_height], crop, turn, 3)
}

fn image_to_display_channels(
    pixels: &[u8],
    [x, y, placed_width, placed_height]: [u32; 4],
    crop: [u32; 4],
    turn: u32,
    channels: usize,
) -> Result<([u32; 4], Vec<u8>), Box<dyn Error>> {
    if !matches!(channels, 3 | 4)
        || pixels.len() != placed_width as usize * placed_height as usize * channels
    {
        return Err(fail("invalid RGB image patch length"));
    }
    let right = x
        .checked_add(placed_width)
        .ok_or_else(|| fail("image overflow"))?;
    let top = y
        .checked_add(placed_height)
        .ok_or_else(|| fail("image overflow"))?;
    let visible = [
        x.max(crop[0]),
        y.max(crop[1]),
        right.min(crop[2]),
        top.min(crop[3]),
    ];
    if visible[0] >= visible[2] || visible[1] >= visible[3] {
        return Err(fail("image outside visible CropBox"));
    }
    let view_width = visible[2] - visible[0];
    let view_height = visible[3] - visible[1];
    let rect = geometry_v1::rectangle_to_display(
        crop,
        turn,
        visible[0],
        visible[1],
        view_width,
        view_height,
    )?;
    let output_width = (rect[2] - rect[0]) as usize;
    let output_height = (rect[3] - rect[1]) as usize;
    let mut output = vec![0; output_width * output_height * channels];
    let source_left = visible[0] - x;
    let source_top = top - visible[3];
    for out_y in 0..output_height {
        for out_x in 0..output_width {
            let (in_x, in_y) = match turn {
                0 => (out_x, out_y),
                90 => (out_y, view_height as usize - 1 - out_x),
                180 => (
                    view_width as usize - 1 - out_x,
                    view_height as usize - 1 - out_y,
                ),
                270 => (view_width as usize - 1 - out_y, out_x),
                _ => return Err(fail("unsupported image page rotation")),
            };
            let source = ((source_top as usize + in_y) * placed_width as usize
                + source_left as usize
                + in_x)
                * channels;
            let target = (out_y * output_width + out_x) * channels;
            output[target..target + channels].copy_from_slice(&pixels[source..source + channels]);
        }
    }
    Ok((rect, output))
}

fn clip_display_image(
    rect: [u32; 4],
    pixels: Vec<u8>,
    clip: [u32; 4],
    channels: usize,
) -> Result<Option<([u32; 4], Vec<u8>)>, Box<dyn Error>> {
    let Some(visible) = intersect_rect(rect, Some(clip)) else {
        return Ok(None);
    };
    if visible == rect {
        return Ok(Some((rect, pixels)));
    }
    let source_width = (rect[2] - rect[0]) as usize;
    let target_width = (visible[2] - visible[0]) as usize;
    let target_height = (visible[3] - visible[1]) as usize;
    let row_bytes = target_width * channels;
    let mut output = Vec::with_capacity(row_bytes * target_height);
    for y in visible[1]..visible[3] {
        let from =
            ((y - rect[1]) as usize * source_width + (visible[0] - rect[0]) as usize) * channels;
        output.extend_from_slice(&pixels[from..from + row_bytes]);
    }
    Ok(Some((visible, output)))
}

fn fractional_clip_to_display(
    [x, y, w, h]: [f64; 4],
    crop: [u32; 4],
    turn: u32,
) -> Result<[f64; 4], Box<dyn Error>> {
    let left = x.max(f64::from(crop[0]));
    let bottom = y.max(f64::from(crop[1]));
    // lopdf stores PDF real operands as f32. Hayro's path builder adds the
    // rectangle extent at that precision before antialias coverage is found.
    // Keeping the sum in f64 can move a nominal half-pixel edge just below
    // the half-pixel and change one 8-bit coverage value.
    let right = f64::from((x as f32) + (w as f32)).min(f64::from(crop[2]));
    let top = f64::from((y as f32) + (h as f32)).min(f64::from(crop[3]));
    if left >= right || bottom >= top {
        return Err(fail("fractional clip is outside the visible CropBox"));
    }
    // The 180-degree page transform uses f32 subtraction in Hayro's path
    // coordinates. Widening the edge before that subtraction shifts some
    // half-pixel coverages by one byte. The other turns retain f64 page
    // subtraction, as their f32 form has different edge rounding.
    let display = match turn {
        0 => [
            left - f64::from(crop[0]),
            f64::from(crop[3]) - top,
            right - f64::from(crop[0]),
            f64::from(crop[3]) - bottom,
        ],
        90 => [
            bottom - f64::from(crop[1]),
            left - f64::from(crop[0]),
            top - f64::from(crop[1]),
            right - f64::from(crop[0]),
        ],
        180 => [
            f64::from((crop[2] as f32) - (right as f32)),
            bottom - f64::from(crop[1]),
            f64::from((crop[2] as f32) - (left as f32)),
            top - f64::from(crop[1]),
        ],
        270 => [
            f64::from(crop[3]) - top,
            f64::from(crop[2]) - right,
            f64::from(crop[3]) - bottom,
            f64::from(crop[2]) - left,
        ],
        _ => return Err(fail("unsupported fractional clip rotation")),
    };
    Ok(display)
}

fn mask_fractional_image_clip(
    rect: [u32; 4],
    pixels: Vec<u8>,
    clip: [f64; 4],
    channels: usize,
) -> Result<Option<([u32; 4], Vec<u8>)>, Box<dyn Error>> {
    let bounds = [
        (clip[0].floor() as u32).max(rect[0]),
        (clip[1].floor() as u32).max(rect[1]),
        (clip[2].ceil() as u32).min(rect[2]),
        (clip[3].ceil() as u32).min(rect[3]),
    ];
    let Some((rect, pixels)) = clip_display_image(rect, pixels, bounds, channels)? else {
        return Ok(None);
    };
    let mut output = vec![0; (rect[2] - rect[0]) as usize * (rect[3] - rect[1]) as usize * 4];
    for y in rect[1]..rect[3] {
        let coverage_y =
            (clip[3].min(f64::from(y + 1)) - clip[1].max(f64::from(y))).clamp(0.0, 1.0);
        for x in rect[0]..rect[2] {
            let coverage_x =
                (clip[2].min(f64::from(x + 1)) - clip[0].max(f64::from(x))).clamp(0.0, 1.0);
            let pixel =
                (y - rect[1]) as usize * (rect[2] - rect[0]) as usize + (x - rect[0]) as usize;
            let source = pixel * channels;
            let target = pixel * 4;
            output[target..target + 3].copy_from_slice(&pixels[source..source + 3]);
            let scaled_coverage = coverage_x * coverage_y * 255.0;
            let fractional_part = scaled_coverage.fract();
            let quarter_aligned = |value: f64| (value * 4.0 - (value * 4.0).round()).abs() < 1e-8;
            if (fractional_part - 0.5).abs() < 0.01
                && !(quarter_aligned(coverage_x) && quarter_aligned(coverage_y))
            {
                // A decimal PDF edge near a half-integer alpha threshold can
                // round either way after Hayro's f32 path transform. Until the
                // full path rasterizer is available, use the established
                // renderer rather than silently returning an off-by-one pixel.
                return Err(fail("ambiguous fractional clip coverage"));
            }
            let coverage = (scaled_coverage + 0.5) as u8;
            output[target + 3] = if channels == 4 && pixels[source + 3] == 0 {
                0
            } else {
                coverage
            };
        }
    }
    Ok(Some((rect, output)))
}

fn image_source_dimensions(
    document: &Document,
    resources: &lopdf::Dictionary,
    name: &[u8],
) -> Result<[u32; 2], Box<dyn Error>> {
    let (_, xobjects) = document.dereference(resources.get(b"XObject")?)?;
    let (_, object) = document.dereference(xobjects.as_dict()?.get(name)?)?;
    let dict = &object.as_stream()?.dict;
    Ok([
        u32::try_from(dict.get(b"Width")?.as_i64()?)?,
        u32::try_from(dict.get(b"Height")?.as_i64()?)?,
    ])
}

fn fractional_image_to_display(
    pixels: &[u8],
    raster_size: [u32; 2],
    placement: [f64; 4],
    media: [u32; 4],
    crop: [u32; 4],
    turn: u32,
) -> Result<([u32; 4], Vec<u8>), Box<dyn Error>> {
    let [x, y, width, height] = placement;
    if [x, y, width, height].iter().any(|value| !value.is_finite())
        || x < 0.0
        || y < 0.0
        || width <= 0.0
        || height <= 0.0
        || x + width > f64::from(media[2])
        || y + height > f64::from(media[3])
    {
        return Err(fail("fractional image placement outside page"));
    }
    let bbox = [
        x.floor() as u32,
        y.floor() as u32,
        (x + width).ceil() as u32,
        (y + height).ceil() as u32,
    ];
    let patch_width = bbox[2] - bbox[0];
    let patch_height = bbox[3] - bbox[1];
    if patch_width == 0
        || patch_height == 0
        || patch_width
            .checked_mul(patch_height)
            .is_none_or(|n| n > 6_000_000)
        || pixels.len() != raster_size[0] as usize * raster_size[1] as usize * 3
    {
        return Err(fail("invalid fractional RGB patch size"));
    }
    // Hayro/Vello thresholds the image rectangle's 8-bit area coverage at 1.
    // A covered pixel is fully opaque; a tiny corner under half an 8-bit step
    // is transparent, preserving earlier layers below the image.
    let mut patch = vec![0; patch_width as usize * patch_height as usize * 4];
    for row in 0..patch_height as usize {
        let page_y = f64::from(bbox[3]) - row as f64 - 0.5;
        let pixel_bottom = f64::from(bbox[3]) - row as f64 - 1.0;
        let overlap_y =
            ((y + height).min(pixel_bottom + 1.0) - y.max(pixel_bottom)).clamp(0.0, 1.0);
        let sample_y = ((y + height - page_y) * f64::from(raster_size[1]) / height)
            .floor()
            .clamp(0.0, f64::from(raster_size[1] - 1)) as usize;
        for column in 0..patch_width as usize {
            let page_x = f64::from(bbox[0]) + column as f64 + 0.5;
            let pixel_left = f64::from(bbox[0]) + column as f64;
            let overlap_x = ((x + width).min(pixel_left + 1.0) - x.max(pixel_left)).clamp(0.0, 1.0);
            let sample_x = ((page_x - x) * f64::from(raster_size[0]) / width)
                .floor()
                .clamp(0.0, f64::from(raster_size[0] - 1)) as usize;
            let source = (sample_y * raster_size[0] as usize + sample_x) * 3;
            let target = (row * patch_width as usize + column) * 4;
            patch[target..target + 3].copy_from_slice(&pixels[source..source + 3]);
            patch[target + 3] = if overlap_x * overlap_y * 255.0 + 0.5 >= 1.0 {
                255
            } else {
                0
            };
        }
    }
    image_to_display_channels(
        &patch,
        [bbox[0], bbox[1], patch_width, patch_height],
        crop,
        turn,
        4,
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct AxisCtm {
    sx: f64,
    sy: f64,
    tx: f64,
    ty: f64,
}

impl AxisCtm {
    const IDENTITY: Self = Self {
        sx: 1.0,
        sy: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    fn concat(self, [a, b, c, d, e, f]: [f64; 6]) -> Result<Self, Box<dyn Error>> {
        if [a, b, c, d, e, f].iter().any(|value| !value.is_finite())
            || b != 0.0
            || c != 0.0
            || a <= 0.0
            || d <= 0.0
        {
            return Err(fail("only positive axis-aligned PDF CTMs are supported"));
        }
        let result = Self {
            sx: self.sx * a,
            sy: self.sy * d,
            tx: self.sx.mul_add(e, self.tx),
            ty: self.sy.mul_add(f, self.ty),
        };
        if !(0.000_001..=4096.0).contains(&result.sx)
            || !(0.000_001..=4096.0).contains(&result.sy)
            || !result.tx.is_finite()
            || !result.ty.is_finite()
            || result.tx.abs() > 1_000_000.0
            || result.ty.abs() > 1_000_000.0
        {
            return Err(fail("PDF CTM exceeds bounded axis-aligned range"));
        }
        Ok(result)
    }

    fn point(self, [x, y]: [f64; 2]) -> [f64; 2] {
        [self.sx.mul_add(x, self.tx), self.sy.mul_add(y, self.ty)]
    }

    fn pixel_rect(self, [x, y, w, h]: [f64; 4], limit: u32) -> Result<[u32; 4], Box<dyn Error>> {
        if w <= 0.0 || h <= 0.0 {
            return Err(fail("nonpositive transformed rectangle"));
        }
        let [x, y] = self.point([x, y]);
        let rect = [
            pixel_aligned_u32(x, limit)?,
            pixel_aligned_u32(y, limit)?,
            pixel_aligned_u32(w * self.sx, limit)?,
            pixel_aligned_u32(h * self.sy, limit)?,
        ];
        if rect[2] == 0 || rect[3] == 0 {
            return Err(fail("subpixel rectangle needs coverage rendering"));
        }
        Ok(rect)
    }
}

fn pixel_aligned_u32(value: f64, limit: u32) -> Result<u32, Box<dyn Error>> {
    if !value.is_finite() || value < 0.0 || value > f64::from(limit) {
        return Err(fail("PDF CTM coordinate is outside page bounds"));
    }
    let rounded = value.round();
    if (value - rounded).abs() > 0.000_1 {
        return Err(fail("subpixel PDF CTM placement needs coverage rendering"));
    }
    Ok(rounded as u32)
}

#[cfg(test)]
mod axis_ctm_tests {
    use super::AxisCtm;

    #[test]
    fn composes_pdf_scale_and_local_image_matrix() {
        let global = AxisCtm::IDENTITY
            .concat([0.1, 0.0, 0.0, 0.1, 0.0, 0.0])
            .unwrap();
        let image = global
            .concat([800.0, 0.0, 0.0, 600.0, 300.0, 200.0])
            .unwrap();
        assert_eq!(
            image.pixel_rect([0.0, 0.0, 1.0, 1.0], 4096).unwrap(),
            [30, 20, 80, 60]
        );
        assert!(global
            .concat([800.0, 1.0, 0.0, 600.0, 300.0, 200.0])
            .is_err());
        assert!(global
            .concat([-800.0, 0.0, 0.0, 600.0, 300.0, 200.0])
            .is_err());
        assert!(global.pixel_rect([0.0, 0.0, 0.1, 10.0], 4096).is_err());
    }
}

fn visible_rect_to_display(
    crop: [u32; 4],
    turn: u32,
    [x, y, w, h]: [u32; 4],
) -> Result<[u32; 4], Box<dyn Error>> {
    let right = x.checked_add(w).ok_or_else(|| fail("rectangle overflow"))?;
    let top = y.checked_add(h).ok_or_else(|| fail("rectangle overflow"))?;
    let visible = [
        x.max(crop[0]),
        y.max(crop[1]),
        right.min(crop[2]),
        top.min(crop[3]),
    ];
    if visible[0] >= visible[2] || visible[1] >= visible[3] {
        return Ok([0, 0, 0, 0]);
    }
    geometry_v1::rectangle_to_display(
        crop,
        turn,
        visible[0],
        visible[1],
        visible[2] - visible[0],
        visible[3] - visible[1],
    )
}

#[derive(Clone, Copy, Debug, Default)]
pub struct AdapterTimings {
    pub read_ms: f64,
    pub document_ms: f64,
    pub page_decode_ms: f64,
    pub content_ms: f64,
    pub operator_decode_ms: f64,
    pub operators_ms: f64,
    pub encode_ms: f64,
}

pub fn page_to_opdl(path: &Path) -> Result<Vec<u8>, Box<dyn Error>> {
    Ok(page_to_opdl_with_timings(path)?.0)
}

pub fn page_to_opdl_with_timings(path: &Path) -> Result<(Vec<u8>, AdapterTimings), Box<dyn Error>> {
    let started = Instant::now();
    let bytes = fs::read(path)?;
    let read_ms = started.elapsed().as_secs_f64() * 1000.0;
    let (scene, mut timings) = page_to_opdl_bytes_with_timings(&bytes)?;
    timings.read_ms = read_ms;
    Ok((scene, timings))
}

pub fn page_to_opdl_bytes(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    Ok(page_to_opdl_bytes_with_timings(bytes)?.0)
}

/// Experimental ordered image/vector scene. The native caller validates the
/// complete byte stream before passing it to the DynLex ABI.
pub fn page_to_opdl_ordered_bytes(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    Ok(page_to_opdl_bytes_internal(bytes, true)?.0)
}

fn page_to_opdl_bytes_with_timings(
    bytes: &[u8],
) -> Result<(Vec<u8>, AdapterTimings), Box<dyn Error>> {
    page_to_opdl_bytes_internal(bytes, false)
}

fn page_to_opdl_bytes_internal(
    bytes: &[u8],
    ordered_images: bool,
) -> Result<(Vec<u8>, AdapterTimings), Box<dyn Error>> {
    let mut timings = AdapterTimings::default();
    if bytes.len() > 16_000_000 {
        return Err(fail("fixture PDF is too large"));
    }
    let started = Instant::now();
    let document = Document::load_mem(&bytes)?;
    timings.document_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let pages = document.get_pages();
    if pages.len() != 1 {
        return Err(fail("fixture must have exactly one page"));
    }
    let page_id = *pages.values().next().ok_or_else(|| fail("missing page"))?;
    let page = document.get_dictionary(page_id)?;
    if !ordered_images
        && (page.has(b"Rotate")
            || page.has(b"CropBox")
            || page.has(b"Annots")
            || page.has(b"UserUnit"))
    {
        return Err(fail(
            "rotation, crop, user units and annotations are not in this subset",
        ));
    }
    if !ordered_images {
        let mut parent = page
            .get(b"Parent")
            .ok()
            .map(Object::as_reference)
            .transpose()?;
        let mut visited = std::collections::HashSet::new();
        while let Some(parent_id) = parent {
            if !visited.insert(parent_id) || visited.len() > 16 {
                return Err(fail("cyclic or deep page tree"));
            }
            let node = document.get_dictionary(parent_id)?;
            if node.has(b"Rotate") || node.has(b"CropBox") || node.has(b"UserUnit") {
                return Err(fail("inherited page geometry is outside this subset"));
            }
            parent = node
                .get(b"Parent")
                .ok()
                .map(Object::as_reference)
                .transpose()?;
        }
    }
    let geometry = ordered_images
        .then(|| geometry_v1::page_geometry(&document, page_id))
        .transpose()?;
    let coordinate_limit = if ordered_images { 4096 } else { LIMIT };
    let (width, height, media, crop, turn) = if let Some(page_geometry) = geometry {
        let media = page_geometry.media;
        let crop = page_geometry.crop;
        let (width, height) = if matches!(page_geometry.turn, 90 | 270) {
            (crop[3] - crop[1], crop[2] - crop[0])
        } else {
            (crop[2] - crop[0], crop[3] - crop[1])
        };
        (width, height, media, crop, page_geometry.turn)
    } else {
        let media_box = page.get(b"MediaBox")?.as_array()?;
        let [x0, y0, x1, y1] = operands::<4>(media_box)?;
        if x0 != 0.0 || y0 != 0.0 {
            return Err(fail("fixture page must have zero origin"));
        }
        let width = exact_u32_with_limit(x1, coordinate_limit)?;
        let height = exact_u32_with_limit(y1, coordinate_limit)?;
        (
            width,
            height,
            [0, 0, width, height],
            [0, 0, width, height],
            0,
        )
    };
    if width == 0 || height == 0 {
        return Err(fail("empty page"));
    }
    if width
        .checked_mul(height)
        .is_none_or(|pixels| pixels > 6_000_000)
    {
        return Err(fail("page exceeds bounded pixel count"));
    }
    let exact_u32 = |value| exact_u32_with_limit(value, coordinate_limit);
    let (resources, inherited) = document.get_page_resources(page_id)?;
    if !inherited.is_empty() {
        return Err(fail("inherited resources are not in this subset"));
    }
    let resources = resources.ok_or_else(|| fail("missing resources"))?;
    let content_bytes = document.get_page_content(page_id)?;
    if has_compound_operand(&content_bytes) {
        return Err(fail("compound PDF operands are outside the fixture subset"));
    }
    timings.content_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let content = if std::env::var_os("OPDS_OPDL_LEGACY_DECODE").is_some() {
        Content::decode(&content_bytes)?
    } else if let Some(content) = decode_canonical_content(&content_bytes) {
        content
    } else {
        Content::decode(&content_bytes)?
    };
    timings.operator_decode_ms = started.elapsed().as_secs_f64() * 1000.0;
    timings.page_decode_ms = timings.content_ms + timings.operator_decode_ms;
    let mut depth = 0_u8;
    let mut image_matrix = false;
    let mut image_placement: Option<[u32; 4]> = None;
    let mut image_drawn = false;
    let mut image_count = 0_usize;
    let mut translation: Option<[f64; 2]> = None;
    let mut ctm = AxisCtm::IDENTITY;
    let mut saved_ctms = Vec::new();
    let mut saved_styles = Vec::new();
    let mut image = None;
    let mut color = [0_u8; 3];
    let mut stroke_color = [0_u8; 3];
    let mut one_pixel_stroke = false;
    let mut pending_rect = None;
    let mut pending_fractional_rect: Option<[f64; 4]> = None;
    let mut pending_clip = false;
    let mut clip = None;
    let mut saved_clips: Vec<Option<[u32; 4]>> = Vec::new();
    let mut fractional_clip: Option<[f64; 4]> = None;
    let mut saved_fractional_clips: Vec<Option<[f64; 4]>> = Vec::new();
    let mut pending_path: Option<PathStage> = None;
    let mut commands: Vec<(u32, [u32; 4], [u8; 4])> = Vec::new();
    let mut ordered_image_bytes = Vec::new();
    let mut has_fractional_image = false;
    let mut has_fractional_clip_image = false;
    let started = Instant::now();
    for op in &content.operations {
        match op.operator.as_str() {
            "q" if op.operands.is_empty()
                && depth < if ordered_images { 16 } else { 2 }
                && !image_matrix
                && translation.is_none()
                && pending_rect.is_none()
                && !pending_clip
                && pending_path.is_none() =>
            {
                saved_clips.push(clip);
                saved_fractional_clips.push(fractional_clip);
                saved_ctms.push(ctm);
                saved_styles.push((color, stroke_color, one_pixel_stroke));
                depth += 1;
            }
            "Q" if op.operands.is_empty()
                && depth > 0
                && pending_rect.is_none()
                && !pending_clip
                && pending_path.is_none()
                && (ordered_images || !image_matrix || image_drawn) =>
            {
                clip = saved_clips
                    .pop()
                    .ok_or_else(|| fail("restore without save"))?;
                fractional_clip = saved_fractional_clips
                    .pop()
                    .ok_or_else(|| fail("fractional clip restore without save"))?;
                depth -= 1;
                ctm = saved_ctms
                    .pop()
                    .ok_or_else(|| fail("CTM restore without save"))?;
                (color, stroke_color, one_pixel_stroke) = saved_styles
                    .pop()
                    .ok_or_else(|| fail("style restore without save"))?;
                translation = None;
                image_matrix = false;
                image_placement = None;
                image_drawn = false;
            }
            "cm" if ordered_images
                && depth > 0
                && pending_rect.is_none()
                && !pending_clip
                && pending_path.is_none() =>
            {
                ctm = ctm.concat(operands::<6>(&op.operands)?)?;
            }
            "cm" if depth == 1
                && !ordered_images
                && !image_matrix
                && translation.is_none()
                && pending_path.is_none() =>
            {
                let [a, b, c, d, e, f] = operands::<6>(&op.operands)?;
                if b == 0.0
                    && c == 0.0
                    && !(a == 1.0 && d == 1.0)
                    && image_count
                        < if ordered_images {
                            MAX_ORDERED_IMAGES
                        } else {
                            2
                        }
                    && (ordered_images || commands.is_empty())
                    && clip.is_none()
                {
                    let [placed_width, placed_height, x, y] =
                        [exact_u32(a)?, exact_u32(d)?, exact_u32(e)?, exact_u32(f)?];
                    if placed_width == 0
                        || placed_height == 0
                        || x.checked_add(placed_width)
                            .is_none_or(|right| right > media[2])
                        || y.checked_add(placed_height)
                            .is_none_or(|top| top > media[3])
                    {
                        return Err(fail("RGB image placement is outside the page"));
                    }
                    image_matrix = true;
                    image_placement = Some([x, y, placed_width, placed_height]);
                    image_drawn = false;
                } else if [a, b, c, d] == [1.0, 0.0, 0.0, 1.0]
                    && e.fract() == 0.0
                    && f.fract() == 0.0
                    && e.abs() <= f64::from(coordinate_limit)
                    && f.abs() <= f64::from(coordinate_limit)
                {
                    translation = Some([e, f]);
                } else {
                    return Err(fail(
                        "only integer translation of vector rectangles and axis-aligned strokes is supported",
                    ));
                }
            }
            "Do" if depth > 0
                && (ordered_images || depth == 1 && image_matrix)
                && image_count
                    < if ordered_images {
                        MAX_ORDERED_IMAGES
                    } else {
                        2
                    } =>
            {
                let [name] = op.operands.as_slice() else {
                    return Err(fail("Do requires one name"));
                };
                if ordered_images {
                    let name = name.as_name()?;
                    let (kind, rect, display_pixels) =
                        if let Ok([x, y, placed_width, placed_height]) =
                            ctm.pixel_rect([0.0, 0.0, 1.0, 1.0], coordinate_limit)
                        {
                            if x.checked_add(placed_width)
                                .is_none_or(|right| right > media[2])
                                || y.checked_add(placed_height)
                                    .is_none_or(|top| top > media[3])
                            {
                                return Err(fail("RGB image placement is outside the page"));
                            }
                            let pixels = image_bytes(
                                &document,
                                resources,
                                name,
                                placed_width,
                                placed_height,
                                true,
                                false,
                            )?;
                            let (rect, display_pixels) = image_to_display(
                                &pixels,
                                [x, y, placed_width, placed_height],
                                crop,
                                turn,
                            )?;
                            (6, rect, display_pixels)
                        } else {
                            let [source_width, source_height] =
                                image_source_dimensions(&document, resources, name)?;
                            let needs_resize = ctm.sx < f64::from(source_width)
                                || ctm.sy < f64::from(source_height);
                            let raster_width = if needs_resize {
                                ctm.sx.ceil() as u32
                            } else {
                                source_width
                            };
                            let raster_height = if needs_resize {
                                ctm.sy.ceil() as u32
                            } else {
                                source_height
                            };
                            let pixels = image_bytes(
                                &document,
                                resources,
                                name,
                                raster_width,
                                raster_height,
                                true,
                                needs_resize,
                            )?;
                            let (rect, display_pixels) = fractional_image_to_display(
                                &pixels,
                                [raster_width, raster_height],
                                [ctm.tx, ctm.ty, ctm.sx, ctm.sy],
                                media,
                                crop,
                                turn,
                            )?;
                            has_fractional_image = true;
                            (7, rect, display_pixels)
                        };
                    let clipped = if let Some(clip) = clip {
                        clip_display_image(
                            rect,
                            display_pixels,
                            clip,
                            if kind == 7 { 4 } else { 3 },
                        )?
                    } else {
                        Some((rect, display_pixels))
                    };
                    if let Some((rect, display_pixels)) = clipped {
                        let clipped = if let Some(clip) = fractional_clip {
                            mask_fractional_image_clip(
                                rect,
                                display_pixels,
                                clip,
                                if kind == 7 { 4 } else { 3 },
                            )?
                            .map(|(rect, pixels)| (8, rect, pixels))
                        } else {
                            Some((kind, rect, display_pixels))
                        };
                        if let Some((kind, rect, display_pixels)) = clipped {
                            if kind == 8 {
                                has_fractional_clip_image = true;
                            }
                            if ordered_image_bytes
                                .len()
                                .saturating_add(display_pixels.len())
                                > MAX_ORDERED_SCENE_BYTES
                            {
                                return Err(fail("ordered image data exceeds scene byte limit"));
                            }
                            let offset = u32::try_from(ordered_image_bytes.len())?;
                            commands.push((kind, rect, offset.to_le_bytes()));
                            if commands.len() > MAX_COMMANDS {
                                return Err(fail("too many commands"));
                            }
                            ordered_image_bytes.extend_from_slice(&display_pixels);
                        }
                    }
                } else {
                    let [x, y, placed_width, placed_height] =
                        image_placement.ok_or_else(|| fail("image placement is missing"))?;
                    let pixels = image_bytes(
                        &document,
                        resources,
                        name.as_name()?,
                        placed_width,
                        placed_height,
                        false,
                        false,
                    )?;
                    if image_count == 0
                        && [x, y, placed_width, placed_height] == [0, 0, width, height]
                    {
                        image = Some(pixels);
                    } else {
                        let mut page_rgb = image
                            .take()
                            .map(std::borrow::Cow::into_owned)
                            .unwrap_or_else(|| vec![255_u8; width as usize * height as usize * 3]);
                        let row_bytes = placed_width as usize * 3;
                        let top = height - y - placed_height;
                        for row in 0..placed_height as usize {
                            let source = row * row_bytes;
                            let target = ((top as usize + row) * width as usize + x as usize) * 3;
                            page_rgb[target..target + row_bytes]
                                .copy_from_slice(&pixels[source..source + row_bytes]);
                        }
                        image = Some(std::borrow::Cow::Owned(page_rgb));
                    }
                }
                image_count += 1;
                image_drawn = true;
            }
            "rg" if (ordered_images || depth == 0)
                && pending_rect.is_none()
                && pending_path.is_none() =>
            {
                let rgb = operands::<3>(&op.operands)?;
                for (target, channel) in color.iter_mut().zip(rgb) {
                    if !channel.is_finite() || !(0.0..=1.0).contains(&channel) {
                        return Err(fail("invalid RGB channel"));
                    }
                    *target = (channel * 255.0).round() as u8;
                }
            }
            "re" if !image_matrix && pending_rect.is_none() && pending_path.is_none() => {
                let [x, y, w, h] = operands::<4>(&op.operands)?;
                let [x, y, w, h] = if ordered_images {
                    let [tx, ty] = ctm.point([x, y]);
                    let transformed = [tx, ty, w * ctm.sx, h * ctm.sy];
                    if transformed.iter().any(|value| !value.is_finite())
                        || transformed[0] < 0.0
                        || transformed[1] < 0.0
                        || transformed[2] <= 0.0
                        || transformed[3] <= 0.0
                        || transformed[0] + transformed[2] > f64::from(media[2])
                        || transformed[1] + transformed[3] > f64::from(media[3])
                    {
                        return Err(fail("rectangle outside page"));
                    }
                    if let Ok(aligned) = ctm.pixel_rect([x, y, w, h], coordinate_limit) {
                        aligned
                    } else {
                        pending_fractional_rect = Some(transformed);
                        [0, 0, 0, 0]
                    }
                } else {
                    let [tx, ty] = translation.unwrap_or([0.0, 0.0]);
                    [
                        exact_u32(x + tx)?,
                        exact_u32(y + ty)?,
                        exact_u32(w)?,
                        exact_u32(h)?,
                    ]
                };
                if x + w > media[2] || y + h > media[3] {
                    return Err(fail("rectangle outside page"));
                }
                pending_rect = Some(if pending_fractional_rect.is_some() {
                    [0, 0, 0, 0]
                } else {
                    visible_rect_to_display(crop, turn, [x, y, w, h])?
                });
            }
            "W" | "W*"
                if !image_matrix
                    && op.operands.is_empty()
                    && pending_rect.is_some()
                    && !pending_clip =>
            {
                pending_clip = true;
            }
            "W" | "W*"
                if ordered_images
                    && !image_matrix
                    && op.operands.is_empty()
                    && pending_rect.is_none()
                    && pending_path.is_some()
                    && !pending_clip =>
            {
                let rect = pending_path
                    .take()
                    .ok_or_else(|| fail("clip path is missing"))?
                    .closed_axis_rectangle()?;
                if rect[0] < 0.0
                    || rect[1] < 0.0
                    || rect[0] + rect[2] > f64::from(media[2])
                    || rect[1] + rect[3] > f64::from(media[3])
                {
                    return Err(fail("polygonal rectangle clip is outside page"));
                }
                let aligned = rect
                    .map(|value| pixel_aligned_u32(value, coordinate_limit))
                    .into_iter()
                    .collect::<Result<Vec<_>, _>>();
                if let Ok(aligned) = aligned {
                    pending_rect = Some(visible_rect_to_display(
                        crop,
                        turn,
                        [aligned[0], aligned[1], aligned[2], aligned[3]],
                    )?);
                } else {
                    pending_fractional_rect = Some(rect);
                    pending_rect = Some([0, 0, 0, 0]);
                }
                pending_clip = true;
            }
            "n" if !image_matrix && op.operands.is_empty() && pending_clip => {
                let rect = pending_rect
                    .take()
                    .ok_or_else(|| fail("clip without rectangle"))?;
                if let Some(fractional) = pending_fractional_rect.take() {
                    let display = fractional_clip_to_display(fractional, crop, turn)?;
                    fractional_clip = Some(if let Some(prior) = fractional_clip {
                        [
                            display[0].max(prior[0]),
                            display[1].max(prior[1]),
                            display[2].min(prior[2]),
                            display[3].min(prior[3]),
                        ]
                    } else {
                        display
                    });
                } else {
                    clip = Some(intersect_rect(rect, clip).unwrap_or([0, 0, 0, 0]));
                }
                pending_clip = false;
            }
            "f" if !image_matrix && op.operands.is_empty() && !pending_clip => {
                if pending_fractional_rect.is_some() || fractional_clip.is_some() {
                    return Err(fail(
                        "fractional clip or fill requires general path coverage",
                    ));
                }
                let rect = pending_rect
                    .take()
                    .ok_or_else(|| fail("fill without rectangle"))?;
                if let Some(rect) = intersect_rect(rect, clip) {
                    commands.push((1, rect, [color[0], color[1], color[2], 255]));
                }
                if commands.len() > MAX_COMMANDS {
                    return Err(fail("too many commands"));
                }
            }
            "w" if (ordered_images || depth == 0)
                && pending_rect.is_none()
                && pending_path.is_none() =>
            {
                let [weight] = operands::<1>(&op.operands)?;
                if weight != 1.0 {
                    return Err(fail("only one-pixel butt-cap strokes are supported"));
                }
                one_pixel_stroke = true;
            }
            "RG" if (ordered_images || depth == 0)
                && pending_rect.is_none()
                && pending_path.is_none() =>
            {
                let rgb = operands::<3>(&op.operands)?;
                for (target, channel) in stroke_color.iter_mut().zip(rgb) {
                    if !channel.is_finite() || !(0.0..=1.0).contains(&channel) {
                        return Err(fail("invalid stroke RGB channel"));
                    }
                    *target = (channel * 255.0).round() as u8;
                }
            }
            "m" if !image_matrix && pending_rect.is_none() && pending_path.is_none() => {
                let [x, y] = operands::<2>(&op.operands)?;
                let point = if ordered_images {
                    ctm.point([x, y])
                } else {
                    let [tx, ty] = translation.unwrap_or([0.0, 0.0]);
                    [x + tx, y + ty]
                };
                pending_path = Some(PathStage::start(point));
            }
            "l" if !image_matrix && pending_rect.is_none() => {
                let [x, y] = operands::<2>(&op.operands)?;
                let end = if ordered_images {
                    ctm.point([x, y])
                } else {
                    let [tx, ty] = translation.unwrap_or([0.0, 0.0]);
                    [x + tx, y + ty]
                };
                pending_path
                    .as_mut()
                    .ok_or_else(|| fail("line without move"))?
                    .line_to(end)?;
            }
            "c" if !image_matrix && pending_rect.is_none() => {
                let [x1, y1, x2, y2, x3, y3] = operands::<6>(&op.operands)?;
                let curve = if ordered_images {
                    [
                        ctm.point([x1, y1]),
                        ctm.point([x2, y2]),
                        ctm.point([x3, y3]),
                    ]
                } else {
                    let [tx, ty] = translation.unwrap_or([0.0, 0.0]);
                    [[x1 + tx, y1 + ty], [x2 + tx, y2 + ty], [x3 + tx, y3 + ty]]
                };
                pending_path
                    .as_mut()
                    .ok_or_else(|| fail("cubic without move"))?
                    .cubic_to(curve)?;
            }
            "h" if ordered_images && !image_matrix && op.operands.is_empty() => {
                pending_path
                    .as_mut()
                    .ok_or_else(|| fail("close without path"))?
                    .close()?;
            }
            "S" if !image_matrix && op.operands.is_empty() && one_pixel_stroke => {
                if fractional_clip.is_some() {
                    return Err(fail("strokes under fractional clip are not yet supported"));
                }
                if ordered_images && (ctm.sx != 1.0 || ctm.sy != 1.0) {
                    return Err(fail("scaled stroke width is outside the proven subset"));
                }
                let path = pending_path
                    .take()
                    .ok_or_else(|| fail("stroke without move"))?;
                if path.closed {
                    return Err(fail("closed stroke is outside the proven subset"));
                }
                if !path.cubics.is_empty() {
                    return Err(fail("cubic stroke rasterization is not yet verified"));
                }
                if path.points.len() >= 3 && path.collapsed_line().is_err() {
                    if clip.is_some() || (ordered_images && (crop != media || turn != 0)) {
                        return Err(fail("orthogonal path with active clip is unsupported"));
                    }
                    let rectangles = path.orthogonal_rects(width, height)?;
                    if commands.len() + rectangles.len() > MAX_COMMANDS {
                        return Err(fail("too many commands"));
                    }
                    let ink = [stroke_color[0], stroke_color[1], stroke_color[2], 255];
                    for rect in rectangles {
                        commands.push((3, rect, ink));
                    }
                    continue;
                }
                let (start, end) = path.collapsed_line()?;
                let oriented_page = ordered_images && (crop != media || turn != 0);
                let (kind, rect) = if start[1] == end[1] {
                    let left = exact_u32(start[0].min(end[0]))?;
                    let right = exact_u32(start[0].max(end[0]))?;
                    if oriented_page {
                        let y = exact_u32(start[1] - 0.5)?;
                        (
                            3,
                            visible_rect_to_display(crop, turn, [left, y, right - left, 1])?,
                        )
                    } else {
                        let top = exact_u32(f64::from(height) - start[1] - 0.5)?;
                        (3, [left, top, right, top + 1])
                    }
                } else if start[0] == end[0] {
                    let left = exact_u32(start[0] - 0.5)?;
                    if oriented_page {
                        let bottom = exact_u32(start[1].min(end[1]))?;
                        let top = exact_u32(start[1].max(end[1]))?;
                        (
                            3,
                            visible_rect_to_display(crop, turn, [left, bottom, 1, top - bottom])?,
                        )
                    } else {
                        let top = height - exact_u32(start[1].max(end[1]))?;
                        let bottom = height - exact_u32(start[1].min(end[1]))?;
                        (3, [left, top, left + 1, bottom])
                    }
                } else if !oriented_page
                    && clip.is_none()
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
                } else if !oriented_page
                    && clip.is_none()
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
                if commands.len() > MAX_COMMANDS {
                    return Err(fail("too many commands"));
                }
            }
            _ => return Err(fail("PDF operator is outside the supported fixture subset")),
        }
    }
    if depth != 0
        || pending_rect.is_some()
        || pending_fractional_rect.is_some()
        || pending_clip
        || pending_path.is_some()
        || image_matrix
        || translation.is_some()
        || !saved_clips.is_empty()
        || !saved_fractional_clips.is_empty()
        || !saved_ctms.is_empty()
        || !saved_styles.is_empty()
    {
        return Err(fail("incomplete fixture content stream"));
    }
    timings.operators_ms = started.elapsed().as_secs_f64() * 1000.0;
    let started = Instant::now();
    let image = image.unwrap_or(std::borrow::Cow::Borrowed(&[]));
    let base_image: &[u8] = if ordered_images { &[] } else { &image };
    let mut output =
        Vec::with_capacity(40 + base_image.len() + commands.len() * 24 + ordered_image_bytes.len());
    output.extend_from_slice(b"OPDL");
    for value in [
        if ordered_images {
            if has_fractional_clip_image {
                6
            } else if has_fractional_image {
                5
            } else {
                3
            }
        } else {
            2
        },
        width,
        height,
        0,
        0,
        width,
        height,
        base_image.len() as u32,
    ] {
        output.extend_from_slice(&value.to_le_bytes());
    }
    output.extend_from_slice(base_image);
    output.extend_from_slice(&(commands.len() as u32).to_le_bytes());
    for (kind, rect, rgba) in commands {
        output.extend_from_slice(&kind.to_le_bytes());
        for value in rect {
            output.extend_from_slice(&value.to_le_bytes());
        }
        output.extend_from_slice(&rgba);
    }
    output.extend_from_slice(&ordered_image_bytes);
    if ordered_images && output.len() > MAX_ORDERED_SCENE_BYTES {
        return Err(fail("ordered scene exceeds byte limit"));
    }
    timings.encode_ms = started.elapsed().as_secs_f64() * 1000.0;
    Ok((output, timings))
}
