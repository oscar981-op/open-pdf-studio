#!/usr/bin/env python3
"""Check experimental pure-Rust Type0 font staging + DynLex against Hayro."""
import argparse
import subprocess
import tempfile
from pathlib import Path

from verify_real_quad_glyphs import EXPECTED


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", type=Path, required=True)
    parser.add_argument("--dynlex", type=Path, required=True)
    parser.add_argument("--hayro", type=Path, required=True)
    parser.add_argument("--pdf", type=Path, required=True)
    args = parser.parse_args()
    equal = 0
    with tempfile.TemporaryDirectory(prefix="opds-native-quad-") as folder:
        root = Path(folder)
        scene, isolated = root / "glyph.opm", root / "isolated.pdf"
        coverage, rgba = root / "glyph.raw", root / "hayro.rgba"
        for index in EXPECTED:
            for path in (scene, isolated, coverage, rgba):
                path.unlink(missing_ok=True)
            stage = subprocess.run(
                [args.stage, "--isolate-quad-skrifa", args.pdf, "2", str(index),
                 scene, isolated], check=True, capture_output=True, text=True)
            if f"selected_record={index}" not in stage.stdout:
                raise AssertionError(f"wrong selected glyph record {index}")
            subprocess.run([args.dynlex, scene, coverage], check=True,
                           capture_output=True)
            subprocess.run([args.hayro, isolated, "2", rgba], check=True,
                           capture_output=True)
            actual, reference = coverage.read_bytes(), rgba.read_bytes()
            if len(actual) != 256 * 192 or len(reference) != len(actual) * 4:
                raise AssertionError("wrong output length")
            wrong = [pixel for pixel, alpha in enumerate(actual)
                     if reference[pixel * 4:pixel * 4 + 4] !=
                     bytes((255 - alpha, 255 - alpha, 255 - alpha, 255))]
            if wrong:
                p = wrong[0]
                raise AssertionError(f"record {index}: {len(wrong)} pixel mismatches, "
                                     f"first=({p % 256},{p // 256})")
            equal += 1
        for invalid in (16, 999999):
            scene.unlink(missing_ok=True)
            isolated.unlink(missing_ok=True)
            result = subprocess.run(
                [args.stage, "--isolate-quad-skrifa", args.pdf, "2", str(invalid),
                 scene, isolated], capture_output=True)
            if result.returncode == 0 or scene.exists() or isolated.exists():
                raise AssertionError(f"unsupported record {invalid} was accepted")
    print(f"native Rust font staging + DynLex: {equal} real quadratic glyphs "
          f"pixel-exact against Hayro; 2 unsupported records rejected")


if __name__ == "__main__":
    main()
