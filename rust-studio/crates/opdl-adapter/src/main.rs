//! CLI for the narrow PDF fixture adapter.
#![forbid(unsafe_code)]

use opds_opdl_adapter::page_to_opdl;
use std::{env, error::Error, fs, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: opds-opdl-adapter INPUT.pdf OUTPUT.opdl")?;
    let output = args.next().ok_or("missing output path")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    fs::write(output, page_to_opdl(Path::new(&input))?)?;
    Ok(())
}
