#!/usr/bin/env python3
"""Research-only original page-5 text layers with the one image paint removed."""
import argparse
import hashlib
import json
import re
import subprocess
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.generic import ContentStream, NameObject


def sha(data):
    return hashlib.sha256(data).hexdigest()


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source", type=Path, required=True)
    p.add_argument("--probe", type=Path, required=True)
    p.add_argument("--output-dir", type=Path, required=True)
    a = p.parse_args()
    a.output_dir.mkdir(parents=True, exist_ok=True)
    reader = PdfReader(a.source)
    page = reader.pages[4]
    content = ContentStream(page.get_contents(), reader)
    ops = content.operations
    if [op for _, op in ops[13:17]] != [b"q", b"cm", b"Do", b"Q"]:
        raise ValueError("unexpected image paint sequence")
    if ops[15][0] != [NameObject("/Im4")]:
        raise ValueError("unexpected image resource")
    content.operations = ops[:13] + ops[17:]
    writer = PdfWriter()
    out_page = writer.add_page(page)
    out_page[NameObject("/Contents")] = writer._add_object(content)
    pdf = a.output_dir / "tekst-page5-original-text-only.pdf"
    with pdf.open("wb") as f:
        writer.write(f)
    rows = []
    for scale in (0.25, 0.87, 1.0, 1.15, 1.5625, 2.0):
        ours = a.output_dir / f"tekst-page5-text-z{scale}.dynlex.rgba"
        oracle = a.output_dir / f"tekst-page5-text-z{scale}.hayro.rgba"
        for backend, path in (("--dynlex-strict", ours), ("--hayro", oracle)):
            path.unlink(missing_ok=True)
            result = subprocess.run([str(a.probe), str(pdf), str(path), backend,
                                     f"--scale={scale}"], capture_output=True, text=True,
                                    timeout=120)
            if backend == "--hayro" and result.returncode:
                raise RuntimeError(result.stderr)
            if backend == "--dynlex-strict":
                strict = result
            else:
                hayro = result
        size = re.search(r"(\d+)x(\d+)", hayro.stderr)
        if not size:
            raise ValueError("missing oracle dimensions")
        w, h = map(int, size.groups())
        expected = oracle.read_bytes()
        row = {"scale": scale, "width": w, "height": h,
               "oracle_rgba_bytes": len(expected), "oracle_sha256": sha(expected),
               "strict_exit": strict.returncode, "strict_status": strict.stderr.strip(),
               "strict_output_exists": ours.exists()}
        if strict.returncode == 0:
            actual = ours.read_bytes()
            row["different_channels"] = sum(x != y for x, y in zip(actual, expected))
            row["size_mismatch"] = len(actual) != len(expected)
            row["dynlex_sha256"] = sha(actual)
        rows.append(row)
    report = {"source_sha256": sha(a.source.read_bytes()),
              "isolated_pdf_sha256": sha(pdf.read_bytes()),
              "probe_sha256": sha(a.probe.read_bytes()), "cases": rows}
    path = a.output_dir / "tekst-page5-text-isolation-2026-10-09.json"
    path.write_text(json.dumps(report, indent=2) + "\n")
    print(path)
    print([(r["scale"], r["strict_exit"], r.get("different_channels")) for r in rows])
    if any(r["strict_exit"] or r.get("size_mismatch") or
           r.get("different_channels") != 0 for r in rows):
        raise SystemExit("original text isolation is not byte-exact")


if __name__ == "__main__":
    main()
