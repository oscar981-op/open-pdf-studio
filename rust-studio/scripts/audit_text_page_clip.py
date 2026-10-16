#!/usr/bin/env python3
"""Check page-edge intersection of bounded text clips against Hayro."""
import argparse
import hashlib
import json
import subprocess
from pathlib import Path

from audit_text_winansi import fixture


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", type=Path, required=True)
    parser.add_argument("--font", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    args.output_dir.mkdir(parents=True, exist_ok=True)
    content = (b"q -0.25 -0.25 640.5 800.5 re W n\n"
               b"0.2 0.4 0.6 rg 8 8 624 784 re f\n"
               b"BT /F 40 Tf 1 0 0 1 8 80 Tm (H) Tj ET\nQ\n")
    rows = []
    for rotation in (0, 90, 180, 270):
        for crop in ("0 0 640 800", "8 8 632 792"):
            stem = f"pageclip-r{rotation}-s{int(crop.startswith('8'))}"
            pdf = args.output_dir / f"{stem}.pdf"
            pdf.write_bytes(fixture(args.font.read_bytes(), content, rotation, crop))
            for scale in (0.25, 1., 2.):
                ours = args.output_dir / f"{stem}-z{scale}.dynlex.rgba"
                oracle = args.output_dir / f"{stem}-z{scale}.hayro.rgba"
                for backend, output in (("--dynlex-strict", ours), ("--hayro", oracle)):
                    result = subprocess.run([str(args.probe), str(pdf), str(output), backend,
                                             f"--scale={scale}"], capture_output=True,
                                            text=True, timeout=45)
                    if result.returncode:
                        raise RuntimeError(f"{backend}: {result.stderr}")
                left, right = ours.read_bytes(), oracle.read_bytes()
                row = {"rotation": rotation, "crop": crop, "scale": scale,
                       "pdf_sha256": sha(pdf), "rgba_bytes": len(left),
                       "different_channels": sum(a != b for a, b in zip(left, right)),
                       "dynlex_sha256": sha(ours), "hayro_sha256": sha(oracle)}
                if len(left) != len(right) or row["different_channels"]:
                    raise AssertionError(row)
                rows.append(row)
    report = {"scope": "bounded page-edge rectangular clip plus fill/text",
              "probe_sha256": sha(args.probe), "font_sha256": sha(args.font),
              "exact_rgba_bytes": sum(row["rgba_bytes"] for row in rows), "cases": rows}
    output = args.output_dir / "dynlex-text-page-clip-2026-10-09.json"
    output.write_text(json.dumps(report, indent=2) + "\n")
    print(output, len(rows), "exact cases", report["exact_rgba_bytes"], "RGBA bytes")


if __name__ == "__main__":
    main()
