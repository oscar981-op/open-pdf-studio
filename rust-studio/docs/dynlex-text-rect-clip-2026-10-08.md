# Beperkte rechthoekige tekstclips in DynLex

De native DynLex-tekstworker accepteert nu precies één gesloten `re W n` of
`re W* n`-pad wanneer de vier getransformeerde randen **exact op gehele
pixelgrenzen** liggen. De clip wordt met de eerdere clip doorsneden, volgt de
graphics state door `q/Q`, en wordt per geordende tekst-/rechthoekpaint in de
voorbereide scene opgeslagen. De zelfstandige DynLex-compositor rasteriseert
de glyph of rechthoek zoals voorheen en begrenst pas daarna de te schrijven
pixels. Er is geen Hayro- of Vello-renderaanroep in deze route.

Het workerbericht `PWPG` heeft versie 2 met een expliciete 16-byte clip per
paint. Ongeclipte pagina's blijven als v1 uitgaan, zodat een reeds draaiende
oudere apphelper-combinatie bestaande tekstpagina's blijft verwerken. De ouder
valideert versie, recordlengte, oorsprong, volledige
outlinegeometrie en clipgrenzen vóór de private ABI. Versie 1 blijft leesbaar
als een scene met paginabrede clips. De totale scenegrens blijft 32 MiB en het
maximum 8.192 paints. De diagnostische oude per-paint-compositor weigert
geclipte scenes, zodat hij ze niet ongemerkt verkeerd rendert.

`scripts/audit_text_rect_clip.py` genereert 24 complete PDF-gevallen met
glyphs die de cliprand kruisen, overlappende gekleurde rechthoeken en tekst,
geneste `W/W*`-clips, `q/Q`, rotaties 0/90/180/270, twee CropBoxes en zoom
25/100/200%. Alle **81.098.496 RGBA-bytes** zijn exact gelijk aan Hayro.
Zes ongeldige clipstreams leveren geen bitmap of fallback. Unit-tests toetsen
ook IPC-corruptie, oude v1-framing, clipdoorsnede, paintvolgorde en herstel.
Het auditrapport met bron- en binaryhashes staat in
`docs/dynlex-text-rect-clip-2026-10-08.json`.

De volledige originele Albany-contractaudit blijft **84/147 exact,
63 unsupported, nul verkeerde bitmaps of procesfouten**. Dit werk ontsluit nog
geen nieuwe praktijkpagina: de eerste clip op pagina 11 heeft fractionele
schermranden en blijft expliciet geweigerd. Hetzelfde geldt voor fractionele
clips, niet-rechthoekige/compound paden, lege clipintersecties en onbekende
graphics state. Er is geen algemene PDF- of snelheidsclaim.
De volledige Albany-matrix is bewaard in
`docs/dynlex-text-rect-clip-albany-2026-10-08.json`.
De vaste openbare selectie blijft zes exact / 24 unsupported / nul fouten.

Workspace-tests met alle features, 24 clipfixtures, de bestaande zestien
tekstgevallen, 48 WinAnsi/kleurgevallen, formattering, Python-compile en
diffcheck slagen. De oude v2/v4-pixelverifier blijft exact: 47 pagina's /
4.964.352 pixels en drie v4-pagina's / 147.456 pixels.

Reproduceren na een releasebuild van `backend_probe` en `opds-text-worker`:

```sh
python3 scripts/audit_text_rect_clip.py --output /tmp/opds-text-rect-clip.json
python3 scripts/audit_strict_dynlex_corpus.py \
  --probe target/release/backend_probe --output /tmp/opds-clip-albany.json \
  ~/.cache/opds-benchmarks/internet-2026-10-08/albany-contract.pdf
```

Volgende stap: fractionele rechthoekclips als zelfstandige analytische
dekkingsroute ontwerpen en per pixel tegen echte volledige tekstpagina's
toetsen. Bij onzekere randen blijft strikte afwijzing. Behoud daarnaast de
bestaande JPEG-, tekst-, UI- en annotatiewerkmapwijzigingen; deze stap is nog
niet gecommit.
