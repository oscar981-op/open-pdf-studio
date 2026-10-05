#!/usr/bin/env python3
"""Reproduce the once-mismatched diamond pixel against direct Vello and Hayro."""
import argparse
import subprocess
import tempfile
from pathlib import Path

from verify_glyph_multi_geometry import opm_bytes, pdf_bytes


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--vello", type=Path, required=True)
    parser.add_argument("--hayro", type=Path, required=True)
    parser.add_argument("--dynlex", type=Path, required=True)
    args = parser.parse_args()
    cx, cy, rx, ry, hx, hy = 193, 136, 9, 21, 2, 6
    contours = [
        [(cx, cy-ry), (cx+rx, cy), (cx, cy+ry), (cx-rx, cy)],
        [(cx, cy-hy), (cx-hx, cy), (cx, cy+hy), (cx+hx, cy)],
    ]
    with tempfile.TemporaryDirectory(prefix="opds-glyph-vello-") as folder:
        root = Path(folder)
        pdf, scene = root / "diamond.pdf", root / "diamond.opm"
        pdf.write_bytes(pdf_bytes(contours))
        scene.write_bytes(opm_bytes(contours))
        vello, hayro, dynlex = (root / "vello.raw", root / "hayro.rgba",
                                root / "dynlex.raw")
        subprocess.run([args.vello, *map(str, (cx, cy, rx, ry, hx, hy)), vello], check=True)
        subprocess.run([args.hayro, pdf, "1", hayro], check=True, capture_output=True)
        subprocess.run([args.dynlex, scene, dynlex], check=True, capture_output=True)
        v, h, d = vello.read_bytes(), hayro.read_bytes(), dynlex.read_bytes()
        if len(v) != 256*192 or len(d) != len(v) or len(h) != len(v)*4:
            raise AssertionError("incorrect oracle buffer length")
        if any(h[4*i:4*i+4] != bytes((255-a, 255-a, 255-a, 255))
               for i, a in enumerate(v)):
            raise AssertionError("direct Vello and PDF/Hayro disagree")
        if v != d:
            raise AssertionError(f"DynLex differs from Vello on {sum(a!=b for a,b in zip(v,d))} pixels")
        target = 135*256 + 191
        if v[target] != 42:
            raise AssertionError(f"diagnostic pixel changed: {v[target]}")
        invalid = subprocess.run([args.vello, "193", "136", "9", "21", "9", "6", root / "bad.raw"],
                                 capture_output=True)
        if invalid.returncode == 0 or (root / "bad.raw").exists():
            raise AssertionError("invalid oracle dimensions accepted")
        print("49,152 direct Vello/Hayro/DynLex coverage bytes exact; pixel (191,135)=42")


if __name__ == "__main__":
    main()
