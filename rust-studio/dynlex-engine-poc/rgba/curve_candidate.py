#!/usr/bin/env python3
"""Experimental supersampled cubic stroke; diagnose differences from Hayro.

This is deliberately separate from every PDF adapter and DynLex render path.
It tests a candidate geometry/coverage rule, not a supported PDF feature.
"""

import argparse
import bisect
import json
import math
import zlib
from pathlib import Path
from sys import path as search_path

search_path.insert(0, str(Path(__file__).resolve().parent.parent))
from pdf_fixture import HEIGHT, WIDTH

# PDF control points transformed to the top-left page coordinate system.
CURVES = {
    "arch": ((20.5, 111.5), (35.5, 61.5), (75.5, 61.5), (90.5, 111.5)),
    "s-bend": ((20.5, 111.5), (55.5, 51.5), (55.5, 171.5), (90.5, 111.5)),
}


def cubic(points, t):
    a, b, c, d = points
    u = 1 - t
    weights = (u * u * u, 3 * u * u * t, 3 * u * t * t, t * t * t)
    return tuple(sum(point[axis] * weight for point, weight in zip(points, weights))
                 for axis in (0, 1))


def candidate(points, samples, segments=512):
    polyline = [cubic(points, step / segments) for step in range(segments + 1)]
    xs = [point[0] for point in polyline]
    result = bytearray(WIDTH * HEIGHT)
    low_x = max(0, math.floor(min(xs)) - 2)
    high_x = min(WIDTH, math.ceil(max(xs)) + 2)
    low_y = max(0, math.floor(min(point[1] for point in polyline)) - 2)
    high_y = min(HEIGHT, math.ceil(max(point[1] for point in polyline)) + 2)
    first_direction = (polyline[1][0] - polyline[0][0],
                       polyline[1][1] - polyline[0][1])
    last_direction = (polyline[-1][0] - polyline[-2][0],
                      polyline[-1][1] - polyline[-2][1])
    for y in range(low_y, high_y):
        for x in range(low_x, high_x):
            inside = 0
            for sy in range(samples):
                py = y + (sy + 0.5) / samples
                for sx in range(samples):
                    px = x + (sx + 0.5) / samples
                    if ((px - polyline[0][0]) * first_direction[0]
                            + (py - polyline[0][1]) * first_direction[1] < 0
                            or (px - polyline[-1][0]) * last_direction[0]
                            + (py - polyline[-1][1]) * last_direction[1] > 0):
                        continue
                    first = max(0, bisect.bisect_left(xs, px - 0.5) - 1)
                    last = min(segments, bisect.bisect_right(xs, px + 0.5))
                    for index in range(first, last):
                        ax, ay = polyline[index]
                        bx, by = polyline[index + 1]
                        dx, dy = bx - ax, by - ay
                        fraction = min(1.0, max(0.0, ((px - ax) * dx + (py - ay) * dy)
                                                      / (dx * dx + dy * dy)))
                        nearest_x = ax + fraction * dx
                        nearest_y = ay + fraction * dy
                        if (px - nearest_x) ** 2 + (py - nearest_y) ** 2 <= 0.25:
                            inside += 1
                            break
            result[y * WIDTH + x] = round(inside * 255 / (samples * samples))
    return bytes(result)


def compare(actual, expected):
    differences = [index for index, (a, b) in enumerate(zip(actual, expected)) if a != b]
    return {
        "different_pixels": len(differences),
        "absolute_error_sum": sum(abs(actual[i] - expected[i]) for i in differences),
        "max_error": max((abs(actual[i] - expected[i]) for i in differences), default=0),
        "false_positive_pixels": sum(actual[i] > 0 and expected[i] == 0 for i in differences),
        "false_negative_pixels": sum(actual[i] == 0 and expected[i] > 0 for i in differences),
    }


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--coverage-dir", type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument("--samples", type=int, nargs="+", default=[4, 8, 16])
    parser.add_argument("--segments", type=int, default=512)
    parser.add_argument("--expected-report", type=Path)
    args = parser.parse_args()
    if args.segments < 8 or any(sample < 1 or sample > 64 for sample in args.samples):
        parser.error("invalid sample or segment count")
    report = {}
    for name, points in CURVES.items():
        reference = zlib.decompress((args.coverage_dir / f"curve-coverage-{name}.zlib").read_bytes())
        if len(reference) != WIDTH * HEIGHT:
            raise RuntimeError(f"wrong reference coverage length: {name}")
        report[name] = {str(sample): compare(candidate(points, sample, args.segments), reference)
                        for sample in args.samples}
    if args.expected_report and report != json.loads(args.expected_report.read_text()):
        raise RuntimeError("curve candidate report changed")
    print(json.dumps(report, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
