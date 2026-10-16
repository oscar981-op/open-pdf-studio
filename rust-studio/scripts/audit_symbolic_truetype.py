#!/usr/bin/env python3
"""Byte-audit the embedded Macintosh-cmap TrueType symbol from Tekst.pdf.

Fixtures are complete PDF pages, but this is a font-subset audit, not proof
that the original multi-paint pages are supported. Outputs stay outside Git.
"""

import argparse
import hashlib
import json
import subprocess
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject, NumberObject, RectangleObject


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    source = PdfReader(args.source)
    rows = []
    for rotation in (0, 90, 180, 270):
        for crop in (False, True):
            writer = PdfWriter()
            page = writer.add_page(source.pages[0])
            fonts = page["/Resources"]["/Font"].get_object()
            page["/Resources"][NameObject("/Font")] = DictionaryObject(
                {NameObject("/F2"): fonts["/F2"]}
            )
            stream = DecodedStreamObject()
            stream.set_data(b"0.2 0.4 0.7 rg BT /F2 80 Tf 1 0 0 1 60 50 Tm <01> Tj ET\n")
            page[NameObject("/Contents")] = writer._add_object(stream)
            page[NameObject("/MediaBox")] = RectangleObject([0, 0, 256, 192])
            page[NameObject("/CropBox")] = RectangleObject(
                [20, 10, 236, 182] if crop else [0, 0, 256, 192]
            )
            page[NameObject("/Rotate")] = NumberObject(rotation)
            pdf = args.output_dir / f"mac-symbol-r{rotation}-crop{int(crop)}.pdf"
            writer.write(pdf)
            for scale in (0.25, 1.0, 2.0):
                hayro = args.output_dir / "hayro.rgba"
                dynlex = args.output_dir / "dynlex.rgba"
                tail = [f"--scale={scale}", "--page=1"]
                subprocess.run(
                    [str(args.probe), str(pdf), str(hayro), "--hayro", *tail],
                    check=True, capture_output=True,
                )
                result = subprocess.run(
                    [str(args.probe), str(pdf), str(dynlex), "--dynlex-strict", *tail],
                    capture_output=True, text=True,
                )
                a = hayro.read_bytes()
                b = dynlex.read_bytes() if result.returncode == 0 else b""
                rows.append({
                    "pdf": str(pdf), "pdf_sha256": sha(pdf), "rotation": rotation,
                    "crop": crop, "scale": scale, "native_exit": result.returncode,
                    "rgba_bytes": len(a), "different_channels": (
                        sum(x != y for x, y in zip(a, b)) + abs(len(a) - len(b))
                    ), "hayro_sha256": hashlib.sha256(a).hexdigest(),
                    "dynlex_sha256": hashlib.sha256(b).hexdigest() if b else None,
                    "diagnostic": result.stderr[-300:] if result.returncode else "",
                })
    rejects = []
    for case in ("nonsymbolic", "encoding-override", "unmapped-code"):
        writer = PdfWriter()
        page = writer.add_page(source.pages[0])
        fonts = page["/Resources"]["/Font"].get_object()
        font = fonts["/F2"].get_object()
        if case == "nonsymbolic":
            font["/FontDescriptor"].get_object()[NameObject("/Flags")] = NumberObject(0)
        elif case == "encoding-override":
            font[NameObject("/Encoding")] = NameObject("/WinAnsiEncoding")
        page["/Resources"][NameObject("/Font")] = DictionaryObject({NameObject("/F2"): fonts["/F2"]})
        stream = DecodedStreamObject()
        stream.set_data(
            b"BT /F2 80 Tf 1 0 0 1 60 50 Tm <02> Tj ET\n"
            if case == "unmapped-code"
            else b"BT /F2 80 Tf 1 0 0 1 60 50 Tm <01> Tj ET\n"
        )
        page[NameObject("/Contents")] = writer._add_object(stream)
        page[NameObject("/MediaBox")] = RectangleObject([0, 0, 256, 192])
        pdf = args.output_dir / f"mac-symbol-invalid-{case}.pdf"
        writer.write(pdf)
        invalid_output = args.output_dir / "invalid.rgba"
        invalid_output.unlink(missing_ok=True)
        result = subprocess.run(
            [str(args.probe), str(pdf), str(invalid_output),
             "--dynlex-strict", "--scale=1", "--page=1"],
            capture_output=True, text=True,
        )
        rejects.append({"case": case, "rejected": result.returncode != 0 and not invalid_output.exists(),
                        "pdf_sha256": sha(pdf), "diagnostic": result.stderr[-300:]})
    report = {"source_sha256": sha(args.source), "probe_sha256": sha(args.probe),
              "rows": rows, "invalid": rejects}
    (args.output_dir / "audit.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"exact={sum(row['different_channels'] == 0 for row in rows)}/{len(rows)}")
    if any(row["different_channels"] for row in rows) or any(not row["rejected"] for row in rejects):
        raise SystemExit("symbolic TrueType subset has mismatches or rejections")


if __name__ == "__main__":
    main()
