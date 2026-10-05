#!/usr/bin/env python3
"""Paired kernel-only A/B for two OPM2 native probes on identical scenes."""
import argparse
import json
import re
import statistics
import subprocess
import tempfile
from pathlib import Path


KERNEL_MS = re.compile(r"native_dynlex_ms_per_call=([0-9.]+)")
MAX_RSS = re.compile(r"opds_max_rss_kib=(\d+)")


def run(binary, scene, output, repeats):
    process = subprocess.run(
        ["/usr/bin/time", "-f", "opds_max_rss_kib=%M",
         binary, scene, output, str(repeats)],
        check=True, text=True, capture_output=True,
    )
    elapsed = KERNEL_MS.search(process.stderr)
    rss = MAX_RSS.search(process.stderr)
    if not elapsed or not rss:
        raise AssertionError(f"probe timings missing: {process.stderr}")
    return {"kernel_ms_per_call": float(elapsed.group(1)),
            "process_max_rss_kib": int(rss.group(1)),
            "coverage": output.read_bytes()}


def bench(old, new, scenes, rounds, repeats):
    result = {"rounds": rounds, "repeats_per_process": repeats,
              "note": "Warm process A/B; kernel timing excludes validation, PDF and process startup",
              "scenes": {}}
    with tempfile.TemporaryDirectory(prefix="opds-glyph-bench-") as folder:
        root = Path(folder)
        for scene in scenes:
            if not scene.is_file():
                raise FileNotFoundError(scene)
            old_output, new_output = root / "old.raw", root / "new.raw"
            baseline = run(old, scene, old_output, 1)["coverage"]
            if baseline != run(new, scene, new_output, 1)["coverage"]:
                raise AssertionError(f"{scene}: old/new coverage differs")
            samples = []
            for index in range(rounds):
                order = (("old", old, old_output), ("new", new, new_output))
                if index % 2:
                    order = tuple(reversed(order))
                pair = {}
                for name, binary, output in order:
                    measured = run(binary, scene, output, repeats)
                    if measured.pop("coverage") != baseline:
                        raise AssertionError(f"{scene}: output changed in round {index}")
                    pair[name] = measured
                samples.append(pair)
            old_times = [p["old"]["kernel_ms_per_call"] for p in samples]
            new_times = [p["new"]["kernel_ms_per_call"] for p in samples]
            result["scenes"][scene.name] = {
                "median_old_ms": statistics.median(old_times),
                "median_new_ms": statistics.median(new_times),
                "median_paired_delta_ms": statistics.median(n - o for n, o in zip(new_times, old_times)),
                "new_wins": sum(n < o for n, o in zip(new_times, old_times)),
                "median_old_max_rss_kib": statistics.median(p["old"]["process_max_rss_kib"] for p in samples),
                "median_new_max_rss_kib": statistics.median(p["new"]["process_max_rss_kib"] for p in samples),
                "samples": samples,
            }
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--old", required=True, type=Path)
    parser.add_argument("--new", required=True, type=Path)
    parser.add_argument("--scene", required=True, type=Path, action="append")
    parser.add_argument("--rounds", type=int, default=30)
    parser.add_argument("--repeats", type=int, default=100)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if not 1 <= args.rounds <= 100 or not 1 <= args.repeats <= 1000:
        parser.error("rounds/repeats out of bounds")
    report = bench(args.old.resolve(), args.new.resolve(), args.scene,
                   args.rounds, args.repeats)
    encoded = json.dumps(report, indent=2) + "\n"
    if args.output:
        args.output.write_text(encoded)
    print(encoded)


if __name__ == "__main__":
    main()
