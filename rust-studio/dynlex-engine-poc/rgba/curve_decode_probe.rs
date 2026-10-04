//! Rust-validates OPLN and checks a tiny DynLex f64 buffer-read boundary.

use std::{env, error::Error, fs};
#[cfg(not(target_endian = "little"))]
compile_error!("The experimental DynLex OPLN ABI requires little-endian memory");
#[allow(dead_code)]
mod curve_scene;

#[link(name = "opds_dynlex_curve_decode", kind = "static")]
unsafe extern "C" {
    fn opds_curve_decode(scene: *const u8, scene_len: i32, result: *mut u8, result_len: i32)
    -> i32;
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args.next().ok_or("usage: curve_decode_probe INPUT.opln")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let bytes = fs::read(input)?;
    let points = curve_scene::decode(&bytes)?;
    let mut words = Vec::with_capacity(3 + points.len() * 2);
    words.push(u64::from_le_bytes(bytes[..8].try_into()?));
    words.push(u64::from_le_bytes(bytes[8..16].try_into()?));
    words.push(u64::from(points.len() as u32).to_le());
    for point in &points {
        words.push(point[0].to_bits().to_le());
        words.push(point[1].to_bits().to_le());
    }
    let mut result = [0_u64; 4];
    // The C ABI must reject invalid pointers and lengths before DynLex runs.
    let null_status = unsafe { opds_curve_decode(std::ptr::null(), 0, std::ptr::null_mut(), 0) };
    let short_status = unsafe {
        opds_curve_decode(
            words.as_ptr().cast(),
            i32::try_from(words.len() * 8)?,
            result.as_mut_ptr().cast(),
            8,
        )
    };
    if null_status == 0 || short_status == 0 {
        return Err("DynLex boundary accepted invalid C ABI lengths".into());
    }
    // SAFETY: OPLN was fully decoded and validated. The normalized source is
    // 8-byte aligned and has 24 + 16*N bytes, so the final f64 read is in
    // bounds. `result` is an aligned 32-byte output buffer.
    let status = unsafe {
        opds_curve_decode(
            words.as_ptr().cast(),
            i32::try_from(words.len() * 8)?,
            result.as_mut_ptr().cast(),
            32,
        )
    };
    if status != 0 {
        return Err(format!("DynLex decode status {status}").into());
    }
    let count = (result[0] & 0xffff_ffff) as usize;
    let first_x = f64::from_bits(u64::from_le(result[1]));
    let last_x = f64::from_bits(u64::from_le(result[2]));
    let sum_x = f64::from_bits(u64::from_le(result[3]));
    let expected_sum = points.iter().map(|point| point[0]).sum::<f64>();
    if count != points.len()
        || first_x != points[0][0]
        || last_x != points.last().unwrap()[0]
        || (sum_x - expected_sum).abs() > 0.000001
    {
        return Err("DynLex f64 boundary mismatch".into());
    }
    println!("{count} {first_x:.9} {last_x:.9} {sum_x:.9}");
    Ok(())
}
