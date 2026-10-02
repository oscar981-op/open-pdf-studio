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

## RGBA-proef met OPDL v1 (2 oktober 2026)

[`rgba/verify.py`](rgba/verify.py) maakt een vaste 256×192-display-list. Het
`OPDL`-formaat heeft een magic, versie 1, paginabreedte/-hoogte, een clipvak en
de lengte van een RGB-beeldlaag in een 36-byte little-endian header. Daarna
volgen de beeldbytes, een opdrachtenteller en 24 bytes per tekenopdracht.
Opdracht 1 is een gevuld rechthoekpad; opdracht 2 is voorlopig een 45°-lijn
van drie pixels breed. Elke opdracht heeft een RGBA-kleur. De hybride agent
tekent ze in bestandsvolgorde over het beeld, met integer-alpha-compositing;
de andere agenten tekenen alleen hun eigen laag. Buiten het clipvak is de
pagina wit. Dit is een expliciet **tussenformaat**, nog geen PDF-interpreter.

```sh
python3 rgba/verify.py --compiler /pad/naar/dynlex/bin/dynlex \
  --runs 5 --artifacts /tmp/opds-dynlex-rgba-qa
```

De drie DynLex-programma's schrijven een rauwe RGBA-bitmap. De Python-runner
berekent alle 49.152 pixels per agent opnieuw en vergelijkt **iedere byte**.
Hij weigert ook een verkeerde formaatversie, een afgekapt bestand en een
onbekende opdracht. In de eerste vijf-runsproef waren de mediane
proceslooptijden inclusief lezen en schrijven 3,1 ms (raster), 7,5 ms
(vector) en 7,6 ms (hybride); de compileertijd was circa 7,7 seconden
**per agent**. Deze getallen zijn uitsluitend voor deze kleine synthetische
fixture en kunnen niet tegen de oudere checksumproef of een echte PDF-render
worden afgezet. De optionele PNG's zijn voor visuele controle; de bytevergelijking
is de exacte acceptatietest. De v1-lezer accepteert alleen vertrouwde
testfixtures en heeft nog geen productierijpe beveiliging voor willekeurige PDF-
of display-list-input.

De volgende grens is een PDF-front-end die echte afbeeldings- en padobjecten
naar OPDL omzet, plus text, clipping paths, transformaties, fonts en volledige
transparantie. Daarna meten we dezelfde raster-, vector- en gemengde
PDF-pagina's op identieke resolutie, warme en koude cache, piekgeheugen en
beeldverschillen tegen de bestaande Hayro-renderer. Een snelle synthetische
kernel is daarvoor nog geen bewijs.

DynLex compileert volgens het [upstream project](https://github.com/OpenAEC-Foundation/dynlex)
via LLVM; de huidige compiler is in C++ gebouwd. De DynLex-testprogramma's
gebruiken bovendien libc voor bestandstoegang en allocatie. De keuze om de enginekernel
in DynLex te schrijven wijzigt dus de eerdere eis dat **alle** applicatiecode
en afhankelijkheden Rust zijn. De bestaande Rust-engine blijft voorlopig
functioneel en ongewijzigd totdat een DynLex-kernel op echte PDF's is bewezen.
