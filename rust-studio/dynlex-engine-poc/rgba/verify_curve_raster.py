#!/usr/bin/env python3
"""Verify full DynLex coverage buffers and rejection before the unsafe boundary."""
import argparse
import subprocess
import tempfile
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--probe", required=True, type=Path)
    parser.add_argument("--scene-tool", type=Path)
    args = parser.parse_args()
    probe = args.probe.resolve(strict=True)
    scene_tool = args.scene_tool.resolve(strict=True) if args.scene_tool else None
    with tempfile.TemporaryDirectory(prefix="opds-curve-raster-") as folder:
        work = Path(folder)
        output = work/"coverage.raw"
        for name in ("arch", "s-bend"):
            source = HERE/f"curve-flat-{name}.txt"
            expected = zlib.decompress((HERE/f"curve-coverage-{name}.zlib").read_bytes())
            subprocess.run([probe,source,output],check=True,capture_output=True)
            if output.read_bytes()!=expected:
                raise RuntimeError(f"{name}: DynLex differs from Hayro coverage")
            if scene_tool:
                scene = work/f"{name}.opln"
                subprocess.run([scene_tool,"encode",source,scene],check=True,capture_output=True)
                subprocess.run([probe,scene,output],check=True,capture_output=True)
                if output.read_bytes()!=expected:
                    raise RuntimeError(f"{name}: binary OPLN differs")
            print(f"{name}: 49,152 DynLex coverage bytes exact")
        for name, (left, top, right, bottom) in {
            "outside": (-3.0, 10.0, -1.0, 13.0),
            "left-edge": (0.0, 0.0, 1.0, 1.0),
            "right-edge": (255.0, 190.0, 257.0, 192.0),
            "fractional": (10.25, 20.25, 12.75, 22.75),
            "full-page": (0.0, 0.0, 256.0, 192.0),
        }.items():
            vertices = [(left, top), (right, top), (right, bottom),
                        (left, bottom), (left, top)]
            source = work/f"rect-{name}.txt"
            source.write_text("".join(
                f"{a[0]} {a[1]} {b[0]} {b[1]}\n"
                for a,b in zip(vertices, vertices[1:])))
            subprocess.run([probe,source,output],check=True,capture_output=True)
            expected = bytearray(256*192)
            for y in range(192):
                for x in range(256):
                    width = max(0.0, min(x+1,right)-max(x,left))
                    height = max(0.0, min(y+1,bottom)-max(y,top))
                    expected[y*256+x] = round(width*height*255)
            if output.read_bytes()!=expected:
                raise RuntimeError(f"{name}: rectangular edge coverage differs")
        print("5 bounded rectangle edge cases exact")
        bad = {
            "open": b"0 0 1 0\n1 0 1 1\n",
            "disconnected": b"0 0 1 0\n2 0 0 0\n",
            "nonfinite": b"0 0 NaN 1\n",
            "extra": b"0 0 1 0 3\n",
            "truncated-opln": b"OPLN\x01\x00",
        }
        for name,data in bad.items():
            source=work/f"bad-{name}"
            source.write_bytes(data)
            output.unlink(missing_ok=True)
            result=subprocess.run([probe,source,output],capture_output=True)
            if result.returncode==0 or output.exists():
                raise RuntimeError(f"DynLex probe accepted invalid {name}")
        print(f"{len(bad)} invalid inputs rejected without output")


if __name__ == "__main__":
    main()
