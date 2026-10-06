//! Standalone command for the shared experimental glyph-run core.
use std::{env, error::Error, fs};
#[path = "glyph_run_core.rs"]
mod core;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args.next().ok_or("usage: glyph_run_probe INPUT.opgs OUTPUT.rgba")?;
    let output = args.next().ok_or("missing RGBA output")?;
    let layer = match args.next() {
        Some(option) if option == "--layer" => true,
        Some(_) => return Err("unknown argument".into()),
        None => false,
    };
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let bytes = fs::read(input)?;
    let rgba = if layer {
        core::render_opgs_layer(&bytes)?
    } else {
        core::render_opgs(&bytes)?
    };
    fs::write(output, rgba)?;
    Ok(())
}
