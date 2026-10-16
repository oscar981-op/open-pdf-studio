//! Independent DynLex coverage experiment. OPNW is not native PDF support.
#[cfg(feature = "kernel")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Read;
    let a: Vec<_> = std::env::args().collect();
    if a.len() != 3 {
        return Err("usage: winding_probe INPUT.opnw OUTPUT.coverage".into());
    }
    let mut bytes = Vec::new();
    std::fs::File::open(&a[1])?
        .take(4149)
        .read_to_end(&mut bytes)?;
    let output = opds_dynlex_backend::experimental_winding::coverage(&bytes)?;
    std::fs::write(&a[2], output)?;
    Ok(())
}
#[cfg(not(feature = "kernel"))]
fn main() {
    eprintln!("requires kernel feature");
    std::process::exit(1);
}
