#!/usr/bin/env python3
"""Cache pinned public drawing sets and derived annotation test copies.

The source PDFs are left untouched. Derived comments are synthetic test data;
they are never attributed to the publishers of the drawings.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import urllib.request
from pathlib import Path

from pypdf import PdfReader, PdfWriter
from pypdf.annotations import FreeText, Highlight, Line, Text
from pypdf.generic import ArrayObject, FloatObject, NameObject, TextStringObject


SOURCES = (
    (
        "nps-woodson-drawings.pdf",
        "https://parkplanning.nps.gov/showFile.cfm?projectID=43543&sfid=139526",
        "399b008200711dce07ea9edf653ac5f2af96c62e89ca30de21bd0434a048ab95",
    ),
    (
        "nps-yosemite-schematics.pdf",
        "https://parkplanning.nps.gov/showFile.cfm?projectID=27563&sfid=112832",
        "669029e6729e064ca97821cba4fc59d1d0763f46417d265fda00a6fcc0c57ec8",
    ),
    (
        "caltrans-xs16-034-bridge-detail.pdf",
        "https://dot.ca.gov/-/media/dot-media/programs/engineering/documents/bridgestandarddetails/chap-16/2024-xs16-034-a11y.pdf",
        "f5a396a0e8e45c4aef2c3dd2cd37fdaf49bb75622538f8b531752fd61e1f6b10",
    ),
    (
        "caltrans-xs16-010-bridge-detail.pdf",
        "https://dot.ca.gov/-/media/dot-media/programs/engineering/documents/bridgestandarddetails/chap-16/202504-xs16-010-a11y.pdf",
        "892af5275108e07ef8c7dc367a282b9cbbe69e2fd663de510f263469d150ff9c",
    ),
    (
        "caltrans-xs16-020-bridge-detail.pdf",
        "https://dot.ca.gov/-/media/dot-media/programs/engineering/documents/bridgestandarddetails/chap-16/202504-xs16-020-a11y.pdf",
        "e091c23f7e2226c064ceac31fd7846ef1309fdb39cb5b5de5e01fa7ca10f9c72",
    ),
    (
        "caltrans-xs16-025-bridge-detail.pdf",
        "https://dot.ca.gov/-/media/dot-media/programs/engineering/documents/bridgestandarddetails/chap-16/202607_xs16-025-a11y.pdf",
        "041cc56454b2fa292f08da758a333f9cbaa14781fdef3e78c9f82fa29f36c9aa",
    ),
    (
        "caltrans-xs16-026-bridge-detail.pdf",
        "https://dot.ca.gov/-/media/dot-media/programs/engineering/documents/bridgestandarddetails/chap-16/202504-xs16-026-a11y.pdf",
        "955cca5f14826cec0583f246f755eed354e4293d624690937d5ee29ee54b8614",
    ),
    (
        "caltrans-xs16-027-bridge-detail.pdf",
        "https://dot.ca.gov/-/media/dot-media/programs/engineering/documents/bridgestandarddetails/chap-16/202504-xs16-027-a11y.pdf",
        "4df0c1bed93e9882948e2b31a08d0b64ef9c9951c6606a1b3b184337545dfa28",
    ),
    (
        "caltrans-xs16-030-bridge-detail.pdf",
        "https://dot.ca.gov/-/media/dot-media/programs/engineering/documents/bridgestandarddetails/chap-16/202504-xs16-030-a11y.pdf",
        "2aaea11945e3b7e9d9c5c7df79424b29dc648f557ddad4d14d32ee1b8ce06d44",
    ),
)
MAX_DOWNLOAD = 20 * 1024 * 1024


def digest(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            h.update(block)
    return h.hexdigest()


def get_source(root: Path, name: str, url: str, expected: str) -> Path:
    path = root / name
    if not path.exists():
        request = urllib.request.Request(url, headers={"User-Agent": "OpenPDFStudio-test-corpus/1.0"})
        with urllib.request.urlopen(request, timeout=30) as response, path.open("wb") as target:
            total = 0
            while block := response.read(1024 * 1024):
                total += len(block)
                if total > MAX_DOWNLOAD:
                    raise ValueError(f"download exceeds limit: {name}")
                target.write(block)
    actual = digest(path)
    if actual != expected:
        raise ValueError(f"source hash mismatch for {name}: {actual}")
    return path


def annotate(source: Path, target: Path, drawing_page: int | None = None) -> dict:
    reader = PdfReader(source, strict=True)
    writer = PdfWriter()
    writer.append(reader)
    if not writer.pages:
        raise ValueError("drawing set unexpectedly empty")
    review_page = drawing_page if drawing_page is not None else min(1, len(writer.pages) - 1)
    if not 0 <= review_page < len(writer.pages):
        raise ValueError("drawing page outside PDF")

    def rect(page: int, x0: float, y0: float, x1: float, y1: float):
        box = writer.pages[page].mediabox
        w, h = float(box.width), float(box.height)
        ox, oy = float(box.left), float(box.bottom)
        return (ox + w * x0, oy + h * y0, ox + w * x1, oy + h * y1)

    box = rect(review_page, .15, .72, .47, .81)
    highlight = rect(review_page, .53, .75, .82, .78)
    line = rect(review_page, .20, .20, .65, .43)
    first_page = review_page if drawing_page is not None else 0
    annotations = (
        (first_page, Text(rect=rect(first_page, .12, .78, .15, .82), text="Synthetic test comment: check drawing revision", open=False)),
        (review_page, FreeText(text="Synthetic test comment: verify dimension", rect=box, font_size="18pt", font_color="d02020", border_color="d02020")),
        (review_page, Highlight(rect=highlight, quad_points=ArrayObject([FloatObject(v) for v in (highlight[0], highlight[3], highlight[2], highlight[3], highlight[0], highlight[1], highlight[2], highlight[1])]), highlight_color="ffe020")),
        (review_page, Line((line[0], line[1]), (line[2], line[3]), rect=line, text="Synthetic test comment: inspect connection")),
    )
    for index, (page, annotation) in enumerate(annotations, 1):
        annotation[NameObject("/T")] = TextStringObject("Open PDF Studio test")
        annotation[NameObject("/NM")] = TextStringObject(f"opds-synthetic-{index}")
        writer.add_annotation(page_number=page, annotation=annotation)
    with target.open("wb") as output:
        writer.write(output)
    result = PdfReader(target, strict=True)
    counts = [len(page.get("/Annots", [])) for page in result.pages]
    expected = [0] * len(result.pages)
    for page, _ in annotations:
        expected[page] += 1
    if counts != expected:
        raise AssertionError(f"wrong annotation distribution: {counts}")
    return {
        "path": str(target), "sha256": digest(target), "bytes": target.stat().st_size,
        "pages": len(result.pages), "annotation_subtypes": ["Text", "FreeText", "Highlight", "Line"],
        "annotation_pages_1_based": [first_page + 1, review_page + 1, review_page + 1, review_page + 1],
    }


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, default=Path.home() / ".cache/opds-benchmarks/drawings-2026-10-09")
    args = parser.parse_args()
    args.cache.mkdir(parents=True, exist_ok=True)
    manifest = {"description": "Original public drawings and clearly synthetic commented copies", "sources": []}
    for name, url, expected in SOURCES:
        original = get_source(args.cache, name, url, expected)
        reader = PdfReader(original, strict=True)
        if any(page.get("/Annots") for page in reader.pages):
            raise ValueError("source has annotations; revise corpus description")
        derived = annotate(original, args.cache / name.replace(".pdf", "-synthetic-comments.pdf"))
        drawing_index = 3 if "woodson" in name else 2 if "yosemite" in name else 0
        drawing_comments = annotate(original, args.cache / name.replace(".pdf", "-drawing-comments.pdf"), drawing_index)
        manifest["sources"].append({"source_url": url, "original": {"path": str(original), "sha256": expected, "bytes": original.stat().st_size, "pages": len(reader.pages), "annotations": 0}, "derived_synthetic_comments": derived, "derived_drawing_comments": drawing_comments})
    out = args.cache / "manifest.json"
    out.write_text(json.dumps(manifest, indent=2) + "\n")
    print(out)


if __name__ == "__main__":
    main()
