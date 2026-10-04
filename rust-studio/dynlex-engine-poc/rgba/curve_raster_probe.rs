//! Rust validates OPLN, then the experimental DynLex kernel writes 256x192 coverage.
use std::{env, error::Error, fs, time::Instant};
#[cfg(not(target_endian = "little"))]
compile_error!("OPLN f64 ABI requires little endian");
#[allow(dead_code)]
mod curve_scene;

#[link(name = "opds_dynlex_curve_pixel", kind = "static")]
unsafe extern "C" {
    fn opds_curve_raster(scene: *const u8, scene_len: i32, output: *mut u8, output_len: i32)
        -> i32;
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let source = args
        .next()
        .ok_or("usage: curve_raster_probe POINTS.txt OUTPUT.raw [REPEATS]")?;
    let target = args.next().ok_or("missing output path")?;
    let repeats: usize = args
        .next()
        .map(|s| s.to_string_lossy().parse())
        .transpose()?
        .unwrap_or(1);
    if args.next().is_some() || !(1..=1000).contains(&repeats) {
        return Err("invalid repeats or extra argument".into());
    }
    let bytes = fs::read(source)?;
    let points = if bytes.starts_with(b"OPLN") {
        curve_scene::decode(&bytes)?
    } else {
        curve_scene::parse_lines(std::str::from_utf8(&bytes)?)?
    };
    let validated = curve_scene::encode(&points)?;
    let decoded = curve_scene::decode(&validated)?;
    let mut words = Vec::with_capacity(3 + decoded.len() * 2);
    words.push(u64::from_le_bytes(validated[..8].try_into()?));
    words.push(u64::from_le_bytes(validated[8..16].try_into()?));
    words.push(u64::from(decoded.len() as u32).to_le());
    for point in &decoded {
        words.push(point[0].to_bits().to_le());
        words.push(point[1].to_bits().to_le());
    }
    let len = i32::try_from(words.len() * 8)?;
    let mut coverage = vec![0_u8; 256 * 192];
    let null_status = unsafe { opds_curve_raster(std::ptr::null(), 0, std::ptr::null_mut(), 0) };
    let short_status =
        unsafe { opds_curve_raster(words.as_ptr().cast(), len, coverage.as_mut_ptr(), 1) };
    if null_status == 0 || short_status == 0 {
        return Err("invalid ABI accepted".into());
    }
    let start = Instant::now();
    for _ in 0..repeats {
        // SAFETY: Entire OPLN was validated in Rust and normalized to 8-byte-aligned
        // f64 points at offset 24; output has exactly 49,152 writable bytes.
        let status =
            unsafe { opds_curve_raster(words.as_ptr().cast(), len, coverage.as_mut_ptr(), 49_152) };
        if status != 0 {
            return Err(format!("DynLex raster status {status}").into());
        }
    }
    let elapsed = start.elapsed();
    fs::write(target, &coverage)?;
    eprintln!(
        "points={} repeats={} kernel_ms={:.3} per_call_ms={:.3}",
        decoded.len(),
        repeats,
        elapsed.as_secs_f64() * 1000.0,
        elapsed.as_secs_f64() * 1000.0 / repeats as f64
    );
    Ok(())
}
