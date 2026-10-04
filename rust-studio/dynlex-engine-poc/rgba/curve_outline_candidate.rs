//! Independent bounded cubic stroke-outline experiment. No Kurbo/Vello call.
//! The offset is interpolated at t=1/3 and 2/3; this is deliberately a
//! diagnostic approximation, not a PDF rendering implementation.
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

fn offset_at(c: &[Point; 4], t: f64, distance: f64) -> Result<Point, Box<dyn Error>> {
    let u = 1.0 - t;
    let tangent: Point = [0, 1].map(|i| {
        3.0 * u * u * (c[1][i] - c[0][i])
            + 6.0 * u * t * (c[2][i] - c[1][i])
            + 3.0 * t * t * (c[3][i] - c[2][i])
    });
    let length = tangent[0].hypot(tangent[1]);
    if length < 1e-9 {
        return Err("degenerate tangent".into());
    }
    let p = point(c, t);
    Ok([
        p[0] - distance * tangent[1] / length,
        p[1] + distance * tangent[0] / length,
    ])
}

fn offset_cubic(c: &[Point; 4], distance: f64) -> Result<[Point; 4], Box<dyn Error>> {
    let p0 = offset_at(c, 0.0, distance)?;
    let p3 = offset_at(c, 1.0, distance)?;
    let q1 = offset_at(c, 1.0 / 3.0, distance)?;
    let q2 = offset_at(c, 2.0 / 3.0, distance)?;
    let a = 4.0 / 9.0;
    let b = 2.0 / 9.0;
    let det = a * a - b * b;
    let mut p1 = [0.0; 2];
    let mut p2 = [0.0; 2];
    for i in 0..2 {
        let r1 = q1[i] - (8.0 / 27.0) * p0[i] - (1.0 / 27.0) * p3[i];
        let r2 = q2[i] - (1.0 / 27.0) * p0[i] - (8.0 / 27.0) * p3[i];
        p1[i] = (a * r1 - b * r2) / det;
        p2[i] = (a * r2 - b * r1) / det;
    }
    Ok([p0, p1, p2, p3])
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
    let forward = offset_cubic(&c, -0.5)?;
    let backward = offset_cubic(&c, 0.5)?;
    let mut text =
        String::from("# Independent 1px butt-cap cubic offset candidate; diagnostic only\n");
    writeln!(text, "M {:.12} {:.12}", forward[0][0], forward[0][1])?;
    writeln!(
        text,
        "C {:.12} {:.12} {:.12} {:.12} {:.12} {:.12}",
        forward[1][0], forward[1][1], forward[2][0], forward[2][1], forward[3][0], forward[3][1]
    )?;
    writeln!(text, "L {:.12} {:.12}", backward[3][0], backward[3][1])?;
    writeln!(
        text,
        "C {:.12} {:.12} {:.12} {:.12} {:.12} {:.12}",
        backward[2][0],
        backward[2][1],
        backward[1][0],
        backward[1][1],
        backward[0][0],
        backward[0][1]
    )?;
    text.push_str("Z\n");
    fs::write(output, text)?;
    Ok(())
}
