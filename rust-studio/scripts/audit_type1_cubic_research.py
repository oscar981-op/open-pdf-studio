#!/usr/bin/env python3
"""Audit isolated embedded Type1 glyphs through research or strict native route.

Run with a Python environment containing pypdf. Fixtures/results stay outside Git.
"""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject, RectangleObject


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--source", type=Path, required=True)
    parser.add_argument("--hayro-probe", type=Path, required=True)
    parser.add_argument("--type1-probe", type=Path, required=True)
    parser.add_argument("--native-probe", type=Path, help="audit the bounded native Type1 subset")
    parser.add_argument("--require-exact", action="store_true")
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--scalar", action="store_true", help="test bounded curvature scalar candidate")
    parser.add_argument("--all-codes", action="store_true", help="test all embedded Type1 codes")
    parser.add_argument("--font-size", type=int, default=80, help="isolated PDF font size (16..120)")
    args = parser.parse_args()
    if not 16 <= args.font_size <= 120:
        parser.error("--font-size outside research bound")
    if args.font_size != 80 and not args.scalar:
        parser.error("non-default font size requires --scalar")
    args.output_dir.mkdir(parents=True, exist_ok=True)
    reader = PdfReader(args.source)
    rows = []
    codes = range(74) if args.all_codes else (1, 5, 27, 50, 57, 58, 68)
    for code in codes:
        writer = PdfWriter()
        page = writer.add_page(reader.pages[0])
        if args.native_probe:
            resources = page["/Resources"].get_object()
            fonts = resources["/Font"].get_object()
            resources[NameObject("/Font")] = DictionaryObject({NameObject("/F1"): fonts["/F1"]})
        content = DecodedStreamObject()
        content.set_data(
            f"0 0 0 rg BT /F1 {args.font_size} Tf 1 0 0 1 60 50 Tm <{code:02X}> Tj ET\n".encode()
        )
        page[NameObject("/Contents")] = writer._add_object(content)
        page[NameObject("/MediaBox")] = RectangleObject([0, 0, 256, 192])
        page[NameObject("/CropBox")] = RectangleObject([0, 0, 256, 192])
        stem = f"type1-glyph-code{code}" if args.font_size == 80 else f"type1-glyph-size{args.font_size}-code{code}"
        pdf = args.output_dir / f"{stem}.pdf"
        hayro = args.output_dir / f"{stem}.hayro.rgba"
        variant = "native" if args.native_probe else "scalar" if args.scalar else "dynlex"
        candidate = args.output_dir / f"{stem}.{variant}.rgba"
        writer.write(pdf)
        subprocess.run(
            [str(args.hayro_probe), str(pdf), str(hayro), "--hayro", "--scale=1", "--page=1"],
            check=True, capture_output=True,
        )
        if args.native_probe:
            command = [str(args.native_probe), str(pdf), str(candidate),
                       "--dynlex-strict", "--scale=1", "--page=1"]
        else:
            command = [str(args.type1_probe), str(pdf), "1", f"--code={code}",
                       "--tolerance=0.01", str(candidate)]
            if args.scalar:
                command.append("--scalar")
            if args.font_size != 80:
                command.append(f"--font-size={args.font_size}")
        result = subprocess.run(command, capture_output=True, text=True)
        if result.returncode:
            rows.append({
                "code": code,
                "pdf_sha256": sha(pdf.read_bytes()),
                "candidate_rejected": result.stderr.strip()[-400:],
            })
            continue
        reference = hayro.read_bytes()
        result = candidate.read_bytes()
        if len(reference) != 256 * 192 * 4 or len(result) != len(reference):
            raise ValueError("unexpected glyph output dimensions")
        rows.append({
            "code": code,
            "pdf_sha256": sha(pdf.read_bytes()),
            "pixels": 256 * 192,
            "different_pixels": sum(
                reference[i:i + 4] != result[i:i + 4]
                for i in range(0, len(reference), 4)
            ),
            "different_channels": sum(a != b for a, b in zip(reference, result)),
            "max_channel_delta": max(abs(a - b) for a, b in zip(reference, result)),
            "mismatch_samples": [
                {
                    "x": (i // 4) % 256,
                    "y": (i // 4) // 256,
                    "hayro_rgba": list(reference[i:i + 4]),
                    "candidate_rgba": list(result[i:i + 4]),
                }
                for i in range(0, len(reference), 4)
                if reference[i:i + 4] != result[i:i + 4]
            ][:16],
            "hayro_sha256": sha(reference),
            "candidate_sha256": sha(result),
        })
    report = {
        "scope": "bounded native Type1 embedded-PFB subset" if args.native_probe else "UNVERIFIED Type1 cubic research",
        "scalar": args.scalar,
        "all_codes": args.all_codes,
        "font_size": args.font_size,
        "source_sha256": sha(args.source.read_bytes()),
        "type1_probe_sha256": sha(args.type1_probe.read_bytes()),
        "native_probe_sha256": sha(args.native_probe.read_bytes()) if args.native_probe else None,
        "rows": rows,
    }
    suffix = "scalar-all" if args.all_codes and args.scalar else "scalar" if args.scalar else "recursive"
    size_suffix = "" if args.font_size == 80 else f"-size{args.font_size}"
    output = args.output_dir / f"dynlex-type1-cubic-{suffix}{size_suffix}-2026-10-09.json"
    if args.native_probe:
        output = args.output_dir / f"dynlex-type1-native-all{size_suffix}-2026-10-09.json"
    output.write_text(json.dumps(report, indent=2) + "\n")
    print([(row["code"], row.get("different_pixels", "rejected")) for row in rows])
    if args.require_exact and any(row.get("different_pixels") != 0 for row in rows):
        raise SystemExit("Type1 glyph audit has rejected or mismatched pages")


if __name__ == "__main__":
    main()
