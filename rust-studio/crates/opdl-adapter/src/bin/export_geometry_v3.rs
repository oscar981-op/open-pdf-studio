use std::{env, error::Error, fs, path::Path};
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().collect();
    if args.len() != 3 {
        return Err("usage: export_geometry_v3 INPUT.pdf OUTPUT.opgm".into());
    }
    let bytes = opds_opdl_adapter::geometry_v3::page_to_opgm_v3(Path::new(&args[1]))?;
    fs::write(&args[2], bytes)?;
    Ok(())
}
