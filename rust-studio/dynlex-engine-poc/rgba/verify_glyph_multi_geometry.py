#!/usr/bin/env python3
"""Compare experimental native DynLex OPM2 fills with small real PDF paths."""
import argparse
import struct
import subprocess
import tempfile
from pathlib import Path


def pdf_bytes(contours):
    operators = ["0 g"]
    for contour in contours:
        for index, (x, y) in enumerate(contour):
            operators.append(f"{x} {192-y} {'m' if index == 0 else 'l'}")
        operators.append("h")
    operators.append("f")
    content = ("\n".join(operators) + "\n").encode()
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 256 192] /Resources << >> /Contents 4 0 R >>",
        f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"endstream",
    ]
    output = bytearray(b"%PDF-1.7\n")
    offsets = [0]
    for index, obj in enumerate(objects, 1):
        offsets.append(len(output))
        output += f"{index} 0 obj\n".encode() + obj + b"\nendobj\n"
    xref = len(output)
    output += b"xref\n0 5\n0000000000 65535 f \n"
    for offset in offsets[1:]:
        output += f"{offset:010d} 00000 n \n".encode()
    output += f"trailer\n<< /Size 5 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n".encode()
    return bytes(output)


def opm_bytes(contours):
    output = bytearray(b"OPM2")
    output += struct.pack("<IIII", 1, 256, 192, len(contours))
    for contour in contours:
        output += struct.pack("<I", len(contour) + 1)
        for point in contour + [contour[0]]:
            output += struct.pack("<dd", *point)
    return bytes(output)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--hayro", type=Path, required=True)
    args = parser.parse_args()
    outer = [(100, 75), (156, 75), (156, 117), (100, 117)]
    hole = [(110, 84), (110, 108), (146, 108), (146, 84)]
    island = [(119, 91), (137, 91), (137, 101), (119, 101)]
    diamond = [(128, 66), (162, 96), (128, 126), (94, 96)]
    diamond_hole = [(128, 82), (112, 96), (128, 110), (144, 96)]
    cases = {
        "disjoint": [outer, [(174, 80), (194, 80), (194, 102), (174, 102)]],
        "hole": [outer, hole],
        "island": [outer, hole, island],
        "quarter_turn_hole": [[(96 + (y-75), 68 + (156-x)) for x,y in outer],
                              [(96 + (y-75), 68 + (156-x)) for x,y in hole]],
        "diagonal_hole": [diamond, diamond_hole],
    }
    with tempfile.TemporaryDirectory(prefix="opds-glyph-geometry-") as folder:
        root = Path(folder)
        for name, contours in cases.items():
            scene = root / f"{name}.opm"
            pdf = root / f"{name}.pdf"
            actual = root / f"{name}.raw"
            expected = root / f"{name}.rgba"
            scene.write_bytes(opm_bytes(contours))
            pdf.write_bytes(pdf_bytes(contours))
            subprocess.run([args.probe, scene, actual], check=True, capture_output=True)
            subprocess.run([args.hayro, pdf, "1", expected], check=True, capture_output=True)
            coverage, rgba = actual.read_bytes(), expected.read_bytes()
            if len(coverage) != 256*192 or len(rgba) != 256*192*4:
                raise AssertionError(f"{name}: wrong dimensions")
            mismatch = [(i % 256, i // 256, ink, rgba[i*4]) for i, ink in enumerate(coverage)
                        if rgba[i*4:i*4+4] != bytes((255-ink, 255-ink, 255-ink, 255))]
            if name == "diagonal_hole":
                if not mismatch:
                    raise AssertionError("expected diagonal AA diagnostic changed; revalidate")
                max_delta = max(abs(ink-(255-red)) for _,_,ink,red in mismatch)
                print(f"{name}: {len(mismatch)} unsupported edge pixels, max_delta={max_delta}; first={mismatch[:4]}")
                continue
            if mismatch:
                raise AssertionError(f"{name}: {len(mismatch)} mismatches, first={mismatch[:8]}")
            print(f"{name}: 49,152 exact pixels; {sum(v!=0 for v in coverage)} ink pixels")


if __name__ == "__main__":
    main()
