#!/usr/bin/env python3
"""Build and pixel-check the experimental in-process DynLex buffer kernel."""

import argparse
import os
import json
import statistics
import time
import shutil
import subprocess
import tempfile
from pathlib import Path
from sys import path as search_path

HERE = Path(__file__).resolve().parent
search_path.insert(0, str(HERE.parent))
from pdf_fixture import build_pdf, reference_rgba  # noqa: E402

MODES = (("raster", 1), ("vector", 2), ("hybrid", 3))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("compiler", "opt", "llc", "rustc", "adapter", "hayro"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--bench-calls", type=int, default=0,
                        help="repeat each dense page in one Rust process to time kernel calls")
    parser.add_argument("--bench-runs", type=int, default=0,
                        help="paired full-process PDF-to-RGBA runs on dense fixtures")
    parser.add_argument("--bench-many-calls", type=int, default=0,
                        help="repeat the 6000-command kernel call in one process")
    parser.add_argument("--bench-many-runs", type=int, default=0,
                        help="paired full-process runs on 6000-command fixtures")
    parser.add_argument("--bench-many-ab-runs", type=int, default=0,
                        help="alternate fast and lopdf decoders on 6000-command fixtures")
    parser.add_argument("--bench-memory", action="store_true",
                        help="measure per-process peak RSS with /usr/bin/time (Linux)")
    parser.add_argument("--bench-json", type=Path,
                        help="write raw paired full-process samples to JSON")
    args = parser.parse_args()
    if args.bench_memory and args.bench_runs < 1 and args.bench_many_runs < 1:
        parser.error("--bench-memory requires --bench-runs or --bench-many-runs")
    if args.bench_many_calls < 0 or args.bench_many_runs < 0 or args.bench_many_ab_runs < 0:
        parser.error("benchmark counts must be nonnegative")
    time_binary = Path("/usr/bin/time")
    if args.bench_memory and not time_binary.is_file():
        parser.error("/usr/bin/time is required for --bench-memory")
    measurements = {}
    paths = {name: getattr(args, name).resolve(strict=True)
             for name in ("compiler", "opt", "llc", "rustc", "adapter", "hayro")}
    adapter_rlib = paths["adapter"].parent / "libopds_opdl_adapter.rlib"
    if not adapter_rlib.is_file():
        raise RuntimeError(f"build the adapter library first: {adapter_rlib}")
    with tempfile.TemporaryDirectory(prefix="opds-dynlex-buffer-verify-") as temporary:
        work = Path(temporary)
        subprocess.run([
            "python3", HERE / "build_buffer.py", "--compiler", paths["compiler"],
            "--opt", paths["opt"], "--llc", paths["llc"], "--output-dir", work,
        ], check=True, capture_output=True)
        probe = work / "buffer_probe"
        subprocess.run([
            paths["rustc"], "--edition=2024", "-O", HERE / "buffer_probe.rs",
            "-L", f"native={work}",
            "-L", f"dependency={paths['adapter'].parent / 'deps'}",
            "--extern", f"opds_opdl_adapter={adapter_rlib}", "-o", probe,
        ], check=True, capture_output=True)
        (work / "lib").symlink_to(paths["compiler"].parent.parent / "share/dynlex/lib",
                                  target_is_directory=True)
        for name in ("renderer.dl", *(f"{mode}.dl" for mode, _ in MODES)):
            shutil.copy2(HERE / name, work / name)
        for mode, _ in MODES:
            subprocess.run([paths["compiler"], f"{mode}.dl", "-O3", "-o", work / mode],
                           cwd=work, check=True, capture_output=True)
        checked = 0
        checked_pages = 0
        for width, height, dense in ((256, 192, False), (512, 384, False), (1024, 768, True)):
            for mode, number in MODES:
                pdf = work / f"{mode}.pdf"
                scene = work / "scene.opdl"
                buffer_output = work / "buffer.rgba"
                direct_output = work / "direct.rgba"
                cli_output = work / "output.rgba"
                hayro_output = work / "hayro.rgba"
                pdf.write_bytes(build_pdf(mode, width=width, height=height, dense=dense))
                subprocess.run([paths["adapter"], pdf, scene], check=True, capture_output=True)
                subprocess.run([probe, str(number), scene, buffer_output], check=True, capture_output=True)
                subprocess.run([probe, str(number), pdf, direct_output],
                               check=True, capture_output=True)
                subprocess.run([work / mode], cwd=work, check=True, capture_output=True)
                subprocess.run([paths["hayro"], pdf, "1", hayro_output, "1"],
                               check=True, capture_output=True)
                expected = reference_rgba(mode, width, height, dense=dense)
                if not (buffer_output.read_bytes() == direct_output.read_bytes()
                        == cli_output.read_bytes()
                        == hayro_output.read_bytes() == expected):
                    raise RuntimeError(f"pixel mismatch: {width}x{height}/{mode}")
                if dense and args.bench_calls > 1:
                    timed = subprocess.run([probe, str(number), scene, buffer_output,
                                            str(args.bench_calls)], check=True,
                                           capture_output=True, text=True)
                    print(f"{mode}: {timed.stderr.strip()}")
                if dense and args.bench_runs > 0:
                    samples = {"direct": [], "hayro": []}
                    for index in range(args.bench_runs):
                        jobs = (("direct", [probe, str(number), pdf, direct_output]),
                                ("hayro", [paths["hayro"], pdf, "1", hayro_output, "1"]))
                        for label, command in (jobs if index % 2 == 0 else reversed(jobs)):
                            peak_path = work / "peak-kib.txt"
                            if args.bench_memory:
                                command = [time_binary, "-f", "%M", "-o", peak_path, *command]
                            start = time.perf_counter_ns()
                            subprocess.run(command, check=True, capture_output=True)
                            sample = {"wall_ms": (time.perf_counter_ns() - start) / 1_000_000}
                            if args.bench_memory:
                                sample["peak_rss_kib"] = int(peak_path.read_text().strip())
                            samples[label].append(sample)
                    direct_samples = [item["wall_ms"] for item in samples["direct"]]
                    hayro_samples = [item["wall_ms"] for item in samples["hayro"]]
                    paired = [a - b for a, b in zip(direct_samples, hayro_samples)]
                    measurements[mode] = {"page": f"{width}x{height}",
                                          "dense": True, "os_cache_cleared": False,
                                          "time_wrapper": args.bench_memory, "samples": samples}
                    print(f"{mode} dense direct/Hayro median ms: "
                          f"{statistics.median(direct_samples):.2f}/"
                          f"{statistics.median(hayro_samples):.2f}; "
                          f"paired delta {statistics.median(paired):+.2f}; "
                          f"direct wins {sum(delta < 0 for delta in paired)}/{args.bench_runs}")
                    if args.bench_memory:
                        direct_peak = [item["peak_rss_kib"] for item in samples["direct"]]
                        hayro_peak = [item["peak_rss_kib"] for item in samples["hayro"]]
                        print(f"{mode} peak RSS median KiB: "
                              f"{statistics.median(direct_peak):.0f}/"
                              f"{statistics.median(hayro_peak):.0f}; "
                              f"first-process wall ms: "
                              f"{direct_samples[0]:.2f}/{hayro_samples[0]:.2f} "
                              f"(OS cache not cleared)")
                checked += width * height
                checked_pages += 1
        for mode, number in MODES[1:]:
            for diagonal_color in ((0, 0, 0), (255, 0, 0)):
                for falling in (False, True):
                    width, height = 256, 192
                    pdf = work / f"diagonal-{mode}-{diagonal_color[0]}-{falling}.pdf"
                    scene = work / "scene.opdl"
                    direct_output = work / "direct.rgba"
                    cli_output = work / "output.rgba"
                    hayro_output = work / "hayro.rgba"
                    pdf.write_bytes(build_pdf(mode, width=width, height=height,
                                              diagonal=True, diagonal_color=diagonal_color,
                                              falling=falling))
                    subprocess.run([paths["adapter"], pdf, scene], check=True,
                                   capture_output=True)
                    aa_scene = scene.read_bytes()
                    subprocess.run([probe, str(number), pdf, direct_output], check=True,
                                   capture_output=True)
                    subprocess.run([work / mode], cwd=work, check=True, capture_output=True)
                    subprocess.run([paths["hayro"], pdf, "1", hayro_output, "1"], check=True,
                                   capture_output=True)
                    expected = reference_rgba(mode, width, height, diagonal=True,
                                              diagonal_color=diagonal_color, falling=falling)
                    if not (direct_output.read_bytes() == cli_output.read_bytes()
                            == hayro_output.read_bytes() == expected):
                        raise RuntimeError(
                            f"pixel mismatch: 45-degree AA/{mode}/{diagonal_color}/{falling}"
                        )
                    checked += width * height
                    checked_pages += 1
        for mode, number in MODES[1:]:
            width, height = 256, 192
            pdf = work / f"many-rectangles-{mode}.pdf"
            scene = work / "scene.opdl"
            direct_output = work / "direct.rgba"
            cli_output = work / "output.rgba"
            hayro_output = work / "hayro.rgba"
            pdf.write_bytes(build_pdf(mode, many_rects=True))
            subprocess.run([paths["adapter"], pdf, scene], check=True,
                           capture_output=True)
            opdl = scene.read_bytes()
            command_offset = 36 + int.from_bytes(opdl[32:36], "little")
            if int.from_bytes(opdl[command_offset:command_offset + 4], "little") <= 5_763:
                raise RuntimeError("large command fixture did not exceed drawing stroke count")
            direct_run = subprocess.run([probe, str(number), pdf, direct_output],
                                        capture_output=True)
            if direct_run.returncode:
                raise RuntimeError(direct_run.stderr.decode())
            subprocess.run([work / mode], cwd=work, check=True, capture_output=True)
            subprocess.run([paths["hayro"], pdf, "1", hayro_output, "1"], check=True,
                           capture_output=True)
            expected = reference_rgba(mode, many_rects=True)
            if not (direct_output.read_bytes() == cli_output.read_bytes()
                    == hayro_output.read_bytes() == expected):
                raise RuntimeError(f"pixel mismatch: 6000 rectangle draws/{mode}")
            if args.bench_many_calls > 1:
                timed = subprocess.run([probe, str(number), scene, direct_output,
                                        str(args.bench_many_calls)], check=True,
                                       capture_output=True, text=True)
                kernel_ms = float(timed.stderr.strip().split("=")[-1])
                measurements[f"many-{mode}-kernel"] = {
                    "commands": int.from_bytes(opdl[command_offset:command_offset + 4], "little"),
                    "repeat": args.bench_many_calls,
                    "kernel_ms_per_call": kernel_ms,
                    "scope": "validated OPDL to RGBA; excludes PDF parsing and process startup",
                }
                print(f"{mode} 6000-command kernel ms/call: {kernel_ms:.4f}")
            if args.bench_many_runs > 0:
                samples = {"direct": [], "hayro": []}
                for index in range(args.bench_many_runs):
                    jobs = (("direct", [probe, str(number), pdf, direct_output, "--profile"]),
                            ("hayro", [paths["hayro"], pdf, "1", hayro_output, "1"]))
                    for label, command in (jobs if index % 2 == 0 else reversed(jobs)):
                        peak_path = work / "peak-kib.txt"
                        if args.bench_memory:
                            command = [time_binary, "-f", "%M", "-o", peak_path, *command]
                        start = time.perf_counter_ns()
                        completed = subprocess.run(command, check=True, capture_output=True,
                                                   text=True)
                        sample = {"wall_ms": (time.perf_counter_ns() - start) / 1_000_000}
                        if label == "direct":
                            sample["stages_ms"] = {
                                key: float(value)
                                for part in completed.stderr.strip().split()
                                for key, value in (part.split("=", 1),)
                            }
                        if args.bench_memory:
                            sample["peak_rss_kib"] = int(peak_path.read_text().strip())
                        samples[label].append(sample)
                direct_ms = [item["wall_ms"] for item in samples["direct"]]
                hayro_ms = [item["wall_ms"] for item in samples["hayro"]]
                paired = [a - b for a, b in zip(direct_ms, hayro_ms)]
                measurements[f"many-{mode}"] = {
                    "page": f"{width}x{height}", "commands": int.from_bytes(
                        opdl[command_offset:command_offset + 4], "little"),
                    "os_cache_cleared": False, "time_wrapper": args.bench_memory,
                    "samples": samples,
                }
                print(f"{mode} 6000-command direct/Hayro median ms: "
                      f"{statistics.median(direct_ms):.2f}/{statistics.median(hayro_ms):.2f}; "
                      f"paired delta {statistics.median(paired):+.2f}; "
                      f"direct wins {sum(delta < 0 for delta in paired)}/{args.bench_many_runs}")
                stage_medians = {
                    stage: statistics.median(item["stages_ms"][stage]
                                             for item in samples["direct"])
                    for stage in ("load_ms", "read_ms", "document_ms", "page_decode_ms", "content_ms", "operator_decode_ms",
                                  "operators_ms", "encode_ms", "kernel_ms", "write_ms")
                }
                print(f"{mode} direct stage medians ms: "
                      f"load {stage_medians['load_ms']:.2f}, "
                      f"kernel {stage_medians['kernel_ms']:.2f}, "
                      f"write {stage_medians['write_ms']:.2f}")
                print(f"{mode} PDF-to-OPDL detail ms: "
                      f"read {stage_medians['read_ms']:.2f}, "
                      f"document {stage_medians['document_ms']:.2f}, "
                      f"page/content {stage_medians['content_ms']:.2f}, "
                      f"operator decode {stage_medians['operator_decode_ms']:.2f}, "
                      f"operators {stage_medians['operators_ms']:.2f}, "
                      f"encode {stage_medians['encode_ms']:.2f}")
                if args.bench_memory:
                    direct_peak = [item["peak_rss_kib"] for item in samples["direct"]]
                    hayro_peak = [item["peak_rss_kib"] for item in samples["hayro"]]
                    print(f"{mode} 6000-command peak RSS median KiB: "
                          f"{statistics.median(direct_peak):.0f}/"
                          f"{statistics.median(hayro_peak):.0f}; "
                          f"first-process wall ms: {direct_ms[0]:.2f}/{hayro_ms[0]:.2f} "
                          "(OS cache not cleared)")
                if not (direct_output.read_bytes() == hayro_output.read_bytes() == expected):
                    raise RuntimeError(f"benchmark output changed: 6000 rectangles/{mode}")
            if args.bench_many_ab_runs > 0:
                samples = {"fast": [], "legacy": []}
                for index in range(args.bench_many_ab_runs):
                    jobs = ("fast", "legacy") if index % 2 == 0 else ("legacy", "fast")
                    for label in jobs:
                        env = os.environ.copy()
                        env.pop("OPDS_OPDL_LEGACY_DECODE", None)
                        if label == "legacy":
                            env["OPDS_OPDL_LEGACY_DECODE"] = "1"
                        start = time.perf_counter_ns()
                        completed = subprocess.run(
                            [probe, str(number), pdf, direct_output, "--profile"],
                            env=env, check=True, capture_output=True, text=True)
                        sample = {"wall_ms": (time.perf_counter_ns() - start) / 1_000_000,
                                  "stages_ms": {key: float(value)
                                                for part in completed.stderr.strip().split()
                                                for key, value in (part.split("=", 1),)}}
                        if direct_output.read_bytes() != expected:
                            raise RuntimeError(f"{label} decoder changed pixels: {mode}")
                        samples[label].append(sample)
                paired = [a["wall_ms"] - b["wall_ms"]
                          for a, b in zip(samples["fast"], samples["legacy"])]
                measurements[f"many-{mode}-decode-ab"] = {
                    "os_cache_cleared": False, "samples": samples,
                }
                print(f"{mode} fast/lopdf decoder median ms: "
                      f"{statistics.median(item['stages_ms']['operator_decode_ms'] for item in samples['fast']):.2f}/"
                      f"{statistics.median(item['stages_ms']['operator_decode_ms'] for item in samples['legacy']):.2f}; "
                      f"process paired delta {statistics.median(paired):+.2f}; "
                      f"fast wins {sum(delta < 0 for delta in paired)}/{args.bench_many_ab_runs}")
            checked += width * height
            checked_pages += 1
        for mode, number in MODES[1:]:
            for vertical_multi in (False, True):
                width, height = 256, 192
                pdf = work / f"multi-segment-{mode}-{vertical_multi}.pdf"
                scene = work / "scene.opdl"
                direct_output = work / "direct.rgba"
                cli_output = work / "output.rgba"
                hayro_output = work / "hayro.rgba"
                pdf.write_bytes(build_pdf(mode, multi_segment=True, vertical_multi=vertical_multi))
                subprocess.run([paths["adapter"], pdf, scene], check=True,
                               capture_output=True)
                subprocess.run([probe, str(number), pdf, direct_output], check=True,
                               capture_output=True)
                subprocess.run([work / mode], cwd=work, check=True, capture_output=True)
                subprocess.run([paths["hayro"], pdf, "1", hayro_output, "1"], check=True,
                               capture_output=True)
                expected = reference_rgba(mode, multi_segment=True, vertical_multi=vertical_multi)
                if not (direct_output.read_bytes() == cli_output.read_bytes()
                        == hayro_output.read_bytes() == expected):
                    raise RuntimeError(f"pixel mismatch: collinear multi-segment/{mode}/{vertical_multi}")
                checked += width * height
                checked_pages += 1
        for mode, number in MODES[1:]:
            pdf = work / f"elbow-{mode}.pdf"
            scene = work / "scene.opdl"
            direct_output = work / "direct.rgba"
            cli_output = work / "output.rgba"
            hayro_output = work / "hayro.rgba"
            elbow = b"1 w\n1 0 0 RG\n20 151.5 m\n60.5 151.5 l\n60.5 112 l\nS\n"
            pdf.write_bytes(build_pdf(mode, extra_content=elbow))
            subprocess.run([paths["adapter"], pdf, scene], check=True, capture_output=True)
            subprocess.run([probe, str(number), pdf, direct_output], check=True,
                           capture_output=True)
            subprocess.run([work / mode], cwd=work, check=True, capture_output=True)
            subprocess.run([paths["hayro"], pdf, "1", hayro_output, "1"], check=True,
                           capture_output=True)
            expected = bytearray(reference_rgba(mode))
            for x in range(20, 61):
                offset = (40 * 256 + x) * 4
                expected[offset:offset + 4] = b"\xff\x00\x00\xff"
            for y in range(40, 80):
                offset = (y * 256 + 60) * 4
                expected[offset:offset + 4] = b"\xff\x00\x00\xff"
            if not (direct_output.read_bytes() == cli_output.read_bytes()
                    == hayro_output.read_bytes() == expected):
                raise RuntimeError(f"pixel mismatch: right-down elbow/{mode}")
            checked += 256 * 192
            checked_pages += 1
        invalid_transforms = {
            "scale": b"q\n2 0 0 1 0 0 cm\nQ\n",
            "fractional-translation": b"q\n1 0 0 1 0.5 0 cm\nQ\n",
            "out-of-page": b"q\n1 0 0 1 250 0 cm\n10 10 30 30 re\nf\nQ\n",
            "unsupported-diagonal": b"q\n1 0 0 1 10 7 cm\n20 20 m\n30 29 l\nS\nQ\n",
            "unfinished-stroke": b"q\n1 0 0 1 10 7 cm\n20 20 m\nQ\n",
            "missing-clip-end": b"10 10 20 20 re\nW\n",
            "unsupported-evenodd-clip": b"10 10 20 20 re\nW*\nn\n",
            "missing-clip-path": b"W\nn\n",
            "too-deep-save": b"q\nq\nq\nQ\nQ\nQ\n",
            "unfinished-group-clip": b"q\n10 10 20 20 re\nW\nQ\n",
            "bent-multi-segment": b"1 w\n20 151.5 m\n60 151.5 l\n60 131.5 l\nS\n",
            "reversed-multi-segment": b"1 w\n20 151.5 m\n60 151.5 l\n40 151.5 l\nS\n",
            "wrong-elbow-corner": b"1 w\n20 151.5 m\n60 151.5 l\n60 111.5 l\nS\n",
            "half-elbow-end": b"1 w\n20 151.5 m\n60.5 151.5 l\n60.5 111.5 l\nS\n",
            "third-elbow-segment": b"1 w\n20 151.5 m\n60.5 151.5 l\n60.5 112 l\n80.5 112 l\nS\n",
            "reverse-elbow": b"1 w\n60.5 151.5 m\n20 151.5 l\n20 111.5 l\nS\n",
        }
        for name, content in invalid_transforms.items():
            invalid_pdf = work / f"invalid-{name}.pdf"
            invalid_pdf.write_bytes(build_pdf("vector", extra_content=content))
            invalid_output = work / f"invalid-{name}.opdl"
            rejected = subprocess.run([paths["adapter"], invalid_pdf, invalid_output],
                                      capture_output=True)
            if rejected.returncode == 0 or invalid_output.exists():
                raise RuntimeError(f"unsupported transform accepted: {name}")
        legacy_env = os.environ.copy()
        legacy_env["OPDS_OPDL_LEGACY_DECODE"] = "1"
        syntax_pdf = work / "syntax-baseline.pdf"
        syntax_pdf.write_bytes(build_pdf("vector"))
        syntax_baseline = work / "syntax-baseline.opdl"
        subprocess.run([paths["adapter"], syntax_pdf, syntax_baseline], check=True,
                       capture_output=True)
        valid_syntax = {
            "comment": b"% harmless comment\n",
            "spaces": b"1  0 0 rg\n",
            "tabs": b"1\t0 0 rg\n",
            "decimal": b"1.0 0 0 rg\n",
        }
        for name, prefix in valid_syntax.items():
            pdf = work / f"syntax-valid-{name}.pdf"
            pdf.write_bytes(build_pdf("vector", pre_content=prefix))
            output = work / f"syntax-valid-{name}.opdl"
            for env in (None, legacy_env):
                subprocess.run([paths["adapter"], pdf, output], env=env, check=True,
                               capture_output=True)
                if output.read_bytes() != syntax_baseline.read_bytes():
                    raise RuntimeError(f"fallback changed output: {name}")
        invalid_syntax = {
            "missing-operand": b"1 0 rg\n",
            "unknown-operator": b"1 0 0 xx\n",
            "string-operator": b"(text) Tj\n",
            "array-operand": b"[1 2] re\n",
            "unfinished-array": b"[1 2 re\n",
            "extra-operand": b"1 0 0 1 rg\n",
        }
        for name, suffix in invalid_syntax.items():
            pdf = work / f"syntax-invalid-{name}.pdf"
            pdf.write_bytes(build_pdf("vector", extra_content=suffix))
            output = work / f"syntax-invalid-{name}.opdl"
            for env in (None, legacy_env):
                output.unlink(missing_ok=True)
                result = subprocess.run([paths["adapter"], pdf, output], env=env,
                                        capture_output=True)
                if result.returncode == 0 or output.exists():
                    raise RuntimeError(f"invalid content accepted: {name}")
        clipped_image = work / "clip-before-image.pdf"
        clipped_image.write_bytes(build_pdf("raster", pre_content=b"10 10 20 20 re\nW\nn\n"))
        clipped_image_output = work / "clip-before-image.opdl"
        rejected = subprocess.run([paths["adapter"], clipped_image, clipped_image_output],
                                  capture_output=True)
        if rejected.returncode == 0 or clipped_image_output.exists():
            raise RuntimeError("clipped image accepted without image clipping")
        valid = (work / "scene.opdl").read_bytes()
        count_offset = 36 + int.from_bytes(valid[32:36], "little")
        over_limit = (valid[:count_offset] + (16_385).to_bytes(4, "little")
                      + valid[-24:] * 16_385)
        bad = {
            "truncated": valid[:-1],
            "bad-magic": b"BAD!" + valid[4:],
            "unsupported-version": valid[:4] + (1).to_bytes(4, "little") + valid[8:],
            "bad-command": valid[:-24] + (99).to_bytes(4, "little") + valid[-20:],
            "bad-aa-bounds": (aa_scene[:-24] + (4).to_bytes(4, "little") + aa_scene[-20:-8]
                              + (int.from_bytes(aa_scene[-8:-4], "little") + 1).to_bytes(4, "little")
                              + aa_scene[-4:]),
            "bad-aa-alpha": aa_scene[:-1] + bytes((128,)),
            "too-many-commands": over_limit,
            "oversized-scene": valid + bytes(3_538_985 - len(valid)),
        }
        for name, content in bad.items():
            input_path = work / f"{name}.opdl"
            output_path = work / f"{name}.rgba"
            input_path.write_bytes(content)
            result = subprocess.run([probe, "3", input_path, output_path], capture_output=True)
            if result.returncode == 0 or output_path.exists():
                raise RuntimeError(f"invalid scene accepted: {name}")
            if name in ("too-many-commands", "oversized-scene"):
                (work / "scene.opdl").write_bytes(content)
                cli_output = work / "output.rgba"
                cli_output.unlink(missing_ok=True)
                result = subprocess.run([work / "vector"], cwd=work, capture_output=True)
                if result.returncode == 0 or cli_output.exists():
                    raise RuntimeError(f"CLI accepted invalid scene: {name}")
        if args.bench_json:
            args.bench_json.write_text(json.dumps(measurements, indent=2) + "\n")
        print(f"{checked_pages} PDF pages, {checked:,} pixels, direct/CLI/Hayro/reference exact; "
              f"{len(bad)} invalid scenes, {len(invalid_transforms) + 1} unsupported states "
              f"and {len(valid_syntax) + len(invalid_syntax)} syntax variants checked")


if __name__ == "__main__":
    main()
