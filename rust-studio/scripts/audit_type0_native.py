#!/usr/bin/env python3
"""Strict native Type0/Identity-H differential fixtures and malformed rejection."""
import argparse
import hashlib
import json
import subprocess
import tempfile
from pathlib import Path


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def fixture(font, content, rotation=0, crop="0 0 256 192", cmap=b"/Identity", widths=b"[36 [500 530] 55 56 460]"):
    def stream(data):
        return b"<< /Length " + str(len(data)).encode() + b" >>\nstream\n" + data + b"\nendstream"

    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 256 192] /CropBox [{crop}] /Rotate {rotation} /Resources << /Font << /C 5 0 R /F 9 0 R >> >> /Contents 4 0 R >>".encode(),
        stream(content),
        b"<< /Type /Font /Subtype /Type0 /BaseFont /LiberationSans /Encoding /Identity-H /DescendantFonts [6 0 R] >>",
        b"<< /Type /Font /Subtype /CIDFontType2 /BaseFont /LiberationSans /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /CIDToGIDMap " + cmap + b" /DW 1000 /W " + widths + b" /FontDescriptor 7 0 R >>",
        b"<< /Type /FontDescriptor /FontName /LiberationSans /Flags 4 /FontBBox [-543 -303 1301 980] /ItalicAngle 0 /Ascent 905 /Descent -212 /CapHeight 687 /StemV 80 /FontFile2 8 0 R >>",
        stream(font),
        b"<< /Type /Font /Subtype /TrueType /BaseFont /LiberationSans /Encoding /WinAnsiEncoding /FirstChar 32 /LastChar 255 /Widths [" + b"600 " * 224 + b"] /FontDescriptor 10 0 R >>",
        b"<< /Type /FontDescriptor /FontName /LiberationSans /Flags 32 /FontBBox [-543 -303 1301 980] /ItalicAngle 0 /Ascent 905 /Descent -212 /CapHeight 687 /StemV 80 /FontFile2 8 0 R >>",
    ]
    data = bytearray(b"%PDF-1.7\n")
    offsets = [0]
    for index, obj in enumerate(objects, 1):
        offsets.append(len(data))
        data += f"{index} 0 obj\n".encode() + obj + b"\nendobj\n"
    startxref = len(data)
    data += f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode()
    for offset in offsets[1:]:
        data += f"{offset:010} 00000 n \n".encode()
    data += f"trailer << /Root 1 0 R /Size {len(offsets)} >>\nstartxref\n{startxref}\n%%EOF\n".encode()
    return bytes(data)


def content(seed):
    shift = seed * 3
    return (
        f"0.14 0.28 0.5 rg 5 5 246 182 re f\n"
        f"q 1 0.5 0.1 rg BT /C 24 Tf 1 Tc 1 0 0 1 {16+shift} 110 Tm <00240025003700380058> Tj ET Q\n"
        f"0.9 g BT /C 19 Tf 1 0 0 1 {20+shift} 87 Tm [<00580024> -80 <0037>] TJ ET\n"
        f"0 0.8 0.4 rg BT /F 14 Tf 1 0 0 1 {24+shift} 65 Tm (AB) Tj ET\n"
    ).encode()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    root = Path("target/release")
    probe = root / "backend_probe"
    worker = root / "opds-text-worker"
    font = Path("assets/fonts/LiberationSans-Regular.ttf")
    hashes = {name: sha(path) for name, path in [("probe", probe), ("worker", worker), ("font", font)]}
    report = {"hashes": hashes, "protocol": "Full RGBA equality against Hayro; strict DynLex without fallback; 4 rotations, two CropBoxes, three scales, mixed Identity-H/WinAnsi and ordered color/rectangle paints.", "cases": [], "rejected": []}
    with tempfile.TemporaryDirectory(prefix="opds-type0-") as temp:
        folder = Path(temp)
        for seed in [0, 1]:
            for rotation in [0, 90, 180, 270]:
                for crop in ["0 0 256 192", "4 4 252 188"]:
                    pdf = folder / "fixture.pdf"
                    pdf.write_bytes(fixture(font.read_bytes(), content(seed), rotation, crop))
                    for scale in [0.25, 1, 2]:
                        out = folder / "actual.rgba"
                        out.unlink(missing_ok=True)
                        command = [str(probe), str(pdf), str(out), "--dynlex-strict", f"--scale={scale}"]
                        run = subprocess.run(command, capture_output=True, text=True, timeout=40)
                        row = {"seed": seed, "rotation": rotation, "crop": crop, "scale": scale, "source_sha256": sha(pdf), "exit": run.returncode}
                        if run.returncode:
                            row["diagnostic"] = run.stderr
                            row["exact"] = False
                        else:
                            command[2] = str(folder / "expected.rgba")
                            command[3] = "--hayro"
                            subprocess.run(command, capture_output=True, check=True, timeout=40)
                            actual = out.read_bytes()
                            expected = (folder / "expected.rgba").read_bytes()
                            row["bytes"] = len(actual)
                            row["different_channels"] = sum(a != b for a, b in zip(actual, expected))
                            row["exact"] = actual == expected
                        report["cases"].append(row)
                        print(seed, rotation, crop, scale, row["exact"], row.get("different_channels"), flush=True)
        bad = [
            (b"BT /C 20 Tf 1 0 0 1 20 100 Tm <00> Tj ET", b"/Identity", b"[36 [500]]"),
            (b"BT /C 20 Tf 1 0 0 1 20 100 Tm <FFFF> Tj ET", b"/Identity", b"[36 [500]]"),
            (content(0), b"/NotIdentity", b"[36 [500]]"),
            (content(0), b"/Identity", b"[36]"),
            (content(0), b"/Identity", b"[37 36 500]"),
            (content(0), b"/Identity", b"[36 [460.5]]"),
        ]
        for index, (stream, cmap, widths) in enumerate(bad):
            pdf = folder / "invalid.pdf"
            pdf.write_bytes(fixture(font.read_bytes(), stream, cmap=cmap, widths=widths))
            out = folder / "invalid.rgba"
            out.unlink(missing_ok=True)
            run = subprocess.run([str(probe), str(pdf), str(out), "--dynlex-strict"], capture_output=True, text=True, timeout=40)
            assert run.returncode and not out.exists(), f"invalid case {index} painted"
            report["rejected"].append({"case": index, "diagnostic": run.stderr.strip()})
    report["all_exact"] = all(row["exact"] for row in report["cases"])
    args.output.write_text(json.dumps(report, indent=2) + "\n")
    assert hashes == {name: sha(path) for name, path in [("probe", probe), ("worker", worker), ("font", font)]}
    if not report["all_exact"]:
        raise SystemExit("Type0 pixel mismatch or rejection; see report")


if __name__ == "__main__":
    main()
