#!/usr/bin/env python3
"""Paired, warm-cache DynLex/Hayro PDF-to-raw-RGBA process benchmark."""

import argparse
import hashlib
import json
import os
import re
import shutil
import statistics
import subprocess
import tempfile
import time
from pathlib import Path


def run(probe: Path, pdf: Path, output: Path, backend: str, profile_phases: bool = False):
    command = [str(probe), str(pdf), str(output)]
    if backend == "hayro":
        command.append("--hayro")
    elif backend == "selected":
        command.append("--selected")
    started = time.perf_counter_ns()
    completed = subprocess.run(
        ["/usr/bin/time", "-f", "OPDS_PEAK_RSS_KIB=%M", *command],
        capture_output=True,
        text=True,
        check=True,
        env={**os.environ, "OPDS_AP_PROFILE": "1"} if profile_phases else None,
    )
    elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
    rss_line = next(
        line for line in completed.stderr.splitlines()
        if line.startswith("OPDS_PEAK_RSS_KIB=")
    )
    data = output.read_bytes()
    result = {
        "wall_ms": elapsed_ms,
        "peak_rss_kib": int(rss_line.split("=", 1)[1]),
        "sha256": hashlib.sha256(data).hexdigest(),
        "bytes": len(data),
    }
    if profile_phases:
        phases = {}
        for prefix in ("OPDS_PROBE_PHASES", "OPDS_AP_PHASES"):
            match = re.search(rf"^{prefix} (.+)$", completed.stderr, re.MULTILINE)
            if match:
                phases[prefix] = {
                    name: float(value)
                    for name, value in re.findall(r"(\w+)=([0-9.]+)", match.group(1))
                }
        if "OPDS_PROBE_PHASES" not in phases:
            raise RuntimeError("probe did not emit phase timings")
        result["phases_ms"] = phases
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pdf", type=Path)
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--runs", type=int, default=15)
    parser.add_argument(
        "--selected", action="store_true",
        help="measure the app-selected DynLex route, including imported-AP fallback",
    )
    parser.add_argument("--json", type=Path)
    parser.add_argument(
        "--profile-phases", action="store_true",
        help="capture read/open/render/write and selected imported-AP phase timings",
    )
    args = parser.parse_args()
    if not 1 <= args.runs <= 1000:
        parser.error("--runs must be 1..1000")
    if args.profile_phases and not args.selected:
        parser.error("--profile-phases requires --selected")
    if not shutil.which("/usr/bin/time"):
        parser.error("/usr/bin/time is required for per-process peak RSS")
    pdf = args.pdf.resolve(strict=True)
    probe = args.probe.resolve(strict=True)
    candidate = "selected" if args.selected else "dynlex"
    samples = {candidate: [], "hayro": []}
    with tempfile.TemporaryDirectory(prefix="opds-native-bench-") as directory:
        output = Path(directory) / "page.rgba"
        for backend in (candidate, "hayro"):
            run(probe, pdf, output, backend, args.profile_phases)
        for round_index in range(args.runs):
            order = (candidate, "hayro") if round_index % 2 == 0 else ("hayro", candidate)
            pair = {}
            for backend in order:
                pair[backend] = run(probe, pdf, output, backend, args.profile_phases)
            if pair[candidate]["sha256"] != pair["hayro"]["sha256"] or pair[candidate]["bytes"] != pair["hayro"]["bytes"]:
                raise RuntimeError(f"RGBA differs in round {round_index + 1}")
            for backend in (candidate, "hayro"):
                samples[backend].append(pair[backend])
    differences = [
        a["wall_ms"] - b["wall_ms"]
        for a, b in zip(samples[candidate], samples["hayro"])
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
        f"median_paired_{candidate}_minus_hayro_ms": statistics.median(differences),
        f"{candidate}_wins": sum(value < 0 for value in differences),
        "rgba_sha256": samples[candidate][0]["sha256"],
    }
    if args.profile_phases:
        report["median_phases_ms"] = {
            backend: {
                prefix: {
                    name: statistics.median(
                        sample["phases_ms"][prefix][name]
                        for sample in values
                    )
                    for name in values[0]["phases_ms"][prefix]
                }
                for prefix in values[0]["phases_ms"]
            }
            for backend, values in samples.items()
        }
    rendered = json.dumps(report, indent=2)
    if args.json:
        args.json.write_text(rendered + "\n")
    print(rendered)


if __name__ == "__main__":
    main()
