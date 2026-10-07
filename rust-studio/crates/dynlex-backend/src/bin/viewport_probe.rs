//! Warm tile-pass timing; independent Hayro comparisons are outside the timer.
#[cfg(feature = "kernel")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use opds_dynlex_backend::{PixelViewport, PreparedPage};
    use opds_engine_probe::ParsedPdf;
    use std::{sync::Arc, time::Instant};
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: viewport_probe INPUT.pdf SCALE TILE_SIZE".into());
    }
    let scale: f32 = args[2].parse()?;
    let tile: u32 = args[3].parse()?;
    if !(8..=4096).contains(&tile) {
        return Err("tile must be 8..4096".into());
    }
    let source = Arc::new(std::fs::read(&args[1])?);
    let pdf = ParsedPdf::open(source.clone())?;
    let reference = pdf.render_page(1, scale)?;
    let started = Instant::now();
    let prepared = PreparedPage::from_pdf(&source)?.ok_or("unsupported PDF")?;
    let preparation_ms = started.elapsed().as_secs_f64() * 1000.0;
    let mut passes = Vec::new();
    for _ in 0..15 {
        let mut elapsed = 0.0;
        for y in (0..reference.height).step_by(tile as usize) {
            for x in (0..reference.width).step_by(tile as usize) {
                let view = PixelViewport {
                    x,
                    y,
                    width: tile.min(reference.width - x),
                    height: tile.min(reference.height - y),
                };
                let started = Instant::now();
                let bitmap = prepared
                    .render_viewport(scale, view)?
                    .ok_or("unsupported viewport")?;
                elapsed += started.elapsed().as_secs_f64() * 1000.0;
                for row in 0..view.height {
                    let actual = (row * view.width * 4) as usize;
                    let expected = ((row + y) * reference.width * 4 + x * 4) as usize;
                    if bitmap.rgba[actual..actual + (view.width * 4) as usize]
                        != reference.rgba[expected..expected + (view.width * 4) as usize]
                    {
                        return Err("Hayro tile pixel mismatch".into());
                    }
                }
            }
        }
        passes.push(format!("{elapsed:.6}"));
    }
    println!("{{\"scale\":{scale},\"tile\":{tile},\"width\":{},\"height\":{},\"preparation_ms\":{preparation_ms:.6},\"exact\":true,\"pass_ms\":[{}]}}",reference.width,reference.height,passes.join(","));
    Ok(())
}
#[cfg(not(feature = "kernel"))]
fn main() {
    eprintln!("requires --features kernel");
    std::process::exit(1);
}
