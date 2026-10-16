#!/usr/bin/env python3
"""Paired strict four-kernel measurements on pinned public drawing PDFs.

Includes synthetic annotated copies. Rejected pages receive no timing score.
This is a fresh-process CLI measurement, not interactive zoom latency.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import statistics
import tempfile
from pathlib import Path

from bench_public_corpus import BACKENDS, compare, run


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--pdfium-library", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--rounds", type=int, default=3)
    args = parser.parse_args()
    if not 1 <= args.rounds <= 10:
        parser.error("rounds must be between 1 and 10")
    probe = args.probe.resolve(strict=True)
    library = args.pdfium_library.resolve(strict=True)
    env = dict(os.environ, OPDS_PDFIUM_LIBRARY=str(library))
    sources = json.loads(args.manifest.read_text())["sources"]
    if len(sources) != 9:
        raise ValueError("expected nine pinned drawing sources")
    report = {
        "protocol": "Same original or synthetic commented PDF, page and scale. Strict backends, no fallback. "
        "Untimed preflight then alternating process order, warm OS cache. Timed wall includes "
        "process startup, parse, render and RGBA write; /usr/bin/time gives process peak RSS. "
        "Unsupported has no speed score. This is not GUI or first zoom latency.",
        "rounds": args.rounds,
        "probe_sha256": hashlib.sha256(probe.read_bytes()).hexdigest(),
        "pdfium_sha256": hashlib.sha256(library.read_bytes()).hexdigest(),
        "platform": platform.platform(),
        "cases": [],
    }
    args.output.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="opds-drawings-four-") as temporary:
        folder = Path(temporary)
        for item in sources:
            for kind in ("original", "derived_drawing_comments"):
                meta = item[kind]
                pdf = Path(meta["path"]).resolve(strict=True)
                if hashlib.sha256(pdf.read_bytes()).hexdigest() != meta["sha256"]:
                    raise ValueError(f"source hash changed: {pdf}")
                page = 5 if "woodson" in pdf.name else 3 if "yosemite" in pdf.name else 1
                for scale in (0.25, 1.0):
                    case = {"pdf": str(pdf), "kind": kind, "page": page, "scale": scale, "backends": {}}
                    report["cases"].append(case)
                    preflight = {}
                    for backend in BACKENDS:
                        row, bitmap = run(probe, pdf, backend, page, scale, folder, env)
                        preflight[backend] = (row, bitmap)
                        case["backends"][backend] = {"preflight": row, "samples": []}
                    reference, reference_bitmap = preflight["hayro"]
                    for backend, (row, bitmap) in preflight.items():
                        if bitmap is not None and reference_bitmap is not None:
                            case["backends"][backend]["pixels_vs_hayro"] = compare(
                                row, bitmap, reference, reference_bitmap
                            )
                    supported = [backend for backend in BACKENDS if preflight[backend][1] is not None]
                    for round_index in range(args.rounds):
                        ordered = supported[round_index % len(supported):] + supported[:round_index % len(supported)] if supported else []
                        for backend in ordered:
                            row, _ = run(probe, pdf, backend, page, scale, folder, env)
                            if row["status"] != "rendered" or row["sha256"] != preflight[backend][0]["sha256"]:
                                raise ValueError("inconsistent repeated output")
                            case["backends"][backend]["samples"].append(row)
                    for backend in supported:
                        rows = case["backends"][backend]["samples"]
                        case["backends"][backend]["summary"] = {
                            "median_wall_ms": statistics.median(row["wall_ms"] for row in rows),
                            "median_peak_rss_kib": statistics.median(row["peak_rss_kib"] for row in rows),
                        }
                    args.output.write_text(json.dumps(report, indent=2) + "\n")
                    print(pdf.name, page, scale, {
                        backend: case["backends"][backend].get("summary", {}).get(
                            "median_wall_ms", case["backends"][backend]["preflight"]["status"]
                        ) for backend in BACKENDS
                    }, flush=True)


if __name__ == "__main__":
    main()
