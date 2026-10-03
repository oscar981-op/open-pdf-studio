//! Experimental v4→verified v2 axis-stroke bridge for DynLex comparison.
#![forbid(unsafe_code)]

use opds_opdl_adapter::paint_v4::DisplayList;
use std::{env, error::Error, fs};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: bridge_v4_axis INPUT-v4.opdl OUTPUT-v2.opdl")?;
    let output = args.next().ok_or("missing output path")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let list = DisplayList::decode(&fs::read(input)?)?;
    fs::write(output, list.to_v2_axis_strokes()?)?;
    Ok(())
}
