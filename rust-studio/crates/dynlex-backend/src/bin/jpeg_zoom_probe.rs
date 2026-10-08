//! App-route zoom timings: original source retained, no previous zoom bitmap reuse.
#[cfg(feature = "kernel")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use opds_dynlex_backend::DynlexDocument;
    use opds_engine_probe::ParsedPdf;
    use std::{fs, sync::Arc, time::Instant};
    let args: Vec<_> = std::env::args().collect();
    if !(5..=6).contains(&args.len()) || !matches!(args[2].as_str(), "dynlex" | "hayro") {
        return Err("usage: jpeg_zoom_probe PDF dynlex|hayro ROUNDS REFERENCE_DIRECTORY [SCALES_COMMA_SEPARATED]".into());
    }
    let scales: Vec<f32> = args
        .get(5)
        .map(String::as_str)
        .unwrap_or("0.25,1,2")
        .split(',')
        .map(str::parse)
        .collect::<Result<_, _>>()?;
    if scales.is_empty()
        || scales.len() > 8
        || scales
            .iter()
            .any(|s| !s.is_finite() || !(0.01..=8.).contains(s))
    {
        return Err("invalid bounded scales".into());
    }
    let rounds: usize = args[3].parse()?;
    if !(1..=30).contains(&rounds) {
        return Err("rounds outside 1..30".into());
    }
    let started = Instant::now();
    let bytes = Arc::new(fs::read(&args[1])?);
    let pdf = ParsedPdf::open(bytes.clone())?;
    let document = DynlexDocument::new(bytes);
    let open_ms = started.elapsed().as_secs_f64() * 1000.;
    let mut rows = Vec::new();
    for round in 0..=rounds {
        for (index, scale) in scales.iter().copied().enumerate() {
            let started = Instant::now();
            let rgba = if args[2] == "dynlex" {
                document
                    .try_render(1, scale)?
                    .ok_or("unsupported; no fallback")?
                    .rgba
            } else {
                pdf.render_page(1, scale)?.rgba
            };
            let ms = started.elapsed().as_secs_f64() * 1000.;
            // Byte comparison and file I/O excluded from the render timer.
            let expected = fs::read(std::path::Path::new(&args[4]).join(format!("{index}.rgba")))?;
            if rgba != expected {
                return Err(format!("RGBA mismatch at round {round} scale {scale}").into());
            }
            rows.push(format!(
                "{{\"round\":{round},\"scale\":{scale},\"ms\":{ms:.6}}}"
            ));
        }
    }
    let (retained, decodes) = document.raster_cache_stats()?;
    println!("{{\"mode\":\"{}\",\"open_ms\":{open_ms:.6},\"retained_rgb_bytes\":{retained},\"decodes\":{decodes},\"all_bytes_equal\":true,\"samples\":[{}]}}",args[2],rows.join(","));
    Ok(())
}
#[cfg(not(feature = "kernel"))]
fn main() {
    eprintln!("jpeg_zoom_probe requires the kernel feature");
    std::process::exit(1);
}
