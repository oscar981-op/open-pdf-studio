#!/usr/bin/env python3
"""Verify a two-font Type0 run in true page coordinates, including /Rotate90."""
import argparse
import hashlib
import subprocess
import tempfile
from pathlib import Path


def run(*argv):
    return subprocess.run([str(item) for item in argv], check=True, capture_output=True, text=True)


def names(scene):
    data = scene.read_bytes()
    if data[:4] != b"OPGS" or int.from_bytes(data[16:20], "little") != 2:
        raise AssertionError("wrong two-font scene header")
    at = 20
    result = []
    for _ in range(2):
        result.append(data[at:at + 32].split(b"\0", 1)[0])
        length = int.from_bytes(data[at + 40:at + 44], "little")
        at += 44 + length
    if at != len(data):
        raise AssertionError("wrong scene record lengths")
    return result


def exact(reference, candidate, label):
    a, b = reference.read_bytes(), candidate.read_bytes()
    if len(a) != 256 * 192 * 4 or len(a) != len(b):
        raise AssertionError(f"{label}: invalid RGBA dimensions")
    wrong = [i for i, (x, y) in enumerate(zip(a, b)) if x != y]
    if wrong:
        raise AssertionError(f"{label}: {len(wrong)} differing bytes; first={wrong[:8]}")


def main():
    parser = argparse.ArgumentParser()
    for name in ("fixture", "stage", "probe", "hayro", "vello", "pdf"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    source_hash = hashlib.sha256(args.pdf.read_bytes()).digest()
    with tempfile.TemporaryDirectory(prefix="opds-two-font-") as folder:
        root = Path(folder)
        scenes = []
        references = []
        for mode in ("normal", "rotated", "rotated-180", "rotated-270"):
            source = root / f"{mode}.pdf"
            scene = root / f"{mode}.opgs"
            isolated = root / f"{mode}-isolated.pdf"
            actual = root / f"{mode}-dynlex.rgba"
            expected = root / f"{mode}-hayro.rgba"
            oracle = root / f"{mode}-vello.rgba"
            run(args.fixture, args.pdf, "2", mode, source)
            run(args.stage, "--isolate-text-run", source, "2", "0", "2",
                scene, isolated, "page-coords")
            if names(scene) != [b"F1", b"F2"]:
                raise AssertionError("font switch not preserved in OPGS")
            run(args.probe, scene, actual)
            run(args.hayro, source, "2", expected)
            run(args.vello, scene, oracle)
            exact(expected, actual, mode + " DynLex")
            exact(expected, oracle, mode + " Vello oracle")
            scenes.append(scene.read_bytes())
            references.append(expected.read_bytes())
            print(f"{mode}: two fonts, 196,608/196,608 RGBA bytes exact")
        if any(scene != scenes[0] for scene in scenes[1:]) or any(
            reference != references[0] for reference in references[1:]
        ):
            raise AssertionError("rotation/CropBox changed visible two-font pixels")

        for mode in ("bad-rotate", "bad-crop", "user-unit", "bad-alpha"):
            source = root / f"{mode}.pdf"
            scene = root / f"{mode}.opgs"
            isolated = root / f"{mode}-isolated.pdf"
            run(args.fixture, args.pdf, "2", mode, source)
            attempt = subprocess.run(
                [str(args.stage), "--isolate-text-run", str(source), "2", "0", "2",
                 str(scene), str(isolated), "page-coords"], capture_output=True)
            if attempt.returncode == 0 or scene.exists() or isolated.exists():
                raise AssertionError(f"unsupported {mode} page accepted")
        print("4 unsupported page or paint states rejected without output")
    if hashlib.sha256(args.pdf.read_bytes()).digest() != source_hash:
        raise AssertionError("source PDF changed")


if __name__ == "__main__":
    main()
