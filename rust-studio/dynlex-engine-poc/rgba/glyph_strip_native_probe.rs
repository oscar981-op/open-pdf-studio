//! Validate OPM2 in Rust, then compare native DynLex composition to legacy probe.
use std::{env, error::Error, fs, time::Instant};
#[path = "glyph_multi_probe.rs"]
mod legacy;

#[link(name = "opds_dynlex_glyph_strip", kind = "static")]
unsafe extern "C" {
    fn opds_glyph_strip_raster(
        scene: *const u8,
        scene_len: i32,
        output: *mut u8,
        output_len: i32,
    ) -> i32;
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: glyph_strip_native_probe INPUT.opm OUTPUT.raw")?;
    let output = args.next().ok_or("missing output")?;
    let repeats = args
        .next()
        .map(|value| value.to_string_lossy().parse::<usize>())
        .transpose()?
        .unwrap_or(1);
    if !(1..=1000).contains(&repeats) {
        return Err("repeats outside limit".into());
    }
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let contours = legacy::read_scene(&fs::read(input)?)?;
    let mut quantized = Vec::with_capacity(contours.len());
    for contour in &contours {
        let mut path = Vec::with_capacity(contour.len());
        for &[x, y] in contour {
            if !(0.0..=256.0).contains(&x) || !(0.0..=192.0).contains(&y) {
                return Err("f32 strip proof requires an outline inside the page".into());
            }
            path.push([(x as f32) as f64, (y as f32) as f64]);
        }
        quantized.push(path);
    }
    // Topology must still hold after the screen-coordinate f32 narrowing
    // actually used by the DynLex tile rasterizer.
    legacy::validate_topology(&quantized)?;
    let mut words = Vec::<u64>::new();
    words.push(u64::from_le_bytes(*b"OPM2\x01\0\0\0"));
    words.push((192_u64 << 32) | 256);
    words.push(contours.len() as u64);
    for contour in &contours {
        words.push(contour.len() as u64);
        for point in contour {
            for value in point {
                words.push(value.to_bits().to_le());
            }
        }
    }
    let len = i32::try_from(words.len() * 8)?;
    let mut coverage = vec![0_u8; 256 * 192];
    // SAFETY: ABI guards reject null/short arguments before entering DynLex.
    let invalid = unsafe { opds_glyph_strip_raster(std::ptr::null(), 0, std::ptr::null_mut(), 0) };
    let short =
        unsafe { opds_glyph_strip_raster(words.as_ptr().cast(), len, coverage.as_mut_ptr(), 1) };
    if invalid == 0 || short == 0 {
        return Err("invalid ABI accepted".into());
    }
    // SAFETY: OPM2 was fully validated; `words` owns 8-byte-aligned input and
    // the 49,152-byte output remains writable for the whole kernel call.
    let start = Instant::now();
    for _ in 0..repeats {
        let status = unsafe {
            opds_glyph_strip_raster(
                words.as_ptr().cast(),
                len,
                coverage.as_mut_ptr(),
                coverage.len() as i32,
            )
        };
        if status != 0 {
            return Err(format!("DynLex strip status {status}").into());
        }
    }
    let kernel_ms = start.elapsed().as_secs_f64() * 1000.0 / repeats as f64;
    fs::write(output, coverage)?;
    eprintln!("native_dynlex_ms_per_call={kernel_ms:.4} repeats={repeats}");
    Ok(())
}
