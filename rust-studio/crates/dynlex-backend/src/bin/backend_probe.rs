use opds_dynlex_backend::Backend;
use opds_engine_probe::ParsedPdf;
use std::{env, error::Error, fs, sync::Arc};

fn main() -> Result<(), Box<dyn Error>> {
    let started = std::time::Instant::now();
    let args: Vec<_> = env::args_os().collect();
    if args.len() != 3 && args.len() != 4 {
        return Err("usage: backend_probe INPUT.pdf OUTPUT.rgba [--hayro|--selected|--dynlex|--pdfium|--own-rust]".into());
    }
    let bytes = fs::read(&args[1])?;
    let read_done = std::time::Instant::now();
    if args.len() == 4 {
        match args[3].to_str() {
            Some("--hayro") => {
                let pdf = ParsedPdf::open(Arc::new(bytes))?;
                let open_done = std::time::Instant::now();
                let raster = pdf.render_page(1, 1.0)?;
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
            Some("--selected" | "--dynlex" | "--pdfium" | "--own-rust") => {
                let backend = match args[3].to_str() {
                    Some("--pdfium") => Backend::Pdfium,
                    Some("--own-rust") => Backend::OwnRust,
                    _ => Backend::Dynlex,
                };
                let pdf = ParsedPdf::open(Arc::new(bytes.clone()))?;
                let open_done = std::time::Instant::now();
                let result =
                    opds_dynlex_backend::render_with_report(&pdf, &bytes, 1, 1.0, backend)?;
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
        let bitmap = opds_dynlex_backend::try_render(&bytes, 1, 1.0)?
            .ok_or("PDF is outside the bounded DynLex subset")?;
        fs::write(&args[2], bitmap.rgba)?;
        eprintln!("{}x{}", bitmap.width, bitmap.height);
    }
    Ok(())
}
