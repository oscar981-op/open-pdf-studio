#!/usr/bin/env python3
"""Check Type1 staging, native pixels and malformed-font rejection."""
import argparse
import ast
import hashlib
import json
import subprocess
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import DecodedStreamObject, DictionaryObject, NameObject, NumberObject, RectangleObject


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def run(*args):
    return subprocess.run([str(a) for a in args], capture_output=True, text=True)


def fixture(source, destination, mutation=None):
    reader = PdfReader(source)
    writer = PdfWriter()
    page = writer.add_page(reader.pages[0])
    resources = page["/Resources"].get_object()
    fonts = resources["/Font"].get_object()
    resources[NameObject("/Font")] = DictionaryObject({NameObject("/F1"): fonts["/F1"]})
    page[NameObject("/MediaBox")] = RectangleObject([0, 0, 256, 192])
    page[NameObject("/CropBox")] = RectangleObject([0, 0, 256, 192])
    content = DecodedStreamObject()
    content.set_data(b"0 0 0 rg BT /F1 20 Tf 1 0 0 1 60 50 Tm <01171E3244> Tj ET\n")
    page[NameObject("/Contents")] = writer._add_object(content)
    font = resources["/Font"]["/F1"].get_object()
    if mutation == "encoding_override":
        font[NameObject("/Encoding")] = NameObject("/WinAnsiEncoding")
    elif mutation == "fractional_width":
        from pypdf.generic import FloatObject
        font["/Widths"].get_object()[1] = FloatObject(447.5)
    elif mutation == "missing_embedded_file":
        font["/FontDescriptor"].get_object().pop(NameObject("/FontFile"))
    writer.write(destination)


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source", type=Path, required=True)
    p.add_argument("--stage-probe", type=Path, required=True)
    p.add_argument("--oracle", type=Path, required=True)
    p.add_argument("--native-probe", type=Path, required=True)
    p.add_argument("--output-dir", type=Path, required=True)
    args = p.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    rows = []
    for mutation in (None, "encoding_override", "fractional_width", "missing_embedded_file"):
        name = mutation or "valid"
        pdf = args.output_dir / f"type1-worker-{name}.pdf"
        fixture(args.source, pdf, mutation)
        stage_dir = args.output_dir / f"stage-{name}"
        stage_dir.mkdir(exist_ok=True)
        stage = run(args.stage_probe, pdf, 1, 1, args.output_dir / "unused.rgba",
                    "--stage-only", stage_dir)
        output = args.output_dir / f"native-{name}.rgba"
        output.unlink(missing_ok=True)
        native = run(args.native_probe, pdf, output, "--dynlex-strict", "--scale=1", "--page=1")
        if mutation and (native.returncode != 1 or output.exists() or "No fallback rendered" not in native.stderr):
            raise AssertionError(f"malformed Type1 must remain strictly unsupported: {name}")
        row = {"fixture": name, "pdf_sha256": sha(pdf), "stage_returncode": stage.returncode,
               "native_returncode": native.returncode, "native_no_bitmap": True,
               "stage_error": stage.stderr.strip()[-240:] if stage.returncode else None}
        if mutation:
            if stage.returncode == 0:
                raise AssertionError(f"malformed Type1 font accepted: {name}")
        else:
            if stage.returncode:
                raise RuntimeError(stage.stderr)
            if native.returncode or not output.exists():
                raise RuntimeError(f"valid bounded Type1 render failed: {native.stderr}")
            hayro = args.output_dir / "hayro-valid.rgba"
            oracle_pixels = run(args.native_probe, pdf, hayro, "--hayro", "--scale=1", "--page=1")
            if oracle_pixels.returncode or output.read_bytes() != hayro.read_bytes():
                raise AssertionError("native Type1 pixels differ from Hayro")
            row.update({"native_no_bitmap": False, "rgba_bytes_exact": len(output.read_bytes()),
                        "native_rgba_sha256": sha(output)})
            trace = args.output_dir / "oracle.tsv"
            oracle = run(args.oracle, pdf, 1, 1, trace)
            if oracle.returncode:
                raise RuntimeError(oracle.stderr)
            candidate = [line.split("\t") for line in (stage_dir / "staged.tsv").read_text().splitlines()
                         if "\tG\t" in line]
            expected = [line.split("\t") for line in trace.read_text().splitlines()
                        if line.startswith("G\t")]
            if len(candidate) != 5 or len(expected) != 5:
                raise AssertionError("unexpected Type1 glyph count")
            delta = 0.0
            for ours, reference in zip(candidate, expected):
                if int(ours[3]) != int(reference[1]):
                    raise AssertionError("Type1 code-to-GID mismatch")
                for a, b in zip(ast.literal_eval(ours[4]), ast.literal_eval(reference[2])):
                    delta = max(delta, abs(a - b))
            if delta > 1e-10:
                raise AssertionError(f"Type1 glyph position mismatch {delta}")
            row.update({"glyphs": 5, "maximum_matrix_delta": delta,
                        "stage_sha256": sha(stage_dir / "staged.tsv"), "oracle_sha256": sha(trace)})
        rows.append(row)
    report = {"scope": "bounded Type1 embedded-PFB subset, isolated native pixel parity",
              "source_sha256": sha(args.source), "stage_probe_sha256": sha(args.stage_probe),
              "native_probe_sha256": sha(args.native_probe), "rows": rows}
    path = args.output_dir / "dynlex-type1-worker-native-2026-10-09.json"
    path.write_text(json.dumps(report, indent=2) + "\n")
    print(path, "five positions and 196608 RGBA bytes exact; three malformed fonts rejected")


if __name__ == "__main__":
    main()
