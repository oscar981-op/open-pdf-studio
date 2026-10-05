#!/usr/bin/env python3
"""Compare staged real Type0 TrueType quadratic glyphs with Hayro page pixels."""
import argparse
import json
import subprocess
import tempfile
from pathlib import Path

EXPECTED = {
    0: 11, 1: 6, 2: 38, 3: 10, 4: 18, 7: 8, 8: 2, 9: 11,
    11: 6, 12: 2, 18: 1, 27: 0, 30: 0, 52: 1, 53: 0,
    58: 0, 64: 0, 72: 0, 77: 0, 88: 0, 94: 0, 95: 0,
    140: 0, 196: 0, 208: 0,
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", type=Path, required=True)
    parser.add_argument("--hayro", type=Path, required=True)
    parser.add_argument("--dynlex", type=Path, required=True)
    parser.add_argument("--pdf", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    report = {"page": 2, "records": {}, "exact": 0,
              "one_byte_only": 0, "invalid_rejected": 0}
    with tempfile.TemporaryDirectory(prefix="opds-real-quadratic-") as folder:
        root = Path(folder)
        scene, isolated = root / "glyph.opm", root / "isolated.pdf"
        raw, rgba = root / "coverage.raw", root / "hayro.rgba"
        for index, expected_mismatch in EXPECTED.items():
            for path in (scene, isolated, raw, rgba):
                path.unlink(missing_ok=True)
            staged = subprocess.run(
                [args.stage, "--isolate-quad", args.pdf, "2", str(index), scene, isolated],
                check=True, capture_output=True, text=True,
            )
            if f"selected_record={index}" not in staged.stdout:
                raise AssertionError(f"wrong TGR1 record selected: {index}")
            subprocess.run([args.dynlex, scene, raw], check=True, capture_output=True)
            subprocess.run([args.hayro, isolated, "2", rgba], check=True,
                           capture_output=True)
            coverage, pixels = raw.read_bytes(), rgba.read_bytes()
            if len(coverage) != 256 * 192 or len(pixels) != len(coverage) * 4:
                raise AssertionError(f"record {index}: wrong page dimensions")
            mismatches = []
            for pixel, alpha in enumerate(coverage):
                expected = bytes((255 - alpha, 255 - alpha, 255 - alpha, 255))
                actual = pixels[4 * pixel:4 * pixel + 4]
                if actual != expected:
                    if actual[0] != actual[1] or actual[1] != actual[2] or actual[3] != 255:
                        raise AssertionError(f"record {index}: unexpected color")
                    mismatches.append((pixel % 256, pixel // 256, alpha, 255 - actual[0]))
            if len(mismatches) != expected_mismatch or any(
                    abs(dynlex - hayro) != 1 for _, _, dynlex, hayro in mismatches):
                raise AssertionError(
                    f"record {index}: {len(mismatches)} mismatches, first={mismatches[:5]}")
            report["records"][str(index)] = {
                "mismatch_pixels": len(mismatches),
                "ink_pixels": sum(bool(value) for value in coverage),
                "opm_bytes": scene.stat().st_size,
                "first_mismatches": mismatches[:5],
            }
            report["exact" if not mismatches else "one_byte_only"] += 1
        for invalid_index in (16, 999999):
            scene.unlink(missing_ok=True)
            isolated.unlink(missing_ok=True)
            invalid = subprocess.run(
                [args.stage, "--isolate-quad", args.pdf, "2", str(invalid_index), scene, isolated],
                capture_output=True,
            )
            if invalid.returncode == 0 or scene.exists() or isolated.exists():
                raise AssertionError(f"unsupported glyph record {invalid_index} was exported")
            report["invalid_rejected"] += 1
    if report["exact"] != 13 or report["one_byte_only"] != 12:
        raise AssertionError("quadratic glyph parity summary changed")
    encoded = json.dumps(report, indent=2) + "\n"
    if args.output:
        args.output.write_text(encoded)
    print(f"{len(EXPECTED)} real Type0 quadratic glyphs: {report['exact']} exact, "
          f"{report['one_byte_only']} with only one-byte edge differences; "
          f"{report['invalid_rejected']} unsupported records rejected")


if __name__ == "__main__":
    main()
