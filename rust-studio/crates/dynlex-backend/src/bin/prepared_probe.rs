//! Diagnostic warm, in-process prepare-each-time versus retained-scene pairs.
#[cfg(feature = "kernel")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use opds_dynlex_backend::{try_render, DynlexDocument};
    use opds_engine_probe::ParsedPdf;
    use std::{sync::Arc, time::Instant};
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: prepared_probe INPUT.pdf SCALE".into());
    }
    let scale: f32 = args[2].parse()?;
    let source = Arc::new(std::fs::read(&args[1])?);
    let pdf = ParsedPdf::open(source.clone())?;
    let reference = pdf.render_page(1, scale)?;
    let document = DynlexDocument::new(source.clone());
    let started = Instant::now();
    let first = document
        .try_render(1, scale)?
        .ok_or("unsupported scene/scale")?;
    let initial_ms = started.elapsed().as_secs_f64() * 1000.0;
    if first.rgba != reference.rgba {
        return Err("initial Hayro pixel mismatch".into());
    }
    let mut pairs = Vec::new();
    for round in 0..30 {
        let mut timings = [0.0; 2];
        for mode in [round % 2, 1 - round % 2] {
            let started = Instant::now();
            let bitmap = if mode == 0 {
                try_render(&source, 1, scale)?
            } else {
                document.try_render(1, scale)?
            }
            .ok_or("unexpected fallback")?;
            timings[mode] = started.elapsed().as_secs_f64() * 1000.0;
            if bitmap.rgba != reference.rgba {
                return Err("Hayro pixel mismatch".into());
            }
        }
        pairs.push(format!(
            "{{\"fresh_ms\":{:.6},\"reused_ms\":{:.6}}}",
            timings[0], timings[1]
        ));
    }
    println!("{{\"scale\":{scale},\"rgba_bytes\":{},\"initial_ms\":{initial_ms:.6},\"all_pixels_equal\":true,\"pairs\":[{}]}}",reference.rgba.len(),pairs.join(","));
    Ok(())
}
#[cfg(not(feature = "kernel"))]
fn main() {
    eprintln!("prepared_probe requires --features kernel");
    std::process::exit(1);
}
