//! Read-only Kurbo stroke-outline probe for bounded cubic test fixtures.

use std::{env, error::Error, fmt::Write, fs};
use vello_cpu::kurbo::{stroke_with, BezPath, Cap, PathEl, Stroke, StrokeCtx, StrokeOpts};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let name = args
        .next()
        .ok_or("usage: curve_outline_probe arch|s-bend|shallow OUTPUT.txt")?;
    let output = args.next().ok_or("missing output path")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let controls: [(f64, f64); 4] = match name.to_str() {
        Some("arch") => [(20.5, 111.5), (35.5, 61.5), (75.5, 61.5), (90.5, 111.5)],
        Some("s-bend") => [(20.5, 111.5), (55.5, 51.5), (55.5, 171.5), (90.5, 111.5)],
        Some("shallow") => [(20.5, 111.5), (43.5, 93.5), (67.5, 93.5), (90.5, 111.5)],
        _ => return Err("unknown curve".into()),
    };
    let mut path = BezPath::new();
    path.move_to(controls[0]);
    path.curve_to(controls[1], controls[2], controls[3]);
    let mut context = StrokeCtx::default();
    stroke_with(
        path.elements().iter().copied(),
        &Stroke::new(1.0).with_caps(Cap::Butt),
        &StrokeOpts::default(),
        0.25,
        &mut context,
    );
    let mut text = String::from("# Kurbo 0.13.1 stroke_with; width=1 cap=butt tolerance=0.25\n");
    for element in context.output().elements() {
        match element {
            PathEl::MoveTo(p) => writeln!(text, "M {:.12} {:.12}", p.x, p.y)?,
            PathEl::LineTo(p) => writeln!(text, "L {:.12} {:.12}", p.x, p.y)?,
            PathEl::QuadTo(p, q) => {
                writeln!(text, "Q {:.12} {:.12} {:.12} {:.12}", p.x, p.y, q.x, q.y)?
            }
            PathEl::CurveTo(p, q, r) => writeln!(
                text,
                "C {:.12} {:.12} {:.12} {:.12} {:.12} {:.12}",
                p.x, p.y, q.x, q.y, r.x, r.y
            )?,
            PathEl::ClosePath => text.push_str("Z\n"),
        }
    }
    fs::write(output, text)?;
    Ok(())
}
