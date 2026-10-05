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
    fractional = b"30.25 25.25 45.5 35.5 re\nW\nn\n"
    blue = b"0 0 1 rg\n10 10 110 90 re\nf\n"
    output.joinpath("clipped-subpixel-rgb.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + fractional,
        image_source_size=(4, 4), image_source_override=bytes([255, 0, 0]) * 16,
        image_placement=(20, 20, 80, 60)))
    output.joinpath("clipped-subpixel-image.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + fractional, flate_image=True, **common))
    output.joinpath("clipped-subpixel-rotate.pdf").write_bytes(build_pdf(
        "raster", pre_content=fractional,
        pages_extra="/Rotate 90 /CropBox [8 12 248 180]", **common))
    for turn in (180, 270):
        output.joinpath(f"clipped-subpixel-rotate-{turn}.pdf").write_bytes(build_pdf(
            "raster", pre_content=fractional,
            pages_extra=f"/Rotate {turn} /CropBox [8 12 248 180]", **common))
    output.joinpath("clipped-subpixel-global-ctm.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue +
        b"q\n0.1 0 0 0.1 0 0 cm\n302.5 252.5 455 355 re\nW\nn\n",
        image_source_size=(4, 4), image_source_override=rgb,
        image_placement=(212.5, 315, 635, 472.5), extra_content=b"Q\n"))
    output.joinpath("clipped-subpixel-evenodd.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + fractional.replace(b"\nW\n", b"\nW*\n"),
        image_source_size=(4, 4), image_source_override=bytes([255, 0, 0]) * 16,
        image_placement=(20, 20, 80, 60)))
    output.joinpath("clipped-subpixel-unsupported-fill.pdf").write_bytes(build_pdf(
        "raster", pre_content=fractional + b"0 1 0 rg\n10.25 12 20 10 re\nf\n", **common))
    output.joinpath("clipped-subpixel-ambiguous.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + b"29.05 35.01 20.25 5.5 re\nW\nn\n",
        pages_extra="/Rotate 180 /CropBox [8 12 248 180]",
        image_source_size=(4, 4), image_source_override=rgb,
        image_placement=(20, 20, 80, 60)))
    output.joinpath("clipped-fully-outside.pdf").write_bytes(build_pdf(
        "raster", pre_content=b"100 100 20 20 re\nW\nn\n", **common))
    output.joinpath("clipped-nonrect-unsupported.pdf").write_bytes(build_pdf(
        "raster", pre_content=b"30 25 m\n75 25 l\n52 60 l\nh\nW\nn\n", **common))


if __name__ == "__main__":
    main()
