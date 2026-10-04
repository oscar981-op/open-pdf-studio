//! Isolated read-only PDF→OPGM-v2 multi-rectangle experiment.
#![forbid(unsafe_code)]
use opds_opdl_adapter::geometry_v2::page_to_opgm_v2;
use std::{env, error::Error, fs, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: export_geometry_v2 INPUT.pdf OUTPUT.opgm")?;
    let output = args.next().ok_or("missing output path")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let bytes = page_to_opgm_v2(Path::new(&input))?;
    fs::write(output, bytes)?;
    Ok(())
}
