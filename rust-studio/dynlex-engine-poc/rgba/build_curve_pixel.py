#!/usr/bin/env python3
"""Compile experimental one-pixel DynLex polygon area kernel."""
import argparse
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
INNER = r'@"area_3cstring8scene5_at_3integer8pixel_x5_and_3integer8pixel_y5_into_3cstring8result5_i8\*_i32_i32_i8\*"'
RASTER = r'@"raster_3cstring8scene5_into_3cstring8output5_i8\*_i8\*"'


def main():
    parser = argparse.ArgumentParser()
    for name in ("compiler", "opt", "llc", "output-dir"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    compiler, opt, llc = (getattr(args, name).resolve(strict=True) for name in ("compiler", "opt", "llc"))
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="opds-dynlex-curve-pixel-") as folder:
        work = Path(folder)
        (work / "lib").symlink_to(compiler.parent.parent / "share/dynlex/lib", target_is_directory=True)
        for name in ("renderer.dl", "curve_pixel_kernel.dl"):
            shutil.copy2(HERE / name, work / name)
        subprocess.run([compiler, "curve_pixel_kernel.dl", "--emit-llvm", "-O0", "-o", "raw.ll"], cwd=work, check=True, capture_output=True)
        ir = (work / "raw.ll").read_text()
        ir, count = re.subn(r"^define i32 @main\(\) \{.*?^\}\n", "", ir, count=1, flags=re.MULTILINE | re.DOTALL)
        if count != 1:
            raise RuntimeError("DynLex main ABI changed")
        ir, count = re.subn(r"define internal i32 (" + INNER + r")", r"define i32 \1", ir, count=1)
        if count != 1:
            raise RuntimeError("DynLex pixel function ABI changed")
        ir, count = re.subn(r"define internal i32 (" + RASTER + r")", r"define i32 \1", ir, count=1)
        if count != 1:
            raise RuntimeError("DynLex raster function ABI changed")
        ir += f'''
define i32 @opds_curve_pixel(ptr %scene, i32 %scene_len, i32 %x, i32 %y, ptr %result, i32 %result_len) {{
entry:
  %snull = icmp eq ptr %scene, null
  %rnull = icmp eq ptr %result, null
  %null = or i1 %snull, %rnull
  %smin = icmp slt i32 %scene_len, 88
  %smax = icmp sgt i32 %scene_len, 65576
  %rmin = icmp slt i32 %result_len, 8
  %xlo = icmp slt i32 %x, 0
  %xhi = icmp sge i32 %x, 256
  %ylo = icmp slt i32 %y, 0
  %yhi = icmp sge i32 %y, 192
  %a = or i1 %null, %smin
  %b = or i1 %a, %smax
  %c = or i1 %b, %rmin
  %d = or i1 %c, %xlo
  %e = or i1 %d, %xhi
  %f = or i1 %e, %ylo
  %bad = or i1 %f, %yhi
  br i1 %bad, label %reject, label %call
call:
  %ss = alloca ptr
  %xx = alloca i32
  %yy = alloca i32
  %rr = alloca ptr
  store ptr %scene, ptr %ss
  store i32 %x, ptr %xx
  store i32 %y, ptr %yy
  store ptr %result, ptr %rr
  %status = call i32 {re.search(INNER, ir).group(0)}(ptr %ss, ptr %xx, ptr %yy, ptr %rr)
  ret i32 %status
reject:
  ret i32 2
}}

define i32 @opds_curve_raster(ptr %scene, i32 %scene_len, ptr %output, i32 %output_len) {{
entry:
  %snull = icmp eq ptr %scene, null
  %onull = icmp eq ptr %output, null
  %null = or i1 %snull, %onull
  %smin = icmp slt i32 %scene_len, 88
  %smax = icmp sgt i32 %scene_len, 65576
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
  %status = call i32 {re.search(RASTER, ir).group(0)}(ptr %ss, ptr %oo)
  ret i32 %status
reject:
  ret i32 2
}}
'''
        (work / "wrapped.ll").write_text(ir)
        subprocess.run([opt, "-O3", "wrapped.ll", "-S", "-o", "optimized.ll"], cwd=work, check=True, capture_output=True)
        subprocess.run([llc, "-filetype=obj", "-relocation-model=pic", "optimized.ll", "-o", output / "curve_pixel.o"], cwd=work, check=True, capture_output=True)
    subprocess.run(["ar", "rcs", output / "libopds_dynlex_curve_pixel.a", output / "curve_pixel.o"], check=True)
    print(output / "libopds_dynlex_curve_pixel.a")


if __name__ == "__main__":
    main()
