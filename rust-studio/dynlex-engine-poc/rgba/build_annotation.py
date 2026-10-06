#!/usr/bin/env python3
"""Compile the bounded DynLex native-annotation painter as a static library."""

import argparse
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
INNER = r'@"render_3cstring8scene5_into_3cstring8pixels5_i8\*_i8\*"'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, required=True)
    parser.add_argument("--opt", type=Path, required=True)
    parser.add_argument("--llc", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    compiler = args.compiler.resolve(strict=True)
    opt = args.opt.resolve(strict=True)
    llc = args.llc.resolve(strict=True)
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="opds-dynlex-annotation-") as temporary:
        work = Path(temporary)
        (work / "lib").symlink_to(compiler.parent.parent / "share/dynlex/lib", target_is_directory=True)
        for name in ("renderer.dl", "annotation_kernel.dl"):
            shutil.copy2(HERE / name, work / name)
        subprocess.run(
            [compiler, "annotation_kernel.dl", "--emit-llvm", "-O0", "-o", "raw.ll"],
            cwd=work,
            check=True,
            capture_output=True,
        )
        ir = (work / "raw.ll").read_text()
        ir, count = re.subn(
            r"^define i32 @main\(\) \{.*?^\}\n", "", ir, count=1, flags=re.MULTILINE | re.DOTALL
        )
        if count != 1:
            raise RuntimeError("DynLex IR did not contain the expected main function")
        ir, count = re.subn(r"define internal i32 (" + INNER + r")", r"define i32 \1", ir, count=1)
        if count != 1:
            raise RuntimeError("DynLex annotation function ABI changed")
        inner = re.search(INNER, ir).group(0)
        ir += f"""
define i32 @opds_paint_annotations(ptr %scene, i32 %scene_len, ptr %rgba, i32 %rgba_len) {{
entry:
  %null_scene = icmp eq ptr %scene, null
  %null_rgba = icmp eq ptr %rgba, null
  %null = or i1 %null_scene, %null_rgba
  %short = icmp slt i32 %scene_len, 20
  %long = icmp sgt i32 %scene_len, 13844
  %bad_size = or i1 %short, %long
  %bad = or i1 %null, %bad_size
  br i1 %bad, label %reject, label %header
header:
  %version_p = getelementptr i8, ptr %scene, i64 4
  %width_p = getelementptr i8, ptr %scene, i64 8
  %height_p = getelementptr i8, ptr %scene, i64 12
  %count_p = getelementptr i8, ptr %scene, i64 16
  %version = load i32, ptr %version_p, align 1
  %width = load i32, ptr %width_p, align 1
  %height = load i32, ptr %height_p, align 1
  %count = load i32, ptr %count_p, align 1
  %v1 = icmp eq i32 %version, 1
  %v2 = icmp eq i32 %version, 2
  %known_version = or i1 %v1, %v2
  %bad_version = xor i1 %known_version, true
  %bad_width_min = icmp ult i32 %width, 1
  %bad_height_min = icmp ult i32 %height, 1
  %bad_width_max = icmp ugt i32 %width, 4096
  %bad_height_max = icmp ugt i32 %height, 4096
  %bad_count = icmp ugt i32 %count, 576
  %expected = mul i32 %count, 24
  %expected_len = add i32 %expected, 20
  %bad_len = icmp ne i32 %scene_len, %expected_len
  %width64 = zext i32 %width to i64
  %height64 = zext i32 %height to i64
  %pixel_count = mul i64 %width64, %height64
  %too_many = icmp ugt i64 %pixel_count, 6000000
  %expected_rgba = mul i64 %pixel_count, 4
  %provided_rgba = sext i32 %rgba_len to i64
  %small_rgba = icmp slt i64 %provided_rgba, %expected_rgba
  %bad1 = or i1 %bad_version, %bad_width_min
  %bad2 = or i1 %bad1, %bad_height_min
  %bad3 = or i1 %bad2, %bad_width_max
  %bad4 = or i1 %bad3, %bad_height_max
  %bad5 = or i1 %bad4, %bad_count
  %bad6 = or i1 %bad5, %bad_len
  %bad7 = or i1 %bad6, %too_many
  %bad8 = or i1 %bad7, %small_rgba
  br i1 %bad8, label %reject, label %call
call:
  %scene_address = alloca ptr
  %rgba_address = alloca ptr
  store ptr %scene, ptr %scene_address
  store ptr %rgba, ptr %rgba_address
  %result = call i32 {inner}(ptr %scene_address, ptr %rgba_address)
  ret i32 %result
reject:
  ret i32 2
}}
"""
        (work / "wrapped.ll").write_text(ir)
        subprocess.run([opt, "-O3", "wrapped.ll", "-S", "-o", "optimized.ll"], cwd=work, check=True)
        subprocess.run(
            [llc, "-filetype=obj", "-relocation-model=pic", "optimized.ll", "-o", output / "annotation_kernel.o"],
            cwd=work,
            check=True,
        )
    subprocess.run(["ar", "rcs", output / "libopds_dynlex_annotation.a", output / "annotation_kernel.o"], check=True)
    print(output / "libopds_dynlex_annotation.a")


if __name__ == "__main__":
    main()
