#!/usr/bin/env python3
"""Independent polygon-area candidate for the two recorded stroke outlines.

Reads the Kurbo outline as geometry only. It does not call Vello or the app.
"""

import argparse
import json
import math
import zlib
from pathlib import Path

from curve_candidate import compare
from pdf_fixture import HEIGHT, WIDTH

HERE = Path(__file__).resolve().parent


def cubic(a, b, c, d, t):
    u = 1 - t
    return tuple(u**3 * a[i] + 3 * u*u*t * b[i] + 3 * u*t*t * c[i] + t**3 * d[i]
                 for i in (0, 1))


def read_outline(path, steps):
    points = []
    current = None
    for line in path.read_text().splitlines():
        if line.startswith("#"):
            continue
        parts = line.split()
        values = [float(value) for value in parts[1:]]
        if parts[0] == "M":
            current = tuple(values)
            points.append(current)
        elif parts[0] == "L":
            current = tuple(values)
            points.append(current)
        elif parts[0] == "C":
            control1, control2, end = tuple(values[0:2]), tuple(values[2:4]), tuple(values[4:6])
            points.extend(cubic(current, control1, control2, end, i / steps)
                          for i in range(1, steps + 1))
            current = end
        elif parts[0] != "Z":
            raise ValueError(f"unsupported outline command: {parts[0]}")
    return points


def read_flat_lines(path):
    points = []
    for line in path.read_text().splitlines():
        if line.startswith("#"):
            continue
        x0, y0, x1, y1 = map(float, line.split())
        if not points:
            points.append((x0, y0))
        elif abs(points[-1][0] - x0) > 0.000001 or abs(points[-1][1] - y0) > 0.000001:
            raise ValueError("flattened outline has a disconnected segment")
        points.append((x1, y1))
    if not points or any(abs(a - b) > 0.000001 for a, b in zip(points[0], points[-1])):
        raise ValueError("flattened outline does not close")
    return points[:-1]


def clip(poly, axis, boundary, keep_greater):
    if not poly:
        return []
    result = []
    prev = poly[-1]
    prev_inside = prev[axis] >= boundary if keep_greater else prev[axis] <= boundary
    for point in poly:
        inside = point[axis] >= boundary if keep_greater else point[axis] <= boundary
        if inside != prev_inside:
            ratio = (boundary - prev[axis]) / (point[axis] - prev[axis])
            result.append((prev[0] + ratio * (point[0] - prev[0]),
                           prev[1] + ratio * (point[1] - prev[1])))
        if inside:
            result.append(point)
        prev, prev_inside = point, inside
    return result


def pixel_area(poly, x, y):
    for axis, boundary, greater in ((0, x, True), (0, x + 1, False),
                                    (1, y, True), (1, y + 1, False)):
        poly = clip(poly, axis, boundary, greater)
        if not poly:
            return 0.0
    signed = sum(a[0] * b[1] - b[0] * a[1]
                 for a, b in zip(poly, poly[1:] + poly[:1])) / 2
    return min(1.0, abs(signed))


def rasterize(poly):
    result = bytearray(WIDTH * HEIGHT)
    min_x = max(0, math.floor(min(p[0] for p in poly)))
    max_x = min(WIDTH, math.ceil(max(p[0] for p in poly)))
    min_y = max(0, math.floor(min(p[1] for p in poly)))
    max_y = min(HEIGHT, math.ceil(max(p[1] for p in poly)))
    for y in range(min_y, max_y):
        for x in range(min_x, max_x):
            result[y * WIDTH + x] = round(pixel_area(poly, x, y) * 255)
    return bytes(result)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--steps", type=int, nargs="+", default=[16, 64, 256])
    parser.add_argument("--expected-report", type=Path)
    parser.add_argument("--flat-lines", action="store_true",
                        help="use actual Vello-flattened stroke segments instead of uniform steps")
    args = parser.parse_args()
    if any(step < 2 or step > 512 for step in args.steps):
        parser.error("steps must be 2..512")
    report = {}
    for name in ("arch", "s-bend"):
        oracle = zlib.decompress((HERE / f"curve-coverage-{name}.zlib").read_bytes())
        report[name] = {}
        if args.flat_lines:
            polygon = read_flat_lines(HERE / f"curve-flat-{name}.txt")
            report[name]["vello_flatten"] = compare(rasterize(polygon), oracle)
        else:
            for steps in args.steps:
                polygon = read_outline(HERE / f"curve-outline-{name}.txt", steps)
                report[name][str(steps)] = compare(rasterize(polygon), oracle)
    if args.expected_report and report != json.loads(args.expected_report.read_text()):
        raise RuntimeError("area coverage candidate changed")
    print(json.dumps(report, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
