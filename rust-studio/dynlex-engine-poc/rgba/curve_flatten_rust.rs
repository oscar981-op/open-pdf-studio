//! Experimental independent bounded cubic subdivision for stroked outlines.
//! The Kurbo-generated outline is a fixture input, not a PDF production path.
use std::{env, error::Error, fs, io::Write};
#[allow(dead_code)]
mod curve_scene;
type Point = [f64; 2];
const MAX_POINTS: usize = 4097;
const MAX_DEPTH: usize = 16;

fn midpoint(a: Point, b: Point) -> Point {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}
fn distance_sq_to_segment(p: Point, a: Point, b: Point) -> f64 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let len_sq = dx * dx + dy * dy;
    let t = if len_sq == 0.0 {
        0.0
    } else {
        ((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len_sq
    };
    let t = t.clamp(0.0, 1.0);
    let ex = p[0] - (a[0] + t * dx);
    let ey = p[1] - (a[1] + t * dy);
    ex * ex + ey * ey
}
fn flatten(
    c: [Point; 4],
    tol_sq: f64,
    depth: usize,
    out: &mut Vec<Point>,
) -> Result<(), Box<dyn Error>> {
    if depth >= MAX_DEPTH || out.len() >= MAX_POINTS {
        return Err("cubic subdivision limit".into());
    }
    if distance_sq_to_segment(c[1], c[0], c[3]).max(distance_sq_to_segment(c[2], c[0], c[3]))
        <= tol_sq
    {
        out.push(c[3]);
        return Ok(());
    }
    let a = midpoint(c[0], c[1]);
    let b = midpoint(c[1], c[2]);
    let d = midpoint(c[2], c[3]);
    let e = midpoint(a, b);
    let f = midpoint(b, d);
    let g = midpoint(e, f);
    flatten([c[0], a, e, g], tol_sq, depth + 1, out)?;
    flatten([g, f, d, c[3]], tol_sq, depth + 1, out)?;
    Ok(())
}
fn parse_outline(source: &str, tolerance: f64) -> Result<Vec<Point>, Box<dyn Error>> {
    if !tolerance.is_finite() || !(0.01..=1.0).contains(&tolerance) {
        return Err("invalid tolerance".into());
    }
    let mut points = Vec::new();
    let mut current = None;
    let mut start = None;
    let mut curves = 0;
    for line in source
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let mut parts = line.split_whitespace();
        let op = parts.next().ok_or("empty operator")?;
        let values = parts
            .map(str::parse::<f64>)
            .collect::<Result<Vec<_>, _>>()?;
        if values.iter().any(|x| !x.is_finite() || x.abs() > 4096.0) {
            return Err("invalid coordinate".into());
        }
        match (op, values.as_slice()) {
            ("M", [x, y]) if points.is_empty() => {
                let p = [*x, *y];
                points.push(p);
                current = Some(p);
                start = Some(p);
            }
            ("L", [x, y]) if current.is_some() => {
                let p = [*x, *y];
                points.push(p);
                current = Some(p);
            }
            ("C", [x1, y1, x2, y2, x3, y3]) if current.is_some() => {
                curves += 1;
                if curves > 16 {
                    return Err("too many cubics".into());
                }
                let end = [*x3, *y3];
                flatten(
                    [current.unwrap(), [*x1, *y1], [*x2, *y2], end],
                    tolerance * tolerance,
                    0,
                    &mut points,
                )?;
                current = Some(end);
            }
            ("Z", []) if current.is_some() => {
                points.push(start.unwrap());
                current = None;
            }
            _ => return Err("unsupported or malformed path".into()),
        }
        if points.len() > MAX_POINTS {
            return Err("too many points".into());
        }
    }
    if current.is_some() || points.len() < 4 {
        return Err("unclosed or empty outline".into());
    }
    // Match the f32 coordinate precision at Vello's line-buffer boundary.
    for p in &mut points {
        p[0] = (p[0] as f32) as f64;
        p[1] = (p[1] as f32) as f64;
    }
    // Preserve exact closure after quantization.
    *points.last_mut().unwrap() = points[0];
    curve_scene::encode(&points)?;
    Ok(points)
}
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let source = args
        .next()
        .ok_or("usage: curve_flatten_rust OUTLINE.txt OUTPUT.txt [TOLERANCE]")?;
    let output = args.next().ok_or("missing output")?;
    let tolerance = args
        .next()
        .map(|v| v.to_string_lossy().parse())
        .transpose()?
        .unwrap_or(0.25);
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let points = parse_outline(&fs::read_to_string(source)?, tolerance)?;
    let mut file = fs::File::create(output)?;
    writeln!(
        file,
        "# Independent adaptive cubic subdivision; tolerance={tolerance}"
    )?;
    for pair in points.windows(2) {
        writeln!(
            file,
            "{:.9} {:.9} {:.9} {:.9}",
            pair[0][0], pair[0][1], pair[1][0], pair[1][1]
        )?;
    }
    eprintln!("{} segments", points.len() - 1);
    Ok(())
}
