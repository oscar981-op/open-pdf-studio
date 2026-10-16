//! Application-owned text scene preparation helper; launched with OS limits.
#[cfg(all(feature = "kernel", target_os = "linux"))]
#[path = "truetype_page_probe.rs"]
mod frontend;
#[cfg(all(feature = "kernel", target_os = "linux"))]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 6 || args[1] != "-" || args[4] != "-" || args[5] != "--scene-worker" {
        return Err("opds-text-worker only accepts the application stdin/scene protocol".into());
    }
    frontend::main()
}
#[cfg(not(all(feature = "kernel", target_os = "linux")))]
fn main() {
    eprintln!("text worker requires Linux and kernel feature");
    std::process::exit(1);
}
