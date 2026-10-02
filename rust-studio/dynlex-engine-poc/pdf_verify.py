#!/usr/bin/env python3
"""Verify a narrow real-PDF -> OPDL -> DynLex route against Hayro pixels."""

import argparse
import json
import shutil
import statistics
import subprocess
import tempfile
import time
from pathlib import Path

from pdf_fixture import HEIGHT, WIDTH, build_pdf, reference_rgba

HERE = Path(__file__).resolve().parent
MODES = ("raster", "vector", "hybrid")


def timed(command, cwd):
    start = time.perf_counter_ns()
    process = subprocess.run(command, cwd=cwd, check=True, capture_output=True, text=True)
    return (time.perf_counter_ns() - start) / 1_000_000, process.stdout.strip()


def median_runs(command, cwd, runs):
    samples = [timed(command, cwd)[0] for _ in range(runs)]
    return round(statistics.median(samples), 3)


def first_difference(actual, expected):
    return next((index for index, (a, b) in enumerate(zip(actual, expected)) if a != b), None)


def peak_rss(command, cwd):
    memory = cwd / "max-rss-kib.txt"
    subprocess.run(["/usr/bin/time", "-f", "%M", "-o", memory, *command], cwd=cwd,
                   check=True, capture_output=True)
    return int(memory.read_text().strip())


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--compiler", type=Path, default=shutil.which("dynlex"))
    parser.add_argument("--stdlib", type=Path)
    parser.add_argument("--adapter", type=Path, required=True)
    parser.add_argument("--hayro", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--sizes", default="256x192,512x384")
    parser.add_argument("--artifacts", type=Path)
    parser.add_argument("--output", type=Path, help="write machine-readable result JSON")
    args = parser.parse_args()
    if not args.compiler or args.runs < 1:
        parser.error("provide a DynLex compiler and at least one run")
    compiler = Path(args.compiler).resolve()
    stdlib = (args.stdlib or compiler.parent.parent / "share/dynlex/lib").resolve()
    adapter = args.adapter.resolve()
    hayro = args.hayro.resolve()
    sizes = []
    for item in args.sizes.split(","):
        try:
            width, height = (int(value) for value in item.split("x"))
        except ValueError:
            parser.error(f"invalid size {item!r}")
        if not (0 < width <= 1024 and 0 < height <= 1024) or width % WIDTH or height % HEIGHT:
            parser.error("sizes must be multiples of 256x192 within 1024x1024")
        sizes.append((width, height))
    report = {"scope": "restricted-real-PDF-fixtures", "runs": args.runs, "sizes": {}}
    if args.artifacts:
        args.artifacts.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="opds-dynlex-real-pdf-") as temporary:
        work = Path(temporary)
        (work / "lib").symlink_to(stdlib, target_is_directory=True)
        for source in ("renderer", *MODES):
            shutil.copy2(HERE / "rgba" / f"{source}.dl", work)
        compiles = {}
        for mode in MODES:
            compiles[mode], _ = timed([compiler, f"{mode}.dl", "-O3", "-o", work / mode], work)
        for width, height in sizes:
            label = f"{width}x{height}"
            report["sizes"][label] = {}
            for mode in MODES:
                pdf = work / f"{mode}.pdf"
                opdl = work / "scene.opdl"
                output = work / "output.rgba"
                hayro_output = work / f"{mode}-hayro.rgba"
                binary = work / mode
                pdf.write_bytes(build_pdf(mode, width=width, height=height))
                adapter_ms = median_runs([adapter, pdf, opdl], work, args.runs)
                dynlex_ms = median_runs([binary], work, args.runs)
                pipeline_samples = []
                for _ in range(args.runs):
                    started = time.perf_counter_ns()
                    timed([adapter, pdf, opdl], work)
                    timed([binary], work)
                    pipeline_samples.append((time.perf_counter_ns() - started) / 1_000_000)
                pipeline_ms = round(statistics.median(pipeline_samples), 3)
                hayro_ms = median_runs([hayro, pdf, "1", hayro_output, "1"], work, args.runs)
                expected = reference_rgba(mode, width, height)
                actual = output.read_bytes()
                if actual != expected:
                    raise RuntimeError(f"{mode}/{label}: DynLex differs from independent PDF scene at byte {first_difference(actual, expected)}")
                hayro_bytes = hayro_output.read_bytes()
                if hayro_bytes != actual:
                    raise RuntimeError(f"{mode}/{label}: DynLex differs from Hayro at byte {first_difference(actual, hayro_bytes)}")
                adapter_rss = peak_rss([adapter, pdf, opdl], work)
                dynlex_rss = peak_rss([binary], work)
                hayro_rss = peak_rss([hayro, pdf, "1", hayro_output, "1"], work)
                report["sizes"][label][mode] = {
                    "pdf_bytes": pdf.stat().st_size,
                    "opdl_bytes": opdl.stat().st_size,
                    "pixel_differences_vs_hayro": 0,
                    "compile_ms": round(compiles[mode], 3),
                    "adapter_median_ms": adapter_ms,
                    "dynlex_median_ms": dynlex_ms,
                    "adapter_plus_dynlex_median_ms": pipeline_ms,
                    "hayro_pdf_to_rgba_median_ms": hayro_ms,
                    "adapter_peak_rss_kib": adapter_rss,
                    "dynlex_peak_rss_kib": dynlex_rss,
                    "hayro_peak_rss_kib": hayro_rss,
                }
                if args.artifacts:
                    shutil.copy2(pdf, args.artifacts / f"{label}-{mode}.pdf")
                    shutil.copy2(opdl, args.artifacts / f"{label}-{mode}.opdl")
                    shutil.copy2(hayro_output, args.artifacts / f"{label}-{mode}-hayro.rgba")
        bad_cases = {
            "unsupported-operator": b"0 0 c\n",
            "diagonal-stroke": b"0 0 m\n10 10 l\nS\n",
            "wide-stroke": b"2 w\n",
        }
        for reason, extra in bad_cases.items():
            bad = work / f"{reason}.pdf"
            bad.write_bytes(build_pdf("hybrid", extra))
            (work / "scene.opdl").unlink(missing_ok=True)
            rejected = subprocess.run([adapter, bad, work / "scene.opdl"], cwd=work, capture_output=True)
            if rejected.returncode == 0 or (work / "scene.opdl").exists():
                raise RuntimeError(f"adapter silently accepted {reason}")
    rendered = json.dumps(report, indent=2)
    print(rendered)
    if args.output:
        args.output.write_text(rendered + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
