use opds_dynlex_backend::Backend;
use opds_engine_probe::ParsedPdf;
use std::{env, error::Error, fs, sync::Arc};

fn main() -> Result<(), Box<dyn Error>> {
    let started = std::time::Instant::now();
    let args: Vec<_> = env::args_os().collect();
    if !(3..=6).contains(&args.len()) {
        return Err("usage: backend_probe INPUT.pdf OUTPUT.rgba [--hayro|--selected|--dynlex|--dynlex-strict|--pdfium|--pdfium-strict|--own-rust|--own-rust-strict|--pdfcraft-strict] [--scale=FACTOR] [--page=NUMBER]".into());
    }
    let scale: f32 = if args.len() >= 5 {
        args[4]
            .to_str()
            .and_then(|value| value.strip_prefix("--scale="))
            .ok_or("expected --scale=FACTOR")?
            .parse()?
    } else {
        1.0
    };
    if !scale.is_finite() || !(0.01..=8.0).contains(&scale) {
        return Err("scale must be finite and between 0.01 and 8".into());
    }
    let page: usize = if args.len() == 6 {
        args[5]
            .to_str()
            .and_then(|s| s.strip_prefix("--page="))
            .ok_or("expected --page=NUMBER")?
            .parse()?
    } else {
        1
    };
    if page == 0 {
        return Err("page must be >=1".into());
    }
    let bytes = fs::read(&args[1])?;
    let read_done = std::time::Instant::now();
    if args.len() >= 4 {
        match args[3].to_str() {
            Some("--hayro") => {
                let pdf = ParsedPdf::open(Arc::new(bytes))?;
                let open_done = std::time::Instant::now();
                let raster = pdf.render_page(page, scale)?;
                let render_done = std::time::Instant::now();
                fs::write(&args[2], raster.rgba)?;
                let write_done = std::time::Instant::now();
                if env::var_os("OPDS_AP_PROFILE").is_some() {
                    let ms = |duration: std::time::Duration| duration.as_secs_f64() * 1000.0;
                    eprintln!(
                        "OPDS_PROBE_PHASES read={:.3} open={:.3} render={:.3} write={:.3} total={:.3}",
                        ms(read_done - started),
                        ms(open_done - read_done),
                        ms(render_done - open_done),
                        ms(write_done - render_done),
                        ms(write_done - started),
                    );
                }
                eprintln!("{}x{} Hayro", raster.width, raster.height);
            }
            Some("--pdfium-strict" | "--own-rust-strict") => {
                #[cfg(not(any(feature = "pdfium", feature = "own-rust")))]
                return Err("requested strict backend not compiled".into());
                #[cfg(any(feature = "pdfium", feature = "own-rust"))]
                {
                    let raster: opds_engine_probe::Raster = match args[3].to_str() {
                        #[cfg(feature = "pdfium")]
                        Some("--pdfium-strict") => {
                            opds_pdfium_backend::render(&bytes, page, scale)?
                        }
                        #[cfg(feature = "own-rust")]
                        Some("--own-rust-strict") => opds_rust_kernel::render(&bytes, page, scale)?,
                        _ => return Err("requested strict backend not compiled".into()),
                    };
                    fs::write(&args[2], &raster.rgba)?;
                    let used = if args[3].to_str() == Some("--pdfium-strict") {
                        "Pdfium"
                    } else {
                        "OwnRust"
                    };
                    eprintln!(
                        "{}x{} PDF={} imported_AP_DynLex=false",
                        raster.width, raster.height, used
                    );
                }
            }
            Some("--pdfcraft-strict") => {
                let pdf = ParsedPdf::open(Arc::new(bytes.clone()))?;
                let result = opds_dynlex_backend::render_selected_strict(
                    &pdf,
                    &bytes,
                    page,
                    scale,
                    Backend::PdfCraft,
                )?;
                fs::write(&args[2], &result.raster.rgba)?;
                eprintln!(
                    "{}x{} PDF=PdfCraft (Hayro-based)",
                    result.raster.width, result.raster.height
                );
            }
            Some("--selected" | "--dynlex" | "--dynlex-strict" | "--pdfium" | "--own-rust") => {
                let backend = match args[3].to_str() {
                    Some("--pdfium") => Backend::Pdfium,
                    Some("--own-rust") => Backend::OwnRust,
                    _ => Backend::Dynlex,
                };
                let pdf = ParsedPdf::open(Arc::new(bytes.clone()))?;
                let open_done = std::time::Instant::now();
                let result = if args[3].to_str() == Some("--dynlex-strict") {
                    opds_dynlex_backend::DynlexDocument::new(Arc::new(bytes))
                        .render_strict(&pdf, page, scale)?
                } else {
                    opds_dynlex_backend::render_with_report(&pdf, &bytes, page, scale, backend)?
                };
                let render_done = std::time::Instant::now();
                fs::write(&args[2], result.raster.rgba)?;
                let write_done = std::time::Instant::now();
                if env::var_os("OPDS_AP_PROFILE").is_some() {
                    let ms = |duration: std::time::Duration| duration.as_secs_f64() * 1000.0;
                    eprintln!(
                        "OPDS_PROBE_PHASES read={:.3} open={:.3} render={:.3} write={:.3} total={:.3}",
                        ms(read_done - started),
                        ms(open_done - read_done),
                        ms(render_done - open_done),
                        ms(write_done - render_done),
                        ms(write_done - started),
                    );
                }
                eprintln!(
                    "{}x{} PDF={:?} imported_AP_DynLex={}",
                    result.raster.width,
                    result.raster.height,
                    result.used,
                    result.imported_annotations_dynlex
                );
            }
            _ => return Err("unsupported backend flag".into()),
        }
    } else {
        let bitmap = opds_dynlex_backend::try_render(&bytes, page, scale)?
            .ok_or("PDF is outside the bounded DynLex subset")?;
        fs::write(&args[2], bitmap.rgba)?;
        eprintln!("{}x{}", bitmap.width, bitmap.height);
    }
    Ok(())
}
