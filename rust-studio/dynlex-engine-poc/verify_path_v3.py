#!/usr/bin/env python3
"""Exercise the isolated PDF→OPDL-v3 geometry export with real PDF streams."""

import argparse
import struct
import subprocess
import tempfile
from pathlib import Path


def pdf_with_stream(content, width=256, height=192, inherited=b""):
    objects = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Count 1 /Kids [3 0 R] " + inherited + b" >>",
        (f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {width} {height}] "
         "/Resources << >> /Contents 4 0 R >>").encode(),
        f"<< /Length {len(content)} >>\nstream\n".encode() + content + b"endstream",
    ]
    pdf = bytearray(b"%PDF-1.4\n")
    offsets = [0]
    for index, obj in enumerate(objects, 1):
        offsets.append(len(pdf))
        pdf.extend(f"{index} 0 obj\n".encode() + obj + b"\nendobj\n")
    xref = len(pdf)
    pdf.extend(f"xref\n0 {len(offsets)}\n0000000000 65535 f \n".encode())
    for offset in offsets[1:]:
        pdf.extend(f"{offset:010d} 00000 n \n".encode())
    pdf.extend(f"trailer\n<< /Size {len(offsets)} /Root 1 0 R >>\n"
               f"startxref\n{xref}\n%%EOF\n".encode())
    return bytes(pdf)


def inspect(data):
    assert data[:4] == b"OPDL"
    version, width, height, count = struct.unpack_from("<IIII", data, 4)
    assert version == 3
    offset = 20
    segments = []
    for _ in range(count):
        length, opcode = struct.unpack_from("<IB", data, offset)
        assert data[offset + 5:offset + 8] == b"\0\0\0"
        size = {1: 24, 2: 24, 3: 56, 4: 8}[opcode]
        assert length == size
        coordinates = struct.unpack_from("<" + "d" * ((size - 8) // 8), data, offset + 8)
        segments.append((opcode, coordinates))
        offset += size
    assert offset == len(data)
    return width, height, segments


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exporter", type=Path, required=True)
    parser.add_argument("--v2-adapter", type=Path, required=True)
    args = parser.parse_args()
    exporter = args.exporter.resolve(strict=True)
    v2_adapter = args.v2_adapter.resolve(strict=True)
    valid = {
        "mixed": (b"20.5 80.5 m\n30.5 80.5 l\n40.5 100.5 70.5 100.5 90.5 80.5 c\nh\nS\n",
                  256, 192, [1, 2, 3, 4]),
        "a1-like": (b"20.5 80.5 m\n35.5 130.5 75.5 130.5 90.5 80.5 c\nS\n",
                    1684, 2384, [1, 3]),
    }
    invalid = {
        "missing-move": b"30 40 l\nS\n",
        "missing-stroke": b"20 30 m\n40 50 l\n",
        "unsupported-state": b"1 w\n20 30 m\n40 50 l\nS\n",
        "content-after-stroke": b"20 30 m\n40 50 l\nS\n10 20 m\n",
        "unfinished-cubic": b"20 30 m\n40 50 60 c\nS\n",
    }
    with tempfile.TemporaryDirectory(prefix="opds-v3-export-") as directory:
        work = Path(directory)
        for name, (content, width, height, tags) in valid.items():
            pdf = work / f"{name}.pdf"
            output = work / f"{name}.opdl"
            pdf.write_bytes(pdf_with_stream(content, width, height))
            subprocess.run([exporter, pdf, output], check=True, capture_output=True)
            got_width, got_height, segments = inspect(output.read_bytes())
            assert (got_width, got_height) == (width, height)
            assert [tag for tag, _ in segments] == tags
            if name == "mixed":
                assert segments[2][1] == (40.5, 100.5, 70.5, 100.5, 90.5, 80.5)
            rejected = subprocess.run([v2_adapter, pdf, work / f"{name}-v2.opdl"],
                                      capture_output=True)
            assert rejected.returncode != 0
        for name, content in invalid.items():
            pdf = work / f"bad-{name}.pdf"
            output = work / f"bad-{name}.opdl"
            pdf.write_bytes(pdf_with_stream(content))
            rejected = subprocess.run([exporter, pdf, output], capture_output=True)
            assert rejected.returncode != 0 and not output.exists(), name
        rotated = work / "inherited-rotation.pdf"
        rotated.write_bytes(pdf_with_stream(valid["mixed"][0], inherited=b"/Rotate 90"))
        rotated_output = work / "inherited-rotation.opdl"
        rejected = subprocess.run([exporter, rotated, rotated_output], capture_output=True)
        assert rejected.returncode != 0 and not rotated_output.exists()
    print(f"{len(valid)} real PDF path exports roundtrip; {len(invalid) + 1} invalid PDFs rejected")


if __name__ == "__main__":
    main()
