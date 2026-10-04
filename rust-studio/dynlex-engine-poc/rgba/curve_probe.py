#!/usr/bin/env python3
"""Measure small PDF cubic strokes in the existing Hayro renderer.

This is reference evidence for a future OPDL path command, not a DynLex
curve renderer or a pixel-parity test.
"""

import argparse
import hashlib
import json
import subprocess
import tempfile
import zlib
from pathlib import Path
from sys import path as search_path

search_path.insert(0, str(Path(__file__).resolve().parent.parent))
from pdf_fixture import HEIGHT, WIDTH, build_pdf  # noqa: E402

CURVES = {
    "arch": b"1 w\n0 0 0 RG\n20.5 80.5 m\n35.5 130.5 75.5 130.5 90.5 80.5 c\nS\n",
    "s-bend": b"1 w\n0 0 0 RG\n20.5 80.5 m\n55.5 140.5 55.5 20.5 90.5 80.5 c\nS\n",
}


def render(engine, work, mode, name, extra):
    pdf = work / f"{mode}-{name}.pdf"
    rgba = work / f"{mode}-{name}.rgba"
    pdf.write_bytes(build_pdf(mode, extra_content=extra))
    subprocess.run([engine, pdf, "1", rgba], check=True, capture_output=True)
    pixels = rgba.read_bytes()
    if len(pixels) != WIDTH * HEIGHT * 4:
        raise RuntimeError(f"wrong RGBA length for {mode}/{name}")
    return pixels, pdf


def measure(reference, curve):
    changed = [i for i in range(WIDTH * HEIGHT)
               if reference[4 * i:4 * i + 4] != curve[4 * i:4 * i + 4]]
    if not changed:
        raise RuntimeError("curve did not change any pixels")
    xs = [i % WIDTH for i in changed]
    ys = [i // WIDTH for i in changed]
    delta = b"".join(curve[4 * i:4 * i + 4] for i in changed)
    return {
        "changed_pixels": len(changed),
        "bounds_xy_inclusive": [min(xs), min(ys), max(xs), max(ys)],
        "distinct_changed_rgba": len(set(delta[i:i + 4] for i in range(0, len(delta), 4))),
        "delta_sha256": hashlib.sha256(delta).hexdigest(),
    }


def curve_coverage(vector_base, vector_curve, hybrid_base, hybrid_curve):
    """Recover Vello's 8-bit coverage from black ink on two backgrounds."""
    coverage = bytearray(WIDTH * HEIGHT)
    for pixel in range(WIDTH * HEIGHT):
        offset = pixel * 4
        vb = vector_base[offset:offset + 3]
        vc = vector_curve[offset:offset + 3]
        hb = hybrid_base[offset:offset + 3]
        hc = hybrid_curve[offset:offset + 3]
        if vb == vc and hb == hc:
            continue
        candidates = [alpha for alpha in range(256)
                      if bytes((channel * (255 - alpha) + 255) // 256
                               for channel in vb) == vc
                      and bytes((channel * (255 - alpha) + 255) // 256
                                for channel in hb) == hc]
        if len(candidates) != 1:
            raise RuntimeError(f"curve pixel {pixel} has {len(candidates)} coverages")
        coverage[pixel] = candidates[0]
    return bytes(coverage)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--hayro", type=Path, required=True)
    parser.add_argument("--adapter", type=Path, help="also verify curves remain rejected")
    parser.add_argument("--coverage-dir", type=Path,
                        help="write compressed 8-bit reference coverage maps")
    parser.add_argument("--expected-report", type=Path,
                        help="fail if measured pixels differ from a saved reference report")
    args = parser.parse_args()
    engine = args.hayro.resolve(strict=True)
    adapter = args.adapter.resolve(strict=True) if args.adapter else None
    report = {"width": WIDTH, "height": HEIGHT, "samples": {}}
    with tempfile.TemporaryDirectory(prefix="opds-curve-probe-") as folder:
        work = Path(folder)
        rendered = {}
        for mode in ("vector", "hybrid"):
            baseline, _ = render(engine, work, mode, "baseline", b"")
            for name, content in CURVES.items():
                pixels, pdf = render(engine, work, mode, name, content)
                rendered[mode, name] = (baseline, pixels)
                report["samples"][f"{mode}/{name}"] = measure(baseline, pixels)
                if adapter:
                    output = work / f"{mode}-{name}.opdl"
                    rejected = subprocess.run([adapter, pdf, output], capture_output=True)
                    if rejected.returncode == 0 or output.exists():
                        raise RuntimeError(f"unverified curve accepted: {mode}/{name}")
        for name in CURVES:
            coverage = curve_coverage(*rendered["vector", name],
                                      *rendered["hybrid", name])
            report["samples"][f"vector/{name}"]["coverage_sha256"] = (
                hashlib.sha256(coverage).hexdigest())
            report["samples"][f"vector/{name}"]["nonzero_coverage_pixels"] = (
                sum(bool(value) for value in coverage))
            if args.coverage_dir:
                args.coverage_dir.mkdir(parents=True, exist_ok=True)
                (args.coverage_dir / f"curve-coverage-{name}.zlib").write_bytes(
                    zlib.compress(coverage, level=9))
    if args.expected_report and report != json.loads(args.expected_report.read_text()):
        raise RuntimeError("curve coverage or pixel report changed")
    print(json.dumps(report, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
