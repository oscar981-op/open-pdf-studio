//! Read-only Vello-common oracle for bounded OPQ1 quadratic glyph paths.
//! OPQ1 is produced by the experimental Type0 stager, not by the app.
use fearless_simd::Level;
use std::{env, error::Error, fmt::Write, fs};
use vello_common::{
    flatten::{self, FlattenCtx},
    geometry::RectU16,
    kurbo::{Affine, BezPath},
};
use vello_cpu::{color::palette::css, Pixmap, RenderContext, Resources};

fn path(bytes: &[u8]) -> Result<BezPath, Box<dyn Error>> {
    if bytes.len() < 100 || bytes.len() > 10900 || &bytes[..4] != b"OPQ1" {
        return Err("invalid OPQ1 size or magic".into());
    }
    let u32_at = |at: usize| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
    let contours = u32_at(8) as usize;
    let count = u32_at(12) as usize;
    if u32_at(4) != 1
        || u32_at(16) != 0
        || !(1..=8).contains(&contours)
        || !(3..=272).contains(&count)
        || bytes.len() != 20 + count * 40
    {
        return Err("invalid OPQ1 header or record length".into());
    }
    let mut path = BezPath::new();
    let mut open = false;
    let mut seen_contours = 0;
    let mut segments = 0;
    for record in bytes[20..].chunks_exact(40) {
        let opcode = u32::from_le_bytes(record[..4].try_into()?);
        if record[4..8] != [0; 4] {
            return Err("nonzero OPQ1 flags".into());
        }
        let mut p = [0_f64; 4];
        for (index, value) in p.iter_mut().enumerate() {
            *value = f64::from_le_bytes(record[8 + index * 8..16 + index * 8].try_into()?);
            if !value.is_finite() || !(0.0..=4096.0).contains(value) {
                return Err("nonfinite or unbounded OPQ1 coordinate".into());
            }
        }
        match opcode {
            1 if !open && p[2..] == [0.0, 0.0] => {
                path.move_to((p[0], p[1]));
                open = true;
                seen_contours += 1;
                segments = 0;
            }
            2 if open && p[2..] == [0.0, 0.0] => {
                path.line_to((p[0], p[1]));
                segments += 1;
            }
            3 if open => {
                path.quad_to((p[0], p[1]), (p[2], p[3]));
                segments += 1;
            }
            4 if open && p == [0.0; 4] && segments > 0 => {
                path.close_path();
                open = false;
            }
            _ => return Err("invalid OPQ1 path grammar".into()),
        }
    }
    if open || seen_contours != contours {
        return Err("unclosed OPQ1 path or wrong contour count".into());
    }
    Ok(path)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: quad_flatten_oracle INPUT.opq OUTPUT.txt [OUTPUT.opm] [OUTPUT.raw]")?;
    let output = args.next().ok_or("missing output")?;
    let opm_output = args.next();
    let raw_output = args.next();
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let path = path(&fs::read(input)?)?;
    if let Some(raw_output) = raw_output {
        let mut context = RenderContext::new(256, 192);
        context.set_paint(css::BLACK);
        context.fill_path(&path);
        context.flush();
        let mut target = Pixmap::new(256, 192);
        context.render_to_pixmap(&mut Resources::new(), &mut target);
        fs::write(
            raw_output,
            target.data().iter().map(|p| p.a).collect::<Vec<_>>(),
        )?;
    }
    let mut lines = Vec::new();
    flatten::fill(
        Level::new(),
        path.elements().iter().copied(),
        Affine::IDENTITY,
        &mut lines,
        &mut FlattenCtx::default(),
        RectU16::new(0, 0, 256, 192),
    );
    let mut text = String::from("# Vello-common 0.0.8 fill flattening; x0 y0 x1 y1\n");
    let mut contours = Vec::<Vec<[f64; 2]>>::new();
    let mut current = Vec::<[f64; 2]>::new();
    for line in lines {
        let start = [f64::from(line.p0.x), f64::from(line.p0.y)];
        let end = [f64::from(line.p1.x), f64::from(line.p1.y)];
        if current.is_empty() {
            current.push(start);
        } else if current.last() != Some(&start) {
            return Err("Vello line sequence is discontinuous".into());
        }
        current.push(end);
        if current.len() > 256 {
            return Err("Vello flattened contour exceeds OPM2 limit".into());
        }
        if current.first() == current.last() {
            contours.push(std::mem::take(&mut current));
        }
        writeln!(
            text,
            "{:.12} {:.12} {:.12} {:.12}",
            line.p0.x, line.p0.y, line.p1.x, line.p1.y
        )?;
    }
    if !current.is_empty()
        || contours.is_empty()
        || contours.len() > 8
        || contours.iter().map(Vec::len).sum::<usize>() > 256
    {
        return Err("Vello flattened path exceeds OPM2 limits".into());
    }
    fs::write(output, text)?;
    if let Some(opm_output) = opm_output {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"OPM2");
        for word in [1_u32, 256, 192, contours.len() as u32] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        for contour in contours {
            bytes.extend_from_slice(&(contour.len() as u32).to_le_bytes());
            for point in contour {
                for value in point {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
            }
        }
        fs::write(opm_output, bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncated_and_open_records_rejected() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"OPQ1");
        for word in [1_u32, 1, 3, 0] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.resize(20 + 3 * 40, 0);
        assert!(path(&bytes[..bytes.len() - 1]).is_err());
        assert!(path(&bytes).is_err());
    }
}
