//! Read-only Type1 outline inventory. This does not enable Type1 in the app.
//! The input is a trusted research PDF; native parsing remains in the worker.
use lopdf::{Document, Object};
use skrifa::{outline::OutlinePen, raw::ps::type1::Type1Font, GlyphId};
use std::{collections::BTreeSet, error::Error, io::Read, sync::Arc};
#[path = "type1_research/type1_cubic_scalar.rs"]
mod type1_cubic_scalar;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

#[derive(Default)]
struct Counts {
    lines: usize,
    quads: usize,
    cubics: usize,
    contours: usize,
    closes: usize,
    invalid: bool,
}

impl Counts {
    fn point(&mut self, x: f32, y: f32) {
        if !x.is_finite() || !y.is_finite() || x.abs().max(y.abs()) > 100_000. {
            self.invalid = true;
        }
    }
    fn bounded(&self) -> bool {
        !self.invalid && self.contours <= 8 && self.lines + self.quads + self.cubics <= 256
    }
}

impl OutlinePen for Counts {
    fn move_to(&mut self, x: f32, y: f32) {
        self.contours += 1;
        self.point(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.lines += 1;
        self.point(x, y);
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        self.quads += 1;
        self.point(cx, cy);
        self.point(x, y);
    }
    fn curve_to(&mut self, cx0: f32, cy0: f32, cx1: f32, cy1: f32, x: f32, y: f32) {
        self.cubics += 1;
        self.point(cx0, cy0);
        self.point(cx1, cy1);
        self.point(x, y);
    }
    fn close(&mut self) {
        self.closes += 1;
    }
}

type Point = [f64; 2];
enum Edge {
    Line(Point),
    Cubic(Point, Point, Point),
}
struct Contour {
    start: Point,
    edges: Vec<Edge>,
    closed: bool,
}
struct PathPen {
    contours: Vec<Contour>,
    invalid: bool,
    scale: f64,
    origin: Point,
    space: opds_dynlex_backend::page_space::PageSpace,
}
impl PathPen {
    fn point(&mut self, x: f32, y: f32) -> Point {
        let pdf = [
            self.origin[0] + f64::from(x) * self.scale,
            self.origin[1] + f64::from(y) * self.scale,
        ];
        let p = match self.space.screen_point(pdf) {
            Ok(p) => p.map(|v| v as f32 as f64),
            Err(_) => {
                self.invalid = true;
                return [0., 0.];
            }
        };
        if p.iter()
            .any(|v| !v.is_finite() || !(0.0..=256.0).contains(v))
        {
            self.invalid = true;
        }
        p
    }
    fn emit(&mut self, edge: Edge) {
        if let Some(c) = self.contours.last_mut().filter(|c| !c.closed) {
            c.edges.push(edge);
            if c.edges.len() > 256 {
                self.invalid = true;
            }
        } else {
            self.invalid = true;
        }
    }
    fn scene(&self, tolerance: f64, scalar: bool) -> Result<Vec<u8>> {
        if self.invalid || self.contours.is_empty() || self.contours.len() > 8 {
            return Err("Type1 path limit".into());
        }
        let mut scene = b"OPNW".to_vec();
        for n in [1_u32, 256, 192, self.contours.len() as u32] {
            scene.extend(n.to_le_bytes());
        }
        let mut total = 0;
        for c in &self.contours {
            if !c.closed || c.edges.is_empty() {
                return Err("open Type1 contour".into());
            }
            let mut points = vec![c.start];
            let mut prev = c.start;
            for e in &c.edges {
                match *e {
                    Edge::Line(p) => {
                        points.push(p);
                        prev = p;
                    }
                    Edge::Cubic(a, b, p) => {
                        if scalar {
                            type1_cubic_scalar::flatten([prev, a, b, p], &mut points)?;
                        } else {
                            flatten_cubic(prev, a, b, p, tolerance, 0, &mut points)?;
                        }
                        prev = p;
                    }
                }
                if points.len() > 256 {
                    return Err("flattened Type1 point limit".into());
                }
            }
            if points.last() != Some(&c.start) {
                points.push(c.start);
            }
            total += points.len();
            if points.len() < 4 || total > 256 {
                return Err("Type1 contour point limit".into());
            }
            scene.extend((points.len() as u32).to_le_bytes());
            for p in points {
                if !(0.0..=256.0).contains(&p[0]) || !(0.0..=192.0).contains(&p[1]) {
                    return Err("Type1 point outside tile".into());
                }
                scene.extend(p[0].to_le_bytes());
                scene.extend(p[1].to_le_bytes());
            }
        }
        Ok(scene)
    }
}
impl OutlinePen for PathPen {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        if self.contours.last().is_some_and(|c| !c.closed) || self.contours.len() >= 8 {
            self.invalid = true;
        }
        self.contours.push(Contour {
            start: p,
            edges: Vec::new(),
            closed: false,
        });
    }
    fn line_to(&mut self, x: f32, y: f32) {
        let p = self.point(x, y);
        self.emit(Edge::Line(p));
    }
    fn quad_to(&mut self, _: f32, _: f32, _: f32, _: f32) {
        self.invalid = true;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let a = self.point(x1, y1);
        let b = self.point(x2, y2);
        let p = self.point(x, y);
        self.emit(Edge::Cubic(a, b, p));
    }
    fn close(&mut self) {
        if let Some(c) = self.contours.last_mut().filter(|c| !c.closed) {
            c.closed = true;
        } else {
            self.invalid = true;
        }
    }
}
fn midpoint(a: Point, b: Point) -> Point {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}
fn flatten_cubic(
    p0: Point,
    p1: Point,
    p2: Point,
    p3: Point,
    tolerance: f64,
    depth: u8,
    out: &mut Vec<Point>,
) -> Result<()> {
    let chord = [p3[0] - p0[0], p3[1] - p0[1]];
    let length = chord[0].hypot(chord[1]);
    let cross = |p: Point| ((p[0] - p0[0]) * chord[1] - (p[1] - p0[1]) * chord[0]).abs();
    if (length > 0. && cross(p1).max(cross(p2)) <= tolerance * length)
        || (length == 0.
            && [p1, p2]
                .iter()
                .all(|p| (p[0] - p0[0]).hypot(p[1] - p0[1]) <= tolerance))
    {
        out.push(p3);
        return Ok(());
    }
    if depth >= 12 || out.len() >= 256 {
        return Err("Type1 cubic subdivision limit".into());
    }
    let a = midpoint(p0, p1);
    let b = midpoint(p1, p2);
    let c = midpoint(p2, p3);
    let d = midpoint(a, b);
    let e = midpoint(b, c);
    let m = midpoint(d, e);
    flatten_cubic(p0, a, d, m, tolerance, depth + 1, out)?;
    flatten_cubic(m, e, c, p3, tolerance, depth + 1, out)?;
    Ok(())
}

