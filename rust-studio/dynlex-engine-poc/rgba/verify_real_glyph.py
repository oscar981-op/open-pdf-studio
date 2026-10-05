#!/usr/bin/env python3
"""Verify one real CIDFontType2 line-only glyph through experimental DynLex coverage."""
import argparse
import subprocess
import tempfile
from pathlib import Path


def main():
    parser = argparse.ArgumentParser()
    for name in ("stage", "hayro", "dynlex", "pdf"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="opds-real-glyph-") as folder:
        root = Path(folder)
        outline = root / "glyph.opln"
        isolated = root / "isolated.pdf"
        hayro = root / "hayro.rgba"
        dynlex = root / "dynlex.coverage"
        stage = subprocess.run(
            [args.stage, "--isolate", args.pdf, "2", "auto", outline, isolated],
            check=True, text=True, capture_output=True,
        )
        if "selected_record=16" not in stage.stdout or "skipped_text_ops=0" not in stage.stdout:
            raise AssertionError(f"unexpected real-PDF glyph staging:\n{stage.stdout}")
        rejected = root / "unsupported.opln"
        bad = subprocess.run(
            [args.stage, "--outline", args.pdf, "2", "0", rejected],
            text=True, capture_output=True,
        )
        if bad.returncode == 0 or rejected.exists():
            raise AssertionError("unsupported glyph was exported")
        subprocess.run([args.hayro, isolated, "2", hayro], check=True, capture_output=True)
        subprocess.run([args.dynlex, outline, dynlex], check=True, capture_output=True)
        expected = hayro.read_bytes()
        actual = dynlex.read_bytes()
        assert len(expected) == 256 * 192 * 4
        assert len(actual) == 256 * 192
        mismatch = []
        for index, coverage in enumerate(actual):
            pixel = expected[index * 4:index * 4 + 4]
            if pixel != bytes((255 - coverage, 255 - coverage, 255 - coverage, 255)):
                mismatch.append((index % 256, index // 256, coverage, tuple(pixel)))
        if mismatch:
            raise AssertionError(f"{len(mismatch)} differing pixels; first={mismatch[:8]}")
        print(f"real glyph: {len(actual)} exact coverage bytes; "
              f"{sum(value != 0 for value in actual)} ink pixels; record 16")


if __name__ == "__main__":
    main()
