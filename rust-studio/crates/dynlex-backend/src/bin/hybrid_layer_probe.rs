//! Trusted-fixture research: replay a validated text scene over an opaque
//! image-only DynLex candidate. This does not parse or accept hybrid PDFs.
use opds_dynlex_backend::{
    hybrid_page::PreparedHybridPage, winding_page::PreparedWindingPage, Bitmap,
};
use std::{error::Error, fs};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: hybrid_layer_probe text.pwpg image.rgba combined.rgba".into());
    }
    let scene = fs::read(&args[1])?;
    let page = PreparedWindingPage::decode(&scene)?;
    let [width, height] = page.dimensions();
    let image = fs::read(&args[2])?;
    let hybrid = PreparedHybridPage::new(
        Bitmap {
            width,
            height,
            rgba: image,
        },
        page,
    )?;
    // Exercise the same versioned, length-checked parent IPC boundary that
    // the bounded worker will use. No PDF interpretation happens here.
    let output = PreparedHybridPage::decode(&hybrid.encode()?)?.render()?;
    fs::write(&args[3], output.rgba)?;
    eprintln!("{}x{} validated hybrid IPC", width, height);
    Ok(())
}
