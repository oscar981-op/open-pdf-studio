#!/usr/bin/env python3
"""Research audit of the unverified fractional JPEG placement candidate."""
import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--candidate", type=Path, required=True)
    p.add_argument("--oracle-dir", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--require-exact", action="store_true")
    args = p.parse_args()
    fixture = args.oracle_dir / "tekst-page5-original-image-only.pdf"
    report = {"scope": "research only; native strict dispatch unchanged",
              "fixture_sha256": sha(fixture), "candidate_sha256": sha(args.candidate),
              "cases": []}
    for scale in (0.25, 0.87, 1.0, 1.15, 1.5625, 2.0):
        oracle = args.oracle_dir / f"tekst-page5-image-z{scale}.hayro.rgba"
        candidate = args.oracle_dir / f"tekst-page5-image-z{scale}.candidate.rgba"
        result = subprocess.run([str(args.candidate), str(fixture), str(candidate), str(scale)],
                                capture_output=True, text=True, timeout=120, check=True)
        m = re.search(r"(\d+)x(\d+) intermediate=(\d+)x(\d+)", result.stderr)
        if not m:
            raise ValueError(f"missing candidate dimensions: {result.stderr}")
        width, height, iw, ih = map(int, m.groups())
        actual, expected = candidate.read_bytes(), oracle.read_bytes()
        if len(actual) != len(expected) or len(actual) != width * height * 4:
            raise ValueError("bitmap length mismatch")
        indices = [i for i, (a, b) in enumerate(zip(actual, expected)) if a != b]
        pixels = {i // 4 for i in indices}
        edge = {pixel for pixel in pixels if pixel % width < 3
                or pixel % width > width - 4 or pixel // width < 3
                or pixel // width > height - 4}
        report["cases"].append({
            "scale": scale, "width": width, "height": height,
            "intermediate": [iw, ih], "rgba_bytes": len(actual),
            "oracle_sha256": sha(oracle), "candidate_sha256": sha(candidate),
            "different_channels": len(indices), "different_pixels": len(pixels),
            "edge_pixels": len(edge),
            "max_channel_error": max((abs(actual[i] - expected[i]) for i in indices), default=0),
            "first_differences": [{"x": i // 4 % width, "y": i // 4 // width,
                                   "channel": i % 4, "candidate": actual[i],
                                   "hayro": expected[i]} for i in indices[:12]],
        })
    if sha(args.candidate) != report["candidate_sha256"]:
        raise RuntimeError("candidate binary changed during audit")
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    print([(c["scale"], c["different_channels"], c["different_pixels"])
           for c in report["cases"]])
    if args.require_exact and any(c["different_channels"] for c in report["cases"]):
        raise SystemExit("partial-image probe has non-exact RGBA output")


if __name__ == "__main__":
    main()
