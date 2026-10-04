#!/usr/bin/env python3
"""Compile the tiny OPLN f64 boundary probe as a DynLex static library."""

import argparse
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
INNER = r'@"decode_3cstring8scene5_into_3cstring8result5_i8\*_i8\*"'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("compiler", "opt", "llc", "output-dir"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    compiler, opt, llc = (getattr(args, name).resolve(strict=True)
                          for name in ("compiler", "opt", "llc"))
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="opds-dynlex-curve-decode-") as folder:
        work = Path(folder)
        (work / "lib").symlink_to(compiler.parent.parent / "share/dynlex/lib", target_is_directory=True)
        for name in ("renderer.dl", "curve_decode_kernel.dl"):
            shutil.copy2(HERE / name, work / name)
        subprocess.run([compiler, "curve_decode_kernel.dl", "--emit-llvm", "-O0", "-o", "raw.ll"],
                       cwd=work, check=True, capture_output=True)
        ir = (work / "raw.ll").read_text()
        ir, count = re.subn(r"^define i32 @main\(\) \{.*?^\}\n", "", ir,
                            count=1, flags=re.MULTILINE | re.DOTALL)
        if count != 1:
            raise RuntimeError("DynLex main ABI changed")
        ir, count = re.subn(r"define internal i32 (" + INNER + r")", r"define i32 \1", ir,
                            count=1)
        if count != 1:
            raise RuntimeError("DynLex curve decode function ABI changed")
        ir += f"""
define i32 @opds_curve_decode(ptr %scene, i32 %scene_len, ptr %result, i32 %result_len) {{
entry:
  %bad_scene = icmp eq ptr %scene, null
  %bad_result = icmp eq ptr %result, null
  %bad_ptr = or i1 %bad_scene, %bad_result
  %short = icmp slt i32 %scene_len, 24
  %too_long = icmp sgt i32 %scene_len, 65576
  %short_result = icmp slt i32 %result_len, 32
  %bad_len_a = or i1 %short, %too_long
  %bad_len_b = or i1 %bad_len_a, %short_result
  %bad = or i1 %bad_ptr, %bad_len_b
  br i1 %bad, label %reject, label %call
call:
  %scene_slot = alloca ptr
  %result_slot = alloca ptr
  store ptr %scene, ptr %scene_slot
  store ptr %result, ptr %result_slot
  %status = call i32 {re.search(INNER, ir).group(0)}(ptr %scene_slot, ptr %result_slot)
  ret i32 %status
reject:
  ret i32 2
}}
"""
        (work / "wrapped.ll").write_text(ir)
        subprocess.run([opt, "-O3", "wrapped.ll", "-S", "-o", "optimized.ll"],
                       cwd=work, check=True, capture_output=True)
        subprocess.run([llc, "-filetype=obj", "-relocation-model=pic", "optimized.ll",
                        "-o", output / "curve_decode.o"], cwd=work, check=True, capture_output=True)
    subprocess.run(["ar", "rcs", output / "libopds_dynlex_curve_decode.a",
                    output / "curve_decode.o"], check=True)
    print(output / "libopds_dynlex_curve_decode.a")


if __name__ == "__main__":
    main()
