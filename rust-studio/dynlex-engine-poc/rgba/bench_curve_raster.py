#!/usr/bin/env python3
"""Paired warm process benchmark for experimental flattened-outline rasterizers."""
import argparse
import json
import statistics
import subprocess
import tempfile
import time
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent


def timed(binary, source, output):
    with tempfile.NamedTemporaryFile(prefix="opds-curve-rss-") as tmp:
        start = time.perf_counter_ns()
        subprocess.run(["/usr/bin/time", "-f", "%M", "-o", tmp.name,
                        str(binary), str(source), str(output)],
                       check=True, capture_output=True)
        elapsed = (time.perf_counter_ns() - start) / 1e6
        rss = int(Path(tmp.name).read_text().strip())
    return {"wall_ms": elapsed, "peak_rss_kib": rss}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--dynlex", required=True, type=Path)
    parser.add_argument("--rust", required=True, type=Path)
    parser.add_argument("--rounds", type=int, default=15)
    parser.add_argument("--json", type=Path)
    args = parser.parse_args()
    if not 1 <= args.rounds <= 100:
        parser.error("rounds must be 1..100")
    binaries = {"dynlex": args.dynlex.resolve(strict=True), "rust": args.rust.resolve(strict=True)}
    report = {"note": "paired warm process runs; includes startup and file I/O; OS cache not cleared", "rounds": args.rounds, "fixtures": {}}
    with tempfile.TemporaryDirectory(prefix="opds-curve-bench-") as folder:
        output = Path(folder)/"coverage.raw"
        for name in ("arch", "s-bend"):
            source = HERE/f"curve-flat-{name}.txt"
            expected = zlib.decompress((HERE/f"curve-coverage-{name}.zlib").read_bytes())
            samples = {key: [] for key in binaries}
            for index in range(args.rounds):
                order = ("dynlex", "rust") if index%2==0 else ("rust", "dynlex")
                for key in order:
                    samples[key].append(timed(binaries[key],source,output))
                    if output.read_bytes()!=expected:
                        raise RuntimeError(f"{name}: {key} differs from Hayro")
            medians = {key: {metric: statistics.median(s[metric] for s in values)
                             for metric in ("wall_ms","peak_rss_kib")}
                       for key,values in samples.items()}
            report["fixtures"][name] = {"median": medians, "samples": samples}
            print(f"{name}: DynLex/Rust wall {medians['dynlex']['wall_ms']:.3f}/{medians['rust']['wall_ms']:.3f} ms, "
                  f"RSS {medians['dynlex']['peak_rss_kib']}/{medians['rust']['peak_rss_kib']} KiB")
    if args.json:
        args.json.write_text(json.dumps(report,indent=2)+"\n")


if __name__ == "__main__":
    main()
