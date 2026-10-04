#!/usr/bin/env python3
"""Check isolated Vello stroke coverage against Hayro-derived oracle maps."""

import argparse
import hashlib
import json
import subprocess
import tempfile
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", type=Path, required=True)
    args = parser.parse_args()
    probe = args.probe.resolve(strict=True)
    report = json.loads((HERE / "curve-reference-2026-10-03.json").read_text())
    with tempfile.TemporaryDirectory(prefix="opds-vello-curve-") as folder:
        output = Path(folder) / "coverage.raw"
        for name in ("arch", "s-bend"):
            reference = zlib.decompress((HERE / f"curve-coverage-{name}.zlib").read_bytes())
            expected_hash = report["samples"][f"vector/{name}"]["coverage_sha256"]
            if len(reference) != 256 * 192 or hashlib.sha256(reference).hexdigest() != expected_hash:
                raise RuntimeError(f"corrupt oracle map: {name}")
            subprocess.run([probe, name, output], check=True, capture_output=True)
            actual = output.read_bytes()
            if actual != reference:
                mismatches = sum(a != b for a, b in zip(actual, reference))
                raise RuntimeError(f"Vello {name}: {mismatches} coverage pixels differ")
            print(f"{name}: 49,152 coverage bytes exact")
        output.unlink()
        rejected = subprocess.run([probe, "unsupported", output], capture_output=True)
        if rejected.returncode == 0 or output.exists():
            raise RuntimeError("isolated Vello probe accepted an unknown curve")


if __name__ == "__main__":
    main()
