#!/usr/bin/env python3
"""Research-only oracle for the original Tekst.pdf page-5 image paint.

The PDF writer is used only to make an isolated input; it is not part of
DynLex's parsing or rendering path.
"""
import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import ContentStream, DecodedStreamObject, NameObject


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source", type=Path, required=True)
    p.add_argument("--probe", type=Path, required=True)
    p.add_argument("--output-dir", type=Path, required=True)
    a = p.parse_args()
    a.output_dir.mkdir(parents=True, exist_ok=True)
    reader = PdfReader(a.source)
    page = reader.pages[4]
    ops = ContentStream(page.get_contents(), reader).operations
    actual = [op for _, op in ops[9:17]]
    if actual != [b"q", b"re", b"W*", b"n", b"q", b"cm", b"Do", b"Q"]:
        raise ValueError(f"unexpected paint sequence: {actual!r}")
    if ops[15][0] != [NameObject("/Im4")]:
        raise ValueError("unexpected image resource")
    image = page["/Resources"]["/XObject"]["/Im4"].get_object()
    if image.get("/Subtype") != "/Image" or image.get("/Filter") != "/DCTDecode":
        raise ValueError("expected original JPEG image")
    writer = PdfWriter()
    out_page = writer.add_page(page)
    stream = DecodedStreamObject()
    stream.set_data(b"q\n1.199 1.789 594.1 840.1 re W* n\n"
                    b"q\n594.1 0 0 842.75 1.2 1.789 cm\n/Im4 Do\nQ\nQ\n")
    out_page[NameObject("/Contents")] = writer._add_object(stream)
    pdf = a.output_dir / "tekst-page5-original-image-only.pdf"
    with pdf.open("wb") as f:
        writer.write(f)
    rows = []
    for scale in (0.25, 0.87, 1.0, 1.15, 1.5625, 2.0):
        oracle = a.output_dir / f"tekst-page5-image-z{scale}.hayro.rgba"
        result = subprocess.run([str(a.probe), str(pdf), str(oracle), "--hayro",
                                 f"--scale={scale}"], capture_output=True, text=True,
                                timeout=120, check=True)
        size = re.search(r"(\d+)x(\d+)", result.stderr)
        if not size:
            raise ValueError(f"missing dimensions: {result.stderr}")
        w, h = map(int, size.groups())
        pixels = oracle.read_bytes()
        if len(pixels) != w * h * 4:
            raise ValueError("oracle output length")
        rejected = a.output_dir / f"tekst-page5-image-z{scale}.dynlex.rgba"
        rejected.unlink(missing_ok=True)
        strict = subprocess.run([str(a.probe), str(pdf), str(rejected), "--dynlex-strict",
                                 f"--scale={scale}"], capture_output=True, text=True,
                                timeout=120)
        rows.append({"scale": scale, "width": w, "height": h,
                     "oracle_rgba_bytes": len(pixels), "oracle_sha256": digest(pixels),
                     "strict_exit": strict.returncode,
                     "strict_status": strict.stderr.strip(),
                     "strict_output_exists": rejected.exists()})
    report = {"source_sha256": digest(a.source.read_bytes()),
              "isolated_pdf_sha256": digest(pdf.read_bytes()),
              "probe_sha256": digest(a.probe.read_bytes()),
              "jpeg_encoded_sha256": digest(image._data),
              "jpeg_width": int(image["/Width"]), "jpeg_height": int(image["/Height"]),
              "content": stream.get_data().decode("ascii"), "cases": rows}
    path = a.output_dir / "tekst-page5-image-isolation-2026-10-09.json"
    path.write_text(json.dumps(report, indent=2) + "\n")
    print(path)
    print([(r["scale"], r["width"], r["height"], r["strict_exit"])
           for r in rows])


if __name__ == "__main__":
    main()
