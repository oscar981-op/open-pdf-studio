//! Inspect Vello's actual flattened stroke-outline segments, no raster call.

use fearless_simd::Level;
use std::{env, error::Error, fmt::Write, fs};
use vello_common::{
    flatten::{self, FlattenCtx},
    geometry::RectU16,
    kurbo::{Affine, BezPath, Cap, Stroke, StrokeCtx},
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let name = args
        .next()
        .ok_or("usage: curve_flatten_probe arch|s-bend|shallow OUTPUT.txt")?;
    let output = args.next().ok_or("missing output path")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let controls: [(f64, f64); 4] = match name.to_str() {
        Some("arch") => [(20.5, 111.5), (35.5, 61.5), (75.5, 61.5), (90.5, 111.5)],
        Some("s-bend") => [(20.5, 111.5), (55.5, 51.5), (55.5, 171.5), (90.5, 111.5)],
        Some("shallow") => [(20.5, 111.5), (43.5, 93.5), (67.5, 93.5), (90.5, 111.5)],
        Some("asymmetric") => [(25.5, 125.5), (39.5, 48.5), (103.5, 79.5), (111.5, 133.5)],
        _ => return Err("unknown curve".into()),
    };
    let mut path = BezPath::new();
    path.move_to(controls[0]);
    path.curve_to(controls[1], controls[2], controls[3]);
    let mut lines = Vec::new();
    flatten::stroke(
        Level::new(),
        path.elements().iter().copied(),
        &Stroke::new(1.0).with_caps(Cap::Butt),
        Affine::IDENTITY,
        &mut lines,
        &mut FlattenCtx::default(),
        &mut StrokeCtx::default(),
        RectU16::new(0, 0, 256, 192),
    );
    let mut text = String::from("# Vello common 0.0.8 flattened stroke; x0 y0 x1 y1\n");
    for line in lines {
        writeln!(
            text,
            "{:.9} {:.9} {:.9} {:.9}",
            line.p0.x, line.p0.y, line.p1.x, line.p1.y
        )?;
    }
    fs::write(output, text)?;
    Ok(())
}
