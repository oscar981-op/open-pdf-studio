//! Read-only Vello CPU oracle for the experimental OPGS glyph-run scene.
//! This diagnostic is never used by the DynLex kernel or native app.
use std::{env, error::Error, fs};
use vello_cpu::{
    color::{palette::css, AlphaColor, Srgb},
    kurbo::{BezPath, Rect, Shape},
    Pixmap, RenderContext, Resources,
};

fn u32_at(bytes: &[u8], at: usize) -> Result<u32, Box<dyn Error>> {
    Ok(u32::from_le_bytes(
        bytes.get(at..at + 4).ok_or("truncated word")?.try_into()?,
    ))
}

fn path(bytes: &[u8]) -> Result<BezPath, Box<dyn Error>> {
    if bytes.len() < 88
        || bytes.len() > 4148
        || &bytes[..4] != b"OPM2"
        || u32_at(bytes, 4)? != 1
        || u32_at(bytes, 8)? != 256
        || u32_at(bytes, 12)? != 192
    {
        return Err("invalid nested OPM2 header".into());
    }
    let count = u32_at(bytes, 16)? as usize;
    if !(1..=8).contains(&count) {
        return Err("invalid contour count".into());
    }
    let mut result = BezPath::new();
    let mut at = 20;
    let mut total = 0;
    for _ in 0..count {
        let points = u32_at(bytes, at)? as usize;
        at += 4;
        total += points;
        if !(4..=256).contains(&points) || total > 256 || at + points * 16 > bytes.len() {
            return Err("invalid contour length".into());
        }
        let mut first = None;
        let mut last = None;
        for index in 0..points {
            let x = f64::from_le_bytes(bytes[at..at + 8].try_into()?);
            let y = f64::from_le_bytes(bytes[at + 8..at + 16].try_into()?);
            at += 16;
            if !x.is_finite()
                || !y.is_finite()
                || !(0.0..=256.0).contains(&x)
                || !(0.0..=192.0).contains(&y)
            {
                return Err("invalid contour coordinate".into());
            }
            if index == 0 {
                first = Some((x, y));
                result.move_to((x, y));
            } else if index + 1 != points {
                result.line_to((x, y));
            }
            last = Some((x, y));
        }
        if first != last {
            return Err("open contour".into());
        }
        result.close_path();
    }
    if at != bytes.len() {
        return Err("trailing contour data".into());
    }
    Ok(result)
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let input = args
        .next()
        .ok_or("usage: glyph_run_vello_oracle INPUT.opgs OUTPUT.rgba")?;
    let output = args.next().ok_or("missing output")?;
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let bytes = fs::read(input)?;
    if bytes.len() < 20
        || bytes.len() > 140_000
        || &bytes[..4] != b"OPGS"
        || u32_at(&bytes, 4)? != 1
        || u32_at(&bytes, 8)? != 256
        || u32_at(&bytes, 12)? != 192
        || !(2..=32).contains(&u32_at(&bytes, 16)?)
    {
        return Err("invalid OPGS header".into());
    }
    let count = u32_at(&bytes, 16)? as usize;
    let mut context = RenderContext::new(256, 192);
    context.set_paint(css::WHITE);
    context.fill_rect(&Rect::new(0.0, 0.0, 256.0, 192.0));
    context.push_clip_path(&Rect::new(0.0, 0.0, 256.0, 192.0).to_path(0.1));
    context.push_layer(None, None, Some(1.0), None, None);
    let mut at = 20;
    for _ in 0..count {
        if at + 44 > bytes.len() {
            return Err("truncated OPGS record".into());
        }
        let cid = u16::from_le_bytes(bytes[at + 32..at + 34].try_into()?);
        let gid = u16::from_le_bytes(bytes[at + 34..at + 36].try_into()?);
        if cid != gid || bytes[at + 39] != 0 {
            return Err("unsupported OPGS glyph mapping or flags".into());
        }
        let color = &bytes[at + 36..at + 39];
        context.set_paint(AlphaColor::<Srgb>::from_rgb8(color[0], color[1], color[2]));
        let length = u32_at(&bytes, at + 40)? as usize;
        at += 44;
        if length > 4148 || at + length > bytes.len() {
            return Err("invalid nested OPM2 length".into());
        }
        context.fill_path(&path(&bytes[at..at + length])?);
        at += length;
    }
    if at != bytes.len() {
        return Err("trailing OPGS bytes".into());
    }
    context.pop_layer();
    context.pop_clip_path();
    context.flush();
    let mut pixmap = Pixmap::new(256, 192);
    context.render_to_pixmap(&mut Resources::new(), &mut pixmap);
    let rgba: Vec<u8> = pixmap
        .data()
        .iter()
        .flat_map(|pixel| [pixel.r, pixel.g, pixel.b, pixel.a])
        .collect();
    fs::write(output, rgba)?;
    Ok(())
}
