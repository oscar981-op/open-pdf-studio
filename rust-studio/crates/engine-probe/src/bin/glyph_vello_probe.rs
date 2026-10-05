//! Read-only Vello CPU oracle for one bounded filled diamond or OPM2 glyph.
//! This is a test probe; the DynLex kernel and native app do not call it.
use std::{env, error::Error, fs};
use vello_cpu::{color::palette::css, kurbo::BezPath, Pixmap, RenderContext, Resources};

fn opm_path(bytes: &[u8]) -> Result<BezPath, Box<dyn Error>> {
    if bytes.len() < 20 || bytes.len() > 4148 || &bytes[..4] != b"OPM2" {
        return Err("invalid OPM2 length or magic".into());
    }
    let word = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let count = word(16) as usize;
    if word(4) != 1 || word(8) != 256 || word(12) != 192 || !(1..=8).contains(&count) {
        return Err("unsupported OPM2 header".into());
    }
    let mut path = BezPath::new();
    let mut at = 20;
    let mut total = 0;
    for _ in 0..count {
        if at + 4 > bytes.len() {
            return Err("truncated contour".into());
        }
        let points = word(at) as usize;
        at += 4;
        total += points;
        if !(4..=256).contains(&points) || total > 256 || at + points * 16 > bytes.len() {
            return Err("invalid OPM2 point count".into());
        }
        let mut first = None;
        let mut last = None;
        for index in 0..points {
            let x = f64::from_le_bytes(bytes[at..at + 8].try_into()?);
            let y = f64::from_le_bytes(bytes[at + 8..at + 16].try_into()?);
            at += 16;
            if !x.is_finite()
                || !y.is_finite()
                || !(0.0..=4096.0).contains(&x)
                || !(0.0..=4096.0).contains(&y)
            {
                return Err("unbounded OPM2 point".into());
            }
            if index == 0 {
                first = Some((x, y));
                path.move_to((x, y));
            } else if index + 1 != points {
                path.line_to((x, y));
            }
            last = Some((x, y));
        }
        if first != last {
            return Err("open OPM2 contour".into());
        }
        path.close_path();
    }
    if at != bytes.len() {
        return Err("trailing OPM2 data".into());
    }
    Ok(path)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    if let Some(mode) = args.next() {
        if mode == "--opm" {
            let input = args.next().ok_or("missing OPM2 input")?;
            let output = args.next().ok_or("missing output")?;
            if args.next().is_some() {
                return Err("extra argument".into());
            }
            let path = opm_path(&fs::read(input)?)?;
            return render(&path, output);
        }
        // The legacy numeric diamond probe is still used by prior diagnostics.
        let mut all = vec![mode];
        all.extend(args);
        return diamond(all.into_iter());
    }
    Err("missing arguments".into())
}

fn diamond(mut args: impl Iterator<Item = std::ffi::OsString>) -> Result<(), Box<dyn Error>> {
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
    render(&path, output)
}

fn render(path: &BezPath, output: std::ffi::OsString) -> Result<(), Box<dyn Error>> {
    let mut context = RenderContext::new(256, 192);
    context.set_paint(css::BLACK);
    context.fill_path(path);
    context.flush();
    let mut target = Pixmap::new(256, 192);
    context.render_to_pixmap(&mut Resources::new(), &mut target);
    let coverage: Vec<u8> = target.data().iter().map(|pixel| pixel.a).collect();
    fs::write(output, coverage)?;
    Ok(())
}
