# DynLex PDF-engine POC: drie renderagents

Dit is de eerste **kernelproef**, geïsoleerd van de werkende Rust-app. De agenten
zijn uitvoerstrategieën, geen aparte processen of AI-agenten:

| Agent | Eerste kernelwerk |
| --- | --- |
| Raster | bemonstert een gesimuleerde gedecodeerde beeldlaag |
| Vector | evalueert herhaalde verticale, horizontale en diagonale lijnen |
| Hybride | componeert dezelfde vectorlaag over de rasterlaag |

Alle drie lopen viermaal over 1024×768 pixels. Een Python-referentie berekent de
verwachte checksums onafhankelijk; de runner verwerpt afwijkende of instabiele
uitvoer. Dit meet alleen CPU-kernwerk plus processtart. Er worden nog **geen echte
PDF's geparseerd, pixels opgeslagen, tegels gerenderd of afbeeldingen visueel
vergeleken**. De drie getallen zijn daarom geen onderlinge kwaliteits- of
eind-tot-eind-snelheidsranglijst.

## Reproduceren

Gebruik de Linux-release 0.0.1 van
[OpenAEC DynLex](https://github.com/OpenAEC-Foundation/dynlex/releases/tag/0.0.1)
(SHA-256 van `dynlex.tar.gz`:
`922d6b0ba714a8d152a7ee30f22f4edc92d176cb3f3d1aeb00b3d6e2efe5a5f8`).
De tarball is op de werkmachine buiten de repository uitgepakt en is niet
meegenomen in deze POC.

```sh
python3 bench.py --compiler /pad/naar/dynlex/bin/dynlex --runs 15
```

De runner gebruikt de bij die compiler horende `share/dynlex/lib` en maakt
een tijdelijke bouwmap. `--stdlib` kan een afwijkende installatie aanwijzen.
De uitvoer bevat versie, checksums, compileertijd en mediane/minimale/maximale
proceslooptijd. Andere DynLex-versies moeten opnieuw gecompileerd en gemeten
worden; de huidige bron op `master` heeft al andere taal-/stdlib-patronen.

## Eerste meting (2 oktober 2026)

Release 0.0.1, `-O3`, 15 runs, inclusief processtart maar exclusief compileertijd:

| Agent | Checksum | Mediaan | Bereik |
| --- | ---: | ---: | ---: |
| Raster | 401080320 | 4,295 ms | 2,785–5,688 ms |
| Vector | 45907200 | 13,025 ms | 10,332–20,476 ms |
| Hybride | 422315840 | 14,502 ms | 10,688–20,997 ms |

Deze run is een basislijn voor de DynLex-kern, geen vergelijking met Hayro,
PDFium of de bestaande app: die verwerken echte PDF-objecten, fonts en beelden.
De bestaande Rust/Poppler-corpusmeting staat in
[`../docs/engine-probe-results.json`](../docs/engine-probe-results.json).

## Volgende functionele grens

De volgende POC-stap is een vast, versieerbaar display-list-formaat met een
gedecodeerde afbeeldingslaag, lijnen/paden, clipping en laagvolgorde. Alle drie
de agenten moeten dan een RGBA-bitmap produceren; de hybride uitvoer moet
pixelgewijs aan een referentierender voldoen. Pas daarna meten we dezelfde
echte raster-, vector- en gemengde PDF-pagina's met identieke resolutie, warme
en koude cache, piekgeheugen en beeldverschillen. PDF-parser, font- en
transparantiecorrectheid blijven afzonderlijke acceptatiecriteria.

DynLex compileert volgens het [upstream project](https://github.com/OpenAEC-Foundation/dynlex)
via LLVM; de huidige compiler is in C++ gebouwd. De keuze om de enginekernel
in DynLex te schrijven wijzigt dus de eerdere eis dat **alle** applicatiecode
en afhankelijkheden Rust zijn. De bestaande Rust-engine blijft voorlopig
functioneel en ongewijzigd totdat een DynLex-kernel op echte PDF's is bewezen.
