#!/usr/bin/env python3
"""Diagnose the experimental OPM2 diamond's 8-bit edge disagreements.

The exact polygon area is computed with rational arithmetic. This is a
diagnostic oracle, not a replacement for Vello's f32 strip rasterizer.
"""
import argparse
import json
import subprocess
import tempfile
from fractions import Fraction
from pathlib import Path

from verify_glyph_multi_geometry import opm_bytes, pdf_bytes


OUTER = [(128, 66), (162, 96), (128, 126), (94, 96)]
HOLE = [(128, 82), (112, 96), (128, 110), (144, 96)]


def clip(points, axis, boundary, keep_greater):
    result = []
    for previous, current in zip(points[-1:] + points[:-1], points):
        a, b = previous[axis], current[axis]
        previous_inside = a >= boundary if keep_greater else a <= boundary
        current_inside = b >= boundary if keep_greater else b <= boundary
        if previous_inside != current_inside:
            ratio = (boundary - a) / (b - a)
            result.append((previous[0] + ratio * (current[0] - previous[0]),
                           previous[1] + ratio * (current[1] - previous[1])))
        if current_inside:
            result.append(current)
    return result


def signed_pixel_area(contour, x, y):
    points = [(Fraction(px), Fraction(py)) for px, py in contour]
    for axis, boundary, keep_greater in (
        (0, x, True), (0, x + 1, False),
        (1, y, True), (1, y + 1, False),
    ):
        points = clip(points, axis, Fraction(boundary), keep_greater)
    return sum((a[0] * b[1] - b[0] * a[1]
                for a, b in zip(points, points[1:] + points[:1])),
               Fraction(0)) / 2


def diagnose(probe, hayro):
    contours = [OUTER, HOLE]
    with tempfile.TemporaryDirectory(prefix="opds-glyph-edge-") as folder:
        root = Path(folder)
        scene = root / "diamond.opm"
        pdf = root / "diamond.pdf"
        actual = root / "dynlex.raw"
        expected = root / "hayro.rgba"
        scene.write_bytes(opm_bytes(contours))
        pdf.write_bytes(pdf_bytes(contours))
        subprocess.run([probe, scene, actual], check=True, capture_output=True)
        subprocess.run([hayro, pdf, "1", expected], check=True, capture_output=True)
        coverage, rgba = actual.read_bytes(), expected.read_bytes()
    if len(coverage) != 256 * 192 or len(rgba) != 256 * 192 * 4:
        raise AssertionError("wrong renderer dimensions")

    disagreements = []
    for index, dynlex in enumerate(coverage):
        pixel = rgba[index * 4:index * 4 + 4]
        if pixel == bytes((255 - dynlex,) * 3 + (255,)):
            continue
        if pixel[0] != pixel[1] or pixel[1] != pixel[2] or pixel[3] != 255:
            raise AssertionError(f"unexpected color at pixel {index}")
        x, y = index % 256, index // 256
        exact = abs(sum((signed_pixel_area(contour, x, y)
                         for contour in contours), Fraction(0))) * 255
        disagreements.append({
            "x": x, "y": y, "dynlex": dynlex,
            "hayro": 255 - pixel[0], "exact_255_area": str(exact),
        })

    # The known diamond case must remain a diagnostic until a generally
    # correct f32 strip/rounding rule is proven on more than this one shape.
    if len(disagreements) != 72:
        raise AssertionError(f"diagnostic changed: {len(disagreements)} edge pixels")
    if any(Fraction(item["exact_255_area"]).denominator != 2
           for item in disagreements):
        raise AssertionError("a disagreement is not an exact half-byte tie")
    if any(abs(item["dynlex"] - item["hayro"]) != 1
           for item in disagreements):
        raise AssertionError("a disagreement exceeds one coverage byte")
    return {
        "fixture": "diagonal_hole",
        "width": 256, "height": 192,
        "disagreements": len(disagreements),
        "exact_half_byte_ties": len(disagreements),
        "hayro_higher": sum(i["hayro"] > i["dynlex"] for i in disagreements),
        "hayro_lower": sum(i["hayro"] < i["dynlex"] for i in disagreements),
        "first_pixels": disagreements[:12],
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--hayro", type=Path, required=True)
    args = parser.parse_args()
    print(json.dumps(diagnose(args.probe, args.hayro), indent=2))


if __name__ == "__main__":
    main()
