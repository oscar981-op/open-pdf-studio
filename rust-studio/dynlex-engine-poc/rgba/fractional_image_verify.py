#!/usr/bin/env python3
"""Differentially probe bounded fractional RGB XObjects against Hayro.

This is a deterministic diagnostic, not a PDF conformance test. It invokes
the two backend_probe modes as separate processes on identical PDF bytes.
"""

import argparse
import json
import random
import subprocess
import sys
import tempfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from pdf_fixture import build_pdf  # noqa: E402


def run_probe(probe: Path, pdf: Path, output: Path, hayro: bool) -> bytes:
    command = [str(probe), str(pdf), str(output)]
    if hayro:
        command.append("--hayro")
    result = subprocess.run(command, capture_output=True, check=False)
    if result.returncode:
        raise RuntimeError(result.stderr.decode(errors="replace")[:300])
    return output.read_bytes()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--cases", type=int, default=80)
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
    lengths = (1.01, 1.49, 2.5, 3.99, 4.01, 5.5, 6.5, 8.99)
    mismatches = []
    with tempfile.TemporaryDirectory(prefix="opds-fractional-verify-") as temporary:
        directory = Path(temporary)
        pdf = directory / "case.pdf"
        for index in range(args.cases):
            x, y = 10 + rng.choice(fractions), 20 + rng.choice(fractions)
            width, height = rng.choice(lengths), rng.choice(lengths)
            pdf.write_bytes(build_pdf("raster", image_placement=(x, y, width, height),
                                      image_source_size=(4, 4), image_source_override=rgb))
            dynlex = run_probe(probe, pdf, directory / "dynlex.rgba", False)
            hayro = run_probe(probe, pdf, directory / "hayro.rgba", True)
            if dynlex != hayro:
                first = next(i for i, (a, b) in enumerate(zip(dynlex, hayro)) if a != b)
                mismatches.append(dict(case=index, placement=[x, y, width, height],
                                       first_byte=first, dynlex=dynlex[first], hayro=hayro[first]))
                if args.failed_pdf and len(mismatches) == 1:
                    args.failed_pdf.write_bytes(pdf.read_bytes())
    print(json.dumps(dict(cases=args.cases, seed=args.seed,
                          exact=args.cases - len(mismatches), mismatches=mismatches[:20]), indent=2))
    if mismatches:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
