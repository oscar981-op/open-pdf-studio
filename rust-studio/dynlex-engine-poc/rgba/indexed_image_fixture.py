#!/usr/bin/env python3
"""Generate bounded Indexed/DeviceRGB image PDFs for native DynLex tests."""

import argparse
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from pdf_fixture import build_pdf  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    out = args.output
    out.mkdir(parents=True, exist_ok=True)
    palette = bytes((255, 0, 0, 0, 180, 0, 0, 0, 255, 30, 30, 30))
    indices = bytes((x + 2 * y) % 4 for y in range(4) for x in range(4))
    base = dict(image_source_size=(4, 4), image_placement=(21.25, 31.5, 63.5, 47.25),
                indexed_palette=palette, indexed_pixels=indices)
    out.joinpath("indexed-raw-raster.pdf").write_bytes(build_pdf("raster", **base))
    out.joinpath("indexed-flate-hybrid.pdf").write_bytes(
        build_pdf("hybrid", flate_image=True, **base))
    out.joinpath("indexed-rotate-crop.pdf").write_bytes(
        build_pdf("raster", flate_image=True,
                  pages_extra="/Rotate 90 /CropBox [8 12 248 180]", **base))
    real_style_palette = bytes(channel for i in range(256)
                               for channel in (i, 255 - i, (i * 73) & 255))
    real_style_indices = bytes((x * 5 + y * 11) & 255
                               for y in range(232) for x in range(1428))
    out.joinpath("indexed-256-palette-large.pdf").write_bytes(build_pdf(
        "raster", width=1684, height=1191, flate_image=True,
        image_source_size=(1428, 232),
        image_placement=(510.25, 412.5, 357, 58),
        indexed_palette=real_style_palette, indexed_pixels=real_style_indices))
    out.joinpath("indexed-invalid-index.pdf").write_bytes(build_pdf(
        "raster", indexed_palette=palette,
        indexed_pixels=indices[:-1] + bytes([4]),
        image_source_size=(4, 4), image_placement=(21, 31, 64, 48)))
    valid = build_pdf("raster", indexed_palette=palette, indexed_pixels=indices,
                      image_source_size=(4, 4), image_placement=(21, 31, 64, 48))
    wrong_high = valid.replace(b"/Indexed /DeviceRGB 3 <", b"/Indexed /DeviceRGB 4 <", 1)
    assert len(wrong_high) == len(valid) and wrong_high != valid
    out.joinpath("indexed-invalid-palette.pdf").write_bytes(wrong_high)
    out.joinpath("indexed-unsupported-decode.pdf").write_bytes(build_pdf(
        "raster", image_extra="/Decode [1 0]", indexed_palette=palette,
        indexed_pixels=indices, image_source_size=(4, 4),
        image_placement=(21, 31, 64, 48)))


if __name__ == "__main__":
    main()
