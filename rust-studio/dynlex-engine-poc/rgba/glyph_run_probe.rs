//! Standalone command for the shared experimental glyph-run core.
use std::{env, error::Error, fs};
#[path = "glyph_run_core.rs"]
mod core;

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args.next().ok_or("usage: glyph_run_probe INPUT.opgs OUTPUT.rgba")?;
    let output = args.next().ok_or("missing RGBA output")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    fs::write(output, core::render_opgs(&fs::read(input)?)?)?;
    Ok(())
}
