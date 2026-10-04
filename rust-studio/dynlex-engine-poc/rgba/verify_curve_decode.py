#!/usr/bin/env python3
"""Check validated OPLN-to-DynLex f64 boundary and malformed-input rejection."""

import argparse
import subprocess
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--scene-tool", type=Path, required=True)
    parser.add_argument("--probe", type=Path, required=True)
    args = parser.parse_args()
    scene_tool = args.scene_tool.resolve(strict=True)
    probe = args.probe.resolve(strict=True)
    with tempfile.TemporaryDirectory(prefix="opds-curve-decode-") as folder:
        work = Path(folder)
        for name, count in (("arch", 29), ("s-bend", 31)):
            scene = work / f"{name}.opln"
            subprocess.run([scene_tool, "encode", HERE / f"curve-flat-{name}.txt", scene],
                           check=True, capture_output=True)
            output = subprocess.run([probe, scene], check=True, capture_output=True, text=True)
            fields = output.stdout.split()
            if len(fields) != 4 or fields[0] != str(count) or fields[1] != fields[2]:
                raise RuntimeError(f"wrong DynLex point boundary for {name}: {fields}")
            print(f"{name}: {count} aligned f64 points validated")
        valid = (work / "arch.opln").read_bytes()
        for name, malformed in {
            "truncated": valid[:-1],
            "extra": valid + b"\0",
            "wrong-version": valid[:4] + (2).to_bytes(4, "little") + valid[8:],
            "nan": valid[:20] + bytes.fromhex("000000000000f87f") + valid[28:],
        }.items():
            scene = work / f"{name}.opln"
            scene.write_bytes(malformed)
            rejected = subprocess.run([probe, scene], capture_output=True)
            if rejected.returncode == 0:
                raise RuntimeError(f"DynLex boundary accepted invalid OPLN: {name}")


if __name__ == "__main__":
    main()
