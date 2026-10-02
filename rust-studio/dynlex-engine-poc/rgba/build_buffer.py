#!/usr/bin/env python3
"""Build the experimental DynLex OPDL-v2 buffer kernel as a static library."""

import argparse
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
INNER = r'@"render_3cstring8scene5_into_3cstring8rgba5_i8\*_i8\*"'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, required=True)
    parser.add_argument("--opt", type=Path, required=True, help="LLVM opt matching DynLex IR")
    parser.add_argument("--llc", type=Path, required=True, help="LLVM llc matching DynLex IR")
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    compiler = args.compiler.resolve(strict=True)
    opt = args.opt.resolve(strict=True)
    llc = args.llc.resolve(strict=True)
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="opds-dynlex-buffer-") as temporary:
        work = Path(temporary)
        (work / "lib").symlink_to(compiler.parent.parent / "share/dynlex/lib", target_is_directory=True)
        for name in ("renderer.dl", "buffer_kernel.dl"):
            shutil.copy2(HERE / name, work / name)
        subprocess.run([compiler, "buffer_kernel.dl", "--emit-llvm", "-O0", "-o", "raw.ll"],
                       cwd=work, check=True, capture_output=True)
        ir = (work / "raw.ll").read_text()
        ir, count = re.subn(r"^define i32 @main\(\) \{.*?^\}\n", "", ir,
                            count=1, flags=re.MULTILINE | re.DOTALL)
        if count != 1:
            raise RuntimeError("DynLex IR did not contain the expected main function")
        ir, count = re.subn(r"define internal i32 (" + INNER + r")", r"define i32 \1", ir,
                            count=1)
        if count != 1:
            raise RuntimeError("DynLex buffer function ABI changed")
        ir += f"""
define i32 @opds_render_buffer(ptr %scene, i32 %scene_len, ptr %output, i32 %output_len, i32 %mode) {{
entry:
  %bad_scene = icmp eq ptr %scene, null
  %bad_output = icmp eq ptr %output, null
  %bad_ptr = or i1 %bad_scene, %bad_output
  %short = icmp slt i32 %scene_len, 40
  %too_long = icmp sgt i32 %scene_len, 3147312
  %small_output = icmp slt i32 %output_len, 12
  %bad_size1 = or i1 %short, %too_long
  %bad_size2 = or i1 %bad_size1, %small_output
  %bad = or i1 %bad_ptr, %bad_size2
  br i1 %bad, label %reject, label %call
call:
  store i32 %mode, ptr %output, align 1
  %length_slot = getelementptr i8, ptr %output, i64 4
  store i32 %scene_len, ptr %length_slot, align 1
  %capacity_slot = getelementptr i8, ptr %output, i64 8
  %pixel_capacity = sub i32 %output_len, 12
  store i32 %pixel_capacity, ptr %capacity_slot, align 1
  %scene_address = alloca ptr
  %output_address = alloca ptr
  store ptr %scene, ptr %scene_address
  store ptr %output, ptr %output_address
  %result = call i32 {re.search(INNER, ir).group(0)}(ptr %scene_address, ptr %output_address)
  ret i32 %result
reject:
  ret i32 2
}}
"""
        (work / "wrapped.ll").write_text(ir)
        subprocess.run([opt, "-O3", "wrapped.ll", "-S", "-o", "optimized.ll"],
                       cwd=work, check=True, capture_output=True)
        subprocess.run([llc, "-filetype=obj", "-relocation-model=pic", "optimized.ll",
                        "-o", output / "buffer_kernel.o"], cwd=work, check=True, capture_output=True)
    subprocess.run(["ar", "rcs", output / "libopds_dynlex_buffer.a",
                    output / "buffer_kernel.o"], check=True)
    print(output / "libopds_dynlex_buffer.a")


if __name__ == "__main__":
    main()
