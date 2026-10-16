# Inactieve fractionele tekstclip, 8 oktober 2026

De native DynLex-tekstworker accepteert nu een fractionele rechthoekclip
uitsluitend wanneer die **geen enkele paint kan raken**. Hij bewaart de
oorspronkelijke f64-schermrechthoek, laat `q`/`Q` de staat herstellen en
snijdt opeenvolgende clips. Voor elke glyph en rechthoek moet het volledige
schrijfgebied ten minste twee schermpixels binnen alle fractionele randen
liggen. De glyphgrens komt uit de voorbereide outline; voor rechthoeken wordt
dezelfde f32-grens gebruikt als bij de rasterisatie. Zodra een fractionele
rand de inkt of haar AA-gebied kan raken, weigert de worker de hele pagina
vóór bitmapuitvoer. De bestaande pixeluitgelijnde clip in PWPG-v2 blijft
ongewijzigd. Er is geen afgerond glyphmasker × clipmasker in productie.

Een nieuwe audit vergelijkt 24 volledige gegenereerde PDF-pagina's met
Hayro: geneste `W`/`W*`-clips, kleur en tekst, `q`/`Q`, vier kwartslagen,
twee CropBoxes en 25/100/200% zoom. Alle 24 zijn bytegelijk
(81.098.496 RGBA-bytes); acht gevallen waarin een glyph de fractionele rand
raakt worden zonder bitmap geweigerd. De ruwe hashes en resultaten staan in
`dynlex-inactive-fractional-clip-2026-10-08.json`.

Op het **originele Albany-contract**, alle 49 pagina's bij 25/100/200%,
stijgt de strikte audit van 84 naar **88 exacte pagina/schaalgevallen**;
59 blijven unsupported en er zijn nul verkeerde bitmaps of procesfouten.
De vier nieuwe volledige praktijkgevallen zijn pagina's 10, 11, 12 en 20
bij 200%. Zij bevatten inactieve fractionele clips op die schaal. Dezelfde
pagina's blijven op andere schalen waar de clip actief is geweigerd.
Alle 305.848.224 bytes van de geaccepteerde resultaten zijn met Hayro
vergeleken. Zie `dynlex-inactive-fractional-clip-albany-2026-10-08.json`.
De vaste publieke selectie blijft 6/30 exact, 24 unsupported.

Reproduceren met de release-`backend_probe` en helper naast elkaar:

```sh
python3 scripts/audit_inactive_fractional_text_clip.py \
  --output /tmp/opds-inactive-fractional-clip.json
python3 scripts/audit_strict_dynlex_corpus.py \
  --probe target/release/backend_probe --output /tmp/opds-inactive-albany.json \
  ~/.cache/opds-benchmarks/internet-2026-10-08/albany-contract.pdf
```

De marge van twee pixels is bewust conservatief voor de huidige begrensde
strip-/AA-route; zij is geen algemene PDF-clipimplementatie. Actieve
fractionele randen, andere clipvormen en overige unsupported tekst-/PDF-state
blijven zonder Hayro-fallback. De eerder onderzochte vermenigvuldiging van
afgeronde maskers was slechts 12/24 exact; zie
`dynlex-fractional-clip-diagnosis-2026-10-08.md`. Er is geen nieuwe
algemene snelheidsclaim. De bestaande dirty werkmap is niet gecommit of
gereset.

## Volledig weggeclipte paints

Een volgende conservatieve stap slaat een paint over wanneer zijn volledige
voorbereide grens **meer dan twee pixels buiten** minstens één fractionele
cliprand ligt. Een paint volledig binnen de clip blijft ongewijzigd; ieder
tussenliggend geval blijft unsupported. Dit is een geometrische no-op, geen
fractionele randrasterisatie. De bounds zijn eindig en worden vóór deze keuze
gevalideerd. Paintvolgorde en `q`/`Q` blijven intact.

Dezelfde 24 PDF-fixtures bevatten nu ook een rechthoek en tekst geheel buiten
de geneste fractionele clip. Alle 81.098.496 bytes blijven exact; acht
actieve randgevallen worden geweigerd. Rapport:
`dynlex-inactive-fractional-clip-outside-2026-10-08.json`. De volledige
Albany-audit blijft 88 exact / 59 unsupported / nul fouten; er is dus **geen
nieuwe praktijkpagina** door deze stap. Rapport:
`dynlex-inactive-fractional-clip-outside-albany-2026-10-08.json`.

Dit zijn historische resultaten vóór de aparte actieve-cliproute. De
huidige native audit en het actuele replayscript staan in
`dynlex-active-fractional-clip-2026-10-08.md`; de eerdere acht
randgevallen worden daar pixelgewijs getest in plaats van afgewezen.
