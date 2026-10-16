#!/usr/bin/env python3
"""Pixel-check bounded native Type1 plus WinAnsi TrueType paints in one page."""
import argparse
import hashlib
import json
import subprocess
from io import BytesIO
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject, NumberObject, RectangleObject

from audit_text_winding import text_fixture


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def render(probe, pdf, output, backend, scale):
    result = subprocess.run([str(probe), str(pdf), str(output), backend, f"--scale={scale}", "--page=1"],
                            capture_output=True, text=True, timeout=40)
    if result.returncode:
        raise RuntimeError(f"{backend} at {scale}: {result.stderr}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--ttf", type=Path, required=True)
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    original = PdfReader(args.source)
    ttf_pdf = PdfReader(BytesIO(text_fixture(args.ttf.read_bytes(), 0, "0 0 640 800")))
    rows = []
    for rotate in (0, 90, 180, 270):
        for shifted in (False, True):
            writer = PdfWriter()
            writer.append(original, pages=[0])
            writer.append(ttf_pdf, pages=[0])
            page = writer.pages[0]
            type1 = page["/Resources"]["/Font"]["/F1"]
            truetype = writer.pages[1]["/Resources"]["/Font"]["/F"]
            page[NameObject("/Resources")] = DictionaryObject({
                NameObject("/Font"): DictionaryObject({NameObject("/T1"): type1,
                                                        NameObject("/TT"): truetype})})
            page.pop(NameObject("/Annots"), None)
            page.pop(NameObject("/Group"), None)
            page[NameObject("/MediaBox")] = RectangleObject([0, 0, 656, 816])
            left, bottom = (8, 8) if shifted else (0, 0)
            page[NameObject("/CropBox")] = RectangleObject([left, bottom, left + 640, bottom + 800])
            page[NameObject("/Rotate")] = NumberObject(rotate)
            stream = DecodedStreamObject()
            stream.set_data((
                f"0 0 0 rg BT /T1 80 Tf 1 0 0 1 {80+left} {400+bottom} Tm "
                "<01171E3244> Tj ET\n"
                f"0.2 0.4 0.6 rg BT /TT 60 Tf 1 0 0 1 {100+left} {390+bottom} Tm "
                "(ABCD) Tj ET\n"
            ).encode())
            page[NameObject("/Contents")] = writer._add_object(stream)
            pdf = args.output_dir / f"mixed-r{rotate}-s{int(shifted)}.pdf"
            writer.write(pdf)
            for scale in (0.25, 1.0, 2.0):
                tag = f"r{rotate}-s{int(shifted)}-z{scale}"
                actual = args.output_dir / f"{tag}.dynlex.rgba"
                expected = args.output_dir / f"{tag}.hayro.rgba"
                render(args.probe, pdf, actual, "--dynlex-strict", scale)
                render(args.probe, pdf, expected, "--hayro", scale)
                a, b = actual.read_bytes(), expected.read_bytes()
                row = {"rotate": rotate, "shifted_crop": shifted, "scale": scale,
                       "pdf_sha256": sha(pdf), "rgba_bytes": len(a),
                       "different_channels": sum(x != y for x, y in zip(a, b)),
                       "dynlex_sha256": sha(actual), "hayro_sha256": sha(expected)}
                if len(a) != len(b) or row["different_channels"]:
                    raise AssertionError(row)
                rows.append(row)
    report = {"scope": "bounded native mixed embedded Type1/WinAnsi TrueType subset",
              "source_sha256": sha(args.source), "ttf_sha256": sha(args.ttf),
              "probe_sha256": sha(args.probe), "cases": rows,
              "exact_rgba_bytes": sum(row["rgba_bytes"] for row in rows)}
    output = args.output_dir / "dynlex-type1-mixed-native-2026-10-09.json"
    output.write_text(json.dumps(report, indent=2) + "\n")
    print(output, len(rows), "exact cases", report["exact_rgba_bytes"], "RGBA bytes")


if __name__ == "__main__":
    main()
