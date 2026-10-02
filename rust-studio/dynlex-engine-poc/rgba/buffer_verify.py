#!/usr/bin/env python3
"""Build and pixel-check the experimental in-process DynLex buffer kernel."""

import argparse
import json
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
    parser.add_argument("--bench-memory", action="store_true",
                        help="measure per-process peak RSS with /usr/bin/time (Linux)")
    parser.add_argument("--bench-json", type=Path,
                        help="write raw paired full-process samples to JSON")
    args = parser.parse_args()
    if args.bench_memory and args.bench_runs < 1:
        parser.error("--bench-memory requires --bench-runs")
    time_binary = Path("/usr/bin/time")
    if args.bench_memory and not time_binary.is_file():
        parser.error("/usr/bin/time is required for --bench-memory")
    measurements = {}
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
                    samples = {"direct": [], "hayro": []}
                    for index in range(args.bench_runs):
                        jobs = (("direct", [probe, str(number), pdf, direct_output]),
                                ("hayro", [paths["hayro"], pdf, "1", hayro_output, "1"]))
                        for label, command in (jobs if index % 2 == 0 else reversed(jobs)):
                            peak_path = work / "peak-kib.txt"
                            if args.bench_memory:
                                command = [time_binary, "-f", "%M", "-o", peak_path, *command]
                            start = time.perf_counter_ns()
                            subprocess.run(command, check=True, capture_output=True)
                            sample = {"wall_ms": (time.perf_counter_ns() - start) / 1_000_000}
                            if args.bench_memory:
                                sample["peak_rss_kib"] = int(peak_path.read_text().strip())
                            samples[label].append(sample)
                    direct_samples = [item["wall_ms"] for item in samples["direct"]]
                    hayro_samples = [item["wall_ms"] for item in samples["hayro"]]
                    paired = [a - b for a, b in zip(direct_samples, hayro_samples)]
                    measurements[mode] = {"page": f"{width}x{height}",
                                          "dense": True, "os_cache_cleared": False,
                                          "time_wrapper": args.bench_memory, "samples": samples}
                    print(f"{mode} dense direct/Hayro median ms: "
                          f"{statistics.median(direct_samples):.2f}/"
                          f"{statistics.median(hayro_samples):.2f}; "
                          f"paired delta {statistics.median(paired):+.2f}; "
                          f"direct wins {sum(delta < 0 for delta in paired)}/{args.bench_runs}")
                    if args.bench_memory:
                        direct_peak = [item["peak_rss_kib"] for item in samples["direct"]]
                        hayro_peak = [item["peak_rss_kib"] for item in samples["hayro"]]
                        print(f"{mode} peak RSS median KiB: "
                              f"{statistics.median(direct_peak):.0f}/"
                              f"{statistics.median(hayro_peak):.0f}; "
                              f"first-process wall ms: "
                              f"{direct_samples[0]:.2f}/{hayro_samples[0]:.2f} "
                              f"(OS cache not cleared)")
                checked += width * height
        for mode, number in MODES[1:]:
            width, height = 256, 192
            pdf = work / f"translated-{mode}.pdf"
            scene = work / "scene.opdl"
            direct_output = work / "direct.rgba"
            cli_output = work / "output.rgba"
            hayro_output = work / "hayro.rgba"
            pdf.write_bytes(build_pdf(mode, width=width, height=height, translated=True))
            subprocess.run([paths["adapter"], pdf, scene], check=True, capture_output=True)
            subprocess.run([probe, str(number), pdf, direct_output],
                           check=True, capture_output=True)
            subprocess.run([work / mode], cwd=work, check=True, capture_output=True)
            subprocess.run([paths["hayro"], pdf, "1", hayro_output, "1"],
                           check=True, capture_output=True)
            expected = reference_rgba(mode, width, height, translated=True)
            if not (direct_output.read_bytes() == cli_output.read_bytes()
                    == hayro_output.read_bytes() == expected):
                raise RuntimeError(f"pixel mismatch: translated {mode}")
            checked += width * height
        invalid_transforms = {
            "scale": b"q\n2 0 0 1 0 0 cm\nQ\n",
            "fractional-translation": b"q\n1 0 0 1 0.5 0 cm\nQ\n",
            "out-of-page": b"q\n1 0 0 1 250 0 cm\n10 10 30 30 re\nf\nQ\n",
        }
        for name, content in invalid_transforms.items():
            invalid_pdf = work / f"invalid-{name}.pdf"
            invalid_pdf.write_bytes(build_pdf("vector", extra_content=content))
            invalid_output = work / f"invalid-{name}.opdl"
            rejected = subprocess.run([paths["adapter"], invalid_pdf, invalid_output],
                                      capture_output=True)
            if rejected.returncode == 0 or invalid_output.exists():
                raise RuntimeError(f"unsupported transform accepted: {name}")
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
        if args.bench_json:
            args.bench_json.write_text(json.dumps(measurements, indent=2) + "\n")
        print(f"11 PDF pages, {checked:,} pixels, direct/CLI/Hayro/reference exact; 4 invalid scenes and 3 unsupported transforms rejected")


if __name__ == "__main__":
    main()
