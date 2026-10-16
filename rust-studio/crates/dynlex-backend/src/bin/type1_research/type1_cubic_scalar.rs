//! Research-only scalar cubic subdivision. The numeric construction follows
//! vello_common 0.0.8's MIT/Apache-2.0 flatten_simd.rs; no Vello code runs.
//! Kept separate from the native PDF path until complete pixel parity exists.

pub type Point = [f64; 2];
const MAX_QUADS: usize = 16;

#[derive(Clone, Copy)]
struct Params {
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
    a0: f32,
    da: f32,
    u0: f32,
    uscale: f32,
    val: f32,
}

fn integral(x: f32) -> f32 {
    // Keep the f32 subtraction boundary: 1 - 0.67 is one ULP below the
    // directly rounded literal 0.33 and shifts rare flattened edge points.
    x / ((1.0_f32 - 0.67_f32) + 0.25_f32.mul_add(x * x, 0.201_511_2).sqrt().sqrt())
}

fn inverse(x: f32) -> f32 {
    x * (0.61 + 0.25_f32.mul_add(x * x, 0.39 * 0.39).sqrt())
}

fn cubic_at(c: [Point; 4], t: f32) -> [f32; 2] {
    std::array::from_fn(|axis| {
        let p: [f32; 4] = std::array::from_fn(|i| c[i][axis] as f32);
        let a = (p[1] - p[2]).mul_add(3.0, p[3] - p[0]);
        let b = p[1].mul_add(-2.0, p[0] + p[2]) * 3.0;
        let cc = (p[1] - p[0]) * 3.0;
        a.mul_add(t, b).mul_add(t, cc).mul_add(t, p[0])
    })
}

fn estimate_quads(c: [Point; 4]) -> usize {
    let err: f64 = (0..2)
        .map(|axis| {
            let a = 3.0 * c[1][axis] - c[0][axis];
            let b = 3.0 * c[2][axis] - c[3][axis];
            (b - a) * (b - a)
        })
        .sum();
    let scaled = err / (432.0 * 0.025 * 0.025);
    (1..=MAX_QUADS)
        .find(|&n| scaled <= (n as f64).powi(6))
        .unwrap_or(MAX_QUADS)
}

fn quad_params(p0: [f32; 2], half: [f32; 2], p2: [f32; 2]) -> Params {
    // Preserve the two f32 FMA boundaries of the source-informed quadratic
    // conversion. Combining the two half-point offsets changes rare AA bytes.
    let p1 = std::array::from_fn(|i| {
        let first = half[i].mul_add(2.0, p0[i] * -0.5);
        p2[i].mul_add(-0.5, first)
    });
    let d01: [f32; 2] = std::array::from_fn(|i| p1[i] - p0[i]);
    let d12: [f32; 2] = std::array::from_fn(|i| p2[i] - p1[i]);
    let dd: [f32; 2] = std::array::from_fn(|i| d01[i] - d12[i]);
    let d02: [f32; 2] = std::array::from_fn(|i| d01[i] + d12[i]);
    let cross = (-d02[1]).mul_add(dd[0], d02[0] * dd[1]);
    let x0 = d12[1].mul_add(dd[1], d12[0] * dd[0]) / cross;
    let x2 = d01[1].mul_add(dd[1], d01[0] * dd[0]) / cross;
    let dd_hypot = dd[1].mul_add(dd[1], dd[0] * dd[0]).sqrt();
    let scale = (cross / (dd_hypot * (x2 - x0))).abs();
    let a0 = integral(x0);
    let a2 = integral(x2);
    let da = a2 - a0;
    let sqrt_tol = (0.25_f32 * 0.9).sqrt();
    let sqrt_scale = scale.sqrt();
    let val = if x0 >= 0.0 && x2 >= 0.0 {
        da.abs() * sqrt_scale
    } else {
        sqrt_tol * da.abs() / integral(sqrt_tol / sqrt_scale)
    };
    Params {
        p0,
        p1,
        p2,
        a0,
        da,
        u0: inverse(a0),
        uscale: 1.0 / (inverse(a2) - inverse(a0)),
        val: if val.is_finite() { val } else { 0.0 },
    }
}

