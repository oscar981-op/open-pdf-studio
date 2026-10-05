//! Experimental Type0 TrueType quadratic-outline staging, outside the normal adapter.
//!
//! The parabola integral/subdivision mathematics follows the Apache-2.0/MIT
//! licensed vello_common 0.0.8 `flatten_simd.rs`. This code calls neither
//! Vello nor Kurbo at runtime. DynLex rasterization remains a separate step.
use std::error::Error;

type Point = [f64; 2];

#[derive(Clone, Copy)]
enum Segment {
    Line(Point),
    Quad(Point, Point),
}

#[derive(Default)]
struct Contour {
    start: Point,
    segments: Vec<Segment>,
    closed: bool,
}

#[derive(Default)]
struct Outline {
    contours: Vec<Contour>,
    quads: usize,
    cubics: usize,
    segments: usize,
    overflow: bool,
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.contours.len() >= 8 {
            self.overflow = true;
            return;
        }
        self.contours.push(Contour {
            start: [f64::from(x), f64::from(y)],
            ..Contour::default()
        });
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.segments = self.segments.saturating_add(1);
        if self.segments > 256 {
            self.overflow = true;
            return;
        }
        if let Some(contour) = self.contours.last_mut() {
            if contour.closed {
                self.overflow = true;
                return;
            }
            contour
                .segments
                .push(Segment::Line([f64::from(x), f64::from(y)]));
        } else {
            self.overflow = true;
        }
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.quads = self.quads.saturating_add(1);
        self.segments = self.segments.saturating_add(1);
        if self.segments > 256 {
            self.overflow = true;
            return;
        }
        if let Some(contour) = self.contours.last_mut() {
            if contour.closed {
                self.overflow = true;
                return;
            }
            contour.segments.push(Segment::Quad(
                [f64::from(x1), f64::from(y1)],
                [f64::from(x), f64::from(y)],
            ));
        } else {
            self.overflow = true;
        }
    }
    fn curve_to(&mut self, _: f32, _: f32, _: f32, _: f32, _: f32, _: f32) {
        self.cubics = self.cubics.saturating_add(1);
    }
    fn close(&mut self) {
        if let Some(contour) = self.contours.last_mut() {
            if contour.closed {
                self.overflow = true;
            }
            contour.closed = true;
        } else {
            self.overflow = true;
        }
    }
}

