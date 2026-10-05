#!/usr/bin/env python3
"""Generate controlled PDFs with rectangular clips around RGB/Indexed images."""

import argparse
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from pdf_fixture import build_pdf  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    output = args.output
    output.mkdir(parents=True, exist_ok=True)
    placement = (21.25, 31.5, 63.5, 47.25)
    rgb = bytes(channel for y in range(4) for x in range(4)
                for channel in (x * 70, y * 70, (x + y) * 25))
    common = dict(image_source_size=(4, 4), image_source_override=rgb,
                  image_placement=placement)
    clipped = b"30 25 45 35 re\nW\nn\n"
    output.joinpath("clipped-fractional-rgb.pdf").write_bytes(build_pdf(
        "raster", pre_content=b"0 0 1 rg\n15 20 100 80 re\nf\n" + clipped,
        flate_image=True, **common))
    output.joinpath("clipped-fractional-rotate-crop.pdf").write_bytes(build_pdf(
        "raster", pre_content=clipped, flate_image=True,
        pages_extra="/Rotate 90 /CropBox [8 12 248 180]", **common))
    output.joinpath("clipped-double-intersection.pdf").write_bytes(build_pdf(
        "raster", pre_content=clipped + b"45 30 22 44 re\nW\nn\n", **common))
    output.joinpath("clipped-restore-after-image.pdf").write_bytes(build_pdf(
        "raster", pre_content=b"q\n" + clipped,
        extra_content=b"Q\nq\n20 0 0 16 100 90 cm\n/Im0 Do\nQ\n", **common))
    palette = bytes((255, 0, 0, 0, 180, 0, 0, 0, 255, 30, 30, 30))
    indices = bytes((x + 2 * y) % 4 for y in range(4) for x in range(4))
    output.joinpath("clipped-indexed-rgb.pdf").write_bytes(build_pdf(
        "raster", pre_content=clipped, indexed_palette=palette,
        indexed_pixels=indices, image_source_size=(4, 4),
        image_placement=placement, flate_image=True))
    output.joinpath("clipped-fully-outside.pdf").write_bytes(build_pdf(
        "raster", pre_content=b"100 100 20 20 re\nW\nn\n", **common))
    output.joinpath("clipped-nonrect-unsupported.pdf").write_bytes(build_pdf(
        "raster", pre_content=b"30 25 m\n75 25 l\n52 60 l\nh\nW\nn\n", **common))


if __name__ == "__main__":
    main()
