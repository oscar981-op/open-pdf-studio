#!/usr/bin/env python3
"""OCR all Woodson drawing sheets as *unverified* machine-readable text.

Uses Poppler rendering and Tesseract; never modifies source PDFs. This is a
test-corpus aid, not a DynLex text extraction or rendering implementation.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import hashlib
import json
import os
import re
import subprocess
import tempfile
from pathlib import Path

from catalog_public_drawings_text import raw_text_pages, text_pages


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def ocr_one(source: Path, page: int, tesseract: Path, tessdata: Path, dpi: int) -> dict:
    with tempfile.TemporaryDirectory(prefix=f"opds-woodson-ocr-{page}-") as temporary:
        prefix = Path(temporary) / "page"
        subprocess.run(
            ["pdftoppm", "-f", str(page), "-l", str(page), "-r", str(dpi),
             "-gray", "-png", str(source), str(prefix)],
            check=True, capture_output=True, timeout=40,
        )
        images = list(Path(temporary).glob("page-*.png"))
        if len(images) != 1:
            raise ValueError(f"expected one rendered page {page}, got {len(images)}")
        env = dict(os.environ, TESSDATA_PREFIX=str(tessdata))
        result = subprocess.run(
            [str(tesseract), str(images[0]), "stdout", "-l", "eng", "--psm", "11"],
            check=True, capture_output=True, text=True, timeout=40, env=env,
        )
        text = result.stdout.strip()
        if len(text.encode("utf-8")) > 2_000_000:
            raise ValueError("OCR output exceeds page text limit")
        return {"page": page, "text": text, "characters": len(text), "lines": len(text.splitlines())}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, default=Path.home() / ".cache/opds-benchmarks/drawings-2026-10-09")
    parser.add_argument("--dpi", type=int, default=120)
    parser.add_argument("--workers", type=int, default=2)
    parser.add_argument("--combine-only", action="store_true", help="reuse the existing validated 38-page OCR file")
    args = parser.parse_args()
    if not 72 <= args.dpi <= 200 or not 1 <= args.workers <= 4:
        parser.error("DPI must be 72..200 and worker count 1..4")
    manifest = json.loads((args.cache / "manifest.json").read_text())
    source_meta = manifest["sources"][0]["original"]
    source = Path(source_meta["path"]).resolve(strict=True)
    if sha256(source) != source_meta["sha256"] or source_meta["pages"] != 41:
        raise ValueError("Woodson source changed")
    catalogue = json.loads((args.cache / "drawings-text-index.json").read_text())
    sheets = catalogue["woodson"]["sheets"]
    if len(sheets) != 38 or [x["pdf_page"] for x in sheets] != list(range(4, 42)):
        raise ValueError("Woodson sheet index changed")
    if args.combine_only:
        previous = json.loads((args.cache / "woodson-ocr-all-38.json").read_text())
        if previous["source_sha256"] != source_meta["sha256"] or previous["dpi"] != args.dpi:
            raise ValueError("existing OCR source or DPI differs")
        markdown = (args.cache / "woodson-ocr-all-38.md").read_text()
        matches = list(re.finditer(r"^## PDF-pagina (\d+) — [^\n]+\n\n", markdown, re.MULTILINE))
        if len(matches) != 38:
            raise ValueError("existing OCR file does not contain 38 sheets")
        results = []
        for index, match in enumerate(matches):
            end = matches[index + 1].start() if index + 1 < len(matches) else len(markdown)
            value = markdown[match.end():end].strip()
            results.append({"page": int(match[1]), "text": value,
                            "characters": len(value), "lines": len(value.splitlines())})
    else:
        local = args.cache / "tesseract-local"
        tesseract = local / "usr/bin/tesseract"
        tessdata = local / "usr/share/tesseract-ocr/5/tessdata"
        if not tesseract.is_file() or not (tessdata / "eng.traineddata").is_file():
            raise FileNotFoundError("local Tesseract and English language data are required")
        with concurrent.futures.ThreadPoolExecutor(max_workers=args.workers) as pool:
            futures = {pool.submit(ocr_one, source, row["pdf_page"], tesseract, tessdata, args.dpi): row for row in sheets}
            results = []
            for future in concurrent.futures.as_completed(futures):
                result = future.result()
                results.append(result)
                print(f"OCR page {result['page']}: {result['characters']} characters", flush=True)
        results.sort(key=lambda row: row["page"])
        md = ["# Woodson-tekeningen — machine-OCR (niet geverifieerd)", "",
              "Deze tekst is automatisch herkend uit gerenderde vectorcontouren. Letters, maten en cijfers kunnen fout zijn. "
              "Gebruik de originele PDF voor technische beslissingen; dit is geen oorspronkelijke selecteerbare PDF-tekst.", ""]
        by_page = {row["page"]: row for row in results}
        for sheet in sheets:
            page = sheet["pdf_page"]
            md.extend([f"## PDF-pagina {page} — {sheet['sheet']} {sheet['title']}", "",
                       by_page[page]["text"] or "[Geen tekst herkend]", ""])
        (args.cache / "woodson-ocr-all-38.md").write_text("\n".join(md))
    by_page = {row["page"]: row for row in results}
    if sorted(by_page) != list(range(4, 42)):
        raise ValueError("OCR page sequence incomplete")
    combined = ["# Alle 54 tekeningen als tekst", "",
                "Woodson: machine-OCR uit zichtbare vectorcontouren, **niet geverifieerd**. "
                "Yosemite en Caltrans: direct uit oorspronkelijke PDF-tekst geëxtraheerd. "
                "Maten en cijfers uit OCR kunnen onjuist zijn.", ""]
    for sheet in sheets:
        page = sheet["pdf_page"]
        combined.extend([f"## Woodson PDF-pagina {page} — {sheet['sheet']} {sheet['title']} [OCR]", "",
                         by_page[page]["text"] or "[Geen tekst herkend]", ""])
    yosemite = catalogue["yosemite"]
    yosemite_pages = text_pages(Path(yosemite["pdf"]))
    for sheet in yosemite["sheets"]:
        page = sheet["pdf_page"]
        combined.extend([f"## Yosemite PDF-pagina {page} — {sheet['sheet']} {sheet['title']} [PDF-tekst]", "",
                         yosemite_pages[page - 1].strip(), ""])
    for sheet in catalogue["caltrans"]["sheets"]:
        combined.extend([f"## Caltrans {sheet['sheet']} — {sheet['title']} [PDF-tekst]", "",
                         raw_text_pages(Path(sheet["pdf"]))[0].strip(), ""])
    (args.cache / "drawings-all-54-text-with-ocr.md").write_text("\n".join(combined))
    report = {"source": str(source), "source_sha256": source_meta["sha256"], "dpi": args.dpi,
              "engine": "Tesseract 5.3.4 English, --psm 11", "verified": False,
              "pages": [{key: value for key, value in row.items() if key != "text"} for row in results]}
    (args.cache / "woodson-ocr-all-38.json").write_text(json.dumps(report, indent=2) + "\n")
    print(f"38 sheets, {sum(row['characters'] for row in results)} unverified OCR characters")


if __name__ == "__main__":
    main()
