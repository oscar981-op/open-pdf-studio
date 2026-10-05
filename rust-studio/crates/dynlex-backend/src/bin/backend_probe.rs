use std::{env, error::Error, fs};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().collect();
    if args.len() != 3 {
        return Err("usage: backend_probe INPUT.pdf OUTPUT.rgba".into());
    }
    let bytes = fs::read(&args[1])?;
    let bitmap = opds_dynlex_backend::try_render(&bytes, 1, 1.0)?
        .ok_or("PDF is outside the bounded DynLex subset")?;
    fs::write(&args[2], bitmap.rgba)?;
    eprintln!("{}x{}", bitmap.width, bitmap.height);
    Ok(())
}
