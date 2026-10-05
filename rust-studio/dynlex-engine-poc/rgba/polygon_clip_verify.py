#!/usr/bin/env python3
"""Compare bounded convex PDF image clips with Hayro, including fallback cases."""

import argparse
import json
import random
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from pdf_fixture import build_pdf  # noqa: E402
from fractional_image_verify import run_probe  # noqa: E402


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--cases", type=int, default=400)
    parser.add_argument("--seed", type=int, default=20261005)
    parser.add_argument("--quarter", action="store_true",
                        help="offset polygon vertices by quarter-pixel increments")
    parser.add_argument("--decimal", action="store_true",
                        help="offset polygon vertices by ordinary PDF decimal values")
    parser.add_argument("--concave", action="store_true",
                        help="use a bounded simple five-vertex notch")
    parser.add_argument("--global-ctm", action="store_true",
                        help="encode geometry under a global 0.1 PDF scale")
    parser.add_argument("--failed-pdf", type=Path)
    args = parser.parse_args()
    if not 1 <= args.cases <= 2000:
        parser.error("--cases must be 1..2000")
    if args.quarter and args.decimal:
        parser.error("choose either --quarter or --decimal")
    probe = args.probe.resolve(strict=True)
    rng = random.Random(args.seed)
    rgb = bytes(channel for y in range(4) for x in range(4)
                for channel in (x * 70, y * 70, (x + y) * 25))
    mismatch = []
    fallback = 0
    with tempfile.TemporaryDirectory(prefix="opds-polygon-clip-") as temporary:
        directory = Path(temporary)
        pdf = directory / "case.pdf"
        for index in range(args.cases):
            x = rng.randrange(25, 45)
            y = rng.randrange(20, 35)
            width = rng.randrange(30, 60)
            height = rng.randrange(35, 60)
            if args.concave:
                notch = rng.randrange(4, 16)
                middle = y + rng.randrange(8, height - 8)
                points = [(x, y), (x + width, y),
                          (x + width - notch, middle),
                          (x + width, y + height), (x, y + height)]
            elif index % 2:
                skew = rng.randrange(-10, 11)
                points = [(x, y), (x + width, y),
                          (x + width + skew, y + height),
                          (x + skew, y + height)]
            else:
                points = [(x, y), (x + width, y), (x, y + height)]
            if args.quarter:
                dx = rng.randrange(4) / 4
                dy = rng.randrange(4) / 4
                points = [(px + dx, py + dy) for px, py in points]
            if args.decimal:
                dx = rng.choice((.01, .05, .24, .49, .51, .75, .99))
                dy = rng.choice((.01, .05, .24, .49, .51, .75, .99))
                points = [(px + dx, py + dy) for px, py in points]
            if args.global_ctm:
                points = [(px * 10, py * 10) for px, py in points]
            turn = rng.choice((0, 90, 180, 270))
            rule = "W*" if rng.randrange(2) else "W"
            path = f"{points[0][0]} {points[0][1]} m\n"
            path += "".join(f"{px} {py} l\n" for px, py in points[1:])
            path += f"h\n{rule}\nn\n"
            pre = (b"q\n0.1 0 0 0.1 0 0 cm\n"
                   b"0 0 1 rg\n100 100 1100 900 re\nf\n" if args.global_ctm
                   else b"0 0 1 rg\n10 10 110 90 re\nf\n") + path.encode()
            placement = ((212.5, 315, 635, 472.5) if args.global_ctm
                         else (21.25, 31.5, 63.5, 47.25))
            pdf.write_bytes(build_pdf(
                "raster", pre_content=pre,
                extra_content=b"Q\n" if args.global_ctm else b"",
                pages_extra=f"/Rotate {turn} /CropBox [8 12 248 180]",
                image_source_size=(4, 4), image_source_override=rgb,
                image_placement=placement))
            try:
                dynlex = run_probe(probe, pdf, directory / "dynlex.rgba", False)
            except RuntimeError as error:
                if "outside the bounded DynLex subset" not in str(error):
                    raise
                fallback += 1
                continue
            hayro = run_probe(probe, pdf, directory / "hayro.rgba", True)
            if dynlex != hayro:
                first = next(i for i, (actual, wanted) in enumerate(zip(dynlex, hayro))
                             if actual != wanted)
                mismatch.append(dict(case=index, points=points, turn=turn,
                                     rule=rule, first_byte=first,
                                     actual=dynlex[first], expected=hayro[first]))
                if args.failed_pdf and len(mismatch) == 1:
                    args.failed_pdf.write_bytes(pdf.read_bytes())
    print(json.dumps(dict(cases=args.cases, seed=args.seed,
                          quarter=args.quarter, decimal=args.decimal,
                          concave=args.concave, global_ctm=args.global_ctm,
                          supported=args.cases - fallback, fallback=fallback,
                          exact=args.cases - fallback - len(mismatch),
                          mismatches=mismatch[:20]), indent=2))
    if mismatch:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
