//! Experimental OPM2 multi-contour glyph coverage using DynLex per-contour area.
//! Rust validates the scene and combines signed contour areas; no app integration.
use std::{env, error::Error, fs};
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

#[link(name = "opds_dynlex_curve_pixel", kind = "static")]
unsafe extern "C" {
    fn opds_curve_pixel(
        scene: *const u8,
        scene_len: i32,
        x: i32,
        y: i32,
        result: *mut u8,
        result_len: i32,
    ) -> i32;
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

fn signed_area(contour: &[[f64; 2]]) -> f64 {
    contour
        .windows(2)
        .map(|edge| edge[0][0] * edge[1][1] - edge[1][0] * edge[0][1])
        .sum::<f64>()
        * 0.5
}

fn aligned_scene(contour: &[[f64; 2]]) -> Result<Vec<u64>, Box<dyn Error>> {
    let encoded = curve_scene::encode(contour)?;
    let mut words = Vec::with_capacity(3 + contour.len() * 2);
    words.push(u64::from_le_bytes(encoded[..8].try_into()?));
    words.push(u64::from_le_bytes(encoded[8..16].try_into()?));
    words.push(u64::from(contour.len() as u32).to_le());
    for point in contour {
        for value in point {
            words.push(value.to_bits().to_le());
        }
    }
    Ok(words)
}

pub fn render(contours: &[Vec<[f64; 2]>]) -> Result<Vec<u8>, Box<dyn Error>> {
    let aligned = contours
        .iter()
        .map(|c| aligned_scene(c))
        .collect::<Result<Vec<_>, _>>()?;
    let signs = contours
        .iter()
        .map(|c| signed_area(c).signum())
        .collect::<Vec<_>>();
    if signs.iter().any(|v| *v == 0.0) {
        return Err("zero-area contour".into());
    }
    let mut output = vec![0_u8; WIDTH * HEIGHT];
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let mut area = 0.0;
            for (words, sign) in aligned.iter().zip(&signs) {
                let mut value = 0.0_f64;
                // SAFETY: OPM2 and each OPLN contour were validated; words own the
                // aligned input, the f64 output is writable and ABI lengths match.
                let status = unsafe {
                    opds_curve_pixel(
                        words.as_ptr().cast(),
                        (words.len() * 8) as i32,
                        x as i32,
                        y as i32,
                        (&mut value as *mut f64).cast(),
                        8,
                    )
                };
                if status != 0 {
                    return Err(format!("DynLex status {status}").into());
                }
                area += sign * value;
            }
            output[y * WIDTH + x] = (area.abs().clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
    }
    Ok(output)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: glyph_multi_probe INPUT.opm OUTPUT.raw")?;
    let output = args.next().ok_or("missing output")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let contours = read_scene(&fs::read(input)?)?;
    fs::write(output, render(&contours)?)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scene(contours: &[Vec<[f64; 2]>]) -> Vec<u8> {
        let mut bytes = b"OPM2".to_vec();
        for value in [1_u32, 256, 192, contours.len() as u32] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for contour in contours {
            bytes.extend_from_slice(&(contour.len() as u32).to_le_bytes());
            for point in contour {
                for value in point {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        bytes
    }
    #[test]
    fn invalid_scene_rejected_without_panic() {
        let invalid = b"OPM2".to_vec();
        assert!(read_scene(&invalid).is_err());
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"OPM2");
        for value in [1_u32, 256, 192, 2] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.extend_from_slice(&4_u32.to_le_bytes());
        assert!(read_scene(&bytes).is_err());
        let outer = vec![[1.0, 1.0], [9.0, 1.0], [9.0, 9.0], [1.0, 9.0], [1.0, 1.0]];
        let inner = vec![[3.0, 3.0], [3.0, 7.0], [7.0, 7.0], [7.0, 3.0], [3.0, 3.0]];
        let valid = scene(&[outer.clone(), inner.clone()]);
        assert_eq!(read_scene(&valid).unwrap().len(), 2);
        for len in 0..valid.len() {
            assert!(read_scene(&valid[..len]).is_err());
        }
        for index in 0..valid.len() {
            let mut mutated = valid.clone();
            mutated[index] ^= 0xff;
            let _ = read_scene(&mutated); // No one-byte mutation may panic.
        }
        let mut extra = valid.clone();
        extra.push(0);
        assert!(read_scene(&extra).is_err());
        let mut nan = valid.clone();
        nan[24..32].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(read_scene(&nan).is_err());
        let bow = vec![[2.0, 2.0], [8.0, 8.0], [2.0, 8.0], [8.0, 2.0], [2.0, 2.0]];
        assert!(read_scene(&scene(&[outer.clone(), bow])).is_err());
        let touch = vec![[9.0, 3.0], [12.0, 3.0], [12.0, 6.0], [9.0, 6.0], [9.0, 3.0]];
        assert!(read_scene(&scene(&[outer, touch])).is_err());
    }
}
