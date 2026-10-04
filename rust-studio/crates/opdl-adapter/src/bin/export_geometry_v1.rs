//! Isolated read-only PDF→OPGM-v1 experiment.
#![forbid(unsafe_code)]
use opds_opdl_adapter::geometry_v1::page_to_opgm;
use std::{env, error::Error, fs, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: export_geometry_v1 INPUT.pdf OUTPUT.opgm")?;
    let output = args.next().ok_or("missing output path")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let bytes = page_to_opgm(Path::new(&input))?;
    fs::write(output, bytes)?;
    Ok(())
}
