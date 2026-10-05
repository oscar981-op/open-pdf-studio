#!/usr/bin/env python3
"""Generate controlled PDFs with bounded clips around RGB/Indexed images."""

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
    polygon_rectangle = b"30 25 m\n75 25 l\n75 60 l\n30 60 l\nh\nW\nn\n"
    output.joinpath("clipped-path-rectangle.pdf").write_bytes(build_pdf(
        "raster", pre_content=b"0 0 1 rg\n15 20 100 80 re\nf\n" + polygon_rectangle,
        flate_image=True, **common))
    output.joinpath("clipped-path-rectangle-rotate.pdf").write_bytes(build_pdf(
        "raster", pre_content=polygon_rectangle,
        pages_extra="/Rotate 90 /CropBox [8 12 248 180]", **common))
    output.joinpath("clipped-path-rectangle-evenodd.pdf").write_bytes(build_pdf(
        "raster", pre_content=polygon_rectangle.replace(b"\nW\n", b"\nW*\n"), **common))
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
    fractional_polygon = b"30.25 25.25 m\n75.75 25.25 l\n75.75 60.75 l\n30.25 60.75 l\nh\nW*\nn\n"
    output.joinpath("clipped-path-subpixel-rotate.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + fractional_polygon,
        pages_extra="/Rotate 270 /CropBox [8 12 248 180]", **common))
    triangle = b"35 28 m\n90 28 l\n35 68 l\nh\nW\nn\n"
    output.joinpath("clipped-triangle.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + triangle, **common))
    output.joinpath("clipped-triangle-rotate.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + triangle,
        pages_extra="/Rotate 90 /CropBox [8 12 248 180]", **common))
    output.joinpath("clipped-triangle-evenodd.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + triangle.replace(b"\nW\n", b"\nW*\n"), **common))
    diamond = b"40 19 m\n68 36 l\n40 77 l\n30 36 l\nh\nW*\nn\n"
    output.joinpath("clipped-diamond.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + diamond, **common))
    concave = b"33 28 m\n72 28 l\n66 39 l\n72 75 l\n33 75 l\nh\nW*\nn\n"
    output.joinpath("clipped-concave.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + concave, **common))
    output.joinpath("clipped-concave-rotate.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + concave,
        pages_extra="/Rotate 90 /CropBox [8 12 248 180]", **common))
    nested_hole = (b"28 23 m\n90 23 l\n90 75 l\n28 75 l\nh\n"
                   b"43 36 m\n69 36 l\n69 59 l\n43 59 l\nh\nW*\nn\n")
    output.joinpath("clipped-two-contour-hole.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + nested_hole, **common))
    output.joinpath("clipped-two-contour-hole-rotate.pdf").write_bytes(build_pdf(
        "raster", pre_content=nested_hole,
        pages_extra="/Rotate 90 /CropBox [8 12 248 180]", **common))
    output.joinpath("clipped-two-contour-hole-restored.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + b"q\n" + nested_hole,
        extra_content=b"Q\nq\n20 0 0 16 100 90 cm\n/Im0 Do\nQ\n", **common))
    triangle_hole = (b"28 23 m\n90 23 l\n90 75 l\n28 75 l\nh\n"
                     b"44 38 m\n68 38 l\n56 58 l\nh\nW*\nn\n")
    output.joinpath("clipped-two-contour-triangle-hole.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + triangle_hole, **common))
    output.joinpath("clipped-two-contour-hole-reversed.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + nested_hole.replace(
            nested_hole[:nested_hole.index(b"43 36 m")], b"").replace(
            b"h\nW*\nn\n", b"h\n" + nested_hole[:nested_hole.index(b"43 36 m")] + b"W*\nn\n"), **common))
    output.joinpath("clipped-two-contour-nonzero-unsupported.pdf").write_bytes(build_pdf(
        "raster", pre_content=nested_hole.replace(b"W*\n", b"W\n"), **common))
    output.joinpath("clipped-two-contour-overlap-unsupported.pdf").write_bytes(build_pdf(
        "raster", pre_content=nested_hole.replace(b"43 36", b"83 36").replace(
            b"69 36", b"109 36").replace(b"69 59", b"109 59").replace(
            b"43 59", b"83 59"), **common))
    third_contour = b"48 41 m\n56 41 l\n56 48 l\n48 48 l\nh\n"
    output.joinpath("clipped-three-contour-unsupported.pdf").write_bytes(build_pdf(
        "raster", pre_content=nested_hole.replace(b"W*\nn\n",
                                                  third_contour + b"W*\nn\n"), **common))
    output.joinpath("clipped-two-contour-open-unsupported.pdf").write_bytes(build_pdf(
        "raster", pre_content=nested_hole.replace(b"43 59 l\nh\n", b"43 59 l\n"), **common))
    output.joinpath("clipped-triangle-restored.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + b"q\n" + triangle,
        extra_content=b"Q\nq\n20 0 0 16 100 90 cm\n/Im0 Do\nQ\n", **common))
    output.joinpath("clipped-polygon-ambiguous.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue +
        b"25 25 m\n75 25 l\n50 70 l\nh\nW\nn\n", **common))
    output.joinpath("clipped-polygon-ambiguous-rotate.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue +
        b"25 25 m\n75 25 l\n50 70 l\nh\nW\nn\n",
        pages_extra="/Rotate 90 /CropBox [8 12 248 180]", **common))
    output.joinpath("clipped-polygon-fill-unsupported.pdf").write_bytes(build_pdf(
        "raster", pre_content=triangle + b"0 1 0 rg\n15 20 100 80 re\nf\n", **common))
    output.joinpath("clipped-polygon-intersection-unsupported.pdf").write_bytes(build_pdf(
        "raster", pre_content=triangle + diamond, **common))
    # Clip operator 582 from the real combination PDF, isolated around a
    # controlled solid image. Its 0.1 CTM creates decimal edge coordinates.
    real_clip = (b"16825 402 m\n16823.8 400.801 l\n16441 18 l\n"
                 b"16438.6 16.8008 l\n16826.2 15.6016 l\n16826.2 402 l\n"
                 b"h\nW*\nn\n")
    output.joinpath("clipped-real-shape-582.pdf").write_bytes(build_pdf(
        "raster", width=1684, height=1191,
        pre_content=b"q\n0.1 0 0 0.1 0 0 cm\nq\n" + real_clip,
        extra_content=b"Q\nQ\n", image_placement=(16000, 0, 840, 800),
        image_source_size=(4, 4), image_source_override=bytes([255, 0, 0]) * 16))
    output.joinpath("clipped-subpixel-unsupported-fill.pdf").write_bytes(build_pdf(
        "raster", pre_content=fractional + b"0 1 0 rg\n10.25 12 20 10 re\nf\n", **common))
    output.joinpath("clipped-subpixel-ambiguous.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + b"29.05 35.01 20.25 5.5 re\nW\nn\n",
        pages_extra="/Rotate 180 /CropBox [8 12 248 180]",
        image_source_size=(4, 4), image_source_override=rgb,
        image_placement=(20, 20, 80, 60)))
    output.joinpath("clipped-fully-outside.pdf").write_bytes(build_pdf(
        "raster", pre_content=b"100 100 20 20 re\nW\nn\n", **common))
    output.joinpath("clipped-fully-outside-then-visible.pdf").write_bytes(build_pdf(
        "raster", pre_content=b"q\n100 100 20 20 re\nW\nn\n",
        extra_content=b"Q\nq\n20 0 0 16 100 90 cm\n/Im0 Do\nQ\n", **common))
    output.joinpath("clipped-subpixel-fully-outside.pdf").write_bytes(build_pdf(
        "raster", pre_content=blue + b"100.25 100.5 20.5 20.25 re\nW\nn\n", **common))
    output.joinpath("clipped-oversized-polygon-unsupported.pdf").write_bytes(build_pdf(
        "raster", pre_content=b"40 20 m\n55 22 l\n70 30 l\n80 45 l\n75 60 l\n"
        b"60 70 l\n45 68 l\n30 58 l\n25 40 l\nh\nW*\nn\n", **common))
    output.joinpath("clipped-path-open-unsupported.pdf").write_bytes(build_pdf(
        "raster", pre_content=polygon_rectangle.replace(b"h\n", b""), **common))
    output.joinpath("clipped-path-bowtie-unsupported.pdf").write_bytes(build_pdf(
        "raster", pre_content=b"30 25 m\n75 60 l\n75 25 l\n30 60 l\nh\nW*\nn\n", **common))


if __name__ == "__main__":
    main()