pub fn flatten(c: [Point; 4], out: &mut Vec<Point>) -> Result<(), &'static str> {
    let chord = [c[3][0] - c[0][0], c[3][1] - c[0][1]];
    let chord_sq = chord[0] * chord[0] + chord[1] * chord[1];
    let nearest_sq = |p: Point| {
        let d = [p[0] - c[0][0], p[1] - c[0][1]];
        let t = if chord_sq > 0.0 {
            ((d[0] * chord[0] + d[1] * chord[1]) / chord_sq).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let x = d[0] - t * chord[0];
        let y = d[1] - t * chord[1];
        x * x + y * y
    };
    if nearest_sq(c[1]).max(nearest_sq(c[2])) <= (16.0 / 9.0) * 0.25_f64.powi(2) {
        out.push(c[3]);
        return Ok(());
    }
    let n_quads = estimate_quads(c);
    // The source SIMD evaluator starts four lanes at 0/1/2/3 * dt, then
    // increments each lane by 4*dt per block. Recomputing j*dt for every
    // sample changes f32 rounding after the first block.
    let dt = 0.5 / n_quads as f32;
    let t_inc = 4.0 * dt;
    let mut lanes = [0.0 * dt, 1.0 * dt, 2.0 * dt, 3.0 * dt];
    let mut samples = Vec::with_capacity(2 * n_quads + 1);
    for _ in 0..(n_quads + 1).div_ceil(2) {
        for t in lanes {
            if samples.len() < 2 * n_quads {
                samples.push(cubic_at(c, t));
            }
        }
        for lane in &mut lanes {
            *lane += t_inc;
        }
    }
    samples.push([(c[3][0] as f32), (c[3][1] as f32)]);
    let params: Vec<_> = (0..n_quads)
        .map(|j| quad_params(samples[2 * j], samples[2 * j + 1], samples[2 * j + 2]))
        .collect();
    let sum: f32 = params.iter().map(|q| q.val).sum();
    if !sum.is_finite() || sum <= 0.0 {
        return Err("degenerate cubic flattening");
    }
    let sqrt_tol = (0.25_f32 * 0.9).sqrt();
    let n = ((0.5 * sum / sqrt_tol).ceil() as usize).max(1);
    if n > 256 || out.len() + n > 256 {
        return Err("Type1 scalar point limit");
    }
    let step = sum / n as f32;
    let recip = 1.0 / step;
    let mut val_sum = 0.0_f32;
    let mut last_n = 0_usize;
    let mut x0base = 0.0_f32;
    for q in params {
        val_sum += q.val;
        let this_n = val_sum * recip;
        let this_n_next = 1.0 + this_n.floor();
        let next = (this_n_next as usize).min(n);
        let dn = next.saturating_sub(last_n);
        if dn > 0 {
            let dx = step / q.val;
            let x0 = x0base * dx;
            let a_inc = (4.0 * dx) * q.da;
            let mut a_lanes: [f32; 4] =
                std::array::from_fn(|lane| q.da.mul_add((lane as f32).mul_add(dx, x0), q.a0));
            for k in 0..dn {
                let lane = k % 4;
                if k > 0 && lane == 0 {
                    for a in &mut a_lanes {
                        *a += a_inc;
                    }
                }
                let a = a_lanes[lane];
                let t = (inverse(a) - q.u0) * q.uscale;
                let point: Point = std::array::from_fn(|axis| {
                    let aa = q.p1[axis].mul_add(-2.0, q.p0[axis]) + q.p2[axis];
                    let bb = (q.p1[axis] - q.p0[axis]) * 2.0;
                    aa.mul_add(t, bb).mul_add(t, q.p0[axis]) as f64
                });
                if last_n + k > 0 {
                    out.push(point);
                }
            }
        }
        x0base = this_n_next - this_n;
        last_n = next;
    }
    out.push([(c[3][0] as f32) as f64, (c[3][1] as f32) as f64]);
    Ok(())
}
