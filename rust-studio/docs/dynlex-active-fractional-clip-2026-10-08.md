# Actieve fractionele rechthoekclip voor voorbereide DynLex-tekst

De native Linux DynLex-tekstworker kan nu de bestaande begrensde
`re W/W* n`-rechthoek ook met **fractionele schermranden** voorbereiden.
Elke paint bewaart haar eigen f64-clipschermrechthoek en de bestaande gehele
clip. `q/Q` herstelt de state; opeenvolgende rechthoeken worden doorsneden.
De worker geeft uitsluitend gevalideerde OPNW-geometrie, kleur, volgorde en
clipcoördinaten door. `PWPG`-v3 voegt vier f64-randen per record toe en de
ouder controleert versie, exacte lengte, eindigheid, paginabounds,
clipdoorsnede, geometrie en scene-/paintlimieten vóór de private kernel-ABI.
Ongeclipte scenes blijven v1 en uitsluitend geheel-pixelclips v2.

De zelfstandige DynLex-stripkernel rasteriseert per paint zowel glyph/rechthoek
als de plaatselijke rechthoekige clip. Op de twee **afzonderlijke** 8-bit
stripmaskers past de Rust-compositor `(glyph * clip + 255) >> 8` toe en
schildert daarna in PDF-volgorde. Die normalisatie en stripconventie zijn
geïnformeerd door lokale MIT/Apache-2.0-bron van `vello_common` 0.0.8;
er is geen Vello- of Hayro-rasteraanroep in de DynLex-route. Een eerdere
vermenigvuldiging van reeds samengevoegde tekst en clipbitmap was fout.
Een afzonderlijke oracle-ontleding van de twee overblijvende pixels toont
dat glyphs D en G elkaar daar overlappen; zie
`dynlex-fractional-clip-diagnosis-2026-10-08.md`.

Eerst was de kandidaat uitsluitend via `truetype_page_probe
--fractional-research` beschikbaar. De aparte proef gaf 24 vaste plus
96 deterministisch gevarieerde complete PDF's bytegelijk aan Hayro, met
gekleurde en overlappende tekst/rechthoeken, geneste `W/W*`, `q/Q`, vier
rotaties, twee CropBoxes en 25/100/200% zoom. De **native strikte dispatcher**
geeft op dezelfde 120 PDF's ook 120/120 exacte RGBA-resultaten; geen
Hayro-fallback. Rapporten:
`dynlex-active-fractional-clip-research-2026-10-08.json` en
`dynlex-active-fractional-clip-native-2026-10-08.json`.
Een afzonderlijke regressie met 24 volledig binnen/buiten-clips en acht
glyphs die de actieve rand kruisen is 32/32 exact; zie
`dynlex-active-fractional-clip-inout32-2026-10-08.json`. De eerdere
pixeluitgelijnde clip-, algemene tekst-, WinAnsi- en publieke audits blijven
ook exact.

De volledige oorspronkelijke Albany-contractaudit (49 pagina's op drie
schalen) stijgt van 88 naar **105/147 exact**, 42 unsupported, nul verkeerde
bitmaps of procesfouten. In totaal zijn 343.533.960 RGBA-bytes van de
aanvaarde native resultaten met Hayro vergeleken. Zeventien extra
pagina/schaalgevallen zijn vrijgekomen: pagina's 9 en 13 en 33 op alle drie
schalen, plus pagina's 10, 11, 12 en 20 op 25% en 100%. Dit is geen
document-/glyphwhitelist. De pure tekstonderzoekroute zonder eerdere
JPEG-frontend haalt 102/147 exact; de drie extra native gevallen komen van
de bestaande JPEG-pagina. Ruwe matrices:
`dynlex-active-fractional-clip-albany-2026-10-08.json` en
`dynlex-active-fractional-clip-research-albany-2026-10-08.json`.
De vaste openbare selectie van tien pagina's op drie schalen blijft
6/30 exact en 24 unsupported; deze selectie bevat de nieuw vrijgekomen
Albany-pagina's niet. Ook hier zijn er nul verkeerde bitmaps.

Reproduceren na een releasebuild van `backend_probe`, `opds-text-worker` en
`truetype_page_probe`:

```sh
python3 scripts/audit_fractional_text_research.py --native \
  --output /tmp/opds-fractional-native-120.json
python3 scripts/audit_strict_dynlex_corpus.py \
  --probe target/release/backend_probe --output /tmp/opds-fractional-albany.json \
  ~/.cache/opds-benchmarks/internet-2026-10-08/albany-contract.pdf
```

Deze subset ondersteunt **één eenvoudige gesloten asgerichte PDF-`re`-clip
per clipopdracht** en zijn geneste/intersecterende rechthoeken. Algemene
paden, CTM, transparantie, fonts buiten de bestaande WinAnsi TrueType-subset,
beeldclip en annotaties blijven apart onbewezen; unsupported levert geen
vervangende bitmap. De worker is resource-geïsoleerd, geen volledige
securitysandbox. Het oudere JPEG-, UI- en tekstwerk in de dirty werkmap is
behouden en niet stilzwijgend gecommit of gereset.

## Eerste prestatiecontrole

Een veilige no-op-optimalisatie bewaart geen fractioneel masker voor een
paint die aantoonbaar ten minste twee pixels **binnen** de clip ligt; een
paint geheel erbuiten wordt overgeslagen. Alleen werkelijk randrakende
paints rasteriseren een extra clipstrip. De pixel- en IPC-validatie blijft.

Vijf afwisselende procesparen per schaal op de **echte Albany-pagina 11**,
steeds een eerste render en twee warme replays met bytecontrole, geven de
volgende medianen (DynLex/Hayro). Dit is de huidige applicatieroute, met
warme OS-cache en ongelijke interne cachelevensduur; geen kernel-only- of
GUI-latentiemeting.

| Zoom | Warme replay | Eerste render | Piek-RSS |
| --- | --- | --- | --- |
| 25% | 23,21 / 6,96 ms | 142,73 / 9,19 ms | 16,6 / 10,2 MiB |
| 100% | 33,38 / 13,10 ms | 147,20 / 15,24 ms | 18,3 / 14,8 MiB |
| 200% | 67,11 / 27,21 ms | 172,54 / 33,51 ms | 29,3 / 27,3 MiB |

DynLex wint hier **geen** van de vijf paren op enige schaal. Een eerder
afzonderlijk vijf-paarprocesrapport vóór de no-op-optimalisatie stond op
45,76 ms warm bij 100% en 99,42 ms bij 200%, maar die campagnes waren
niet onderling gepaard en leveren geen precieze snelheidsratio. Ruwe
rapporten en binaryhashes staan in
`dynlex-active-fractional-clip-bench-2026-10-08.json` en
`dynlex-active-fractional-clip-bench-optimized-2026-10-08.json`. De
correctheid is bewezen voor de geteste subset; de prestatieambitie is
hier nog niet bereikt.
