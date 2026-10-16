#!/usr/bin/env python3
"""Pixel-check bounded fontless vector pages through native strict DynLex."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

from pypdf import PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, FloatObject, NameObject


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def make_pdf(path: Path, content: bytes, rotation: int = 0, cropped: bool = False,
             smoothness: bool = False) -> None:
    writer = PdfWriter()
    page = writer.add_blank_page(width=256, height=192)
    resources = DictionaryObject()
    if smoothness:
        resources[NameObject("/ExtGState")] = DictionaryObject({
            NameObject("/GS0"): DictionaryObject({NameObject("/SM"): FloatObject(0.02)})
        })
    page[NameObject("/Resources")] = resources
    stream = DecodedStreamObject()
    stream.set_data(content)
    page[NameObject("/Contents")] = writer._add_object(stream)
    if cropped:
        page.cropbox.lower_left = (5, 7)
        page.cropbox.upper_right = (245, 185)
    if rotation:
        page.rotate(rotation)
    with path.open("wb") as output:
        writer.write(output)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--smoothness", action="store_true")
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    probe = args.probe.resolve()
    content = (b"/GS0 gs " if args.smoothness else b"") + (
        b"/P BMC 0 0 1 rg 10 10 80 40 re f "
        b"1 0 0 rg 50 20 60 50 re f EMC"
    )
    rows = []
    for rotation in (0, 90, 180, 270):
        for cropped in (False, True):
            pdf = args.output_dir / f"fontless-r{rotation}-c{int(cropped)}.pdf"
            make_pdf(pdf, content, rotation, cropped, args.smoothness)
            for scale in (0.25, 1.0, 2.0):
                label = f"r{rotation}-c{int(cropped)}-z{scale}"
                own = args.output_dir / f"{label}.dynlex.rgba"
                oracle = args.output_dir / f"{label}.hayro.rgba"
                subprocess.run(
                    [str(probe), str(pdf), str(own), "--dynlex-strict", f"--scale={scale}"],
                    check=True, capture_output=True, text=True, timeout=30,
                )
                subprocess.run(
                    [str(probe), str(pdf), str(oracle), "--hayro", f"--scale={scale}"],
                    check=True, capture_output=True, text=True, timeout=30,
                )
                left, right = own.read_bytes(), oracle.read_bytes()
                rows.append({
                    "rotation": rotation, "crop_box": cropped, "scale": scale,
                    "pdf_sha256": sha(pdf), "rgba_bytes": len(left),
                    "dynlex_sha256": sha(own), "hayro_sha256": sha(oracle),
                    "exact": left == right,
                })
    invalid = args.output_dir / "fontless-missing-Tf.pdf"
    make_pdf(invalid, b"BT /F 12 Tf (A) Tj ET")
    invalid_output = args.output_dir / "fontless-missing-Tf.rgba"
    invalid_output.unlink(missing_ok=True)
    rejected = subprocess.run(
        [str(probe), str(invalid), str(invalid_output), "--dynlex-strict"],
        capture_output=True, text=True, timeout=30,
    )
    report = {
        "scope": "fontless BMC and ordered colored rectangles; not a full vector kernel",
        "smoothness_ext_gstate": args.smoothness,
        "probe_sha256": sha(probe), "cases": rows,
        "exact": sum(row["exact"] for row in rows),
        "bytes_exact": sum(row["rgba_bytes"] for row in rows if row["exact"]),
        "missing_font_rejected": rejected.returncode != 0 and not invalid_output.exists(),
        "missing_font_pdf_sha256": sha(invalid),
    }
    (args.output_dir / "report.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"{report['exact']}/{len(rows)} exact; {report['bytes_exact']} bytes")
    if report["exact"] != len(rows) or not report["missing_font_rejected"]:
        raise SystemExit("fontless vector regression")


if __name__ == "__main__":
    main()
