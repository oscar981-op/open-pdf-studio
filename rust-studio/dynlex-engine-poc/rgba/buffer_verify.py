#!/usr/bin/env python3
"""Build and pixel-check the experimental in-process DynLex buffer kernel."""

import argparse
import shutil
import subprocess
import tempfile
from pathlib import Path
from sys import path as search_path

HERE = Path(__file__).resolve().parent
search_path.insert(0, str(HERE.parent))
from pdf_fixture import build_pdf, reference_rgba  # noqa: E402

MODES = (("raster", 1), ("vector", 2), ("hybrid", 3))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("compiler", "opt", "llc", "rustc", "adapter", "hayro"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--bench-calls", type=int, default=0,
                        help="repeat each dense page in one Rust process to time kernel calls")
    args = parser.parse_args()
    paths = {name: getattr(args, name).resolve(strict=True)
             for name in ("compiler", "opt", "llc", "rustc", "adapter", "hayro")}
    with tempfile.TemporaryDirectory(prefix="opds-dynlex-buffer-verify-") as temporary:
        work = Path(temporary)
        subprocess.run([
            "python3", HERE / "build_buffer.py", "--compiler", paths["compiler"],
            "--opt", paths["opt"], "--llc", paths["llc"], "--output-dir", work,
        ], check=True, capture_output=True)
        probe = work / "buffer_probe"
        subprocess.run([
            paths["rustc"], "--edition=2024", "-O", HERE / "buffer_probe.rs",
            "-L", f"native={work}", "-o", probe,
        ], check=True, capture_output=True)
        (work / "lib").symlink_to(paths["compiler"].parent.parent / "share/dynlex/lib",
                                  target_is_directory=True)
        for name in ("renderer.dl", *(f"{mode}.dl" for mode, _ in MODES)):
            shutil.copy2(HERE / name, work / name)
        for mode, _ in MODES:
            subprocess.run([paths["compiler"], f"{mode}.dl", "-O3", "-o", work / mode],
                           cwd=work, check=True, capture_output=True)
        checked = 0
        for width, height, dense in ((256, 192, False), (512, 384, False), (1024, 768, True)):
            for mode, number in MODES:
                pdf = work / f"{mode}.pdf"
                scene = work / "scene.opdl"
                buffer_output = work / "buffer.rgba"
                cli_output = work / "output.rgba"
                hayro_output = work / "hayro.rgba"
                pdf.write_bytes(build_pdf(mode, width=width, height=height, dense=dense))
                subprocess.run([paths["adapter"], pdf, scene], check=True, capture_output=True)
                subprocess.run([probe, str(number), scene, buffer_output], check=True, capture_output=True)
                subprocess.run([work / mode], cwd=work, check=True, capture_output=True)
                subprocess.run([paths["hayro"], pdf, "1", hayro_output, "1"],
                               check=True, capture_output=True)
                expected = reference_rgba(mode, width, height, dense=dense)
                if not (buffer_output.read_bytes() == cli_output.read_bytes()
                        == hayro_output.read_bytes() == expected):
                    raise RuntimeError(f"pixel mismatch: {width}x{height}/{mode}")
                if dense and args.bench_calls > 1:
                    timed = subprocess.run([probe, str(number), scene, buffer_output,
                                            str(args.bench_calls)], check=True,
                                           capture_output=True, text=True)
                    print(f"{mode}: {timed.stderr.strip()}")
                checked += width * height
        valid = (work / "scene.opdl").read_bytes()
        bad = {
            "truncated": valid[:-1],
            "bad-magic": b"BAD!" + valid[4:],
            "unsupported-version": valid[:4] + (1).to_bytes(4, "little") + valid[8:],
            "bad-command": valid[:-24] + (99).to_bytes(4, "little") + valid[-20:],
        }
        for name, content in bad.items():
            input_path = work / f"{name}.opdl"
            output_path = work / f"{name}.rgba"
            input_path.write_bytes(content)
            result = subprocess.run([probe, "3", input_path, output_path], capture_output=True)
            if result.returncode == 0 or output_path.exists():
                raise RuntimeError(f"invalid scene accepted: {name}")
        print(f"9 PDF pages, {checked:,} pixels, CLI/Hayro/reference exact; 4 invalid scenes rejected")


if __name__ == "__main__":
    main()
