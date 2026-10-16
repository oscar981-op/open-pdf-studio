//! Bounded Type1 cubic staging for the resource-limited text worker only.
//! The scalar subdivision is source-informed by vello_common 0.0.8
//! (MIT/Apache-2.0); no Vello or Hayro renderer is called here.
#[path = "../type1_research/type1_cubic_scalar.rs"]
mod cubic;

use skrifa::{outline::OutlinePen, raw::ps::type1::Type1Font, GlyphId};
use std::error::Error;

type Result<T> = std::result::Result<T, Box<dyn Error>>;
type Point = [f64; 2];

#[derive(Clone, Copy)]
enum Segment {
    Line(Point),
    Cubic(Point, Point, Point),
}

struct Contour {
    start: Point,
    segments: Vec<Segment>,
    closed: bool,
}

#[derive(Default)]
struct Outline {
    contours: Vec<Contour>,
    segments: usize,
    invalid: bool,
}

impl Outline {
    fn point(&mut self, x: f32, y: f32) -> Point {
        if !x.is_finite() || !y.is_finite() || x.abs().max(y.abs()) > 100_000. {
            self.invalid = true;
        }
        [f64::from(x), f64::from(y)]
    }
    fn add(&mut self, segment: Segment) {
        self.segments += 1;
        if self.segments > 256 {
            self.invalid = true;
            return;
        }
        match self.contours.last_mut() {
            Some(contour) if !contour.closed => contour.segments.push(segment),
            _ => self.invalid = true,
        }
    }
}

impl OutlinePen for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        let point = self.point(x, y);
        if self.contours.len() >= 8 || self.contours.last().is_some_and(|c| !c.closed) {
            self.invalid = true;
            return;
        }
        self.contours.push(Contour {
            start: point,
            segments: Vec::new(),
            closed: false,
        });
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let point = self.point(x, y);
        self.add(Segment::Line(point));
    }
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
        self.invalid = true;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let first = self.point(x1, y1);
        let second = self.point(x2, y2);
        let end = self.point(x, y);
        self.add(Segment::Cubic(first, second, end));
    }
    fn close(&mut self) {
        match self.contours.last_mut() {
            Some(contour) if !contour.closed => contour.closed = true,
            _ => self.invalid = true,
        }
    }
}

pub struct PreparedType1Glyph {
    outline: Outline,
}

impl PreparedType1Glyph {
    pub fn new(font_data: &[u8], gid: u16) -> Result<Self> {
        if font_data.len() > 8_000_000 {
            return Err("Type1 font byte limit".into());
        }
        let font = Type1Font::new(font_data)?;
        if u32::from(gid) >= font.num_glyphs() || font.num_glyphs() > 4096 {
            return Err("Type1 glyph index limit".into());
        }
        let mut outline = Outline::default();
        let width = font.draw(GlyphId::new(u32::from(gid)), Some(1000.0), &mut outline)?;
        if width.is_some_and(|n| !n.is_finite() || !(0.0..=10_000.0).contains(&n))
            || outline.invalid
            || outline
                .contours
                .iter()
                .any(|c| !c.closed || c.segments.is_empty())
        {
            return Err("unbounded Type1 outline".into());
        }
        Ok(Self { outline })
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

    pub fn stage(&self, matrix: [f64; 6]) -> Result<Option<(Vec<u8>, Point)>> {
        if matrix
            .iter()
            .any(|n| !n.is_finite() || n.abs() > 1_000_000.)
        {
            return Err("Type1 screen matrix limit".into());
        }
        if self.outline.contours.is_empty() {
            return Ok(None);
        }
        let map = |p: Point| -> Result<Point> {
            let x = matrix[0] * p[0] + matrix[2] * p[1] + matrix[4];
            let y = matrix[1] * p[0] + matrix[3] * p[1] + matrix[5];
            if !x.is_finite() || !y.is_finite() || x.abs().max(y.abs()) > 1_000_000. {
                return Err("Type1 screen point limit".into());
            }
            Ok([(x as f32) as f64, (y as f32) as f64])
        };
        let mut staged = Vec::with_capacity(self.outline.contours.len());
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        let mut total = 0_usize;
        for contour in &self.outline.contours {
            let start = map(contour.start)?;
            let mut points = vec![start];
            let mut current = start;
            for segment in &contour.segments {
                match *segment {
                    Segment::Line(end) => {
                        current = map(end)?;
                        points.push(current);
                    }
                    Segment::Cubic(a, b, end) => {
                        let end = map(end)?;
                        cubic::flatten([current, map(a)?, map(b)?, end], &mut points)?;
                        current = end;
                    }
                }
                if points.len() > 256 {
                    return Err("Type1 flattened point limit".into());
                }
            }
            if points.last() != Some(&start) {
                points.push(start);
            }
            total += points.len();
            if points.len() < 4 || total > 256 {
                return Err("Type1 contour point limit".into());
            }
            for p in &points {
                bounds[0] = bounds[0].min(p[0]);
                bounds[1] = bounds[1].min(p[1]);
                bounds[2] = bounds[2].max(p[0]);
                bounds[3] = bounds[3].max(p[1]);
            }
            staged.push(points);
        }
        if bounds[2] - bounds[0] > 240. || bounds[3] - bounds[1] > 176. {
            return Err("Type1 exceeds 256x192 tile".into());
        }
        let shift = [
            128. - (bounds[0] + bounds[2]) / 2.,
            96. - (bounds[1] + bounds[3]) / 2.,
        ];
        let mut scene = b"OPM2".to_vec();
        for n in [1_u32, 256, 192, staged.len() as u32] {
            scene.extend_from_slice(&n.to_le_bytes());
        }
        for points in staged {
            scene.extend_from_slice(&(points.len() as u32).to_le_bytes());
            for point in points {
                for axis in 0..2 {
                    let n = point[axis] + shift[axis];
                    if !n.is_finite() || !(0.0..=[256., 192.][axis]).contains(&n) {
                        return Err("Type1 point outside tile".into());
                    }
                    scene.extend_from_slice(&n.to_le_bytes());
                }
            }
        }
        Ok(Some((scene, shift)))
    }
}
