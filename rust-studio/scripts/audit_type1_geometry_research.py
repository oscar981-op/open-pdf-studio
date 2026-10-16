#!/usr/bin/env python3
"""Research-only Type1 glyph parity across page rotation, CropBox and RGB."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject, NumberObject, RectangleObject


def digest(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--hayro-probe", type=Path, required=True)
    parser.add_argument("--type1-probe", type=Path, required=True)
    parser.add_argument("--native-probe", type=Path, help="Check the bounded native DynLex worker instead of research rendering")
    parser.add_argument("--require-exact", action="store_true")
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    reader = PdfReader(args.source)
    rows = []
    for code in (1, 23, 30, 50, 68):
        for rotation in (0, 90, 180, 270):
            for shifted in (False, True):
                for rgb in ("0,0,0", "0.2,0.4,0.6"):
                    width, height = (192, 256) if rotation in (90, 270) else (256, 192)
                    left, bottom = (17, 19) if shifted else (0, 0)
                    writer = PdfWriter()
                    page = writer.add_page(reader.pages[0])
                    if args.native_probe:
                        resources = page["/Resources"].get_object()
                        fonts = resources["/Font"].get_object()
                        resources[NameObject("/Font")] = DictionaryObject({NameObject("/F1"): fonts["/F1"]})
                    page[NameObject("/MediaBox")] = RectangleObject(
                        [0, 0, width + left + 17, height + bottom + 19]
                        if shifted else [0, 0, width, height]
                    )
                    page[NameObject("/CropBox")] = RectangleObject(
                        [left, bottom, left + width, bottom + height]
                    )
                    page[NameObject("/Rotate")] = NumberObject(rotation)
                    origin = (left + 60, bottom + 50)
                    content = DecodedStreamObject()
                    content.set_data(
                        f"{' '.join(rgb.split(','))} rg BT /F1 80 Tf 1 0 0 1 "
                        f"{origin[0]} {origin[1]} Tm <{code:02X}> Tj ET\n".encode()
                    )
                    page[NameObject("/Contents")] = writer._add_object(content)
                    stem = f"type1-geo-c{code}-r{rotation}-s{int(shifted)}-rgb{rgb.replace(',', '_')}"
                    pdf = args.output_dir / f"{stem}.pdf"
                    hayro = args.output_dir / f"{stem}.hayro.rgba"
                    candidate = args.output_dir / f"{stem}.dynlex.rgba"
                    writer.write(pdf)
                    oracle = subprocess.run(
                        [str(args.hayro_probe), str(pdf), str(hayro), "--hayro", "--scale=1", "--page=1"],
                        capture_output=True, text=True,
                    )
                    if oracle.returncode:
                        raise RuntimeError(oracle.stderr)
                    if args.native_probe:
                        cmd = [str(args.native_probe), str(pdf), str(candidate),
                               "--dynlex-strict", "--scale=1", "--page=1"]
                    else:
                        cmd = [str(args.type1_probe), str(pdf), "1", f"--code={code}",
                               "--tolerance=0.01", str(candidate), "--scalar", "--font-size=80",
                               f"--origin={origin[0]},{origin[1]}", f"--rgb={rgb}"]
                    result = subprocess.run(cmd, capture_output=True, text=True)
                    row = {"code": code, "rotation": rotation, "shifted_crop": shifted,
                           "rgb": rgb, "pdf_sha256": digest(pdf.read_bytes())}
                    if result.returncode:
                        row["candidate_rejected"] = result.stderr.strip()[-300:]
                    else:
                        reference = hayro.read_bytes()
                        image = candidate.read_bytes()
                        if len(reference) != 256 * 192 * 4 or len(image) != len(reference):
                            raise ValueError("unexpected bitmap dimensions")
                        row.update({
                            "different_pixels": sum(reference[i:i + 4] != image[i:i + 4]
                                                    for i in range(0, len(reference), 4)),
                            "different_channels": sum(a != b for a, b in zip(reference, image)),
                            "max_channel_delta": max(abs(a - b) for a, b in zip(reference, image)),
                            "hayro_sha256": digest(reference),
                            "candidate_sha256": digest(image),
                        })
                    rows.append(row)
    report = {
        "scope": "bounded native DynLex Type1 subset" if args.native_probe else "UNVERIFIED Type1 research; no native acceptance",
        "source_sha256": digest(args.source.read_bytes()),
        "type1_probe_sha256": digest(args.type1_probe.read_bytes()),
        "native_probe_sha256": digest(args.native_probe.read_bytes()) if args.native_probe else None,
        "cases": rows,
    }
    path = args.output_dir / "dynlex-type1-geometry-2026-10-09.json"
    if args.native_probe:
        path = args.output_dir / "dynlex-type1-native-geometry-2026-10-09.json"
    path.write_text(json.dumps(report, indent=2) + "\n")
    print(path, "exact", sum(row.get("different_pixels") == 0 for row in rows),
          "rejected", sum("candidate_rejected" in row for row in rows),
          "mismatched", sum(row.get("different_pixels", 0) > 0 for row in rows))
    if args.require_exact and any(row.get("different_pixels") != 0 for row in rows):
        raise SystemExit("Type1 geometry audit has rejected or mismatched pages")


if __name__ == "__main__":
    main()
