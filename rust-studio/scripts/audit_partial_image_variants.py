#!/usr/bin/env python3
"""Vary the real embedded JPEG's axis-aligned placement and clip; compare bytes."""
import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import DecodedStreamObject, NameObject


VARIANTS = {
    "original": ((1.199, 1.789, 594.1, 840.1), (594.1, 842.75, 1.2, 1.789)),
    "image-inset": ((20.3, 18.2, 520.7, 700.4), (510.6, 680.5, 22.1, 25.6)),
    "active-clip": ((21.75, 32.4, 500.35, 653.25), (510.6, 680.5, 22.1, 25.6)),
    "page-edge": ((0.0, 0.0, 595.303937, 841.889764), (594.1, 842.75, -3.2, 1.789)),
}


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source", type=Path, required=True)
    p.add_argument("--candidate", type=Path, required=True)
    p.add_argument("--backend", type=Path, required=True)
    p.add_argument("--output-dir", type=Path, required=True)
    p.add_argument("--require-exact", action="store_true")
    a = p.parse_args()
    a.output_dir.mkdir(parents=True, exist_ok=True)
    rows = []
    for label, (clip, place) in VARIANTS.items():
        reader = PdfReader(a.source)
        writer = PdfWriter()
        page = writer.add_page(reader.pages[0])
        stream = DecodedStreamObject()
        x, y, w, h = clip
        pw, ph, tx, ty = place
        stream.set_data(f"q {x} {y} {w} {h} re W* n\n"
                        f"q {pw} 0 0 {ph} {tx} {ty} cm /Im4 Do Q Q\n".encode())
        page[NameObject("/Contents")] = writer._add_object(stream)
        pdf = a.output_dir / f"image-{label}.pdf"
        with pdf.open("wb") as f:
            writer.write(f)
        for scale in (0.25, 1.0, 2.0):
            candidate = a.output_dir / f"image-{label}-z{scale}.candidate.rgba"
            oracle = a.output_dir / f"image-{label}-z{scale}.hayro.rgba"
            run = subprocess.run([str(a.candidate), str(pdf), str(candidate), str(scale)],
                                 capture_output=True, text=True, timeout=120)
            if run.returncode:
                raise RuntimeError(run.stderr)
            m = re.search(r"(\d+)x(\d+) intermediate=(\d+)x(\d+)", run.stderr)
            if not m:
                raise ValueError("missing candidate dimensions")
            width, height, iw, ih = map(int, m.groups())
            subprocess.run([str(a.backend), str(pdf), str(oracle), "--hayro",
                            f"--scale={scale}"], capture_output=True, text=True,
                           timeout=120, check=True)
            left, right = candidate.read_bytes(), oracle.read_bytes()
            if len(left) != len(right) or len(left) != width * height * 4:
                raise ValueError("bitmap length mismatch")
            indices = [i for i, (u, v) in enumerate(zip(left, right)) if u != v]
            rows.append({"variant": label, "scale": scale, "pdf_sha256": sha(pdf),
                         "width": width, "height": height,
                         "intermediate": [iw, ih], "rgba_bytes": len(left),
                         "candidate_sha256": sha(candidate), "hayro_sha256": sha(oracle),
                         "different_channels": len(indices),
                         "different_pixels": len({i // 4 for i in indices}),
                         "max_channel_error": max((abs(left[i] - right[i]) for i in indices),
                                                  default=0),
                         "first_differences": [{"x": i // 4 % width, "y": i // 4 // width,
                                                "channel": i % 4, "candidate": left[i],
                                                "hayro": right[i]} for i in indices[:8]]})
    report = {"scope": "trusted research fixture; no native acceptance",
              "source_sha256": sha(a.source), "candidate_binary_sha256": sha(a.candidate),
              "backend_binary_sha256": sha(a.backend), "cases": rows,
              "exact": sum(r["different_channels"] == 0 for r in rows)}
    out = a.output_dir / "partial-image-variants-2026-10-09.json"
    out.write_text(json.dumps(report, indent=2) + "\n")
    print(out, report["exact"], "/", len(rows), "exact")
    print([(r["variant"], r["scale"], r["different_pixels"])
           for r in rows if r["different_channels"]])
    if a.require_exact and report["exact"] != len(rows):
        raise SystemExit("partial-image variants have non-exact RGBA output")


if __name__ == "__main__":
    main()
