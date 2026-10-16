//! Experimental Type0 TrueType quadratic-outline staging, outside the normal adapter.
//!
//! The parabola integral/subdivision mathematics follows the Apache-2.0/MIT
//! licensed vello_common 0.0.8 `flatten_simd.rs`. This code calls neither
//! Vello nor Kurbo at runtime. DynLex rasterization remains a separate step.
use skrifa::instance::{LocationRef, Size};
use skrifa::outline::{DrawSettings, Engine, HintingInstance, HintingOptions, OutlinePen, Target};
use skrifa::{FontRef, GlyphId, MetadataProvider};
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

impl OutlinePen for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        ttf_parser::OutlineBuilder::move_to(self, x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        ttf_parser::OutlineBuilder::line_to(self, x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        ttf_parser::OutlineBuilder::quad_to(self, x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        ttf_parser::OutlineBuilder::curve_to(self, x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        ttf_parser::OutlineBuilder::close(self);
    }
}

#[derive(Clone, Copy)]
pub enum Source {
    RawTtf,
    SkrifaAtThousand,
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

fn opm_point_offsets(scene: &[u8]) -> Result<Vec<usize>, Box<dyn Error>> {
    if scene.len() < 20 || &scene[..4] != b"OPM2" {
        return Err("invalid staged OPM2 header".into());
    }
    let word = |at: usize| u32::from_le_bytes(scene[at..at + 4].try_into().unwrap());
    let contours = word(16) as usize;
    if word(4) != 1 || word(8) != 256 || word(12) != 192 || !(1..=8).contains(&contours) {
        return Err("invalid staged OPM2 geometry".into());
    }
    let mut at = 20_usize;
    let mut offsets = Vec::new();
    for _ in 0..contours {
        if at + 4 > scene.len() {
            return Err("truncated staged OPM2 contour".into());
        }
        let count = word(at) as usize;
        at += 4;
        if !(4..=256).contains(&count)
            || offsets.len() + count > 256
            || count.checked_mul(16).is_none_or(|n| at + n > scene.len())
        {
            return Err("invalid staged OPM2 point count".into());
        }
        for i in 0..count {
            offsets.push(at + i * 16);
        }
        at += count * 16;
    }
    if at != scene.len() {
        return Err("trailing staged OPM2 bytes".into());
    }
    Ok(offsets)
}

/// Bounds in the original PDF user space, before the per-glyph tile shift.
pub fn absolute_bounds(scene: &[u8], shift: Point) -> Result<[f64; 4], Box<dyn Error>> {
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for at in opm_point_offsets(scene)? {
        let x = f64::from_le_bytes(scene[at..at + 8].try_into()?) - shift[0];
        let y = f64::from_le_bytes(scene[at + 8..at + 16].try_into()?) - shift[1];
        if !x.is_finite() || !y.is_finite() {
            return Err("nonfinite staged glyph bound".into());
        }
        bounds[0] = bounds[0].min(x);
        bounds[1] = bounds[1].min(y);
        bounds[2] = bounds[2].max(x);
        bounds[3] = bounds[3].max(y);
    }
    Ok(bounds)
}

/// Rebase a validated staged glyph into the common 256×192 text-run tile.
pub fn rebase(
    scene: &mut [u8],
    old_shift: Point,
    common_shift: Point,
) -> Result<(), Box<dyn Error>> {
    let offsets = opm_point_offsets(scene)?;
    for at in offsets {
        for axis in 0..2 {
            let start = at + axis * 8;
            let value = f64::from_le_bytes(scene[start..start + 8].try_into()?)
                + common_shift[axis]
                - old_shift[axis];
            if !value.is_finite() || value < 0.0 || value > [256.0, 192.0][axis] {
                return Err("glyph run exceeds output tile".into());
            }
            scene[start..start + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(())
}

/// Rebase a PDF screen-space glyph into a movable tile after the f32 screen
/// quantization used by the reference renderer. Quantizing only after the
/// local shift changes edge coverage on large pages by one 8-bit step.
pub fn rebase_screen_f32(
    scene: &mut [u8],
    old_shift: Point,
    tile_origin: [u32; 2],
) -> Result<(), Box<dyn Error>> {
    let offsets = opm_point_offsets(scene)?;
    for at in offsets {
        for axis in 0..2 {
            let start = at + axis * 8;
            let absolute =
                f64::from_le_bytes(scene[start..start + 8].try_into()?) - old_shift[axis];
            let value = (absolute as f32 - tile_origin[axis] as f32) as f64;
            if !value.is_finite() || value < 0.0 || value > [256.0, 192.0][axis] {
                return Err("glyph run exceeds quantized output tile".into());
            }
            scene[start..start + 8].copy_from_slice(&value.to_le_bytes());
        }
    }
    Ok(())
}

/// Immutable bounded font outline; reusable across glyph placements and zooms.
/// Font interpretation happens once. Flattening remains in the requested screen space.
pub struct PreparedGlyph {
    outline: Outline,
    factor: f64,
}

impl PreparedGlyph {
    pub fn new(font_data: &[u8], gid: u16, source: Source) -> Result<Self, Box<dyn Error>> {
        if font_data.len() > 8_000_000 {
            return Err("font bytes exceed outline preparation budget".into());
        }
        let face = ttf_parser::Face::parse(font_data, 0)?;
        let mut outline = Outline::default();
        let factor = match source {
            Source::RawTtf => {
                face.outline_glyph(ttf_parser::GlyphId(gid), &mut outline)
                    .ok_or("glyph has no outline")?;
                1.0
            }
            Source::SkrifaAtThousand => {
                let font = FontRef::from_index(font_data, 0)?;
                let glyphs = font.outline_glyphs();
                let glyph = glyphs
                    .get(GlyphId::new(u32::from(gid)))
                    .ok_or("glyph has no embedded outline")?;
                // Match Hayro's fixed 1000-unit outline request. The returned
                // coordinates are already quantized by the pure-Rust font parser;
                // scale TGR1's original-UPEM basis back before raster staging.
                let instance = if glyphs.require_interpreter() {
                    HintingInstance::new(
                        &glyphs,
                        Size::new(1000.0),
                        LocationRef::default(),
                        HintingOptions {
                            engine: Engine::Interpreter,
                            target: Target::Mono,
                        },
                    )
                    .ok()
                } else {
                    None
                };
                let settings = if let Some(instance) = instance.as_ref() {
                    DrawSettings::hinted(instance, false)
                } else {
                    DrawSettings::unhinted(Size::new(1000.0), LocationRef::default())
                };
                glyph.draw(settings, &mut outline)?;
                f64::from(face.units_per_em()) / 1000.0
            }
        };

        if outline.overflow
            || !(1..=8).contains(&outline.contours.len())
            || outline.cubics != 0
            || outline.segments > 256
            || outline
                .contours
                .iter()
                .any(|c| !c.closed || c.segments.is_empty())
        {
            return Err("glyph outline exceeds preparation bounds".into());
        }
        Ok(Self { outline, factor })
    }

    pub fn retained_bytes(&self) -> usize {
        std::mem::size_of::<Self>()
            + self.outline.contours.capacity() * std::mem::size_of::<Contour>()
            + self
                .outline
                .contours
                .iter()
                .map(|c| c.segments.capacity() * std::mem::size_of::<Segment>())
                .sum::<usize>()
    }

    fn matrix(&self, matrix: [f64; 6]) -> [f64; 6] {
        [
            matrix[0] * self.factor,
            matrix[1] * self.factor,
            matrix[2] * self.factor,
            matrix[3] * self.factor,
            matrix[4],
            matrix[5],
        ]
    }

    pub fn stage(
        &self,
        matrix: [f64; 6],
    ) -> Result<(Vec<u8>, Point, Option<Vec<u8>>), Box<dyn Error>> {
        if self.outline.quads == 0 {
            let (scene, shift) = stage_prepared_lines(&self.outline, self.matrix(matrix))?;
            Ok((scene, shift, None))
        } else {
            let (scene, shift, raw) = stage_prepared_quadratic(&self.outline, self.matrix(matrix))?;
            Ok((scene, shift, Some(raw)))
        }
    }
}

/// Build a bounded OPM2 scene from one embedded quadratic TrueType glyph.
/// The returned shift is used only to make an isolated 256×192 PDF oracle.
pub fn stage(
    font_data: &[u8],
    gid: u16,
    matrix: [f64; 6],
    source: Source,
) -> Result<(Vec<u8>, Point, Vec<u8>), Box<dyn Error>> {
    let glyph = PreparedGlyph::new(font_data, gid, source)?;
    stage_prepared_quadratic(&glyph.outline, glyph.matrix(matrix))
}

fn stage_prepared_quadratic(
    outline: &Outline,
    matrix: [f64; 6],
) -> Result<(Vec<u8>, Point, Vec<u8>), Box<dyn Error>> {
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
    for contour in &outline.contours {
        let map = |p| add(transform(p, matrix), shift);
        let start = map(contour.start);
        let mut points = vec![start];
        let mut current = start;
        for &segment in &contour.segments {
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

/// Stage a bounded glyph whose closed contours contain only straight lines.
/// This complements `stage` for ordinary TrueType letters without quadratic
/// segments; it still leaves curves, open paths and large glyphs unsupported.
pub fn stage_line_only(
    font_data: &[u8],
    gid: u16,
    matrix: [f64; 6],
) -> Result<(Vec<u8>, Point), Box<dyn Error>> {
    let glyph = PreparedGlyph::new(font_data, gid, Source::SkrifaAtThousand)?;
    stage_prepared_lines(&glyph.outline, glyph.matrix(matrix))
}

fn stage_prepared_lines(
    outline: &Outline,
    matrix: [f64; 6],
) -> Result<(Vec<u8>, Point), Box<dyn Error>> {
    if outline.overflow
        || !(1..=8).contains(&outline.contours.len())
        || outline.quads != 0
        || outline.cubics != 0
        || outline.segments + 2 * outline.contours.len() > 256
        || outline.contours.iter().any(|contour| {
            !contour.closed
                || contour.segments.len() < 2
                || contour
                    .segments
                    .iter()
                    .any(|segment| !matches!(segment, Segment::Line(_)))
        })
    {
        return Err("glyph is not bounded closed line-only geometry".into());
    }
    let mut contours = Vec::with_capacity(outline.contours.len());
    let mut bounds = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for contour in &outline.contours {
        let mut points = vec![transform(contour.start, matrix)];
        for &segment in &contour.segments {
            let Segment::Line(point) = segment else {
                return Err("line-only glyph contains a curve".into());
            };
            points.push(transform(point, matrix));
        }
        if points.last() != Some(&points[0]) {
            points.push(points[0]);
        }
        if points.len() < 4 {
            return Err("line-only contour has too few points".into());
        }
        for &[x, y] in &points {
            if !x.is_finite() || !y.is_finite() {
                return Err("line-only glyph has nonfinite coordinates".into());
            }
            bounds[0] = bounds[0].min(x);
            bounds[1] = bounds[1].min(y);
            bounds[2] = bounds[2].max(x);
            bounds[3] = bounds[3].max(y);
        }
        contours.push(points);
    }
    if bounds[2] - bounds[0] > 240.0 || bounds[3] - bounds[1] > 176.0 {
        return Err("line-only glyph exceeds output tile".into());
    }
    let shift = [
        128.0 - (bounds[0] + bounds[2]) / 2.0,
        96.0 - (bounds[1] + bounds[3]) / 2.0,
    ];
    let mut encoded = Vec::new();
    encoded.extend_from_slice(b"OPM2");
    for value in [1_u32, 256, 192, contours.len() as u32] {
        encoded.extend_from_slice(&value.to_le_bytes());
    }
    for contour in contours {
        encoded.extend_from_slice(&(contour.len() as u32).to_le_bytes());
        for mut point in contour {
            point[0] += shift[0];
            point[1] += shift[1];
            for coordinate in point {
                if !(0.0..=4096.0).contains(&coordinate) {
                    return Err("line-only point outside limit".into());
                }
                encoded.extend_from_slice(&coordinate.to_le_bytes());
            }
        }
    }
    Ok((encoded, shift))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepared_outline_reused_across_scales_without_retaining_render_state() {
        let font = include_bytes!("../../../assets/fonts/LiberationSans-Regular.ttf");
        let face = ttf_parser::Face::parse(font, 0).unwrap();
        for character in ['g', 'I', '\u{2019}'] {
            let gid = face.glyph_index(character).unwrap().0;
            let prepared = PreparedGlyph::new(font, gid, Source::SkrifaAtThousand).unwrap();
            let bytes = prepared.retained_bytes();
            let matrix = |scale: f64| {
                [
                    scale * 12. / f64::from(face.units_per_em()),
                    0.,
                    0.,
                    -scale * 12. / f64::from(face.units_per_em()),
                    40.125,
                    70.375,
                ]
            };
            let first = prepared.stage(matrix(1.)).unwrap();
            for scale in [0.25, 0.87, 1.15, 2., 8.] {
                let result = prepared.stage(matrix(scale)).unwrap();
                assert!(result.0.len() <= 4148);
                assert_eq!(prepared.retained_bytes(), bytes);
            }
            assert_eq!(prepared.stage(matrix(1.)).unwrap(), first);
            assert!(prepared.stage([f64::NAN; 6]).is_err());
        }
        assert!(PreparedGlyph::new(&[0; 64], 1, Source::SkrifaAtThousand).is_err());
    }

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
        OutlineBuilder::move_to(&mut outline, 0.0, 0.0);
        for _ in 0..300 {
            OutlineBuilder::line_to(&mut outline, 1.0, 1.0);
        }
        assert!(outline.overflow);
        assert_eq!(outline.contours[0].segments.len(), 256);
    }

    #[test]
    fn raw_quadratic_records_keep_curve_controls() {
        use ttf_parser::OutlineBuilder;
        let mut outline = Outline::default();
        OutlineBuilder::move_to(&mut outline, 1.0, 2.0);
        OutlineBuilder::quad_to(&mut outline, 3.0, 4.0, 5.0, 6.0);
        OutlineBuilder::line_to(&mut outline, 1.0, 2.0);
        OutlineBuilder::close(&mut outline);
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
