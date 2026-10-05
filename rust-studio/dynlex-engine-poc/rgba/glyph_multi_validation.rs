//! Shared bounded OPM2 validator for the DynLex glyph-run kernel.
use std::error::Error;
#[allow(dead_code)] // The standalone curve-scene CLI also lives in this file.
#[path = "curve_scene.rs"]
mod curve_scene;
const WIDTH: usize = 256;
const HEIGHT: usize = 192;
const MAX_CONTOURS: usize = 8;
const MAX_POINTS: usize = 256;

fn cross(a: [f64; 2], b: [f64; 2], c: [f64; 2]) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn segments_touch(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let ab_c = cross(a, b, c);
    let ab_d = cross(a, b, d);
    let cd_a = cross(c, d, a);
    let cd_b = cross(c, d, b);
    let overlap =
        |p: f64, q: f64, r: f64, s: f64| p.min(q) <= r.max(s) + 1e-9 && r.min(s) <= p.max(q) + 1e-9;
    if !overlap(a[0], b[0], c[0], d[0]) || !overlap(a[1], b[1], c[1], d[1]) {
        return false;
    }
    let eps = 1e-9;
    (ab_c.abs() <= eps || ab_d.abs() <= eps || ab_c.signum() != ab_d.signum())
        && (cd_a.abs() <= eps || cd_b.abs() <= eps || cd_a.signum() != cd_b.signum())
}

pub fn validate_topology(contours: &[Vec<[f64; 2]>]) -> Result<(), Box<dyn Error>> {
    for (ci, contour) in contours.iter().enumerate() {
        if signed_area(contour).abs() <= 1e-9 {
            return Err("zero-area contour".into());
        }
        let edges = contour.len() - 1;
        for i in 0..edges {
            if contour[i] == contour[i + 1] {
                return Err("zero-length edge".into());
            }
            for j in i + 1..edges {
                if j == i + 1 || (i == 0 && j == edges - 1) {
                    continue;
                }
                if segments_touch(contour[i], contour[i + 1], contour[j], contour[j + 1]) {
                    return Err("self-crossing contour".into());
                }
            }
            for other in &contours[..ci] {
                for edge in other.windows(2) {
                    if segments_touch(contour[i], contour[i + 1], edge[0], edge[1]) {
                        return Err("touching or crossing contours".into());
                    }
                }
            }
        }
    }
    Ok(())
}

pub fn read_scene(bytes: &[u8]) -> Result<Vec<Vec<[f64; 2]>>, Box<dyn Error>> {
    if bytes.len() < 20 || &bytes[..4] != b"OPM2" || bytes.len() > 4_148 {
        return Err("invalid OPM2 header or size".into());
    }
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let count = word(16) as usize;
    if word(4) != 1
        || word(8) != WIDTH as u32
        || word(12) != HEIGHT as u32
        || !(1..=MAX_CONTOURS).contains(&count)
    {
        return Err("invalid OPM2 version, dimensions or contour count".into());
    }
    let mut at = 20;
    let mut total = 0;
    let mut contours = Vec::with_capacity(count);
    for _ in 0..count {
        if at + 4 > bytes.len() {
            return Err("truncated contour count".into());
        }
        let points = word(at) as usize;
        at += 4;
        total += points;
        if !(4..=MAX_POINTS).contains(&points)
            || total > MAX_POINTS
            || points
                .checked_mul(16)
                .is_none_or(|size| at + size > bytes.len())
        {
            return Err("invalid contour length".into());
        }
        let mut contour = Vec::with_capacity(points);
        for pair in bytes[at..at + points * 16].chunks_exact(16) {
            contour.push([
                f64::from_le_bytes(pair[..8].try_into()?),
                f64::from_le_bytes(pair[8..].try_into()?),
            ]);
        }
        at += points * 16;
        curve_scene::encode(&contour)?; // Reuses closed, finite, bounded OPLN checks.
        contours.push(contour);
    }
    if at != bytes.len() {
        return Err("trailing OPM2 bytes".into());
    }
    validate_topology(&contours)?;
    Ok(contours)
}

pub fn signed_area(contour: &[[f64; 2]]) -> f64 {
    contour
        .windows(2)
        .map(|edge| edge[0][0] * edge[1][1] - edge[1][0] * edge[0][1])
        .sum::<f64>()
        * 0.5
}
