#!/usr/bin/env python3
"""List all pinned public drawing sheets and their extractable PDF text."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
from pathlib import Path

from pypdf import PdfReader


def text_pages(path: Path) -> list[str]:
    result = subprocess.run(["pdftotext", "-layout", str(path), "-"], check=True, capture_output=True)
    pages = result.stdout.decode("utf-8", "replace").split("\f")
    if not pages[-1].strip():
        pages.pop()
    return pages


def raw_text_pages(path: Path) -> list[str]:
    result = subprocess.run(["pdftotext", "-raw", str(path), "-"], check=True, capture_output=True)
    pages = result.stdout.decode("utf-8", "replace").split("\f")
    if not pages[-1].strip():
        pages.pop()
    return pages


def catalogue(root: Path) -> dict:
    manifest = json.loads((root / "manifest.json").read_text())
    if len(manifest["sources"]) != 9:
        raise ValueError("expected the nine pinned drawing sources")
    woodson, yosemite, *caltrans = manifest["sources"]
    woodson_path = Path(woodson["original"]["path"])
    yosemite_path = Path(yosemite["original"]["path"])
    woodson_text = text_pages(woodson_path)
    yosemite_text = text_pages(yosemite_path)
    if (len(woodson_text), len(yosemite_text)) != (41, 12):
        raise ValueError("drawing page count changed")
    woodson_rows = []
    for line in woodson_text[2].splitlines():
        match = re.fullmatch(r"\s*([A-Z][A-Z0-9.-]+)\s+(.+?)\s+(\d{1,2})\s*", line)
        if match and 1 <= int(match[3]) <= 38:
            woodson_rows.append({"pdf_page": int(match[3]) + 3, "sheet": match[1], "title": match[2].strip(), "source": "printed source index"})
    if len(woodson_rows) != 38 or [row["pdf_page"] for row in woodson_rows] != list(range(4, 42)):
        raise ValueError("Woodson source index did not yield all 38 sheets")
    yosemite_rows = []
    for page, text in enumerate(yosemite_text, 1):
        match = re.search(r"Figure\s+B-(\d+)\s+([^\n]+)", text)
        if match:
            yosemite_rows.append({"pdf_page": page, "sheet": f"B-{match[1]}", "title": match[2].strip(), "source": "extractable page text"})
    if len(yosemite_rows) != 9 or [row["pdf_page"] for row in yosemite_rows] != list(range(3, 12)):
        raise ValueError("Yosemite text did not yield all nine drawings")
    if any(woodson_text[page - 1].strip() for page in range(4, 42)):
        raise ValueError("Woodson sheets now contain extractable text; update catalogue method")
    caltrans_rows = []
    for item, sheet, title in zip(
        caltrans,
        ("xs16-034", "xs16-010", "xs16-020", "xs16-025", "xs16-026", "xs16-027", "xs16-030"),
        (
            "Concrete Barrier Transition — Approach End Block Details",
            "Concrete Barrier Transition — Type AGT",
            "Concrete Barrier Transition — AGT Connection to Type 27",
            "Concrete Barrier Transition — AGT Connection to Type 1",
            "Concrete Barrier Transition — AGT Connection to Type 2, Alternatives 1 and 2",
            "Concrete Barrier Transition — AGT Connection to Type 2, Alternative 3",
            "Concrete Barrier Transition — AGT Connection to Type 9",
        ),
    ):
        original = Path(item["original"]["path"])
        text = text_pages(original)
        if len(text) != 1 or len(text[0].strip()) < 1000 or sheet not in text[0].lower():
            raise ValueError(f"bridge drawing lacks expected selectable text: {sheet}")
        caltrans_rows.append({"pdf_page": 1, "sheet": sheet, "title": title, "source": "original vector PDF", "url": item["source_url"], "pdf": str(original), "extractable_characters": len("".join(text[0].split()))})
    for item in manifest["sources"]:
        reader = PdfReader(item["derived_synthetic_comments"]["path"], strict=True)
        expected = [4] if len(reader.pages) == 1 else [1, 3]
        if [len(page.get("/Annots", [])) for page in reader.pages[:len(expected)]] != expected:
            raise ValueError("synthetic comment distribution changed")
        drawing = item["derived_drawing_comments"]
        drawing_reader = PdfReader(drawing["path"], strict=True)
        drawing_page = 4 if "woodson" in drawing["path"] else 3 if "yosemite" in drawing["path"] else 1
        if len(drawing_reader.pages[drawing_page - 1].get("/Annots", [])) != 4:
            raise ValueError("drawing sheet lacks all four synthetic comments")
    return {
        "woodson": {"url": woodson["source_url"], "pdf": str(woodson_path), "sheets": woodson_rows, "sheet_text_note": "These sheets have no selectable PDF text. Sampled pages contain tens of thousands of vector path operators, including outlined lettering; some sheets also contain images. Titles come from the printed source index, not OCR."},
        "yosemite": {"url": yosemite["source_url"], "pdf": str(yosemite_path), "sheets": yosemite_rows, "sheet_text_note": "Figure captions are extractable text; this is not a full transcription of labels inside the drawings."},
        "caltrans": {"sheets": caltrans_rows, "sheet_text_note": "These seven bridge drawings are vector PDFs with selectable source text and no embedded page images."},
        "comments": "Each source has a separate drawing-comments copy with Text, FreeText, Highlight and Line annotations on an actual drawing sheet (Woodson PDF page 4, Yosemite page 3, Caltrans page 1). Earlier cover-page test copies are retained separately. These are test comments, not publisher reviews.",
    }


def markdown(data: dict) -> str:
    lines = ["# Openbare tekeningen als tekst", "", "Dit is een doorzoekbare catalogus van **alle 54 tekeningbladen** in negen openbare PDF's. De 38 Woodson-tekeningen hebben geen selecteerbare PDF-tekst: op onderzochte bladen zijn letters en maatlabels omgezet naar vectorcontouren. Hun titels komen uit de originele, gedrukte PDF-index. Dit is nog geen OCR van interne maatvoering of labels.", ""]
    for key, heading in [("woodson", "Carter G. Woodson Home — 38 tekeningen"), ("yosemite", "The Ahwahnee / Yosemite — 9 tekeningen")]:
        item = data[key]
        lines += [f"## {heading}", "", f"[Originele PDF]({item['url']}) · `{item['pdf']}`", "", "| PDF-pagina | Blad/figuur | Titel (bronformulering) |", "| ---: | --- | --- |"]
        lines += [f"| {row['pdf_page']} | {row['sheet']} | {row['title']} |" for row in item["sheets"]]
        lines += ["", item["sheet_text_note"], ""]
    lines += ["## Caltrans-brugdetails — 7 vector-tekeningen", "", "| PDF-pagina | Blad | Titel | Selecteerbare tekens |", "| ---: | --- | --- | ---: |"]
    lines += [f"| {row['pdf_page']} | [{row['sheet']}]({row['url']}) | {row['title']} | {row['extractable_characters']} |" for row in data["caltrans"]["sheets"]]
    lines += ["", data["caltrans"]["sheet_text_note"], ""]
    lines += ["## Opmerkingen in testkopieën", "", data["comments"], "", "De testkopieën en het hashmanifest staan naast de originele PDF's in de cachemap. De originele PDF's zijn ongewijzigd.", ""]
    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", type=Path, default=Path.home() / ".cache/opds-benchmarks/drawings-2026-10-09")
    args = parser.parse_args()
    data = catalogue(args.cache)
    (args.cache / "drawings-text-index.json").write_text(json.dumps(data, indent=2) + "\n")
    out = args.cache / "drawings-text-index.md"
    out.write_text(markdown(data))
    full = ["# Alle direct extraheerbare PDF-tekst per tekening", "", "Lege secties betekenen dat de originele PDF-pagina geen selecteerbare tekst bevat; vectorcontouren kunnen wel zichtbare letters vormen. Er is geen OCR of interpretatie van maatvoering toegevoegd.", ""]
    for key in ("woodson", "yosemite"):
        item = data[key]
        pages = text_pages(Path(item["pdf"]))
        full += [f"## {key}", ""]
        for row in item["sheets"]:
            full += [f"### PDF-pagina {row['pdf_page']} — {row['sheet']} {row['title']}", "", pages[row["pdf_page"] - 1].strip() or "[Geen direct extraheerbare tekst; zichtbare letters kunnen vectorcontouren zijn]", ""]
    full += ["## caltrans", ""]
    for row in data["caltrans"]["sheets"]:
        full += [f"### {row['sheet']} — {row['title']}", "", raw_text_pages(Path(row["pdf"]))[0].strip(), ""]
    (args.cache / "drawings-full-extracted-text.md").write_text("\n".join(full))
    print(f"{out}: {len(data['woodson']['sheets']) + len(data['yosemite']['sheets']) + len(data['caltrans']['sheets'])} drawings")


if __name__ == "__main__":
    main()
