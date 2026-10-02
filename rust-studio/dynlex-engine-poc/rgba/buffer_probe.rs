//! Safe fixture validation around the experimental DynLex buffer ABI.

use std::{env, error::Error, fs, path::Path, time::Instant};

#[link(name = "opds_dynlex_buffer", kind = "static")]
unsafe extern "C" {
    fn opds_render_buffer(
        scene: *const u8,
        scene_len: i32,
        output: *mut u8,
        output_len: i32,
        mode: i32,
    ) -> i32;
}

fn word(scene: &[u8], offset: usize) -> Result<usize, Box<dyn Error>> {
    let bytes: [u8; 4] = scene
        .get(offset..offset + 4)
        .ok_or("truncated OPDL word")?
        .try_into()?;
    Ok(u32::from_le_bytes(bytes) as usize)
}

fn validate(scene: &[u8]) -> Result<(usize, usize), Box<dyn Error>> {
    if scene.len() < 40 || scene.len() > 3_147_312 || &scene[..4] != b"OPDL" || word(scene, 4)? != 2
    {
        return Err("invalid OPDL-v2 header".into());
    }
    let width = word(scene, 8)?;
    let height = word(scene, 12)?;
    if !(1..=1024).contains(&width) || !(1..=1024).contains(&height) {
        return Err("invalid page dimensions".into());
    }
    let clip = [
        word(scene, 16)?,
        word(scene, 20)?,
        word(scene, 24)?,
        word(scene, 28)?,
    ];
    if clip[0] > clip[2] || clip[2] > width || clip[1] > clip[3] || clip[3] > height {
        return Err("invalid clip rectangle".into());
    }
    let image_len = word(scene, 32)?;
    if image_len != 0 && image_len != width * height * 3 {
        return Err("invalid RGB image length".into());
    }
    let count_offset = 36 + image_len;
    let count = word(scene, count_offset)?;
    if count > 64 || count_offset + 4 + count * 24 != scene.len() {
        return Err("invalid OPDL command count or length".into());
    }
    for index in 0..count {
        let offset = count_offset + 4 + index * 24;
        let kind = word(scene, offset)?;
        let left = word(scene, offset + 4)?;
        let top = word(scene, offset + 8)?;
        let right = word(scene, offset + 12)?;
        let bottom = word(scene, offset + 16)?;
        if !(1..=3).contains(&kind)
            || left > right
            || right > width
            || top > bottom
            || bottom > height
        {
            return Err("invalid drawing command".into());
        }
        if kind == 2 && right - left != bottom - top {
            return Err("invalid diagonal command".into());
        }
        if kind == 3 && right - left != 1 && bottom - top != 1 {
            return Err("invalid stroke command".into());
        }
    }
    Ok((width, height))
}

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args().collect();
    if args.len() != 4 && args.len() != 5 {
        return Err("usage: buffer_probe MODE INPUT.opdl OUTPUT.rgba [REPEAT]".into());
    }
    let mode: i32 = args[1].parse()?;
    if !(1..=3).contains(&mode) {
        return Err("mode must be 1=raster, 2=vector or 3=hybrid".into());
    }
    let scene = fs::read(&args[2])?;
    let (width, height) = validate(&scene)?;
    let mut output = vec![0_u8; 12 + width * height * 4];
    let repeat: usize = if args.len() == 5 { args[4].parse()? } else { 1 };
    if !(1..=10_000).contains(&repeat) {
        return Err("repeat must be between 1 and 10000".into());
    }
    // SAFETY: validate checked every OPDL offset, command and output bound;
    // the generated ABI wrapper writes the 12-byte control header before
    // DynLex writes exactly width*height*4 bytes into the following slice.
    let started = Instant::now();
    for _ in 0..repeat {
        let status = unsafe {
            opds_render_buffer(
                scene.as_ptr(),
                i32::try_from(scene.len())?,
                output.as_mut_ptr(),
                i32::try_from(output.len())?,
                mode,
            )
        };
        if status != 0 {
            return Err(format!("DynLex buffer kernel rejected scene: {status}").into());
        }
    }
    if repeat > 1 {
        eprintln!(
            "kernel_ms_per_call={:.4}",
            started.elapsed().as_secs_f64() * 1000.0 / repeat as f64
        );
    }
    fs::write(Path::new(&args[3]), &output[12..])?;
    Ok(())
}
