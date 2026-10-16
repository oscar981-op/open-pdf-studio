#[cfg(feature = "kernel")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use opds_dynlex_backend::fractional_probe::FractionalProbe;
    use std::{env, fs, io::Read, sync::Arc};
    let args: Vec<_> = env::args().collect();
    if args.len() != 5 {
        return Err("usage: fractional_jpeg_probe INPUT.pdf OUTPUT_DIRECTORY SCALE[,SCALE...] geometric|strip-f32|unverified-tiles".into());
    }
    let scales: Vec<f32> = args[3]
        .split(',')
        .map(str::parse)
        .collect::<Result<_, _>>()?;
    if scales.is_empty() || scales.len() > 32 {
        return Err("expected 1..32 scales".into());
    }
    let strip = match args[4].as_str() {
        "geometric" => false,
        "strip-f32" => true,
        "unverified-tiles" => false,
        _ => return Err("invalid sampling mode".into()),
    };
    if scales
        .iter()
        .any(|s| !s.is_finite() || !(0.01..=8.).contains(s))
    {
        return Err("scale must be finite and within 0.01..8".into());
    }
    if fs::metadata(&args[1])?.len() > 16 * 1024 * 1024 {
        return Err("source exceeds 16 MiB limit".into());
    }
    let mut bytes = Vec::new();
    fs::File::open(&args[1])?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    let probe = FractionalProbe::new(Arc::new(bytes), 1)?;
    fs::create_dir_all(&args[2])?;
    for (i, scale) in scales.into_iter().enumerate() {
        let started = std::time::Instant::now();
        let bitmap = if args[4] == "unverified-tiles" {
            probe.render_unverified_tiled(scale)?
        } else {
            probe.render(scale, strip)?
        };
        let ms = started.elapsed().as_secs_f64() * 1000.;
        fs::write(
            std::path::Path::new(&args[2]).join(format!("{i}.rgba")),
            &bitmap.rgba,
        )?;
        println!(
            "scale={scale} {}x{} ms={ms:.3} decoded_bytes={}",
            bitmap.width,
            bitmap.height,
            probe.decoded_bytes()
        );
    }
    Ok(())
}
#[cfg(not(feature = "kernel"))]
fn main() {
    eprintln!("fractional probe requires kernel feature");
    std::process::exit(1);
}
