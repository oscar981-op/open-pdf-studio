//! Explicit subprocess preparation experiment; no native renderer dispatch.
#[cfg(all(feature = "kernel", target_os = "linux"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 5 {
        return Err("usage: text_worker_probe INPUT.pdf PAGE SCALE OUTPUT.rgba".into());
    }
    let mut pdf = Vec::new();
    std::fs::File::open(&args[1])?
        .take(16_000_001)
        .read_to_end(&mut pdf)?;
    let worker = std::env::current_exe()?.with_file_name("truetype_page_probe");
    let document =
        opds_dynlex_backend::text_worker::TextDocument::new(std::sync::Arc::new(pdf), worker)?;
    let page = args[2].parse()?;
    let scale = args[3].parse()?;
    let start = std::time::Instant::now();
    let bitmap = document.render(page, scale)?;
    let first_render_ms = start.elapsed().as_secs_f64() * 1000.;
    let mut warm_ms = Vec::new();
    for _ in 0..2 {
        let start = std::time::Instant::now();
        let replay = document.render(page, scale)?;
        warm_ms.push(start.elapsed().as_secs_f64() * 1000.);
        if replay.rgba != bitmap.rgba {
            return Err("cached scene replay differs".into());
        }
    }
    let (preparations, retained_bytes) = document.cache_stats()?;
    if preparations != 1 {
        return Err("scene prepared repeatedly".into());
    }
    std::fs::write(&args[4], &bitmap.rgba)?;
    eprintln!("{{\"width\":{},\"height\":{},\"preparations\":{preparations},\"retained_bytes\":{retained_bytes},\"first_render_ms\":{first_render_ms},\"warm_ms\":{warm_ms:?}}}", bitmap.width, bitmap.height);
    Ok(())
}
#[cfg(not(all(feature = "kernel", target_os = "linux")))]
fn main() {
    eprintln!("requires kernel feature and Linux");
    std::process::exit(1);
}
