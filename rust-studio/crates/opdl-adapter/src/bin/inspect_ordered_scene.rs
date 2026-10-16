//! Read-only diagnostic for strict ordered image/vector scene preparation.
use std::{error::Error, fs, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let path = std::env::args()
        .nth(1)
        .ok_or("usage: inspect_ordered_scene PDF [page]")?;
    let page = std::env::args()
        .nth(2)
        .map(|n| n.parse())
        .transpose()?
        .unwrap_or(1);
    let bytes = fs::read(Path::new(&path))?;
    let scene = opds_opdl_adapter::page_to_opdl_numbered_bytes(&bytes, page, true)?;
    println!("ordered scene: {} bytes", scene.len());
    Ok(())
}
