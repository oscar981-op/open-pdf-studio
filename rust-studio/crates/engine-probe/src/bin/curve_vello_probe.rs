//! Isolated Vello CPU stroke probe. No PDF adapter or application routing.

use std::{env, error::Error, fs};
use vello_cpu::{Pixmap, RenderContext, Resources};
use vello_cpu::{
    color::palette::css,
    kurbo::{BezPath, Cap, Stroke},
};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let name = args
        .next()
        .ok_or("usage: curve_vello_probe arch|s-bend|shallow OUTPUT.raw")?;
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
    let mut context = RenderContext::new(256, 192);
    context.set_paint(css::BLACK);
    context.set_stroke(Stroke::new(1.0).with_caps(Cap::Butt));
    context.stroke_path(&path);
    context.flush();
    let mut target = Pixmap::new(256, 192);
    context.render_to_pixmap(&mut Resources::new(), &mut target);
    let coverage: Vec<u8> = target.data().iter().map(|pixel| pixel.a).collect();
    fs::write(output, coverage)?;
    Ok(())
}
