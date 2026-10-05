//! Read-only Vello CPU oracle for one bounded filled diamond with a hole.
//! This is a test probe; the DynLex kernel and native app do not call it.
use std::{env, error::Error, fs};
use vello_cpu::{color::palette::css, kurbo::BezPath, Pixmap, RenderContext, Resources};

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let mut number = || -> Result<i64, Box<dyn Error>> {
        Ok(args
            .next()
            .ok_or("missing diamond coordinate")?
            .to_string_lossy()
            .parse()?)
    };
    let (cx, cy, rx, ry, hx, hy) = (
        number()?,
        number()?,
        number()?,
        number()?,
        number()?,
        number()?,
    );
    let output = args.next().ok_or("missing output path")?;
    if args.next().is_some()
        || !(2..=256).contains(&rx)
        || !(2..=192).contains(&ry)
        || hx < 1
        || hy < 1
        || hx >= rx
        || hy >= ry
        || !(0..=256).contains(&cx)
        || !(0..=192).contains(&cy)
        || cx - rx < 0
        || cx + rx > 256
        || cy - ry < 0
        || cy + ry > 192
    {
        return Err("diamond parameters outside probe limits".into());
    }
    let (cx, cy, rx, ry, hx, hy) = (
        cx as f64, cy as f64, rx as f64, ry as f64, hx as f64, hy as f64,
    );
    let mut path = BezPath::new();
    path.move_to((cx, cy - ry));
    path.line_to((cx + rx, cy));
    path.line_to((cx, cy + ry));
    path.line_to((cx - rx, cy));
    path.close_path();
    path.move_to((cx, cy - hy));
    path.line_to((cx - hx, cy));
    path.line_to((cx, cy + hy));
    path.line_to((cx + hx, cy));
    path.close_path();
    let mut context = RenderContext::new(256, 192);
    context.set_paint(css::BLACK);
    context.fill_path(&path);
    context.flush();
    let mut target = Pixmap::new(256, 192);
    context.render_to_pixmap(&mut Resources::new(), &mut target);
    let coverage: Vec<u8> = target.data().iter().map(|pixel| pixel.a).collect();
    fs::write(output, coverage)?;
    Ok(())
}
