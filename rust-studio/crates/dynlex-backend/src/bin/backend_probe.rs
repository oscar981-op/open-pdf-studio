use opds_dynlex_backend::Backend;
use opds_engine_probe::ParsedPdf;
use std::{env, error::Error, fs, sync::Arc};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().collect();
    if args.len() != 3 && args.len() != 4 {
        return Err("usage: backend_probe INPUT.pdf OUTPUT.rgba [--hayro|--selected]".into());
    }
    let bytes = fs::read(&args[1])?;
    if args.len() == 4 {
        match args[3].to_str() {
            Some("--hayro") => {
                let pdf = ParsedPdf::open(Arc::new(bytes))?;
                let raster = pdf.render_page(1, 1.0)?;
                fs::write(&args[2], raster.rgba)?;
                eprintln!("{}x{} Hayro", raster.width, raster.height);
            }
            Some("--selected") => {
                let pdf = ParsedPdf::open(Arc::new(bytes.clone()))?;
                let result =
                    opds_dynlex_backend::render_with_report(&pdf, &bytes, 1, 1.0, Backend::Dynlex)?;
                fs::write(&args[2], result.raster.rgba)?;
                eprintln!(
                    "{}x{} PDF={:?} imported_AP_DynLex={}",
                    result.raster.width,
                    result.raster.height,
                    result.used,
                    result.imported_annotations_dynlex
                );
            }
            _ => return Err("optional flag must be --hayro or --selected".into()),
        }
    } else {
        let bitmap = opds_dynlex_backend::try_render(&bytes, 1, 1.0)?
            .ok_or("PDF is outside the bounded DynLex subset")?;
        fs::write(&args[2], bitmap.rgba)?;
        eprintln!("{}x{}", bitmap.width, bitmap.height);
    }
    Ok(())
}
