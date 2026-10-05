#!/usr/bin/env python3
"""Verify bounded ordered CID glyph runs against Hayro and a Vello oracle."""
import argparse
import hashlib
import subprocess
import tempfile
from pathlib import Path


def run(*argv):
    return subprocess.run([str(part) for part in argv], check=True, capture_output=True, text=True)


def compare(expected, actual, label):
    source = expected.read_bytes()
    target = actual.read_bytes()
    if len(source) != 256 * 192 * 4 or len(target) != len(source):
        raise AssertionError(f"{label}: wrong RGBA size")
    wrong = [i for i, (a, b) in enumerate(zip(source, target)) if a != b]
    if wrong:
        raise AssertionError(f"{label}: {len(wrong)} differing bytes; first={wrong[:8]}")


def main():
    parser = argparse.ArgumentParser()
    for name in ("stage", "probe", "hayro", "vello", "pdf"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    original_hash = hashlib.sha256(args.pdf.read_bytes()).digest()
    cases = ((2, None), (3, None), (4, None), (4, "red-last"),
             (2, "red-overlap-last"), (3, "red-overlap-last"),
             (4, "red-overlap-last"))
    with tempfile.TemporaryDirectory(prefix="opds-glyph-run-") as folder:
        root = Path(folder)
        first_scene = None
        for count, mode in cases:
            name = f"{count}-{mode or 'black'}"
            scene = root / f"{name}.opgs"
            isolated = root / f"{name}.pdf"
            expected = root / f"{name}-hayro.rgba"
            actual = root / f"{name}-dynlex.rgba"
            oracle = root / f"{name}-vello.rgba"
            command = [args.stage, "--isolate-text-run", args.pdf, "2", "0", str(count),
                       scene, isolated]
            if mode:
                command.append(mode)
            staged = run(*command)
            if f"OPGS glyphs={count} first_record=0" not in staged.stdout:
                raise AssertionError(f"unexpected glyph staging: {staged.stdout}")
            run(args.probe, scene, actual)
            run(args.vello, scene, oracle)
            run(args.hayro, isolated, "2", expected)
            compare(expected, actual, name + " DynLex")
            compare(expected, oracle, name + " Vello oracle")
            print(f"{name}: 196,608/196,608 RGBA bytes exact")
            if first_scene is None:
                first_scene = scene.read_bytes()

        assert first_scene is not None
        corruptions = {
            "truncated": first_scene[:-1],
            "magic": b"BAD!" + first_scene[4:],
            "version": first_scene[:4] + (2).to_bytes(4, "little") + first_scene[8:],
            "count": first_scene[:16] + (33).to_bytes(4, "little") + first_scene[20:],
            "length": first_scene[:60] + (5000).to_bytes(4, "little") + first_scene[64:],
            "cid-gid": first_scene[:54] + (0).to_bytes(2, "little") + first_scene[56:],
            "flags": first_scene[:59] + b"\1" + first_scene[60:],
            "nested-magic": first_scene[:64] + b"BAD!" + first_scene[68:],
        }
        for label, payload in corruptions.items():
            scene = root / f"bad-{label}.opgs"
            output = root / f"bad-{label}.rgba"
            scene.write_bytes(payload)
            result = subprocess.run([str(args.probe), str(scene), str(output)], capture_output=True)
            if result.returncode == 0 or output.exists():
                raise AssertionError(f"invalid OPGS {label} was accepted")
        print(f"{len(corruptions)} malformed OPGS scenes rejected")

        for label, first, count in (("too-few", 0, 1), ("too-many", 0, 33),
                                    ("missing-record", 100000, 2)):
            scene = root / f"bad-{label}.opgs"
            isolated = root / f"bad-{label}.pdf"
            result = subprocess.run(
                [str(args.stage), "--isolate-text-run", str(args.pdf), "2", str(first),
                 str(count), str(scene), str(isolated)], capture_output=True)
            if result.returncode == 0 or scene.exists() or isolated.exists():
                raise AssertionError(f"invalid text-run request {label} was accepted")
        print("3 invalid text-run requests rejected without output")

    if hashlib.sha256(args.pdf.read_bytes()).digest() != original_hash:
        raise AssertionError("source PDF was modified")


if __name__ == "__main__":
    main()
