//! Bounded scalar curvature-parameterized cubic flattening experiment.
//! Uses recorded stroke-outline geometry as input; no Vello/Kurbo runtime call.
//! The quadratic subdivision mathematics follows vello_common 0.0.8's
//! Apache-2.0/MIT-licensed flatten_simd.rs; this is a separate scalar rewrite
//! for diagnosis, not an independently derived rasterization algorithm.
use std::{env, error::Error, fs, io::Write};
#[allow(dead_code)]
mod curve_scene;
type Point = [f64; 2];
const MAX_POINTS: usize = 4097;
const MAX_QUADS: usize = 16;
#[derive(Clone, Copy)]
struct Params {
    q: [f32; 6],
    a0: f32,
    da: f32,
    u0: f32,
    uscale: f32,
    val: f32,
}
fn fma(a: f32, b: f32, c: f32) -> f32 {
    a.mul_add(b, c)
}
fn integral(x: f32) -> f32 {
    x / (0.33 + fma(0.25, x * x, 0.201_511_2).sqrt().sqrt())
}
fn inverse(x: f32) -> f32 {
    x * (0.61 + fma(0.25, x * x, 0.39 * 0.39).sqrt())
}
fn cubic_at(c: [Point; 4], t: f32) -> [f32; 2] {
    let p = |j: usize| {
        [
            c[0][j] as f32,
            c[1][j] as f32,
            c[2][j] as f32,
            c[3][j] as f32,
        ]
    };
    let h = |v: [f32; 4]| {
        let a = fma(v[1] - v[2], 3.0, v[3] - v[0]);
        let b = fma(v[1], -2.0, v[0] + v[2]) * 3.0;
        let d = (v[1] - v[0]) * 3.0;
        fma(fma(fma(a, t, b), t, d), t, v[0])
    };
    [h(p(0)), h(p(1))]
}
fn estimate_quads(c: [Point; 4]) -> usize {
    let mut err = 0.0;
    for axis in 0..2 {
        let a = 3.0 * c[1][axis] - c[0][axis];
        let b = 3.0 * c[2][axis] - c[3][axis];
        err += (b - a) * (b - a);
    }
    let scaled = err / (432.0 * 0.025 * 0.025);
    (1..=MAX_QUADS)
        .find(|&n| scaled <= (n as f64).powi(6))
        .unwrap_or(MAX_QUADS)
}
fn quad_params(p0: [f32; 2], half: [f32; 2], p2: [f32; 2]) -> Params {
    let p1 = [
        fma(half[0], 2.0, -0.5 * (p0[0] + p2[0])),
        fma(half[1], 2.0, -0.5 * (p0[1] + p2[1])),
    ];
    let d01 = [p1[0] - p0[0], p1[1] - p0[1]];
    let d12 = [p2[0] - p1[0], p2[1] - p1[1]];
    let dd = [d01[0] - d12[0], d01[1] - d12[1]];
    let d02 = [d01[0] + d12[0], d01[1] + d12[1]];
    let cross = fma(-d02[1], dd[0], d02[0] * dd[1]);
    let x0 = fma(d12[1], dd[1], d12[0] * dd[0]) / cross;
    let x2 = fma(d01[1], dd[1], d01[0] * dd[0]) / cross;
    let dd_hypot = fma(dd[1], dd[1], dd[0] * dd[0]).sqrt();
    let scale = (cross / (dd_hypot * (x2 - x0))).abs();
    let a0 = integral(x0);
    let a2 = integral(x2);
    let da = a2 - a0;
    let sqrt_scale = scale.sqrt();
    let sqrt_tol = (0.25_f32 * 0.9).sqrt();
    let val = if x0 >= 0.0 && x2 >= 0.0 {
        da.abs() * sqrt_scale
    } else {
        let xmin = sqrt_tol / sqrt_scale;
        sqrt_tol * da.abs() / integral(xmin)
    };
    let val = if val.is_finite() { val } else { 0.0 };
    let u0 = inverse(a0);
    let uscale = 1.0 / (inverse(a2) - u0);
    Params {
        q: [p0[0], p0[1], p1[0], p1[1], p2[0], p2[1]],
        a0,
        da,
        u0,
        uscale,
        val,
    }
}
fn flatten_cubic(c: [Point; 4], out: &mut Vec<Point>) -> Result<usize, Box<dyn Error>> {
    let n_quads = estimate_quads(c);
    let mut p = Vec::with_capacity(n_quads);
    let mut samples = Vec::with_capacity(2 * n_quads + 1);
    for j in 0..=2 * n_quads {
        samples.push(cubic_at(c, j as f32 * (0.5 / n_quads as f32)));
    }
    for j in 0..n_quads {
        p.push(quad_params(
            samples[2 * j],
            samples[2 * j + 1],
            samples[2 * j + 2],
        ));
    }
    let sum: f32 = p.iter().map(|q| q.val).sum();
    if !sum.is_finite() || sum <= 0.0 {
        return Err("degenerate scalar flattening".into());
    }
    let sqrt_tol = (0.25_f32 * 0.9).sqrt();
    let n = ((0.5 * sum / sqrt_tol).ceil() as usize).max(1);
    if n > MAX_POINTS || out.len() + n > MAX_POINTS {
        return Err("too many scalar segments".into());
    }
    let step = sum / n as f32;
    let step_recip = 1.0 / step;
    let mut val_sum = 0.0_f32;
    let mut last_n = 0_usize;
    let mut x0base = 0.0_f32;
    for q in p {
        val_sum += q.val;
        let this_n = val_sum * step_recip;
        let this_n_next = 1.0 + this_n.floor();
        let next = this_n_next as usize;
        let dn = next.saturating_sub(last_n).min(n - last_n);
        if dn > 0 {
            let dx = step / q.val;
            let x0 = x0base * dx;
            for k in 0..dn {
                let x = fma(k as f32, dx, x0);
                let a = fma(q.da, x, q.a0);
                let u = inverse(a);
                let t = (u - q.u0) * q.uscale;
                let eval = |axis: usize| {
                    let p0 = q.q[axis];
                    let p1 = q.q[axis + 2];
                    let p2 = q.q[axis + 4];
                    let aa = fma(p1, -2.0, p0) + p2;
                    let bb = (p1 - p0) * 2.0;
                    fma(fma(aa, t, bb), t, p0)
                };
                // The preceding MoveTo/LineTo already supplies the cubic's
                // first point. Emitting it again creates a zero-length edge.
                if last_n + k > 0 {
                    out.push([eval(0) as f64, eval(1) as f64]);
                }
            }
        }
        x0base = this_n_next - this_n;
        last_n = next;
        if last_n >= n {
            break;
        }
    }
    out.push([(c[3][0] as f32) as f64, (c[3][1] as f32) as f64]);
    Ok(n_quads)
}
fn parse(source: &str) -> Result<(Vec<Point>, Vec<usize>), Box<dyn Error>> {
    let mut points = Vec::new();
    let mut current = None;
    let mut start = None;
    let mut quads = Vec::new();
    for line in source
        .lines()
        .filter(|l| !l.starts_with('#') && !l.trim().is_empty())
    {
        let mut fields = line.split_whitespace();
        let op = fields.next().ok_or("empty")?;
        let v = fields
            .map(str::parse::<f64>)
            .collect::<Result<Vec<_>, _>>()?;
        if v.iter().any(|x| !x.is_finite() || x.abs() > 4096.0) {
            return Err("invalid coordinate".into());
        }
        match (op, v.as_slice()) {
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
            ("C", [x1, y1, x2, y2, x3, y3]) if current.is_some() && quads.len() < 16 => {
                let end = [*x3, *y3];
                quads.push(flatten_cubic(
                    [current.unwrap(), [*x1, *y1], [*x2, *y2], end],
                    &mut points,
                )?);
                current = Some(end);
            }
            ("Z", []) if current.is_some() => {
                points.push(start.unwrap());
                current = None;
            }
            _ => return Err("unsupported path".into()),
        }
        if points.len() > MAX_POINTS {
            return Err("too many points".into());
        }
    }
    if current.is_some() || points.len() < 4 {
        return Err("open outline".into());
    }
    for p in &mut points {
        p[0] = (p[0] as f32) as f64;
        p[1] = (p[1] as f32) as f64;
    }
    *points.last_mut().unwrap() = points[0];
    curve_scene::encode(&points)?;
    Ok((points, quads))
}
fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let source = args
        .next()
        .ok_or("usage: curve_flatten_scalar OUTLINE.txt OUTPUT.txt")?;
    let output = args.next().ok_or("missing output")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let (points, quads) = parse(&fs::read_to_string(source)?)?;
    let mut file = fs::File::create(output)?;
    writeln!(file, "# bounded scalar curvature flattening")?;
    for pair in points.windows(2) {
        writeln!(
            file,
            "{:.9} {:.9} {:.9} {:.9}",
            pair[0][0], pair[0][1], pair[1][0], pair[1][1]
        )?;
    }
    eprintln!(
        "{} segments; quadratic subdivisions {:?}",
        points.len() - 1,
        quads
    );
    Ok(())
}
