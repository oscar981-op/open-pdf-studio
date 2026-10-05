#!/usr/bin/env python3
"""Probe two nested W* image-clip contours against Hayro with fixed seeds."""

import argparse
import json
import random
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from pdf_fixture import build_pdf  # noqa: E402
from fractional_image_verify import run_probe  # noqa: E402


def rectangle(x, y, width, height):
    return (f"{x} {y} m\n{x + width} {y} l\n"
            f"{x + width} {y + height} l\n{x} {y + height} l\nh\n")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", required=True, type=Path)
    parser.add_argument("--cases", type=int, default=200)
    parser.add_argument("--seed", type=int, default=20261005)
    parser.add_argument("--failed-pdf", type=Path)
    args = parser.parse_args()
    if not 1 <= args.cases <= 2000:
        parser.error("--cases must be 1..2000")
    probe = args.probe.resolve(strict=True)
    rng = random.Random(args.seed)
    rgb = bytes(channel for y in range(4) for x in range(4)
                for channel in (x * 70, y * 70, (x + y) * 25))
    mismatch = []
    fallback = 0
    with tempfile.TemporaryDirectory(prefix="opds-nested-clip-") as temporary:
        directory = Path(temporary)
        pdf = directory / "case.pdf"
        for index in range(args.cases):
            fraction = rng.choice((0, .25, .5, .75, .01, .49))
            x, y = rng.randrange(20, 35) + fraction, rng.randrange(15, 30) + fraction
            width, height = rng.randrange(55, 76), rng.randrange(50, 70)
            inset = rng.randrange(6, 16)
            outer = rectangle(x, y, width, height)
            inner = rectangle(x + inset, y + inset,
                              width - 2 * inset, height - 2 * inset)
            turn = rng.choice((0, 90, 180, 270))
            path = (inner + outer if index % 2 else outer + inner) + "W*\nn\n"
            pdf.write_bytes(build_pdf(
                "raster", pre_content=(b"0 0 1 rg\n10 10 110 90 re\nf\n" + path.encode()),
                pages_extra=f"/Rotate {turn} /CropBox [8 12 248 180]",
                image_source_size=(4, 4), image_source_override=rgb,
                image_placement=(21.25, 31.5, 63.5, 47.25)))
            try:
                dynlex = run_probe(probe, pdf, directory / "dynlex.rgba", False)
            except RuntimeError as error:
                if "outside the bounded DynLex subset" not in str(error):
                    raise
                fallback += 1
                continue
            hayro = run_probe(probe, pdf, directory / "hayro.rgba", True)
            if dynlex != hayro:
                first = next(i for i, (a, b) in enumerate(zip(dynlex, hayro)) if a != b)
                mismatch.append(dict(case=index, fraction=fraction, turn=turn,
                                     first_byte=first, actual=dynlex[first],
                                     expected=hayro[first]))
                if args.failed_pdf and len(mismatch) == 1:
                    args.failed_pdf.write_bytes(pdf.read_bytes())
    print(json.dumps(dict(cases=args.cases, seed=args.seed,
                          supported=args.cases - fallback, fallback=fallback,
                          exact=args.cases - fallback - len(mismatch),
                          mismatches=mismatch[:20]), indent=2))
    if mismatch:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
