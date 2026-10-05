#!/usr/bin/env python3
"""Differentially check bounded subpixel PDF image clips against Hayro."""

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
    parser.add_argument("--cases", type=int, default=200)
    parser.add_argument("--seed", type=int, default=20261005)
    parser.add_argument("--failed-pdf", type=Path)
    args = parser.parse_args()
    if not 1 <= args.cases <= 1000:
        parser.error("--cases must be 1..1000")
    probe = args.probe.resolve(strict=True)
    rng = random.Random(args.seed)
    rgb = bytes(channel for y in range(4) for x in range(4)
                for channel in (x * 70, y * 70, (x + y) * 25))
    fractions = (.01, .05, .24, .25, .49, .5, .51, .75, .99)
    lengths = (1.01, 2.5, 3.99, 4.01, 5.5, 6.5, 8.99, 20.25, 35.75)
    mismatches = []
    fallback = 0
    with tempfile.TemporaryDirectory(prefix="opds-fractional-clip-") as temporary:
        directory = Path(temporary)
        pdf = directory / "case.pdf"
        for index in range(args.cases):
            x = 20 + rng.randrange(30) + rng.choice(fractions)
            y = 20 + rng.randrange(30) + rng.choice(fractions)
            width, height = rng.choice(lengths), rng.choice(lengths)
            turn = rng.choice((0, 90, 180, 270))
            clip = f"{x:g} {y:g} {width:g} {height:g} re\nW\nn\n".encode()
            pre = b"0 0 1 rg\n10 10 110 90 re\nf\n" + clip
            pdf.write_bytes(build_pdf("raster", pre_content=pre,
                                      pages_extra=f"/Rotate {turn} /CropBox [8 12 248 180]",
                                      image_source_size=(4, 4), image_source_override=rgb,
                                      image_placement=(20, 20, 80, 60)))
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
                mismatches.append(dict(case=index, clip=[x, y, width, height], turn=turn,
                                       first_byte=first, actual=dynlex[first], expected=hayro[first]))
                if args.failed_pdf and len(mismatches) == 1:
                    args.failed_pdf.write_bytes(pdf.read_bytes())
    print(json.dumps(dict(cases=args.cases, seed=args.seed, supported=args.cases-fallback,
                          exact=args.cases-fallback-len(mismatches), fallback=fallback,
                          mismatches=mismatches[:20]), indent=2))
    if mismatches:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
