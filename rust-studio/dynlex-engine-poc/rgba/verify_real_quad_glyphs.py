#!/usr/bin/env python3
"""Compare staged real Type0 TrueType quadratic glyphs with Hayro page pixels."""
import argparse
import json
import struct
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
    parser.add_argument("--vello", type=Path,
                        help="read-only Vello CPU oracle for the same flattened OPM2")
    parser.add_argument("--quad-oracle", type=Path,
                        help="read-only Vello CPU oracle for unflattened OPQ1 quadratics")
    parser.add_argument("--pdf", type=Path, required=True)
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    report = {"page": 2, "records": {}, "exact": 0,
              "one_byte_only": 0, "invalid_rejected": 0,
              "vello_same_opm_mismatch_bytes": 0,
              "vello_direct_quad_mismatch_bytes": 0,
              "invalid_opq_rejected": 0}
    with tempfile.TemporaryDirectory(prefix="opds-real-quadratic-") as folder:
        root = Path(folder)
        scene, isolated = root / "glyph.opm", root / "isolated.pdf"
        raw, rgba = root / "coverage.raw", root / "hayro.rgba"
        vello_raw = root / "vello.raw"
        quad_raw, quad_flat, quad_oracle_raw = (
            root / "glyph.opq", root / "quad-flat.txt", root / "quad-vello.raw")
        for index, expected_mismatch in EXPECTED.items():
            for path in (scene, isolated, raw, rgba, vello_raw,
                         quad_raw, quad_flat, quad_oracle_raw):
                path.unlink(missing_ok=True)
            command = [args.stage, "--isolate-quad", args.pdf, "2", str(index),
                       scene, isolated]
            if args.quad_oracle:
                command.append(quad_raw)
            staged = subprocess.run(
                command,
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
            if args.vello:
                subprocess.run([args.vello, "--opm", scene, vello_raw], check=True,
                               capture_output=True)
                vello_coverage = vello_raw.read_bytes()
                if len(vello_coverage) != len(coverage):
                    raise AssertionError(f"record {index}: Vello output length changed")
                oracle_mismatches = sum(a != b for a, b in zip(coverage, vello_coverage))
                report["vello_same_opm_mismatch_bytes"] += oracle_mismatches
                if oracle_mismatches:
                    raise AssertionError(
                        f"record {index}: Vello vs DynLex on same OPM2 has "
                        f"{oracle_mismatches} differing bytes")
            if args.quad_oracle:
                subprocess.run([args.quad_oracle, quad_raw, quad_flat,
                                root / "quad-vello.opm", quad_oracle_raw],
                               check=True, capture_output=True)
                direct = quad_oracle_raw.read_bytes()
                if len(direct) != len(coverage):
                    raise AssertionError(f"record {index}: quadratic oracle length changed")
                direct_mismatches = sum(a != b for a, b in zip(coverage, direct))
                report["vello_direct_quad_mismatch_bytes"] += direct_mismatches
                if direct_mismatches:
                    raise AssertionError(f"record {index}: Vello direct quadratic "
                                         f"vs DynLex differs on {direct_mismatches} pixels")
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
        if args.quad_oracle:
            original = quad_raw.read_bytes()
            bad = []
            bad.append(original[:-1])
            bad.append(original + b"x")
            altered = bytearray(original)
            altered[:4] = b"BAD!"
            bad.append(bytes(altered))
            altered = bytearray(original)
            struct.pack_into("<I", altered, 16, 1)
            bad.append(bytes(altered))
            altered = bytearray(original)
            struct.pack_into("<d", altered, 28, float("nan"))
            bad.append(bytes(altered))
            altered = bytearray(original)
            struct.pack_into("<I", altered, len(altered) - 40, 2)
            bad.append(bytes(altered))
            for index, value in enumerate(bad):
                source, destination = root / "invalid.opq", root / "invalid.txt"
                source.write_bytes(value)
                destination.unlink(missing_ok=True)
                result = subprocess.run([args.quad_oracle, source, destination],
                                        capture_output=True)
                if result.returncode == 0 or destination.exists():
                    raise AssertionError(f"invalid OPQ1 variant {index} was accepted")
                report["invalid_opq_rejected"] += 1
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
