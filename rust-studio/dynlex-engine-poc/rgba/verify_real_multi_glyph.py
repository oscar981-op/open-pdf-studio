#!/usr/bin/env python3
"""Pixel-verify two real line-only multi-contour CID glyphs against Hayro."""
import argparse
import subprocess
import tempfile
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    for name in ("stage", "hayro", "probe", "pdf"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="opds-multi-glyph-") as folder:
        root = Path(folder)
        for index, gid, expected_ink in ((26, 76, 18), (264, 23, 14)):
            scene = root / f"{index}.opm"
            isolated = root / f"{index}.pdf"
            actual = root / f"{index}.raw"
            expected = root / f"{index}.rgba"
            staged = subprocess.run(
                [args.stage, "--isolate-multi", args.pdf, "2", str(index), scene, isolated],
                check=True, text=True, capture_output=True,
            )
            if f"selected_record={index}" not in staged.stdout or f"multi_outline gid={gid}" not in staged.stdout:
                raise AssertionError(f"unexpected glyph: {staged.stdout}")
            subprocess.run([args.probe, scene, actual], check=True, capture_output=True)
            subprocess.run([args.hayro, isolated, "2", expected], check=True, capture_output=True)
            coverage = actual.read_bytes()
            rgba = expected.read_bytes()
            if len(coverage) != 256 * 192 or len(rgba) != 256 * 192 * 4:
                raise AssertionError("wrong output dimensions")
            mismatch = [(i % 256, i // 256) for i, ink in enumerate(coverage)
                        if rgba[i * 4:i * 4 + 4] != bytes((255-ink, 255-ink, 255-ink, 255))]
            if mismatch or sum(ink != 0 for ink in coverage) != expected_ink:
                raise AssertionError(f"glyph {index}: {len(mismatch)} pixel mismatches; first={mismatch[:8]}")
            truncated = root / f"{index}-truncated.opm"
            truncated.write_bytes(scene.read_bytes()[:-1])
            rejected = root / f"{index}-invalid.raw"
            bad = subprocess.run([args.probe, truncated, rejected], capture_output=True)
            if bad.returncode == 0 or rejected.exists():
                raise AssertionError("truncated contour scene was accepted")
            print(f"record {index}, GID {gid}: 49,152 exact pixels; {expected_ink} ink pixels")


if __name__ == "__main__":
    main()
