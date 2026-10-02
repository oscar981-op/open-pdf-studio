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


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--compiler", type=Path, default=shutil.which("dynlex"))
    parser.add_argument("--stdlib", type=Path)
    parser.add_argument("--adapter", type=Path, required=True)
    parser.add_argument("--hayro", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--artifacts", type=Path)
    args = parser.parse_args()
    if not args.compiler or args.runs < 1:
        parser.error("provide a DynLex compiler and at least one run")
    compiler = Path(args.compiler).resolve()
    stdlib = (args.stdlib or compiler.parent.parent / "share/dynlex/lib").resolve()
    adapter = args.adapter.resolve()
    hayro = args.hayro.resolve()
    report = {"scope": "restricted-real-PDF-fixtures", "size": [WIDTH, HEIGHT], "runs": args.runs, "modes": {}}
    if args.artifacts:
        args.artifacts.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="opds-dynlex-real-pdf-") as temporary:
        work = Path(temporary)
        (work / "lib").symlink_to(stdlib, target_is_directory=True)
        for source in ("renderer", *MODES):
            shutil.copy2(HERE / "rgba" / f"{source}.dl", work)
        for mode in MODES:
            pdf = work / f"{mode}.pdf"
            opdl = work / "scene.opdl"
            output = work / "output.rgba"
            hayro_output = work / f"{mode}-hayro.rgba"
            binary = work / mode
            pdf.write_bytes(build_pdf(mode))
            compile_ms, _ = timed([compiler, f"{mode}.dl", "-O3", "-o", binary], work)
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
            expected = reference_rgba(mode)
            actual = output.read_bytes()
            if actual != expected:
                raise RuntimeError(f"{mode}: DynLex differs from independent PDF scene at byte {first_difference(actual, expected)}")
            hayro_bytes = hayro_output.read_bytes()
            if hayro_bytes != actual:
                raise RuntimeError(f"{mode}: DynLex differs from Hayro at byte {first_difference(actual, hayro_bytes)}")
            report["modes"][mode] = {
                "pdf_bytes": pdf.stat().st_size,
                "opdl_bytes": opdl.stat().st_size,
                "pixel_differences_vs_hayro": 0,
                "compile_ms": round(compile_ms, 3),
                "adapter_median_ms": adapter_ms,
                "dynlex_median_ms": dynlex_ms,
                "adapter_plus_dynlex_median_ms": pipeline_ms,
                "hayro_pdf_to_rgba_median_ms": hayro_ms,
            }
            if args.artifacts:
                shutil.copy2(pdf, args.artifacts / pdf.name)
                shutil.copy2(opdl, args.artifacts / f"{mode}.opdl")
                shutil.copy2(hayro_output, args.artifacts / hayro_output.name)
        bad = work / "unsupported.pdf"
        bad.write_bytes(build_pdf("hybrid", b"0 0 m\n"))
        (work / "scene.opdl").unlink(missing_ok=True)
        rejected = subprocess.run([adapter, bad, work / "scene.opdl"], cwd=work, capture_output=True)
        if rejected.returncode == 0 or (work / "scene.opdl").exists():
            raise RuntimeError("adapter silently accepted an unsupported PDF operator")
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
