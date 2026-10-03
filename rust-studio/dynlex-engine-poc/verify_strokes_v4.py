#!/usr/bin/env python3
"""Check isolated two-stroke PDF export into the non-rendering OPDL-v4 format."""

import argparse
import struct
import subprocess
import tempfile
from pathlib import Path

from verify_path_v3 import pdf_with_stream

VALID = (b"1 w\n1 0 0 RG\n20.5 80.5 m\n35.5 130.5 75.5 130.5 90.5 80.5 c\nS\n"
         b"2 w\n0 0 1 RG\n40 40 m\n80 40 l\nS\n")
INVALID = {
    "missing-color": b"1 w\n20 30 m\n40 50 l\nS\n",
    "missing-width": b"1 0 0 RG\n20 30 m\n40 50 l\nS\n",
    "style-inside-path": b"1 w\n1 0 0 RG\n20 30 m\n0 0 1 RG\n40 50 l\nS\n",
    "missing-stroke": b"1 w\n1 0 0 RG\n20 30 m\n40 50 l\n",
    "empty-stroke": b"1 w\n1 0 0 RG\n20 30 m\nS\n",
    "zero-width": b"0 w\n1 0 0 RG\n20 30 m\n40 50 l\nS\n",
    "unsupported-graphics-state": b"q\n1 w\n1 0 0 RG\n20 30 m\n40 50 l\nS\nQ\n",
}


def inspect(data):
    assert data[:4] == b"OPDL"
    version, width, height, count = struct.unpack_from("<IIII", data, 4)
    assert version == 4
    offset = 20
    records = []
    for _ in range(count):
        length, opcode = struct.unpack_from("<IB", data, offset)
        assert data[offset + 5:offset + 8] == b"\0\0\0"
        assert length == {1: 24, 2: 24, 3: 56, 4: 8, 5: 24, 6: 8}[opcode]
        payload = data[offset + 8:offset + length]
        records.append((opcode, payload))
        offset += length
    assert offset == len(data)
    return width, height, records


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--exporter", type=Path, required=True)
    parser.add_argument("--v2-adapter", type=Path, required=True)
    args = parser.parse_args()
    exporter = args.exporter.resolve(strict=True)
    v2_adapter = args.v2_adapter.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="opds-v4-export-") as directory:
        work = Path(directory)
        pdf = work / "two-strokes.pdf"
        output = work / "two-strokes.opdl"
        pdf.write_bytes(pdf_with_stream(VALID, 1684, 2384))
        subprocess.run([exporter, pdf, output], check=True, capture_output=True)
        width, height, records = inspect(output.read_bytes())
        assert (width, height) == (1684, 2384)
        assert [tag for tag, _ in records] == [5, 1, 3, 6, 5, 1, 2, 6]
        assert records[0][1][:4] == b"\xff\x00\x00\xff"
        assert records[4][1][:4] == b"\x00\x00\xff\xff"
        assert struct.unpack_from("<d", records[0][1], 8)[0] == 1.0
        assert struct.unpack_from("<d", records[4][1], 8)[0] == 2.0
        assert struct.unpack("<6d", records[2][1]) == (35.5, 130.5, 75.5, 130.5, 90.5, 80.5)
        assert subprocess.run([v2_adapter, pdf, work / "v2.opdl"],
                              capture_output=True).returncode != 0
        for name, content in INVALID.items():
            source = work / f"invalid-{name}.pdf"
            destination = work / f"invalid-{name}.opdl"
            source.write_bytes(pdf_with_stream(content))
            result = subprocess.run([exporter, source, destination], capture_output=True)
            assert result.returncode != 0 and not destination.exists(), name
    print(f"two real PDF strokes preserve style/order; {len(INVALID)} invalid states rejected")


if __name__ == "__main__":
    main()