fn decode_file(stream: &lopdf::Stream) -> Result<Vec<u8>> {
    const LIMIT: usize = 8_000_000;
    if stream.content.len() > LIMIT || stream.dict.has(b"DecodeParms") {
        return Err("Type1 FontFile source/DecodeParms limit".into());
    }
    let result = match stream.dict.get(b"Filter") {
        Err(_) => stream.content.clone(),
        Ok(Object::Name(name)) if name == b"FlateDecode" => {
            miniz_oxide::inflate::decompress_to_vec_zlib_with_limit(&stream.content, LIMIT)
                .map_err(|_| "Type1 FontFile Flate limit")?
        }
        _ => return Err("Type1 FontFile filter unsupported".into()),
    };
    if result.len() > LIMIT {
        return Err("Type1 FontFile decoded limit".into());
    }
    Ok(result)
}

fn run() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() < 3 || args.len() > 11 {
        return Err(
            "usage: type1_research PDF PAGE [--code=N --tolerance=PX OUTPUT.rgba [--scalar [--font-size=N] [--origin=X,Y] [--rgb=R,G,B] [--dump-curves=PATH]]]"
                .into(),
        );
    }
    if args.len() > 3 && args.len() < 6 {
        return Err("incomplete Type1 research render request".into());
    }
    let scalar = args.len() >= 7;
    if scalar && args[6] != "--scalar" {
        return Err("unsupported research flag".into());
    }
    let mut font_size = 80;
    let mut origin = [60., 50.];
    let mut rgb_lexemes = "0 0 0".to_owned();
    let mut color = [0_u8; 3];
    let mut dump_curves = None;
    for flag in args.iter().skip(7) {
        if let Some(value) = flag.strip_prefix("--font-size=") {
            font_size = value.parse::<u16>()?;
            if !(16..=120).contains(&font_size) {
                return Err("research font size limit".into());
            }
        } else if let Some(value) = flag.strip_prefix("--origin=") {
            let values: Vec<_> = value.split(',').collect();
            if values.len() != 2 {
                return Err("research origin arity".into());
            }
            origin = [values[0].parse()?, values[1].parse()?];
            if origin
                .iter()
                .any(|v: &f64| !v.is_finite() || v.abs() > 1024.)
            {
                return Err("research origin limit".into());
            }
        } else if let Some(value) = flag.strip_prefix("--rgb=") {
            let values: Vec<_> = value.split(',').collect();
            if values.len() != 3 {
                return Err("research RGB arity".into());
            }
            let parsed: Vec<f32> = values
                .iter()
                .map(|value| value.parse::<f32>())
                .collect::<std::result::Result<_, _>>()?;
            if parsed
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            {
                return Err("research RGB range".into());
            }
            color = std::array::from_fn(|i| (parsed[i] * 255.0 + 0.5) as u8);
            rgb_lexemes = values.join(" ");
        } else if let Some(value) = flag.strip_prefix("--dump-curves=") {
            if value.is_empty() || dump_curves.is_some() {
                return Err("invalid research curve dump path".into());
            }
            dump_curves = Some(value);
        } else {
            return Err("unsupported research flag".into());
        }
    }
    let selected_code = if args.len() >= 6 {
        Some(
            args[3]
                .strip_prefix("--code=")
                .ok_or("missing --code")?
                .parse::<u8>()?,
        )
    } else {
        None
    };
    let tolerance = if args.len() >= 6 {
        let t = args[4]
            .strip_prefix("--tolerance=")
            .ok_or("missing --tolerance")?
            .parse::<f64>()?;
        if !t.is_finite() || !(0.001..=1.0).contains(&t) {
            return Err("Type1 tolerance bound".into());
        }
        t
    } else {
        0.01
    };
    let mut source = Vec::new();
    std::fs::File::open(&args[1])?
        .take(16_000_001)
        .read_to_end(&mut source)?;
    if source.len() > 16_000_000 {
        return Err("PDF source limit".into());
    }
    opds_dynlex_backend::text_preflight::inspect(&source)?;
    let space = opds_dynlex_backend::page_space::PageSpace::from_pdf(
        Arc::new(source.clone()),
        args[2].parse()?,
        1.0,
    )?;
    if let Some(code) = selected_code {
        if space.pixels() != [256, 192] {
            return Err("Type1 research fixture must produce 256x192".into());
        }
        let expected = format!(
            "{rgb_lexemes} rg BT /F1 {font_size} Tf 1 0 0 1 {} {} Tm <{code:02X}> Tj ET\n",
            origin[0], origin[1]
        );
        let doc = Document::load_mem(&source)?;
        let page_id = *doc
            .get_pages()
            .get(&args[2].parse::<u32>()?)
            .ok_or("missing page")?;
        if doc.get_page_content(page_id)? != expected.as_bytes() {
            return Err("Type1 research content is not isolated fixture".into());
        }
    }
    let doc = Document::load_mem(&source)?;
    if doc.is_encrypted() {
        return Err("encrypted PDF".into());
    }
    let page = args[2].parse::<u32>()?;
    let page_id = *doc.get_pages().get(&page).ok_or("missing page")?;
    let fonts = doc.get_page_fonts(page_id)?;
    for (resource_name, font_dict) in fonts {
        if font_dict.get(b"Subtype")?.as_name()? != b"Type1" {
            continue;
        }
        if font_dict.has(b"Encoding") {
            return Err("PDF Type1 Encoding override requires separate mapping".into());
        }
        let descriptor = doc
            .dereference(font_dict.get(b"FontDescriptor")?)?
            .1
            .as_dict()?;
        let file = doc
            .dereference(descriptor.get(b"FontFile")?)?
            .1
            .as_stream()?;
        let bytes = decode_file(file)?;
        let font = Type1Font::new(&bytes)?;
        if font.num_glyphs() > 4096 {
            return Err("Type1 glyph count limit".into());
        }
        let encoding = font.encoding().ok_or("Type1 built-in encoding missing")?;
        let first = u8::try_from(font_dict.get(b"FirstChar")?.as_i64()?)?;
        let last = u8::try_from(font_dict.get(b"LastChar")?.as_i64()?)?;
        if first > last {
            return Err("Type1 code range".into());
        }
        let mut gids = BTreeSet::new();
        let mut missing = 0;
        for code in first..=last {
            if let Some(gid) = encoding.map(code) {
                gids.insert(gid.to_u32());
            } else {
                missing += 1;
            }
        }
        let mut totals = Counts::default();
        let mut over_limit = 0;
        let mut line_only = Vec::new();
        let mut cubic_counts = Vec::new();
        for gid in &gids {
            let mut counts = Counts::default();
            let width = font.draw(GlyphId::new(*gid), Some(1000.0), &mut counts)?;
            if width.is_some_and(|w| !w.is_finite() || w < 0. || w > 10_000.) {
                return Err("Type1 width outside bound".into());
            }
            if !counts.bounded() {
                over_limit += 1;
            }
            if counts.cubics == 0 && counts.quads == 0 && counts.lines > 0 {
                line_only.push(*gid);
            }
            cubic_counts.push((counts.cubics, *gid));
            totals.lines += counts.lines;
            totals.quads += counts.quads;
            totals.cubics += counts.cubics;
            totals.contours += counts.contours;
            totals.closes += counts.closes;
        }
        println!("{} name={} bytes={} glyphs={} codes={} mapped_gids={} missing={} contours={} closes={} lines={} quads={} cubics={} over_geometry_limit={}",
            String::from_utf8_lossy(&resource_name), font.name().unwrap_or("?"), bytes.len(),
            font.num_glyphs(), usize::from(last-first)+1, gids.len(), missing,
            totals.contours, totals.closes, totals.lines, totals.quads, totals.cubics, over_limit);
        println!("line_only_gids={line_only:?}");
        cubic_counts.sort();
        println!(
            "fewest_cubics={:?}",
            &cubic_counts[..cubic_counts.len().min(8)]
        );
        if let Some(code) = selected_code {
            let gid = encoding.map(code).ok_or("unmapped Type1 code")?;
            let mut path = PathPen {
                contours: Vec::new(),
                invalid: false,
                scale: f64::from(font_size) / f64::from(font.upem()),
                origin,
                space: space.clone(),
            };
            // The PDF text-stage normalizes embedded outlines to 1000 units.
            // Keep the f32 font-size boundary before the independent DynLex path.
            font.draw(gid, Some(1000.0), &mut path)?;
            if path.invalid {
                return Err("invalid Type1 glyph outline".into());
            }
            if let Some(destination) = dump_curves {
                let mut dump = String::new();
                for contour in &path.contours {
                    let mut previous = contour.start;
                    for edge in &contour.edges {
                        match edge {
                            Edge::Line(point) => previous = *point,
                            Edge::Cubic(a, b, point) => {
                                dump.push_str(&format!(
                                    "{:.17} {:.17} {:.17} {:.17} {:.17} {:.17} {:.17} {:.17}\n",
                                    previous[0],
                                    previous[1],
                                    a[0],
                                    a[1],
                                    b[0],
                                    b[1],
                                    point[0],
                                    point[1]
                                ));
                                previous = *point;
                            }
                        }
                    }
                }
                std::fs::write(destination, dump)?;
            }
            // A successfully mapped and decoded glyph can have no contours
            // (for example, a space). It still advances text, but paints no ink.
            let (rgba, scene_len) = if path.contours.is_empty() {
                (vec![255; 256 * 192 * 4], 0)
            } else {
                let scene = path.scene(tolerance, scalar)?;
                let mut layer = opds_dynlex_backend::experimental_winding::paint(
                    &scene,
                    color,
                    vec![0; 256 * 192 * 4],
                    256,
                    192,
                    [0, 0],
                )?;
                for pixel in layer.chunks_exact_mut(4) {
                    let paper = 255 - pixel[3];
                    for channel in &mut pixel[..3] {
                        *channel = channel.saturating_add(paper);
                    }
                    pixel[3] = 255;
                }
                (layer, scene.len())
            };
            std::fs::write(&args[5], rgba)?;
            println!(
                "UNVERIFIED research code={} gid={} contours={} scene_bytes={} tolerance={} scalar={} font_size={} output={}",
                code,
                gid.to_u32(),
                path.contours.len(),
                scene_len,
                tolerance,
                scalar,
                font_size,
                args[5]
            );
        }
    }
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("Type1 research rejected: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_type1_and_unbounded_outline_reject() {
        assert!(Type1Font::new(&[0; 100]).is_err());
        let mut pen = Counts::default();
        pen.move_to(0., 0.);
        pen.curve_to(1., 1., 2., 2., f32::NAN, 3.);
        assert!(!pen.bounded());
        let mut pen = Counts::default();
        for _ in 0..9 {
            pen.move_to(0., 0.);
        }
        assert!(!pen.bounded());
    }

    #[test]
    fn cubic_subdivision_is_bounded_and_closed_scene_required() {
        let mut points = vec![[0., 0.]];
        flatten_cubic(
            [0., 0.],
            [10., 0.],
            [20., 0.],
            [30., 0.],
            0.01,
            0,
            &mut points,
        )
        .unwrap();
        assert_eq!(points, vec![[0., 0.], [30., 0.]]);
        let mut path = PathPen {
            contours: Vec::new(),
            invalid: false,
            scale: 1.,
            origin: [0., 0.],
            space: opds_dynlex_backend::page_space::PageSpace::new(
                [0., 0., 256., 192.],
                [0., 0., 256., 192.],
                0,
                1.,
            )
            .unwrap(),
        };
        path.move_to(10., 10.);
        path.line_to(20., 10.);
        path.line_to(20., 20.);
        assert!(path.scene(0.01, false).is_err());
        path.close();
        let scene = path.scene(0.01, false).unwrap();
        assert_eq!(&scene[..4], b"OPNW");
        assert_eq!(
            opds_dynlex_backend::experimental_winding::coverage(&scene)
                .unwrap()
                .len(),
            256 * 192
        );
    }
}
