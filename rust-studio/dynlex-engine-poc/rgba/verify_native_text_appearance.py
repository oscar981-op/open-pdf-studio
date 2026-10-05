#!/usr/bin/env python3
"""Compare an actual native FreeText /AP with staged DynLex glyph pixels."""

import argparse
import hashlib
import subprocess
import tempfile
from pathlib import Path


def run(*args):
    return subprocess.run([str(arg) for arg in args], check=True, capture_output=True, text=True)


def exact(left, right, label):
    if len(left) != len(right):
        raise AssertionError(f"{label}: length {len(left)} != {len(right)}")
    wrong = [index for index, pair in enumerate(zip(left, right)) if pair[0] != pair[1]]
    if wrong:
        raise AssertionError(f"{label}: {len(wrong)} wrong bytes, first {wrong[:8]}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("saved", "flattened", "stage", "dynlex", "hayro", "vello"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    source_hashes = [hashlib.sha256(path.read_bytes()).hexdigest() for path in (args.saved, args.flattened)]
    with tempfile.TemporaryDirectory(prefix="opds-native-text-ap-") as folder:
        root = Path(folder)
        saved_rgba = root / "saved.rgba"
        flat_rgba = root / "flat.rgba"
        scene = root / "glyphs.opgs"
        isolated = root / "isolated.pdf"
        dynlex_rgba = root / "dynlex.rgba"
        vello_rgba = root / "vello.rgba"
        run(args.hayro, args.saved, 1, saved_rgba)
        run(args.hayro, args.flattened, 1, flat_rgba)
        expected = saved_rgba.read_bytes()
        exact(expected, flat_rgba.read_bytes(), "saved /AP vs flattened Form")
        if len(expected) != 200 * 300 * 4:
            raise AssertionError("unexpected source page dimensions")
        run(args.stage, "--isolate-text-run", args.flattened, 1, 0, 3,
            scene, isolated, "page-coords")
        data = scene.read_bytes()
        if data[:4] != b"OPGS" or int.from_bytes(data[16:20], "little") != 3:
            raise AssertionError("wrong ordered glyph scene")
        if data[20:52].split(b"\0", 1)[0] != b"OPDS0" or data[56:59] != bytes((0, 80, 240)):
            raise AssertionError("font resource or blue text color lost")
        run(args.dynlex, scene, dynlex_rgba)
        run(args.vello, scene, vello_rgba)
        actual = dynlex_rgba.read_bytes()
        if len(actual) != 256 * 192 * 4:
            raise AssertionError("unexpected DynLex tile dimensions")
        exact(actual, vello_rgba.read_bytes(), "DynLex vs independent Vello oracle")
        visible = b"".join(actual[y * 256 * 4:y * 256 * 4 + 200 * 4] for y in range(192))
        reference = b"".join(expected[y * 200 * 4:(y + 1) * 200 * 4] for y in range(192))
        exact(visible, reference, "DynLex glyphs vs actual saved /AP")
        print("3 blue Unicode glyphs: 153,600/153,600 visible RGBA bytes exact against saved /AP")
    if source_hashes != [hashlib.sha256(path.read_bytes()).hexdigest() for path in (args.saved, args.flattened)]:
        raise AssertionError("source fixture changed")


if __name__ == "__main__":
    main()
