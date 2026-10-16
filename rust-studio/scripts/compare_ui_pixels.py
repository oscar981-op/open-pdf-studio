#!/usr/bin/env python3
"""Compare same-size Tauri and Rust UI screenshots without masking differences."""

import argparse
import json
from pathlib import Path

from PIL import Image, ImageChops


def load_image(path: Path) -> Image.Image:
    if path.suffix == ".rgba":
        width, height = map(int, path.with_suffix(".size").read_text().split())
        rgba = path.read_bytes()
        if len(rgba) != width * height * 4:
            raise ValueError(f"invalid RGBA length in {path}")
        return Image.frombytes("RGBA", (width, height), rgba)
    with Image.open(path) as image:
        return image.convert("RGBA")


def metrics(left: Image.Image, right: Image.Image, box: tuple[int, int, int, int]) -> dict:
    a = left.crop(box).convert("RGBA")
    b = right.crop(box).convert("RGBA")
    diff = ImageChops.difference(a, b)
    width, height = a.size
    pixels = width * height
    changed = 0
    channel_error = 0
    for r, g, blue, alpha in diff.getdata():
        changed += bool(r or g or blue or alpha)
        channel_error += r + g + blue + alpha
    return {
        "box": list(box),
        "pixels": pixels,
        "exact_pixels": pixels - changed,
        "changed_pixels": changed,
        "changed_percent": round(100 * changed / pixels, 3),
        "mean_absolute_channel_error": round(channel_error / (pixels * 4), 3),
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("tauri", type=Path)
    parser.add_argument("rust", type=Path)
    parser.add_argument("--diff", type=Path, help="Write a red-highlighted diff PNG")
    args = parser.parse_args()
    tauri = load_image(args.tauri)
    rust = load_image(args.rust)
    if tauri.size != rust.size:
        parser.error(f"screenshot dimensions differ: {tauri.size} vs {rust.size}")
    width, height = tauri.size
    regions = {"whole": (0, 0, width, height)}
    if (width, height) == (1200, 832):
        regions.update(
            titlebar=(0, 0, width, 30),
            tabs=(0, 30, width, 58),
            ribbon=(0, 58, width, 153),
            document_tabs=(0, 153, width, 180),
            workspace=(0, 180, width, 810),
            statusbar=(0, 810, width, height),
            left_rail=(0, 180, 30, 810),
            thumbnails=(30, 180, 200, 810),
            drawing_tools=(200, 180, 285, 810),
            page_area=(285, 180, 680, 810),
            toolpalette=(680, 180, 900, 810),
            properties=(900, 180, width, 810),
        )
    report = {name: metrics(tauri, rust, box) for name, box in regions.items()}
    print(json.dumps(report, indent=2))
    if args.diff:
        diff = ImageChops.difference(tauri, rust).convert("RGB")
        mask = diff.convert("L").point(lambda value: 140 if value else 0)
        red = Image.new("RGB", tauri.size, (255, 32, 32))
        overlay = Image.composite(red, tauri.convert("RGB"), mask)
        args.diff.parent.mkdir(parents=True, exist_ok=True)
        overlay.save(args.diff)


if __name__ == "__main__":
    main()
