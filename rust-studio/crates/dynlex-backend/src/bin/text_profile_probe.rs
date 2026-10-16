//! Split timings on a validated resource-bounded worker scene, no PDF parser here.
#[cfg(all(feature = "kernel", target_os = "linux"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::{io::Read, time::Instant};
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 5 {
        return Err("usage: text_profile_probe PDF PAGE SCALE REFERENCE.rgba".into());
    }
    let mut source = Vec::new();
    std::fs::File::open(&a[1])?
        .take(16_000_001)
        .read_to_end(&mut source)?;
    let start = Instant::now();
    let scene = opds_dynlex_backend::text_worker::prepare(
        &std::env::current_exe()?.with_file_name("opds-text-worker"),
        &source,
        a[2].parse()?,
        a[3].parse()?,
    )?;
    let prepare_ms = start.elapsed().as_secs_f64() * 1000.;
    let reference = std::fs::read(&a[4])?;
    let mut samples = Vec::new();
    for _ in 0..10 {
        let start = Instant::now();
        let (bitmap, stages) = match &scene {
            opds_dynlex_backend::text_worker::PreparedScene::Text(_) => {
                let (bitmap, stages) = scene.profile_replay()?;
                (bitmap, format!("{stages:?}"))
            }
            opds_dynlex_backend::text_worker::PreparedScene::Hybrid(_) => {
                (scene.render()?, "null".to_owned())
            }
        };
        let total = start.elapsed().as_secs_f64() * 1000.;
        if bitmap.rgba != reference {
            return Err("profile bitmap differs from reference".into());
        }
        samples.push(format!("{{\"total_ms\":{total},\"stages_ms\":{stages}}}"));
    }
    println!(
        "{{\"prepare_ms\":{prepare_ms},\"paints\":{},\"retained_bytes\":{},\"samples\":[{}]}}",
        scene.paint_count(),
        scene.retained_bytes(),
        samples.join(",")
    );
    Ok(())
}
#[cfg(not(all(feature = "kernel", target_os = "linux")))]
fn main() {
    eprintln!("requires kernel and Linux");
    std::process::exit(1);
}
