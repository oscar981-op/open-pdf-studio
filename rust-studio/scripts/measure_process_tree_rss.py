#!/usr/bin/env python3
"""Sample parent and live child RSS while running a command.

The peak is a sampled lower bound, not a unique-memory (PSS) measurement:
shared pages can be counted in both processes. The child can disappear
between samples, so this does not replace per-process peak rusage.
"""

import argparse
import json
import os
import subprocess
import time


def rss_kib(pid: int, page_kib: int) -> int | None:
    try:
        with open(f"/proc/{pid}/statm", encoding="ascii") as stream:
            return int(stream.read().split()[1]) * page_kib
    except (FileNotFoundError, ProcessLookupError):
        return None


def children(pid: int) -> list[int]:
    try:
        with open(f"/proc/{pid}/task/{pid}/children", encoding="ascii") as stream:
            return [int(value) for value in stream.read().split()]
    except (FileNotFoundError, ProcessLookupError):
        return []


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--interval-ms", type=float, default=1.0)
    parser.add_argument("--output", required=True)
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    if not 0.5 <= args.interval_ms <= 100:
        parser.error("interval must be 0.5..100 ms")
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("missing command")
    page_kib = os.sysconf("SC_PAGE_SIZE") // 1024
    start = time.monotonic()
    process = subprocess.Popen(command, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    peaks = {"parent_kib": 0, "children_kib": 0, "sum_kib": 0}
    samples = 0
    child_samples = 0
    try:
        while True:
            parent = rss_kib(process.pid, page_kib)
            live_children = children(process.pid)
            child_rss = sum(rss_kib(pid, page_kib) or 0 for pid in live_children)
            if parent is not None:
                samples += 1
                child_samples += bool(live_children)
                peaks["parent_kib"] = max(peaks["parent_kib"], parent)
                peaks["children_kib"] = max(peaks["children_kib"], child_rss)
                peaks["sum_kib"] = max(peaks["sum_kib"], parent + child_rss)
            if process.poll() is not None:
                break
            time.sleep(args.interval_ms / 1000)
    finally:
        if process.poll() is None:
            process.kill()
        returncode = process.wait()
    report = {
        "command": command,
        "returncode": returncode,
        "elapsed_ms": (time.monotonic() - start) * 1000,
        "interval_ms": args.interval_ms,
        "samples": samples,
        "samples_with_child": child_samples,
        "sampled_peak": peaks,
        "caveat": "Sampled RSS lower bound, shared pages double counted, child may be missed between polls.",
    }
    with open(args.output, "w", encoding="utf-8") as stream:
        json.dump(report, stream, indent=2)
        stream.write("\n")
    print(json.dumps(report))
    if returncode:
        raise SystemExit(returncode)


if __name__ == "__main__":
    main()
