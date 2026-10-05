use opds_engine_probe::ParsedPdf;
use std::{env, error::Error, fs, sync::Arc};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().collect();
    if args.len() != 3 && args.len() != 4 {
        return Err("usage: backend_probe INPUT.pdf OUTPUT.rgba [--hayro]".into());
    }
    let bytes = fs::read(&args[1])?;
    if args.len() == 4 {
        if args[3] != "--hayro" {
            return Err("only --hayro is supported as an optional flag".into());
        }
        let pdf = ParsedPdf::open(Arc::new(bytes))?;
        let raster = pdf.render_page(1, 1.0)?;
        fs::write(&args[2], raster.rgba)?;
        eprintln!("{}x{}", raster.width, raster.height);
    } else {
        let bitmap = opds_dynlex_backend::try_render(&bytes, 1, 1.0)?
            .ok_or("PDF is outside the bounded DynLex subset")?;
        fs::write(&args[2], bitmap.rgba)?;
        eprintln!("{}x{}", bitmap.width, bitmap.height);
    }
    Ok(())
}
