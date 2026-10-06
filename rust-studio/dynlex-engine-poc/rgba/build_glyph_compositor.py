#!/usr/bin/env python3
"""Compile the bounded DynLex OPGC direct glyph compositor library."""

import argparse
import re
import shutil
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
INNER = r'@"composite_3cstring8scene5_into_3cstring8rgba5_i8\*_i8\*"'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("compiler", "opt", "llc", "output-dir"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    compiler, opt, llc = (getattr(args, name).resolve(strict=True)
                          for name in ("compiler", "opt", "llc"))
    output = args.output_dir.resolve()
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="opds-glyph-compositor-") as folder:
        work = Path(folder)
        (work / "lib").symlink_to(compiler.parent.parent / "share/dynlex/lib", target_is_directory=True)
        for name in ("renderer.dl", "glyph_compositor.dl"):
            shutil.copy2(HERE / name, work / name)
        compiled = subprocess.run([compiler, "glyph_compositor.dl", "--emit-llvm", "-O0", "-o", "raw.ll"],
                                  cwd=work, capture_output=True, text=True)
        if compiled.returncode:
            raise RuntimeError(f"DynLex glyph compositor compile failed:\n{compiled.stderr}")
        ir = (work / "raw.ll").read_text()
        ir, count = re.subn(r"^define i32 @main\(\) \{.*?^\}\n", "", ir, count=1,
                            flags=re.MULTILINE | re.DOTALL)
        if count != 1:
            raise RuntimeError("DynLex main ABI changed")
        ir, count = re.subn(r"define internal i32 (" + INNER + r")", r"define i32 \1", ir, count=1)
        if count != 1:
            raise RuntimeError("DynLex glyph compositor ABI changed")
        inner = re.search(INNER, ir).group(0)
        ir += f'''
define i32 @opds_glyph_direct_compose(ptr %scene, i32 %scene_len, ptr %rgba, i32 %rgba_len) {{
entry:
  %sn = icmp eq ptr %scene, null
  %rn = icmp eq ptr %rgba, null
  %null = or i1 %sn, %rn
  %old_len = icmp eq i32 %scene_len, 49188
  %new_len = icmp eq i32 %scene_len, 49196
  %known_len = or i1 %old_len, %new_len
  %badlen = xor i1 %known_len, true
  %bad0 = or i1 %null, %badlen
  br i1 %bad0, label %reject, label %header
header:
  %magic = load i32, ptr %scene, align 1
  %verp = getelementptr i8, ptr %scene, i64 4
  %wp = getelementptr i8, ptr %scene, i64 8
  %hp = getelementptr i8, ptr %scene, i64 12
  %lp = getelementptr i8, ptr %scene, i64 16
  %tp = getelementptr i8, ptr %scene, i64 20
  %rp = getelementptr i8, ptr %scene, i64 24
  %bp = getelementptr i8, ptr %scene, i64 28
  %fp = getelementptr i8, ptr %scene, i64 35
  %oxp = getelementptr i8, ptr %scene, i64 36
  %oyp = getelementptr i8, ptr %scene, i64 40
  %version = load i32, ptr %verp, align 1
  %width = load i32, ptr %wp, align 1
  %height = load i32, ptr %hp, align 1
  %left = load i32, ptr %lp, align 1
  %top = load i32, ptr %tp, align 1
  %right = load i32, ptr %rp, align 1
  %bottom = load i32, ptr %bp, align 1
  %flags = load i8, ptr %fp, align 1
  %raw_origin_x = load i32, ptr %oxp, align 1
  %raw_origin_y = load i32, ptr %oyp, align 1
  %badmagic = icmp ne i32 %magic, 1128747087
  %v1 = icmp eq i32 %version, 1
  %v2 = icmp eq i32 %version, 2
  %valid_version = or i1 %v1, %v2
  %badversion = xor i1 %valid_version, true
  %bad_version_len_v1 = and i1 %v1, %new_len
  %bad_version_len_v2 = and i1 %v2, %old_len
  %bad_version_len = or i1 %bad_version_len_v1, %bad_version_len_v2
  %origin_x = select i1 %v2, i32 %raw_origin_x, i32 0
  %origin_y = select i1 %v2, i32 %raw_origin_y, i32 0
  %badwidthmin = icmp ult i32 %width, 1
  %maxwidth = select i1 %v2, i32 4096, i32 256
  %badwidthmax = icmp ugt i32 %width, %maxwidth
  %badheightmin = icmp ult i32 %height, 1
  %badheightmax = icmp ugt i32 %height, 4096
  %badleft = icmp uge i32 %left, %right
  %badright = icmp ugt i32 %right, %width
  %badtop = icmp uge i32 %top, %bottom
  %badbottom = icmp ugt i32 %bottom, %height
  %bad_origin_x = icmp ugt i32 %origin_x, %left
  %bad_origin_y = icmp ugt i32 %origin_y, %top
  %local_right = sub i32 %right, %origin_x
  %local_bottom = sub i32 %bottom, %origin_y
  %bad_tile_right = icmp ugt i32 %local_right, 256
  %bad_tile_bottom = icmp ugt i32 %local_bottom, 192
  %badtile0 = or i1 %bad_origin_x, %bad_origin_y
  %badtile1 = or i1 %bad_tile_right, %bad_tile_bottom
  %badtile = or i1 %badtile0, %badtile1
  %badflags = icmp ugt i8 %flags, 1
  %pix = mul i32 %width, %height
  %too_many_pixels = icmp ugt i32 %pix, 6000000
  %bytes = mul i32 %pix, 4
  %badoutput = icmp slt i32 %rgba_len, %bytes
  %a = or i1 %badmagic, %badversion
  %a1 = or i1 %a, %bad_version_len
  %b = or i1 %a1, %badwidthmin
  %c = or i1 %b, %badwidthmax
  %d = or i1 %c, %badheightmin
  %e = or i1 %d, %badheightmax
  %f = or i1 %e, %badleft
  %g = or i1 %f, %badright
  %h = or i1 %g, %badtop
  %i = or i1 %h, %badbottom
  %j = or i1 %i, %badtile
  %k = or i1 %j, %badflags
  %l = or i1 %k, %too_many_pixels
  %bad = or i1 %l, %badoutput
  br i1 %bad, label %reject, label %call
call:
  %sp = alloca ptr
  %op = alloca ptr
  store ptr %scene, ptr %sp
  store ptr %rgba, ptr %op
  %status = call i32 {inner}(ptr %sp, ptr %op)
  ret i32 %status
reject:
  ret i32 2
}}
'''
        (work / "wrapped.ll").write_text(ir)
        subprocess.run([opt, "-O3", "wrapped.ll", "-S", "-o", "optimized.ll"], cwd=work, check=True,
                       capture_output=True)
        subprocess.run([llc, "-filetype=obj", "-relocation-model=pic", "optimized.ll",
                        "-o", output / "glyph_compositor.o"], cwd=work, check=True, capture_output=True)
    library = output / "libopds_dynlex_glyph_compositor.a"
    subprocess.run(["ar", "rcs", library, output / "glyph_compositor.o"], check=True)
    print(library)


if __name__ == "__main__":
    main()
