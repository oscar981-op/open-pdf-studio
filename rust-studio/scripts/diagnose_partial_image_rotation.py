#!/usr/bin/env python3
"""Show why rotating an already rendered image page is not PDF rotation."""
import argparse
import hashlib
import json
import struct
import subprocess
from pathlib import Path

import numpy as np
from pypdf import PdfReader, PdfWriter
from pypdf.generic import NameObject, NumberObject


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def f32(value):
    return struct.unpack("<f", struct.pack("<f", value))[0]


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source", required=True, type=Path)
    p.add_argument("--backend", required=True, type=Path)
    p.add_argument("--output-dir", required=True, type=Path)
    a = p.parse_args()
    a.output_dir.mkdir(parents=True, exist_ok=True)
    box = list(PdfReader(a.source).pages[0].mediabox)
    base_size = [f32(float(box[2] - box[0])), f32(float(box[3] - box[1]))]
    cases = []
    for scale in (0.25, 1.0, 2.0):
        base_width, base_height = [int(f32(v * f32(scale))) for v in base_size]
        base_file = a.output_dir / f"original-z{scale}.rgba"
        subprocess.run([str(a.backend), str(a.source), str(base_file),
                        "--hayro", f"--scale={scale}"],
                       capture_output=True, check=True, timeout=120)
        original = np.frombuffer(base_file.read_bytes(), dtype=np.uint8)
        original = original.reshape(base_height, base_width, 4)
        for rotation, quarter_turns in ((90, 3), (180, 2), (270, 1)):
            writer = PdfWriter()
            page = writer.add_page(PdfReader(a.source).pages[0])
            page[NameObject("/Rotate")] = NumberObject(rotation)
            pdf = a.output_dir / f"rot{rotation}.pdf"
            with pdf.open("wb") as stream:
                writer.write(stream)
            output = a.output_dir / f"rot{rotation}-z{scale}.rgba"
            subprocess.run([str(a.backend), str(pdf), str(output),
                            "--hayro", f"--scale={scale}"],
                           capture_output=True, check=True, timeout=120)
            width, height = (base_height, base_width) if rotation != 180 else (base_width, base_height)
            actual = np.frombuffer(output.read_bytes(), dtype=np.uint8).reshape(height, width, 4)
            naive = np.rot90(original, quarter_turns)
            different = np.any(actual != naive, axis=2)
            shifts = []
            for dy in (-1, 0, 1):
                for dx in (-1, 0, 1):
                    x0, x1 = max(dx, 0), min(width, width + dx)
                    y0, y1 = max(dy, 0), min(height, height + dy)
                    count = np.count_nonzero(np.any(
                        actual[y0:y1, x0:x1] != naive[y0-dy:y1-dy, x0-dx:x1-dx], axis=2))
                    shifts.append({"dx": dx, "dy": dy, "different_pixels": int(count)})
            cases.append({"scale": scale, "rotation": rotation,
                          "pdf_sha256": sha(pdf), "oracle_sha256": sha(output),
                          "width": width, "height": height,
                          "naive_rotation_different_pixels": int(np.count_nonzero(different)),
                          "best_one_pixel_shift": min(shifts, key=lambda row: row["different_pixels"])})
    report = {"scope": "Hayro-to-Hayro diagnostic; not a DynLex pixel parity test",
              "source_sha256": sha(a.source), "backend_sha256": sha(a.backend),
              "cases": cases}
    out = a.output_dir / "partial-image-rotation-diagnosis-2026-10-09.json"
    out.write_text(json.dumps(report, indent=2) + "\n")
    print([(r["scale"], r["rotation"], r["naive_rotation_different_pixels"])
           for r in cases])


if __name__ == "__main__":
    main()
