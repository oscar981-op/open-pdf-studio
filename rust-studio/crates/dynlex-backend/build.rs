use std::{env, path::PathBuf, process::Command};

fn main() {
    if env::var_os("CARGO_FEATURE_KERNEL").is_none() {
        return;
    }
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"))
        .join("../../dynlex-engine-poc/rgba");
    let output = PathBuf::from(env::var_os("OUT_DIR").expect("out dir"));
    let home = PathBuf::from(env::var_os("HOME").expect("HOME is required for DynLex toolchain"));
    let cache = home.join(".cache/opds-toolchains");
    let compiler = env::var_os("OPDS_DYNLEX_COMPILER")
        .map(PathBuf::from)
        .unwrap_or_else(|| cache.join("dynlex-0.0.1/dynlex/bin/dynlex"));
    let llvm = cache.join("llvm-20/root/usr/lib/llvm-20/bin");
    let opt = env::var_os("OPDS_LLVM_OPT")
        .map(PathBuf::from)
        .unwrap_or_else(|| llvm.join("opt"));
    let llc = env::var_os("OPDS_LLVM_LLC")
        .map(PathBuf::from)
        .unwrap_or_else(|| llvm.join("llc"));
    for name in ["OPDS_DYNLEX_COMPILER", "OPDS_LLVM_OPT", "OPDS_LLVM_LLC"] {
        println!("cargo:rerun-if-env-changed={name}");
    }
    for name in [
        "build_buffer.py",
        "buffer_kernel.dl",
        "build_annotation.py",
        "annotation_kernel.dl",
        "build_glyph_strip.py",
        "glyph_strip_kernel.dl",
        "glyph_run_core.rs",
        "glyph_multi_validation.rs",
        "glyph_multi_probe.rs",
        "curve_scene.rs",
        "renderer.dl",
    ] {
        println!("cargo:rerun-if-changed={}", root.join(name).display());
    }
    let status = Command::new("python3")
        .arg(root.join("build_buffer.py"))
        .args([
            "--compiler",
            compiler.to_str().expect("compiler path"),
            "--opt",
            opt.to_str().expect("opt path"),
            "--llc",
            llc.to_str().expect("llc path"),
            "--output-dir",
            output.to_str().expect("output path"),
        ])
        .status()
        .expect("start DynLex kernel build");
    assert!(
        status.success(),
        "DynLex kernel build failed; see dynlex-engine-poc/README.md"
    );
    let annotation_status = Command::new("python3")
        .arg(root.join("build_annotation.py"))
        .args([
            "--compiler",
            compiler.to_str().expect("compiler path"),
            "--opt",
            opt.to_str().expect("opt path"),
            "--llc",
            llc.to_str().expect("llc path"),
            "--output-dir",
            output.to_str().expect("output path"),
        ])
        .status()
        .expect("start DynLex annotation build");
    assert!(
        annotation_status.success(),
        "DynLex annotation build failed"
    );
    let glyph_status = Command::new("python3")
        .arg(root.join("build_glyph_strip.py"))
        .args([
            "--compiler",
            compiler.to_str().expect("compiler path"),
            "--opt",
            opt.to_str().expect("opt path"),
            "--llc",
            llc.to_str().expect("llc path"),
            "--output-dir",
            output.to_str().expect("output path"),
        ])
        .status()
        .expect("start DynLex glyph-strip build");
    assert!(glyph_status.success(), "DynLex glyph-strip build failed");
    println!("cargo:rustc-link-search=native={}", output.display());
    println!("cargo:rustc-link-lib=static=opds_dynlex_buffer");
    println!("cargo:rustc-link-lib=static=opds_dynlex_annotation");
    println!("cargo:rustc-link-lib=static=opds_dynlex_glyph_strip");
}
