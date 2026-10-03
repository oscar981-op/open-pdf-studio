//! Read-only experimental PDF path geometry exporter; not a renderer.
#![forbid(unsafe_code)]

use opds_opdl_adapter::{page_to_v3_path, path_v3::PathList};
use std::{env, error::Error, fs, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: export_path_v3 INPUT.pdf OUTPUT.opdl")?;
    let output = args.next().ok_or("missing output path")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let bytes = page_to_v3_path(Path::new(&input))?;
    let parsed = PathList::decode(&bytes)?;
    fs::write(output, bytes)?;
    println!(
        "{} segments on {}x{} page",
        parsed.segments.len(),
        parsed.width,
        parsed.height
    );
    Ok(())
}
