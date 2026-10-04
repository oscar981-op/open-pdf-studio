//! Strict Rust v4 validation and staging around the independent DynLex kernel.

use opds_opdl_adapter::paint_v4::DisplayList;
use std::{env, error::Error, fs, path::Path};

#[link(name = "opds_dynlex_v4_buffer", kind = "static")]
unsafe extern "C" {
    fn opds_render_v4_buffer(scene: *const u8, scene_len: i32,
                             output: *mut u8, output_len: i32) -> i32;
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args.next().ok_or("usage: v4_buffer_probe INPUT-v4.opdl OUTPUT.rgba")?;
    let output = args.next().ok_or("missing output path")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let list = DisplayList::decode(&fs::read(Path::new(&input))?)?;
    let strokes = list.verified_strokes()?;
    let mut scene = Vec::with_capacity(12 + strokes.len() * 24);
    scene.extend_from_slice(&list.width.to_le_bytes());
    scene.extend_from_slice(&list.height.to_le_bytes());
    scene.extend_from_slice(&u32::try_from(strokes.len())?.to_le_bytes());
    for stroke in strokes {
        scene.extend_from_slice(&stroke.kind.to_le_bytes());
        for value in stroke.rect {
            scene.extend_from_slice(&value.to_le_bytes());
        }
        scene.extend_from_slice(&[stroke.rgb[0], stroke.rgb[1], stroke.rgb[2], 255]);
    }
    if scene.len() > 393_228 || scene.len() < 12 {
        return Err("invalid normalized v4 scene length".into());
    }
    let pixel_len = usize::try_from(list.width)?
        .checked_mul(usize::try_from(list.height)?)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or("RGBA output size overflow")?;
    let mut rgba = vec![0_u8; 12 + pixel_len];
    // SAFETY: DisplayList validates every v4 record. verified_strokes validates
    // the proven 1px axis/45° subset and all rectangles against page bounds.
    // `scene` contains only 12 + 24*N bytes of bounded normalized commands;
    // `rgba` has the 12 control bytes plus exactly width*height*4 pixel bytes.
    let status = unsafe {
        opds_render_v4_buffer(scene.as_ptr(), i32::try_from(scene.len())?,
                              rgba.as_mut_ptr(), i32::try_from(rgba.len())?)
    };
    if status != 0 {
        return Err(format!("DynLex v4 buffer returned status {status}").into());
    }
    fs::write(output, &rgba[12..])?;
    Ok(())
}
