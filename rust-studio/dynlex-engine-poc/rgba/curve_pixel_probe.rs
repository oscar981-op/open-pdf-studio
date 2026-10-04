//! Compare one-pixel DynLex clipping with independent bounded Rust clipping.
use std::{env, error::Error, fs};
#[cfg(not(target_endian = "little"))]
compile_error!("OPLN f64 ABI requires little endian");
#[allow(dead_code)]
mod curve_scene;
type Point = [f64; 2];

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
fn clip(poly: &[Point], axis: usize, boundary: f64, greater: bool) -> Vec<Point> {
    if poly.is_empty() {
        return Vec::new();
    }
    let mut result = Vec::with_capacity(poly.len() + 2);
    let mut previous = *poly.last().unwrap();
    let mut was_inside = if greater {
        previous[axis] >= boundary
    } else {
        previous[axis] <= boundary
    };
    for &point in poly {
        let inside = if greater {
            point[axis] >= boundary
        } else {
            point[axis] <= boundary
        };
        if inside != was_inside {
            let ratio = (boundary - previous[axis]) / (point[axis] - previous[axis]);
            result.push([
                previous[0] + ratio * (point[0] - previous[0]),
                previous[1] + ratio * (point[1] - previous[1]),
            ]);
        }
        if inside {
            result.push(point);
        }
        previous = point;
        was_inside = inside;
    }
    result
}
fn area(points: &[Point], x: i32, y: i32) -> f64 {
    let mut p = points[..points.len() - 1].to_vec();
    for (axis, boundary, greater) in [
        (0, x as f64, true),
        (0, (x + 1) as f64, false),
        (1, y as f64, true),
        (1, (y + 1) as f64, false),
    ] {
        p = clip(&p, axis, boundary, greater);
    }
    let mut signed = 0.0;
    for i in 0..p.len() {
        let a = p[i];
        let b = p[(i + 1) % p.len()];
        signed += a[0] * b[1] - b[0] * a[1];
    }
    (signed.abs() / 2.0).min(1.0)
}
fn compare(points: &[Point], positions: &[(i32, i32)]) -> Result<(), Box<dyn Error>> {
    let raw = curve_scene::encode(points)?;
    let decoded = curve_scene::decode(&raw)?;
    let mut words = Vec::with_capacity(3 + decoded.len() * 2);
    words.push(u64::from_le_bytes(raw[..8].try_into()?));
    words.push(u64::from_le_bytes(raw[8..16].try_into()?));
    words.push(u64::from(decoded.len() as u32).to_le());
    for point in &decoded {
        words.push(point[0].to_bits().to_le());
        words.push(point[1].to_bits().to_le());
    }
    let len = i32::try_from(words.len() * 8)?;
    let mut output = 0_u64;
    let bad = unsafe { opds_curve_pixel(std::ptr::null(), 0, 0, 0, std::ptr::null_mut(), 0) };
    let short = unsafe {
        opds_curve_pixel(
            words.as_ptr().cast(),
            len,
            0,
            0,
            (&mut output as *mut u64).cast(),
            4,
        )
    };
    if bad == 0 || short == 0 {
        return Err("invalid ABI accepted".into());
    }
    for &(x, y) in positions {
        let status = unsafe {
            opds_curve_pixel(
                words.as_ptr().cast(),
                len,
                x,
                y,
                (&mut output as *mut u64).cast(),
                8,
            )
        };
        if status != 0 {
            return Err(format!("DynLex status {status} at {x},{y}").into());
        }
        let actual = f64::from_bits(u64::from_le(output));
        let expected = area(&decoded, x, y);
        if (actual - expected).abs() > 1e-9 {
            return Err(
                format!("area mismatch at {x},{y}: DynLex {actual}, Rust {expected}").into(),
            );
        }
    }
    println!("{} points, {} pixels match", decoded.len(), positions.len());
    Ok(())
}
fn main() -> Result<(), Box<dyn Error>> {
    compare(
        &[
            [10.25, 20.25],
            [12.75, 20.25],
            [12.75, 22.75],
            [10.25, 22.75],
            [10.25, 20.25],
        ],
        &[(9, 20), (10, 20), (11, 21), (12, 22), (13, 22)],
    )?;
    let path = env::args_os()
        .nth(1)
        .ok_or("usage: curve_pixel_probe FLAT_POINTS.txt")?;
    let points = curve_scene::parse_lines(&fs::read_to_string(path)?)?;
    let positions: Vec<_> = (0..192)
        .flat_map(|y| (0..256).map(move |x| (x, y)))
        .collect();
    compare(&points, &positions)?;
    Ok(())
}
