#!/usr/bin/env python3
"""Read-only coverage diagnosis for the isolated original W* clip at op 338.

Uses extracted PDF path coordinates and a Hayro raw RGBA render. This is a
diagnostic oracle, never an alternate renderer or an acceptance shortcut.
"""

import argparse
import collections
import json
import math
import struct
import subprocess
import tempfile
from pathlib import Path


WIDTH = 1684
HEIGHT = 1191


def f32(value):
    return struct.unpack("<f", struct.pack("<f", value))[0]


def contours_from_pdf(path, mode, rotate_crop):
    contours = []
    for line in path.read_text().splitlines():
        parts = line.split()
        if parts[-1] == "m":
            contours.append([])
        if parts[-1] in ("m", "l"):
            narrow_input = mode == "f32-input-and-display"
            x, y = (f32(float(value)) if narrow_input else float(value) for value in parts[:2])
            scale = f32(.1) if narrow_input else .1
            point = (y * scale - 20, x * scale - 1100) if rotate_crop else (
                x * scale, HEIGHT - y * scale)
            contours[-1].append(tuple(map(f32, point)) if mode != "full-f64" else point)
        elif parts[-1] not in ("h", "W*", "n"):
            raise ValueError(f"unsupported path operation: {line}")
    if len(contours) != 4 or [len(p) for p in contours] != [108, 68, 99, 56]:
        raise ValueError("unexpected corpus path")
    return contours


def inside(point, polygon):
    x, y = point
    state = False
    for a, b in zip(polygon, polygon[1:] + polygon[:1]):
        if (a[1] > y) != (b[1] > y):
            x_cross = a[0] + (y - a[1]) * (b[0] - a[0]) / (b[1] - a[1])
            if x_cross > x:
                state = not state
    return state


def clip_axis(polygon, axis, boundary, greater):
    if not polygon:
        return []
    out = []
    previous = polygon[-1]
    prev_inside = previous[axis] >= boundary if greater else previous[axis] <= boundary
    for point in polygon:
        in_now = point[axis] >= boundary if greater else point[axis] <= boundary
        if in_now != prev_inside:
            t = (boundary - previous[axis]) / (point[axis] - previous[axis])
            out.append((previous[0] + t * (point[0] - previous[0]),
                        previous[1] + t * (point[1] - previous[1])))
        if in_now:
            out.append(point)
        previous, prev_inside = point, in_now
    return out


def pixel_area(polygon, x, y):
    clipped = polygon
    for axis, boundary, greater in ((0, x, True), (0, x + 1, False),
                                    (1, y, True), (1, y + 1, False)):
        clipped = clip_axis(clipped, axis, boundary, greater)
        if not clipped:
            return 0.
    cross = sum(a[0] * b[1] - b[0] * a[1]
                for a, b in zip(clipped, clipped[1:] + clipped[:1]))
    return min(abs(cross) * .5, 1.)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--path", type=Path, default=Path(__file__).with_name("real-clip-338.content"))
    parser.add_argument("--hayro-rgba", type=Path)
    parser.add_argument("--pdf", type=Path)
    parser.add_argument("--probe", type=Path)
    parser.add_argument("--mode", choices=("full-f64", "f32-input-and-display",
                                           "f64-input-f32-display"), required=True)
    parser.add_argument("--expect-mismatches", type=int)
    parser.add_argument("--rotate-crop", action="store_true")
    args = parser.parse_args()
    if args.hayro_rgba is not None:
        pixels = args.hayro_rgba.read_bytes()
    elif args.pdf is not None and args.probe is not None:
        with tempfile.TemporaryDirectory(prefix="opds-clip338-diag-") as directory:
            output = Path(directory) / "hayro.rgba"
            subprocess.run([str(args.probe.resolve()), str(args.pdf.resolve()),
                            str(output), "--hayro"], check=True, capture_output=True)
            pixels = output.read_bytes()
    else:
        parser.error("provide --hayro-rgba or both --pdf and --probe")
    width, height = (90, 200) if args.rotate_crop else (WIDTH, HEIGHT)
    if len(pixels) != width * height * 4:
        parser.error(f"expected {width}x{height} RGBA render")
    contours = contours_from_pdf(args.path, args.mode, args.rotate_crop)
    signs = [1 if sum(inside(poly[0], other) for j, other in enumerate(contours) if i != j) % 2 == 0
             else -1 for i, poly in enumerate(contours)]
    bounds = (math.floor(min(x for poly in contours for x, _ in poly)),
              math.ceil(max(x for poly in contours for x, _ in poly)),
              math.floor(min(y for poly in contours for _, y in poly)),
              math.ceil(max(y for poly in contours for _, y in poly)))
    deviations = []
    near_half = 0
    for y in range(bounds[2], bounds[3]):
        for x in range(bounds[0], bounds[1]):
            area = max(0., min(1., sum(sign * pixel_area(poly, x, y)
                                      for sign, poly in zip(signs, contours))))
            scaled = area * 255.
            if abs(scaled % 1 - .5) < .002:
                near_half += 1
            expected_red = min(255, int(scaled + .5))
            actual = pixels[(y * width + x) * 4:(y * width + x) * 4 + 4]
            # Only image pixels over the blue underlay are comparable.
            in_image = (10 <= x < 60 and 0 <= y < 180) if args.rotate_crop else (
                1100 <= x < 1280 and HEIGHT - 80 <= y < HEIGHT - 30)
            expected = bytes((expected_red, 0, 255 - expected_red, 255))
            if in_image and expected != actual:
                deviations.append(dict(x=x, y=y, analytic=scaled, rounded=expected_red,
                                       hayro=list(actual)))
    print(json.dumps(dict(mode=args.mode, rotate_crop=args.rotate_crop,
                          contours=[len(p) for p in contours], signs=signs,
                          bounds=bounds, near_half=near_half,
                          mismatch_pixels=len(deviations), first=deviations[:30]), indent=2))
    if args.expect_mismatches is not None and len(deviations) != args.expect_mismatches:
        raise SystemExit(f"expected {args.expect_mismatches} mismatches")


if __name__ == "__main__":
    main()