fn add(a: Point, b: Point) -> Point {
    [a[0] + b[0], a[1] + b[1]]
}
fn sub(a: Point, b: Point) -> Point {
    [a[0] - b[0], a[1] - b[1]]
}
fn dot(a: Point, b: Point) -> f64 {
    a[0] * b[0] + a[1] * b[1]
}
fn cross(a: Point, b: Point) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}
fn integral(x: f64) -> f64 {
    const D: f64 = 0.67;
    x / (1.0 - D + (D.powi(4) + 0.25 * x * x).sqrt().sqrt())
}
fn inverse_integral(x: f64) -> f64 {
    const B: f64 = 0.39;
    x * (1.0 - B + (B * B + 0.25 * x * x).sqrt())
}
fn eval(p0: Point, p1: Point, p2: Point, t: f64) -> Point {
    let mt = 1.0 - t;
    let a = mt * mt;
    let b = mt * 2.0;
    [
        p0[0] * a + (p1[0] * b + p2[0] * t) * t,
        p0[1] * a + (p1[1] * b + p2[1] * t) * t,
    ]
}
fn flatten_quad(p0: Point, p1: Point, p2: Point, output: &mut Vec<Point>) -> Result<(), String> {
    let chord = sub(p2, p0);
    let chord_sq = dot(chord, chord);
    let t = if chord_sq > 0.0 {
        (dot(sub(p1, p0), chord) / chord_sq).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let nearest = [p0[0] + chord[0] * t, p0[1] + chord[1] * t];
    let distance = sub(p1, nearest);
    if dot(distance, distance) <= 0.25 {
        output.push(p2);
        return Ok(());
    }
    let d01 = sub(p1, p0);
    let d12 = sub(p2, p1);
    let dd = sub(d01, d12);
    let c = cross(chord, dd);
    let x0 = dot(d01, dd) / c;
    let x2 = dot(d12, dd) / c;
    let scale = (c / (dot(dd, dd).sqrt() * (x2 - x0))).abs();
    let a0 = integral(x0);
    let a2 = integral(x2);
    let val = if scale.is_finite() {
        let da = (a2 - a0).abs();
        let sqrt_scale = scale.sqrt();
        if x0.signum() == x2.signum() {
            da * sqrt_scale
        } else {
            let xmin = 0.5 / sqrt_scale;
            0.5 * da / integral(xmin)
        }
    } else {
        0.0
    };
    if !val.is_finite() || val < 0.0 {
        return Err("quadratic subdivision is not finite".into());
    }
    let n = (val.ceil() as usize).max(1);
    if n > 64 || output.len() + n > 256 {
        return Err("quadratic subdivision exceeds limit".into());
    }
    let u0 = inverse_integral(a0);
    let uscale = (inverse_integral(a2) - u0).recip();
    for i in 1..n {
        let u = (i as f64) / (n as f64);
        let a = a0 + (a2 - a0) * u;
        let t = (inverse_integral(a) - u0) * uscale;
        if !t.is_finite() || !(-0.001..=1.001).contains(&t) {
            return Err("quadratic parameter is not finite or bounded".into());
        }
        output.push(eval(p0, p1, p2, t));
    }
    output.push(p2);
    Ok(())
}

fn transform(point: Point, matrix: [f64; 6]) -> Point {
    let [a, b, c, d, e, f] = matrix;
    [
        a * point[0] + c * point[1] + e,
        b * point[0] + d * point[1] + f,
    ]
}

// OPQ1 is a read-only oracle format. Each fixed-size record has opcode,
// zero flags and four f64 coordinates. It retains the unflattened quadratic
// geometry so the independent flattener can be compared segment by segment.
fn raw_quadratic_path(
    outline: &Outline,
    matrix: [f64; 6],
    shift: Point,
) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut records = Vec::<(u32, [f64; 4])>::new();
    let map = |p| add(transform(p, matrix), shift);
    let bounded = |p: Point| -> Result<Point, Box<dyn Error>> {
        let p = map(p);
        if p.iter()
            .any(|v| !v.is_finite() || !(0.0..=4096.0).contains(v))
        {
            return Err("raw quadratic point outside tile bounds".into());
        }
        Ok(p)
    };
    for contour in &outline.contours {
        let p = bounded(contour.start)?;
        records.push((1, [p[0], p[1], 0.0, 0.0]));
        for segment in &contour.segments {
            match segment {
                Segment::Line(p) => {
                    let p = bounded(*p)?;
                    records.push((2, [p[0], p[1], 0.0, 0.0]));
                }
                Segment::Quad(c, p) => {
                    let c = bounded(*c)?;
                    let p = bounded(*p)?;
                    records.push((3, [c[0], c[1], p[0], p[1]]));
                }
            }
        }
        records.push((4, [0.0; 4]));
    }
    if records.len() > 272 {
        return Err("raw quadratic command limit exceeded".into());
    }
    let mut bytes = Vec::with_capacity(20 + records.len() * 40);
    bytes.extend_from_slice(b"OPQ1");
    for word in [
        1_u32,
        outline.contours.len() as u32,
        records.len() as u32,
        0,
    ] {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    for (opcode, values) in records {
        bytes.extend_from_slice(&opcode.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }
    Ok(bytes)
}

/// Build a bounded OPM2 scene from one embedded quadratic TrueType glyph.
/// The returned shift is used only to make an isolated 256×192 PDF oracle.
pub fn stage(
    font_data: &[u8],
    gid: u16,
    matrix: [f64; 6],
) -> Result<(Vec<u8>, Point, Vec<u8>), Box<dyn Error>> {
    let face = ttf_parser::Face::parse(font_data, 0)?;
    let mut outline = Outline::default();
    face.outline_glyph(ttf_parser::GlyphId(gid), &mut outline)
        .ok_or("glyph has no outline")?;
    if outline.overflow
        || !(1..=8).contains(&outline.contours.len())
        || outline.quads == 0
        || outline.cubics != 0
        || outline
            .contours
            .iter()
            .any(|c| !c.closed || c.segments.is_empty())
    {
        return Err("glyph is not a bounded closed multi-contour quadratic".into());
    }
    let mut all = Vec::new();
    for contour in &outline.contours {
        all.push(transform(contour.start, matrix));
        for segment in &contour.segments {
            match segment {
                Segment::Line(p) => all.push(transform(*p, matrix)),
                Segment::Quad(c, p) => {
                    all.push(transform(*c, matrix));
                    all.push(transform(*p, matrix));
                }
            }
        }
    }
    let (mut min_x, mut max_x, mut min_y, mut max_y) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for [x, y] in all {
        if !x.is_finite() || !y.is_finite() {
            return Err("quadratic glyph has nonfinite point".into());
        }
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    if max_x - min_x > 240.0 || max_y - min_y > 176.0 {
        return Err("quadratic glyph exceeds 256x192 tile".into());
    }
    let shift = [128.0 - (min_x + max_x) / 2.0, 96.0 - (min_y + max_y) / 2.0];
    let raw = raw_quadratic_path(&outline, matrix, shift)?;
    let mut encoded = Vec::new();
    encoded.extend_from_slice(b"OPM2");
    for value in [1_u32, 256, 192, u32::try_from(outline.contours.len())?] {
        encoded.extend_from_slice(&value.to_le_bytes());
    }
    let mut total_points = 0;
    for contour in outline.contours {
        let map = |p| add(transform(p, matrix), shift);
        let start = map(contour.start);
        let mut points = vec![start];
        let mut current = start;
        for segment in contour.segments {
            match segment {
                Segment::Line(p) => {
                    current = map(p);
                    points.push(current);
                }
                Segment::Quad(c, p) => {
                    let end = map(p);
                    flatten_quad(current, map(c), end, &mut points)?;
                    current = end;
                }
            }
            if points.len() > 256 {
                return Err("quadratic glyph point limit exceeded".into());
            }
        }
        if points.last() != Some(&start) {
            points.push(start);
        }
        total_points += points.len();
        if points.len() < 4 || total_points > 256 {
            return Err("quadratic contour size outside limit".into());
        }
        encoded.extend_from_slice(&u32::try_from(points.len())?.to_le_bytes());
        for point in points {
            for value in point {
                if !value.is_finite() || !(0.0..=4096.0).contains(&value) {
                    return Err("quadratic glyph point outside limit".into());
                }
                encoded.extend_from_slice(&value.to_le_bytes());
            }
        }
    }
    Ok((encoded, shift, raw))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_quadratic_flattening() {
        let mut points = vec![[0.0, 0.0]];
        flatten_quad([0.0, 0.0], [10.0, 20.0], [20.0, 0.0], &mut points).unwrap();
        assert!(points.len() >= 3 && points.len() <= 65);
        assert_eq!(points.last(), Some(&[20.0, 0.0]));
        assert!(points.iter().flatten().all(|v| v.is_finite()));
    }

    #[test]
    fn outline_builder_stops_storing_unbounded_segments() {
        use ttf_parser::OutlineBuilder;
        let mut outline = Outline::default();
        outline.move_to(0.0, 0.0);
        for _ in 0..300 {
            outline.line_to(1.0, 1.0);
        }
        assert!(outline.overflow);
        assert_eq!(outline.contours[0].segments.len(), 256);
    }

    #[test]
    fn raw_quadratic_records_keep_curve_controls() {
        use ttf_parser::OutlineBuilder;
        let mut outline = Outline::default();
        outline.move_to(1.0, 2.0);
        outline.quad_to(3.0, 4.0, 5.0, 6.0);
        outline.line_to(1.0, 2.0);
        outline.close();
        let bytes =
            raw_quadratic_path(&outline, [1.0, 0.0, 0.0, 1.0, 0.0, 0.0], [10.0, 20.0]).unwrap();
        assert_eq!(&bytes[..4], b"OPQ1");
        assert_eq!(u32::from_le_bytes(bytes[12..16].try_into().unwrap()), 4);
        assert_eq!(bytes.len(), 20 + 4 * 40);
        assert_eq!(u32::from_le_bytes(bytes[60..64].try_into().unwrap()), 3);
        assert_eq!(f64::from_le_bytes(bytes[68..76].try_into().unwrap()), 13.0);
        assert_eq!(f64::from_le_bytes(bytes[84..92].try_into().unwrap()), 15.0);
    }
}
