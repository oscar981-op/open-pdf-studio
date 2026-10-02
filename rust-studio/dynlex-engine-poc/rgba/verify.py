#!/usr/bin/env python3
"""Build OPDL v1, render all DynLex agents and compare every RGBA byte."""

import argparse
import hashlib
import json
import shutil
import statistics
import struct
import subprocess
import tempfile
import time
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
WIDTH, HEIGHT = 256, 192
CLIP = (16, 12, 240, 180)
# kind, x0, y0, x1, y1, RGBA. Kind 2 is a 45-degree line of width 3.
COMMANDS = (
    (1, 25, 20, 190, 24, (252, 20, 40, 255)),
    (2, 40, 30, 160, 150, (0, 180, 255, 128)),
    (1, 90, 75, 220, 130, (255, 220, 0, 96)),
    (1, 128, 50, 132, 175, (15, 20, 30, 255)),
)
AGENTS = ("raster", "vector", "hybrid")


def image_pixel(x, y):
    return ((x * 3 + y) % 256, (y * 5 + 37) % 256, (x ^ y) % 256)


def make_scene():
    image = bytearray()
    for y in range(HEIGHT):
        for x in range(WIDTH):
            image.extend(image_pixel(x, y))
    header = struct.pack("<4s8I", b"OPDL", 1, WIDTH, HEIGHT, *CLIP, len(image))
    body = b"".join(struct.pack("<5I4B", *command[:5], *command[5]) for command in COMMANDS)
    return header + image + struct.pack("<I", len(COMMANDS)) + body


def reference(mode):
    rgba = bytearray(WIDTH * HEIGHT * 4)
    for y in range(HEIGHT):
        for x in range(WIDTH):
            rgb = [255, 255, 255]
            if CLIP[0] <= x < CLIP[2] and CLIP[1] <= y < CLIP[3]:
                if mode != "vector":
                    rgb = list(image_pixel(x, y))
                if mode != "raster":
                    for kind, x0, y0, x1, y1, ink in COMMANDS:
                        inside = x0 <= x < x1 and y0 <= y < y1
                        hit = inside and (kind == 1 or abs((x - x0) - (y - y0)) <= 1)
                        if hit:
                            rgb = [(foreground * ink[3] + background * (255 - ink[3]) + 127) // 255
                                   for background, foreground in zip(rgb, ink[:3])]
            offset = (y * WIDTH + x) * 4
            rgba[offset:offset + 4] = bytes((*rgb, 255))
    return bytes(rgba)


def png(data):
    def chunk(kind, payload):
        return struct.pack(">I", len(payload)) + kind + payload + struct.pack(">I", zlib.crc32(kind + payload))
    rows = b"".join(b"\0" + data[y * WIDTH * 4:(y + 1) * WIDTH * 4] for y in range(HEIGHT))
    return (b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">2I5B", WIDTH, HEIGHT, 8, 6, 0, 0, 0))
            + chunk(b"IDAT", zlib.compress(rows, 6)) + chunk(b"IEND", b""))


def run(binary, work):
    (work / "output.rgba").unlink(missing_ok=True)
    started = time.perf_counter_ns()
    process = subprocess.run([binary], cwd=work, capture_output=True, text=True, check=True)
    elapsed_ms = (time.perf_counter_ns() - started) / 1_000_000
    return elapsed_ms, process.stdout.strip(), (work / "output.rgba").read_bytes()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--compiler", type=Path, default=shutil.which("dynlex"))
    parser.add_argument("--stdlib", type=Path)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--artifacts", type=Path, help="optional destination for OPDL and PNG output")
    args = parser.parse_args()
    if not args.compiler or args.runs < 1:
        parser.error("provide a DynLex compiler and at least one run")
    compiler = Path(args.compiler).resolve()
    stdlib = (args.stdlib or compiler.parent.parent / "share/dynlex/lib").resolve()
    if not (stdlib / "array.dl").is_file():
        parser.error(f"DynLex array library not found: {stdlib}")
    report = {"format": "OPDL-v1", "pixels": [WIDTH, HEIGHT], "commands": len(COMMANDS), "agents": {}}
    scene = make_scene()
    if args.artifacts:
        args.artifacts.mkdir(parents=True, exist_ok=True)
        (args.artifacts / "scene.opdl").write_bytes(scene)
    with tempfile.TemporaryDirectory(prefix="opds-dynlex-rgba-") as temporary:
        work = Path(temporary)
        (work / "lib").symlink_to(stdlib, target_is_directory=True)
        (work / "scene.opdl").write_bytes(scene)
        for source in ("renderer", *AGENTS):
            shutil.copy2(HERE / f"{source}.dl", work)
        for name in AGENTS:
            started = time.perf_counter_ns()
            binary = work / name
            subprocess.run([compiler, f"{name}.dl", "-O3", "-o", binary], cwd=work,
                           capture_output=True, text=True, check=True)
            compile_ms = (time.perf_counter_ns() - started) / 1_000_000
            expected = reference(name)
            timings = []
            for _ in range(args.runs):
                elapsed, stdout, actual = run(binary, work)
                if stdout != str(len(expected)) or actual != expected:
                    first = next((i for i, (a, b) in enumerate(zip(actual, expected)) if a != b), None)
                    raise RuntimeError(f"{name}: pixel mismatch at byte {first}; got {len(actual)} bytes, expected {len(expected)}")
                timings.append(elapsed)
            report["agents"][name] = {
                "sha256_rgba": hashlib.sha256(expected).hexdigest(),
                "compile_ms": round(compile_ms, 3),
                "median_process_ms": round(statistics.median(timings), 3),
                "min_process_ms": round(min(timings), 3),
            }
            if args.artifacts:
                (args.artifacts / f"{name}.png").write_bytes(png(actual))
            wrong_version = bytearray(scene)
            struct.pack_into("<I", wrong_version, 4, 3)
            unknown_command = bytearray(scene)
            struct.pack_into("<I", unknown_command, 36 + WIDTH * HEIGHT * 3 + 4, 99)
            newer_command_in_v1 = bytearray(scene)
            struct.pack_into("<I", newer_command_in_v1, 36 + WIDTH * HEIGHT * 3 + 4, 3)
            bad_scenes = {
                "wrong-version": wrong_version,
                "truncated": scene[:-10],
                "unknown-command": unknown_command,
                "newer-command-in-v1": newer_command_in_v1,
            }
            for reason, bad in bad_scenes.items():
                (work / "scene.opdl").write_bytes(bad)
                (work / "output.rgba").unlink(missing_ok=True)
                failed = subprocess.run([binary], cwd=work, capture_output=True)
                if failed.returncode == 0 or (work / "output.rgba").exists():
                    raise RuntimeError(f"{name}: {reason} OPDL was accepted")
            (work / "scene.opdl").write_bytes(scene)
    print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
