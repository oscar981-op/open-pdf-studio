#!/usr/bin/env python3
"""Alternating A/B benchmark for two DynLex renderers on identical OPDL input."""

import argparse
import hashlib
import json
import shutil
import statistics
import subprocess
import tempfile
import time
from pathlib import Path

from sys import path as search_path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
search_path.insert(0, str(HERE.parent))
from pdf_fixture import build_pdf, reference_rgba  # noqa: E402

MODES = ("raster", "vector", "hybrid")
RENDERER_PATH = "rust-studio/dynlex-engine-poc/rgba/renderer.dl"


def run(binary, directory):
    started = time.perf_counter_ns()
    subprocess.run([binary], cwd=directory, check=True, capture_output=True)
    return (time.perf_counter_ns() - started) / 1_000_000


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--compiler", type=Path, required=True)
    parser.add_argument("--adapter", type=Path, required=True)
    parser.add_argument("--baseline-ref", required=True,
                        help="Git revision containing the baseline renderer")
    parser.add_argument("--candidate-ref", help="Git revision; default: working-tree renderer")
    parser.add_argument("--size", default="1024x768")
    parser.add_argument("--runs", type=int, default=30)
    parser.add_argument("--dense", action="store_true")
    parser.add_argument("--output", type=Path)
    args = parser.parse_args()
    try:
        width, height = (int(part) for part in args.size.split("x"))
    except ValueError:
        parser.error("size must be WIDTHxHEIGHT")
    if args.runs < 1 or width < 1 or height < 1 or width > 1024 or height > 1024:
        parser.error("runs must be positive and dimensions within 1..1024")
    if width % 256 or height % 192:
        parser.error("dimensions must be multiples of 256x192")

    compiler = args.compiler.resolve(strict=True)
    adapter = args.adapter.resolve(strict=True)
    stdlib = compiler.parent.parent / "share/dynlex/lib"

    def source(revision):
        if revision is None:
            return (HERE / "renderer.dl").read_bytes()
        return subprocess.check_output(["git", "show", f"{revision}:{RENDERER_PATH}"], cwd=ROOT)

    sources = {"baseline": source(args.baseline_ref), "candidate": source(args.candidate_ref)}
    report = {"baseline_ref": args.baseline_ref, "candidate_ref": args.candidate_ref or "working-tree",
              "size": args.size, "dense": args.dense, "warm_runs_per_variant": args.runs,
              "modes": {}}
    with tempfile.TemporaryDirectory(prefix="opds-dynlex-ab-") as temporary:
        work = Path(temporary)
        for mode in MODES:
            pdf = work / f"{mode}.pdf"
            scene = work / f"{mode}.opdl"
            pdf.write_bytes(build_pdf(mode, width=width, height=height, dense=args.dense))
            subprocess.run([adapter, pdf, scene], check=True, capture_output=True)
            expected = reference_rgba(mode, width, height, dense=args.dense)
            variants = {}
            for name, renderer in sources.items():
                directory = work / mode / name
                directory.mkdir(parents=True)
                (directory / "lib").symlink_to(stdlib, target_is_directory=True)
                (directory / "renderer.dl").write_bytes(renderer)
                shutil.copy2(HERE / f"{mode}.dl", directory / f"{mode}.dl")
                shutil.copy2(scene, directory / "scene.opdl")
                binary = directory / mode
                subprocess.run([compiler, f"{mode}.dl", "-O3", "-o", binary],
                               cwd=directory, check=True, capture_output=True)
                variants[name] = (binary, directory)

            first = {}
            for name, (binary, directory) in variants.items():
                first[name] = round(run(binary, directory), 3)
                actual = (directory / "output.rgba").read_bytes()
                if actual != expected:
                    raise RuntimeError(f"{mode}/{name}: RGBA differs from independent reference")
            samples = {name: [] for name in variants}
            for index in range(args.runs):
                order = ("baseline", "candidate") if index % 2 == 0 else ("candidate", "baseline")
                for name in order:
                    binary, directory = variants[name]
                    samples[name].append(run(binary, directory))
            report["modes"][mode] = {
                "opdl_bytes": scene.stat().st_size,
                "sha256_rgba": hashlib.sha256(expected).hexdigest(),
                "first_process_ms": first,
                "warm_median_process_ms": {name: round(statistics.median(values), 3)
                                           for name, values in samples.items()},
            }
    rendered = json.dumps(report, indent=2)
    print(rendered)
    if args.output:
        args.output.write_text(rendered + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
