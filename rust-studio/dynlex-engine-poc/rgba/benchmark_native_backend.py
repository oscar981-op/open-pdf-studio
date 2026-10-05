#!/usr/bin/env python3
"""Paired, warm-cache native DynLex/Hayro PDF-to-raw-RGBA process benchmark."""

import argparse
import hashlib
import json
import shutil
import statistics
import subprocess
import tempfile
import time
from pathlib import Path


def run(probe: Path, pdf: Path, output: Path, backend: str):
    command = [str(probe), str(pdf), str(output)]
    if backend == "hayro":
        command.append("--hayro")
    started = time.perf_counter_ns()
    completed = subprocess.run(
        ["/usr/bin/time", "-f", "OPDS_PEAK_RSS_KIB=%M", *command],
        capture_output=True,
        text=True,
        check=True,
    )
    elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
    rss_line = next(
        line for line in completed.stderr.splitlines()
        if line.startswith("OPDS_PEAK_RSS_KIB=")
    )
    data = output.read_bytes()
    return {
        "wall_ms": elapsed_ms,
        "peak_rss_kib": int(rss_line.split("=", 1)[1]),
        "sha256": hashlib.sha256(data).hexdigest(),
        "bytes": len(data),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pdf", type=Path)
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=15)
    parser.add_argument("--json", type=Path)
    args = parser.parse_args()
    if not 1 <= args.runs <= 1000:
        parser.error("--runs must be 1..1000")
    if not shutil.which("/usr/bin/time"):
        parser.error("/usr/bin/time is required for per-process peak RSS")
    pdf = args.pdf.resolve(strict=True)
    probe = args.probe.resolve(strict=True)
    samples = {"dynlex": [], "hayro": []}
    with tempfile.TemporaryDirectory(prefix="opds-native-bench-") as directory:
        output = Path(directory) / "page.rgba"
        for backend in ("dynlex", "hayro"):
            run(probe, pdf, output, backend)
        for round_index in range(args.runs):
            order = ("dynlex", "hayro") if round_index % 2 == 0 else ("hayro", "dynlex")
            pair = {}
            for backend in order:
                pair[backend] = run(probe, pdf, output, backend)
            if pair["dynlex"]["sha256"] != pair["hayro"]["sha256"] or pair["dynlex"]["bytes"] != pair["hayro"]["bytes"]:
                raise RuntimeError(f"RGBA differs in round {round_index + 1}")
            for backend in ("dynlex", "hayro"):
                samples[backend].append(pair[backend])
    differences = [
        a["wall_ms"] - b["wall_ms"]
        for a, b in zip(samples["dynlex"], samples["hayro"])
    ]
    report = {
        "pdf": str(args.pdf),
        "probe": str(args.probe),
        "runs": args.runs,
        "cache": "warm process and OS cache; cache was not cleared",
        "includes": "PDF parse, render, raw RGBA file write and process startup",
        "samples": samples,
        "median_wall_ms": {name: statistics.median([s["wall_ms"] for s in values]) for name, values in samples.items()},
        "median_peak_rss_kib": {name: statistics.median([s["peak_rss_kib"] for s in values]) for name, values in samples.items()},
        "median_paired_dynlex_minus_hayro_ms": statistics.median(differences),
        "dynlex_wins": sum(value < 0 for value in differences),
        "rgba_sha256": samples["dynlex"][0]["sha256"],
    }
    rendered = json.dumps(report, indent=2)
    if args.json:
        args.json.write_text(rendered + "\n")
    print(rendered)


if __name__ == "__main__":
    main()
