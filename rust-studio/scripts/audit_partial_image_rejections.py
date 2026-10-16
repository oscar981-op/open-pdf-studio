#!/usr/bin/env python3
"""Check that the trusted image probe rejects page states it cannot map."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import (ArrayObject, DecodedStreamObject, DictionaryObject,
                           FloatObject, NameObject, NumberObject)


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source", required=True, type=Path)
    p.add_argument("--candidate", required=True, type=Path)
    p.add_argument("--output-dir", required=True, type=Path)
    a = p.parse_args()
    a.output_dir.mkdir(parents=True, exist_ok=True)
    rows = []
    for case in ("direct-rotate", "inherited-rotate", "direct-crop",
                 "inherited-crop", "user-unit", "group", "annots",
                 "shear-matrix"):
        writer = PdfWriter()
        page = writer.add_page(PdfReader(a.source).pages[0])
        parent = page["/Parent"].get_object()
        crop = ArrayObject([FloatObject(5), FloatObject(7),
                            FloatObject(550), FloatObject(800)])
        if case == "direct-rotate":
            page[NameObject("/Rotate")] = NumberObject(90)
        elif case == "inherited-rotate":
            parent[NameObject("/Rotate")] = NumberObject(180)
        elif case == "direct-crop":
            page[NameObject("/CropBox")] = crop
        elif case == "inherited-crop":
            parent[NameObject("/CropBox")] = crop
        elif case == "user-unit":
            page[NameObject("/UserUnit")] = NumberObject(2)
        elif case == "group":
            page[NameObject("/Group")] = writer._add_object(
                DictionaryObject({NameObject("/S"): NameObject("/Transparency")}))
        elif case == "annots":
            page[NameObject("/Annots")] = ArrayObject()
        else:
            stream = DecodedStreamObject()
            stream.set_data(b"q 1.199 1.789 594.1 840.1 re W* n\n"
                            b"q 594.1 0.1 0 842.75 1.2 1.789 cm /Im4 Do Q Q\n")
            page[NameObject("/Contents")] = writer._add_object(stream)
        pdf = a.output_dir / f"reject-{case}.pdf"
        with pdf.open("wb") as output:
            writer.write(output)
        bitmap = a.output_dir / f"reject-{case}.rgba"
        bitmap.unlink(missing_ok=True)
        result = subprocess.run([str(a.candidate), str(pdf), str(bitmap), "1.0"],
                                capture_output=True, text=True, timeout=20)
        rows.append({"case": case, "pdf_sha256": digest(pdf),
                     "rejected": result.returncode != 0 and not bitmap.exists(),
                     "stderr_tail": result.stderr[-240:]})
    report = {"scope": "trusted research probe; no native acceptance",
              "source_sha256": digest(a.source),
              "candidate_sha256": digest(a.candidate), "cases": rows}
    out = a.output_dir / "partial-image-rejections-2026-10-09.json"
    out.write_text(json.dumps(report, indent=2) + "\n")
    if not all(row["rejected"] for row in rows):
        raise SystemExit(f"unexpected acceptance: {[r['case'] for r in rows if not r['rejected']]}")
    print(f"{len(rows)}/{len(rows)} unsupported states rejected without bitmap")


if __name__ == "__main__":
    main()
