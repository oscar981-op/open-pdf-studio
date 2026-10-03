//! Experimental PDF stroke export; not a renderer or app route.
#![forbid(unsafe_code)]

use opds_opdl_adapter::{page_to_v4_strokes, paint_v4::DisplayList};
use std::{env, error::Error, fs, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: export_strokes_v4 INPUT.pdf OUTPUT.opdl")?;
    let output = args.next().ok_or("missing output path")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let bytes = page_to_v4_strokes(Path::new(&input))?;
    let list = DisplayList::decode(&bytes)?;
    fs::write(output, bytes)?;
    println!(
        "{} records on {}x{} page",
        list.commands.len(),
        list.width,
        list.height
    );
    Ok(())
}
