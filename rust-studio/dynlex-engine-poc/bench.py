#!/usr/bin/env python3
"""Compile and measure three DynLex kernel sketches; no PDF parsing is claimed."""

import argparse
import json
import shutil
import statistics
import subprocess
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
AGENTS = ("raster", "vector", "hybrid")
WIDTH, HEIGHT, PASSES = 1024, 768, 4


def expected_checksums():
    sums = dict.fromkeys(AGENTS, 0)
    for y in range(HEIGHT):
        for x in range(WIDTH):
            raster = (x * 17 + y * 31) % 256
            vector = 255 if x % 97 < 2 or y % 83 < 2 else 180 if (x + y) % 131 < 2 else 0
            sums["raster"] += raster
            sums["vector"] += vector
            sums["hybrid"] += vector or raster
    return {name: value * PASSES for name, value in sums.items()}


def timed(command, cwd):
    started = time.perf_counter_ns()
    process = subprocess.run(command, cwd=cwd, capture_output=True, text=True, check=True)
    return (time.perf_counter_ns() - started) / 1_000_000, process.stdout.strip()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--compiler", type=Path, default=shutil.which("dynlex"))
    parser.add_argument("--stdlib", type=Path, help="directory containing std.dl")
    parser.add_argument("--runs", type=int, default=15)
    parser.add_argument("--output", type=Path, help="write machine-readable JSON result")
    args = parser.parse_args()
    if not args.compiler or args.runs < 1:
        parser.error("provide a DynLex compiler and at least one measured run")
    compiler = Path(args.compiler).resolve()
    stdlib = (args.stdlib or compiler.parent.parent / "share/dynlex/lib").resolve()
    if not (stdlib / "std.dl").is_file():
        parser.error(f"DynLex standard library not found at {stdlib}")
    expected = expected_checksums()
    result = {
        "kind": "synthetic-kernel-only",
        "compiler": str(compiler),
        "compiler_version": subprocess.check_output([compiler, "--version"], text=True).strip(),
        "page_pixels": [WIDTH, HEIGHT],
        "passes": PASSES,
        "runs": args.runs,
        "agents": {},
    }
    with tempfile.TemporaryDirectory(prefix="opds-dynlex-poc-") as temporary:
        work = Path(temporary)
        (work / "lib").symlink_to(stdlib, target_is_directory=True)
        for source in (*AGENTS, "scene"):
            shutil.copy2(HERE / f"{source}.dl", work)
        for name in AGENTS:
            binary = work / name
            compile_ms, _ = timed([str(compiler), f"{name}.dl", "-O3", "-o", str(binary)], work)
            _, warmup = timed([str(binary)], work)
            if warmup != str(expected[name]):
                raise RuntimeError(f"{name}: expected {expected[name]}, got {warmup}")
            measurements = []
            for _ in range(args.runs):
                run_ms, output = timed([str(binary)], work)
                if output != warmup:
                    raise RuntimeError(f"{name}: unstable checksum {output}")
                measurements.append(run_ms)
            result["agents"][name] = {
                "checksum": int(warmup),
                "compile_ms": round(compile_ms, 3),
                "median_process_ms": round(statistics.median(measurements), 3),
                "min_process_ms": round(min(measurements), 3),
                "max_process_ms": round(max(measurements), 3),
            }
    rendered = json.dumps(result, indent=2)
    print(rendered)
    if args.output:
        args.output.write_text(rendered + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
