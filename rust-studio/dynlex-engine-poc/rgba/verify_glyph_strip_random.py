#!/usr/bin/env python3
"""Differentially probe scalar f32 strip coverage on deterministic diamonds."""
import argparse
import json
import random
import subprocess
import tempfile
from fractions import Fraction
from pathlib import Path

from diagnose_glyph_multi_edges import signed_pixel_area
from verify_glyph_multi_geometry import opm_bytes, pdf_bytes


def verify(probe, hayro, cases, seed, compare_probe=None):
    rng = random.Random(seed)
    mismatch_cases = []
    total_mismatches = 0
    with tempfile.TemporaryDirectory(prefix="opds-strip-random-") as folder:
        root = Path(folder)
        scene, pdf = root / "scene.opm", root / "scene.pdf"
        raw, rgba_file = root / "actual.raw", root / "expected.rgba"
        compared = root / "compared.raw"
        for case in range(cases):
            cx, cy = rng.randint(35, 220), rng.randint(35, 155)
            rx, ry = rng.randint(8, 25), rng.randint(8, 25)
            hx, hy = rng.randint(2, rx - 3), rng.randint(2, ry - 3)
            contours = [
                [(cx, cy - ry), (cx + rx, cy), (cx, cy + ry), (cx - rx, cy)],
                [(cx, cy - hy), (cx - hx, cy), (cx, cy + hy), (cx + hx, cy)],
            ]
            scene.write_bytes(opm_bytes(contours))
            pdf.write_bytes(pdf_bytes(contours))
            subprocess.run([probe, scene, raw], check=True, capture_output=True)
            subprocess.run([hayro, pdf, "1", rgba_file], check=True, capture_output=True)
            coverage, rgba = raw.read_bytes(), rgba_file.read_bytes()
            if compare_probe:
                subprocess.run([compare_probe, scene, compared], check=True,
                               capture_output=True)
                if compared.read_bytes() != coverage:
                    raise AssertionError(f"case {case}: DynLex/scalar bytes differ")
            if len(coverage) != 256 * 192 or len(rgba) != 256 * 192 * 4:
                raise AssertionError(f"case {case}: wrong output dimensions")
            mismatches = []
            for index, ink in enumerate(coverage):
                pixel = rgba[index * 4:index * 4 + 4]
                if pixel == bytes((255 - ink,) * 3 + (255,)):
                    continue
                x, y = index % 256, index // 256
                hayro_ink = 255 - pixel[0]
                exact = abs(sum((signed_pixel_area(c, x, y) for c in contours),
                                Fraction(0))) * 255
                if (pixel[0] != pixel[1] or pixel[1] != pixel[2]
                        or pixel[3] != 255 or abs(ink - hayro_ink) != 1
                        or exact.denominator != 2):
                    raise AssertionError(
                        f"case {case} pixel ({x},{y}): non-half-byte difference "
                        f"DynLex={ink} Hayro={hayro_ink} exact={exact}")
                mismatches.append([x, y, ink, hayro_ink, str(exact)])
            if mismatches:
                total_mismatches += len(mismatches)
                mismatch_cases.append({"case": case, "center": [cx, cy],
                                       "outer_radius": [rx, ry],
                                       "hole_radius": [hx, hy],
                                       "pixels": mismatches})
        invalid_contours = [
            [[(-1, 20), (20, 20), (20, 40), (-1, 40)],
             [(40, 20), (50, 20), (50, 40), (40, 40)]],
            [[(100, 80), (100.000001, 80), (100.000001, 90), (100, 90)],
             [(130, 80), (140, 80), (140, 90), (130, 90)]],
        ]
        for index, contours in enumerate(invalid_contours):
            scene.write_bytes(opm_bytes(contours))
            raw.unlink(missing_ok=True)
            rejected = subprocess.run([probe, scene, raw], capture_output=True)
            if rejected.returncode == 0 or raw.exists():
                raise AssertionError(f"invalid f32-strip scene {index} was accepted")
    return {"cases": cases, "seed": seed,
            "exact_cases": cases - len(mismatch_cases),
            "half_byte_mismatch_cases": len(mismatch_cases),
            "half_byte_mismatch_pixels": total_mismatches,
            "invalid_scenes_rejected": len(invalid_contours),
            "mismatch_cases": mismatch_cases}


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--hayro", type=Path, required=True)
    parser.add_argument("--compare-probe", type=Path,
                        help="require identical full buffers from another kernel")
    parser.add_argument("--cases", type=int, default=200)
    parser.add_argument("--seed", type=int, default=20261005)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    if not 1 <= args.cases <= 2000:
        parser.error("case count out of bounds")
    report = verify(args.probe, args.hayro, args.cases, args.seed,
                    args.compare_probe)
    if args.cases == 200 and args.seed == 20261005 and (
        report["half_byte_mismatch_cases"] != 3
        or report["half_byte_mismatch_pixels"] != 6
    ):
        raise AssertionError("golden scalar-strip diagnostic changed; revalidate")
    encoded = json.dumps(report, indent=2) + "\n"
    if args.output:
        args.output.write_text(encoded)
    print(encoded)


if __name__ == "__main__":
    main()
