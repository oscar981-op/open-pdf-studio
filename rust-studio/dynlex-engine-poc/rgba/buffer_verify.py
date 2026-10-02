#!/usr/bin/env python3
"""Build and pixel-check the experimental in-process DynLex buffer kernel."""

import argparse
import statistics
import time
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
    parser.add_argument("--bench-runs", type=int, default=0,
                        help="paired full-process PDF-to-RGBA runs on dense fixtures")
    args = parser.parse_args()
    paths = {name: getattr(args, name).resolve(strict=True)
             for name in ("compiler", "opt", "llc", "rustc", "adapter", "hayro")}
    adapter_rlib = paths["adapter"].parent / "libopds_opdl_adapter.rlib"
    if not adapter_rlib.is_file():
        raise RuntimeError(f"build the adapter library first: {adapter_rlib}")
    with tempfile.TemporaryDirectory(prefix="opds-dynlex-buffer-verify-") as temporary:
        work = Path(temporary)
        subprocess.run([
            "python3", HERE / "build_buffer.py", "--compiler", paths["compiler"],
            "--opt", paths["opt"], "--llc", paths["llc"], "--output-dir", work,
        ], check=True, capture_output=True)
        probe = work / "buffer_probe"
        subprocess.run([
            paths["rustc"], "--edition=2024", "-O", HERE / "buffer_probe.rs",
            "-L", f"native={work}",
            "-L", f"dependency={paths['adapter'].parent / 'deps'}",
            "--extern", f"opds_opdl_adapter={adapter_rlib}", "-o", probe,
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
                direct_output = work / "direct.rgba"
                cli_output = work / "output.rgba"
                hayro_output = work / "hayro.rgba"
                pdf.write_bytes(build_pdf(mode, width=width, height=height, dense=dense))
                subprocess.run([paths["adapter"], pdf, scene], check=True, capture_output=True)
                subprocess.run([probe, str(number), scene, buffer_output], check=True, capture_output=True)
                subprocess.run([probe, str(number), pdf, direct_output],
                               check=True, capture_output=True)
                subprocess.run([work / mode], cwd=work, check=True, capture_output=True)
                subprocess.run([paths["hayro"], pdf, "1", hayro_output, "1"],
                               check=True, capture_output=True)
                expected = reference_rgba(mode, width, height, dense=dense)
                if not (buffer_output.read_bytes() == direct_output.read_bytes()
                        == cli_output.read_bytes()
                        == hayro_output.read_bytes() == expected):
                    raise RuntimeError(f"pixel mismatch: {width}x{height}/{mode}")
                if dense and args.bench_calls > 1:
                    timed = subprocess.run([probe, str(number), scene, buffer_output,
                                            str(args.bench_calls)], check=True,
                                           capture_output=True, text=True)
                    print(f"{mode}: {timed.stderr.strip()}")
                if dense and args.bench_runs > 0:
                    direct_samples, hayro_samples = [], []
                    for index in range(args.bench_runs):
                        jobs = (("direct", [probe, str(number), pdf, direct_output]),
                                ("hayro", [paths["hayro"], pdf, "1", hayro_output, "1"]))
                        for label, command in (jobs if index % 2 == 0 else reversed(jobs)):
                            start = time.perf_counter_ns()
                            subprocess.run(command, check=True, capture_output=True)
                            sample = (time.perf_counter_ns() - start) / 1_000_000
                            (direct_samples if label == "direct" else hayro_samples).append(sample)
                    paired = [a - b for a, b in zip(direct_samples, hayro_samples)]
                    print(f"{mode} dense direct/Hayro median ms: "
                          f"{statistics.median(direct_samples):.2f}/"
                          f"{statistics.median(hayro_samples):.2f}; "
                          f"paired delta {statistics.median(paired):+.2f}; "
                          f"direct wins {sum(delta < 0 for delta in paired)}/{args.bench_runs}")
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
        print(f"9 PDF pages, {checked:,} pixels, direct/CLI/Hayro/reference exact; 4 invalid scenes rejected")


if __name__ == "__main__":
    main()
