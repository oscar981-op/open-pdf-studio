#!/usr/bin/env python3
"""Verify independent Rust area rasterizer on bounded flattened outlines."""

import argparse
import subprocess
import tempfile
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--rasterizer", type=Path, required=True)
    args = parser.parse_args()
    rasterizer = args.rasterizer.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="opds-rust-area-") as folder:
        work = Path(folder)
        output = work / "coverage.raw"
        for name in ("arch", "s-bend"):
            subprocess.run([rasterizer, HERE / f"curve-flat-{name}.txt", output],
                           check=True, capture_output=True)
            expected = zlib.decompress((HERE / f"curve-coverage-{name}.zlib").read_bytes())
            actual = output.read_bytes()
            if actual != expected or len(actual) != 256 * 192:
                raise RuntimeError(f"Rust area coverage differs: {name}")
            print(f"{name}: 49,152 area bytes exact")
        for name, bad in {
            "open": "0 0 1 0\n1 0 1 1\n",
            "disconnected": "0 0 1 0\n2 0 0 0\n",
            "nonfinite": "0 0 NaN 1\n",
            "extra": "0 0 1 0 3\n",
        }.items():
            source = work / f"{name}.txt"
            source.write_text(bad)
            output.unlink(missing_ok=True)
            rejected = subprocess.run([rasterizer, source, output], capture_output=True)
            if rejected.returncode == 0 or output.exists():
                raise RuntimeError(f"Rust area rasterizer accepted {name}")


if __name__ == "__main__":
    main()
