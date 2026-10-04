//! Independent bounded cubic stroke-outline experiment. No Kurbo/Vello call.
//! Bounded adaptive least-squares offset fits preserve endpoint tangents.
//! This remains a diagnostic approximation, not a PDF rendering implementation.
use std::{env, error::Error, fmt::Write, fs};

type Point = [f64; 2];

fn point(c: &[Point; 4], t: f64) -> Point {
    let u = 1.0 - t;
    [0, 1].map(|i| {
        u * u * u * c[0][i]
            + 3.0 * u * u * t * c[1][i]
            + 3.0 * u * t * t * c[2][i]
            + t * t * t * c[3][i]
    })
}

fn tangent(c: &[Point; 4], t: f64) -> Point {
    let u = 1.0 - t;
    [0, 1].map(|i| {
        3.0 * u * u * (c[1][i] - c[0][i])
            + 6.0 * u * t * (c[2][i] - c[1][i])
            + 3.0 * t * t * (c[3][i] - c[2][i])
    })
}

fn offset_at(c: &[Point; 4], t: f64, distance: f64) -> Result<Point, Box<dyn Error>> {
    let tan = tangent(c, t);
    let length = tan[0].hypot(tan[1]);
    if length < 1e-9 {
        return Err("degenerate tangent".into());
    }
    let p = point(c, t);
    Ok([
        p[0] - distance * tan[1] / length,
        p[1] + distance * tan[0] / length,
    ])
}

fn fit_interval(
    c: &[Point; 4],
    distance: f64,
    t0: f64,
    t1: f64,
) -> Result<([Point; 4], f64), Box<dyn Error>> {
    let p0 = offset_at(c, t0, distance)?;
    let p3 = offset_at(c, t1, distance)?;
    let start = tangent(c, t0);
    let end = tangent(c, t1);
    let mut aa = 0.0;
    let mut ab = 0.0;
    let mut bb = 0.0;
    let mut ar = 0.0;
    let mut br = 0.0;
    for k in 1..32 {
        let t = f64::from(k) / 32.0;
        let source_t = t0 + (t1 - t0) * t;
        let u = 1.0 - t;
        let b1 = 3.0 * u * u * t;
        let b2 = 3.0 * u * t * t;
        let target = offset_at(c, source_t, distance)?;
        for axis in 0..2 {
            let base = (u * u * u + b1) * p0[axis] + (b2 + t * t * t) * p3[axis];
            let a = b1 * start[axis];
            let b = -b2 * end[axis];
            let residual = target[axis] - base;
            aa += a * a;
            ab += a * b;
            bb += b * b;
            ar += a * residual;
            br += b * residual;
        }
    }
    let det = aa * bb - ab * ab;
    if det <= 1e-12 * (aa * bb).max(1.0) {
        return Err("ill-conditioned offset fit".into());
    }
    let s0 = (ar * bb - br * ab) / det;
    let s1 = (br * aa - ar * ab) / det;
    let p1 = [p0[0] + s0 * start[0], p0[1] + s0 * start[1]];
    let p2 = [p3[0] - s1 * end[0], p3[1] - s1 * end[1]];
    let fitted = [p0, p1, p2, p3];
    let mut max_error: f64 = 0.0;
    for k in 1..64 {
        let t = f64::from(k) / 64.0;
        let source_t = t0 + (t1 - t0) * t;
        let target = offset_at(c, source_t, distance)?;
        let actual = point(&fitted, t);
        max_error = max_error.max((target[0] - actual[0]).hypot(target[1] - actual[1]));
    }
    Ok((fitted, max_error))
}

fn offset_segments(c: &[Point; 4], distance: f64) -> Result<Vec<[Point; 4]>, Box<dyn Error>> {
    const MAX_DEPTH: usize = 4;
    const TOLERANCE: f64 = 0.02;
    let mut result = Vec::new();
    let mut pending = vec![(0.0, 1.0, 0)];
    while let Some((t0, t1, depth)) = pending.pop() {
        let (fitted, error) = fit_interval(c, distance, t0, t1)?;
        if error <= TOLERANCE {
            result.push(fitted);
        } else if depth < MAX_DEPTH {
            let mid = 0.5 * (t0 + t1);
            pending.push((mid, t1, depth + 1));
            pending.push((t0, mid, depth + 1));
        } else {
            return Err("offset exceeds bounded approximation error".into());
        }
    }
    Ok(result)
}

fn parse_cubic(source: &str) -> Result<[Point; 4], Box<dyn Error>> {
    let mut lines = source
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with('#'));
    let first = lines.next().ok_or("missing M")?;
    let second = lines.next().ok_or("missing C")?;
    if lines.next().is_some() {
        return Err("extra path command".into());
    }
    let parse = |line: &str, command: &str, count: usize| -> Result<Vec<f64>, Box<dyn Error>> {
        let mut fields = line.split_whitespace();
        if fields.next() != Some(command) {
            return Err("wrong path command".into());
        }
        let values = fields
            .map(str::parse::<f64>)
            .collect::<Result<Vec<_>, _>>()?;
        if values.len() != count || values.iter().any(|x| !x.is_finite() || x.abs() > 4096.0) {
            return Err("invalid coordinates".into());
        }
        Ok(values)
    };
    let m = parse(first, "M", 2)?;
    let c = parse(second, "C", 6)?;
    let points = [[m[0], m[1]], [c[0], c[1]], [c[2], c[3]], [c[4], c[5]]];
    if points.windows(2).all(|pair| pair[0] == pair[1]) {
        return Err("zero cubic".into());
    }
    Ok(points)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: curve_outline_candidate INPUT OUTPUT")?;
    let output = args.next().ok_or("missing output")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let c = parse_cubic(&fs::read_to_string(input)?)?;
    let forward = offset_segments(&c, -0.5)?;
    let backward = offset_segments(&c, 0.5)?;
    let mut text =
        String::from("# Independent 1px butt-cap cubic offset candidate; diagnostic only\n");
    writeln!(text, "M {:.12} {:.12}", forward[0][0][0], forward[0][0][1])?;
    for cubic in &forward {
        writeln!(
            text,
            "C {:.12} {:.12} {:.12} {:.12} {:.12} {:.12}",
            cubic[1][0], cubic[1][1], cubic[2][0], cubic[2][1], cubic[3][0], cubic[3][1]
        )?;
    }
    let last = backward.last().ok_or("empty backward offset")?;
    writeln!(text, "L {:.12} {:.12}", last[3][0], last[3][1])?;
    for cubic in backward.iter().rev() {
        writeln!(
            text,
            "C {:.12} {:.12} {:.12} {:.12} {:.12} {:.12}",
            cubic[2][0], cubic[2][1], cubic[1][0], cubic[1][1], cubic[0][0], cubic[0][1]
        )?;
    }
    text.push_str("Z\n");
    fs::write(output, text)?;
    Ok(())
}
