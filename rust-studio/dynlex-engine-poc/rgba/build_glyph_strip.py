#!/usr/bin/env python3
"""Compile the experimental DynLex OPM2 f32 tile-strip buffer kernel."""
import argparse
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
RASTER = r'@"strip_raster_3cstring8scene5_into_3cstring8output5_i8\*_i8\*"'


def main():
    parser = argparse.ArgumentParser()
    for name in ("compiler", "opt", "llc", "output-dir"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    compiler, opt, llc = (getattr(args, name).resolve(strict=True)
                          for name in ("compiler", "opt", "llc"))
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="opds-glyph-strip-") as folder:
        work = Path(folder)
        (work / "lib").symlink_to(compiler.parent.parent / "share/dynlex/lib", target_is_directory=True)
        for name in ("renderer.dl", "glyph_strip_kernel.dl"):
            shutil.copy2(HERE / name, work / name)
        subprocess.run([compiler, "glyph_strip_kernel.dl", "--emit-llvm", "-O0", "-o", "raw.ll"],
                       cwd=work, check=True, capture_output=True)
        ir = (work / "raw.ll").read_text()
        ir, count = re.subn(r"^define i32 @main\(\) \{.*?^\}\n", "", ir, count=1,
                            flags=re.MULTILINE | re.DOTALL)
        if count != 1:
            raise RuntimeError("DynLex main ABI changed")
        ir, count = re.subn(r"define internal i32 (" + RASTER + r")", r"define i32 \1", ir, count=1)
        if count != 1:
            raise RuntimeError("DynLex strip function ABI changed")
        symbol = re.search(RASTER, ir).group(0)
        ir += f'''
define i32 @opds_glyph_strip_raster(ptr %scene, i32 %scene_len, ptr %output, i32 %output_len) {{
entry:
  %snull = icmp eq ptr %scene, null
  %onull = icmp eq ptr %output, null
  %null = or i1 %snull, %onull
  %smin = icmp slt i32 %scene_len, 168
  %smax = icmp sgt i32 %scene_len, 4184
  %omin = icmp slt i32 %output_len, 49152
  %a = or i1 %null, %smin
  %b = or i1 %a, %smax
  %bad = or i1 %b, %omin
  br i1 %bad, label %reject, label %call
call:
  %ss = alloca ptr
  %oo = alloca ptr
  store ptr %scene, ptr %ss
  store ptr %output, ptr %oo
  %status = call i32 {symbol}(ptr %ss, ptr %oo)
  ret i32 %status
reject:
  ret i32 2
}}
'''
        (work / "wrapped.ll").write_text(ir)
        subprocess.run([opt, "-O3", "wrapped.ll", "-S", "-o", "optimized.ll"],
                       cwd=work, check=True, capture_output=True)
        subprocess.run([llc, "-filetype=obj", "-relocation-model=pic", "optimized.ll",
                        "-o", output / "glyph_strip.o"], cwd=work, check=True, capture_output=True)
    library = output / "libopds_dynlex_glyph_strip.a"
    subprocess.run(["ar", "rcs", library, output / "glyph_strip.o"], check=True)
    print(library)


if __name__ == "__main__":
    main()
