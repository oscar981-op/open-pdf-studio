//! Command line harness for the reusable Rust PDF engine.
#![forbid(unsafe_code)]
use opds_engine_probe::render_page;
use std::path::PathBuf;

fn main() {
    if let Err(error) = run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let path = PathBuf::from(
        args.next()
            .ok_or("usage: opds-engine-probe INPUT.pdf PAGE(1-based) OUT.png [scale]")?,
    );
    let page_number: usize = args
        .next()
        .ok_or("missing page")?
        .to_string_lossy()
        .parse()?;
    let output = PathBuf::from(args.next().ok_or("missing output PNG")?);
    let scale: f32 = args
        .next()
        .map(|value| value.to_string_lossy().parse())
        .transpose()?
        .unwrap_or(1.0);
    if args.next().is_some() {
        return Err("extra argument".into());
    }
    let raster = render_page(std::fs::read(&path)?, page_number, scale)?;
    let image = image::RgbaImage::from_raw(raster.width, raster.height, raster.rgba)
        .ok_or("invalid output dimensions")?;
    image.save(&output)?;
    println!("file={:?} page={} total_pages={} pt={}x{} pixels={}x{} parse_ms={} render_ms={} output={:?}",
        path, page_number, raster.total_pages, raster.page_width_pt, raster.page_height_pt,
        raster.width, raster.height, raster.parse_ms, raster.render_ms, output);
    Ok(())
}
