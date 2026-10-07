#!/usr/bin/env python3
"""Paired, warm-process PDF backend comparison with exact RGBA accounting.

Build backend_probe with all features first. This measures the complete CLI
route (file read, Hayro document open, selected render, RGBA write and process
startup), not a cached in-app render. OS caches are left untouched.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import math
import os
import statistics
import subprocess
import tempfile
import time
from pathlib import Path


BACKENDS = ("hayro", "dynlex", "pdfium", "own-rust")


def run_one(
    probe: Path, pdf: Path, backend: str, scale: float, directory: Path, env: dict[str, str]
) -> dict:
    rgba = directory / f"{backend}.rgba"
    time_file = directory / f"{backend}.rss"
    started = time.perf_counter_ns()
    process = subprocess.run(
        ["/usr/bin/time", "-f", "%M", "-o", str(time_file), str(probe), str(pdf), str(rgba), f"--{backend}", f"--scale={scale}"],
        env=env,
        text=True,
        capture_output=True,
        check=False,
    )
    elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
    if process.returncode:
        raise RuntimeError(f"{pdf.name} {backend}: {process.stderr.strip()}")
    actual = process.stderr.strip().splitlines()[-1]
    used = "Hayro" if backend == "hayro" else actual.split("PDF=", 1)[1].split()[0]
    data = rgba.read_bytes()
    return {
        "requested": backend,
        "used": used,
        "wall_ms": round(elapsed_ms, 3),
        "peak_rss_kib": int(time_file.read_text().strip()),
        "rgba_bytes": len(data),
        "sha256": hashlib.sha256(data).hexdigest(),
        "rgba": data,
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", type=Path, default=Path("target/release/backend_probe"))
    parser.add_argument("--pdfium-library", type=Path, required=True)
    parser.add_argument("--rounds", type=int, default=15)
    parser.add_argument("--scale", type=float, default=1.0, help="72-dpi render scale (0.01..8)")
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("pdfs", type=Path, nargs="+")
    args = parser.parse_args()
    if not 1 <= args.rounds <= 100:
        parser.error("rounds must be 1..100")
    if not math.isfinite(args.scale) or not 0.01 <= args.scale <= 8.0:
        parser.error("scale must be finite and 0.01..8")
    probe = args.probe.resolve(strict=True)
    library = args.pdfium_library.resolve(strict=True)
    env = dict(os.environ, OPDS_PDFIUM_LIBRARY=str(library))
    report = {
        "protocol": "rotating-backend-order, same PDF and page 1, warm OS cache, new process per sample",
        "probe": str(probe),
        "pdfium_library": str(library),
        "rounds": args.rounds,
        "scale": args.scale,
        "fixtures": [],
    }
    for pdf_arg in args.pdfs:
        pdf = pdf_arg.resolve(strict=True)
        fixture = {"pdf": str(pdf), "samples": [], "summary": {}}
        report["fixtures"].append(fixture)
        with tempfile.TemporaryDirectory(prefix="opds-four-bench-") as temp:
            directory = Path(temp)
            for round_number in range(args.rounds):
                order = BACKENDS[round_number % len(BACKENDS) :] + BACKENDS[: round_number % len(BACKENDS)]
                results = [run_one(probe, pdf, backend, args.scale, directory, env) for backend in order]
                reference = next(row["rgba"] for row in results if row["requested"] == "hayro")
                for row in results:
                    data = row.pop("rgba")
                    row["different_rgba_channels_vs_hayro"] = (
                        sum(a != b for a, b in zip(data, reference)) + abs(len(data) - len(reference))
                    )
                    row["round"] = round_number + 1
                    fixture["samples"].append(row)
        for backend in BACKENDS:
            samples = [row for row in fixture["samples"] if row["requested"] == backend]
            fixture["summary"][backend] = {
                "median_wall_ms": round(statistics.median(row["wall_ms"] for row in samples), 3),
                "median_peak_rss_kib": statistics.median(row["peak_rss_kib"] for row in samples),
                "used": sorted(set(row["used"] for row in samples)),
                "max_different_rgba_channels_vs_hayro": max(
                    row["different_rgba_channels_vs_hayro"] for row in samples
                ),
            }
        print(pdf.name, json.dumps(fixture["summary"], sort_keys=True))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, indent=2) + "\n")


if __name__ == "__main__":
    main()
