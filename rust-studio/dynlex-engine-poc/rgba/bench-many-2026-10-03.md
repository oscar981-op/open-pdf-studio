# Meting: 6.005 OPDL-opdrachten

De fixture is een PDF-pagina van 256×192 pixels met vijf bestaande
vectoropdrachten en honderd rode rechthoeken die zestigmaal worden getekend.
De vector- en hybride uitvoer zijn pixelgelijk aan Hayro en de onafhankelijke
referentie. Dit is een herhalings-/capaciteitsproef, geen representatieve A1-tekening.

Op een AMD Ryzen 7 3700U (x86_64) zijn vijftien gepaarde procesruns per modus
afwisselend DynLex direct en Hayro uitgevoerd. Beide processen liepen onder
dezelfde `/usr/bin/time`-wrapper. De OS-cache is niet gewist; resultaten zijn
wandtijd inclusief processtart, PDF-parsing, renderen en bestandsuitvoer.
De JSON met alle ruwe paren en RSS-waarden staat in
[`bench-many-2026-10-03.json`](bench-many-2026-10-03.json).

| Modus | DynLex/Hayro mediaan wandtijd | Gepaarde mediaan DynLex−Hayro | DynLex wint | Piek-RSS mediaan DynLex/Hayro |
| --- | ---: | ---: | ---: | ---: |
| Vector | 20,48 / 18,24 ms | +2,27 ms | 3/15 | 9.624 / 7.256 KiB |
| Hybride | 21,09 / 19,61 ms | +1,65 ms | 7/15 | 10.000 / 7.644 KiB |

Dezelfde DynLex-kernel op een al gevalideerde OPDL-buffer, honderdmaal binnen
één Rust-proces aangeroepen, nam 0,455 ms/vector en 0,626 ms/hybride per call.
Bij de volledige directe route waren de mediane Rust-fasen:

| Modus | PDF lezen + OPDL bouwen | DynLex-kernel | RGBA schrijven |
| --- | ---: | ---: | ---: |
| Vector | 14,68 ms | 0,42 ms | 0,45 ms |
| Hybride | 15,35 ms | 0,56 ms | 0,44 ms |

Het grootste gemeten deel zit dus in de beperkte Rust-PDF-adapter, niet in de
DynLex-tekenlus. De som van de fasen is kleiner dan de totale wandtijd door
processtart, validatie, allocaties en meetoverhead. De piek-RSS is per proces;
deze cijfers beschrijven niet het geheugen van de draaiende app. De
procespaaruitkomsten variëren, vooral bij hybride, en dragen geen algemene
snelheidsclaim. De volgende profilerstap is lezen/parsen, operatorverwerking
en OPDL-opbouw binnen `page_to_opdl` apart timen, daarna pas optimaliseren.

## Interne adapterfasen

Een vervolgmeting met vijftien gepaarde warme procesruns per modus staat in
[`bench-many-stages-2026-10-03.json`](bench-many-stages-2026-10-03.json).
De medianen in de DynLex-route waren:

| Modus | PDF lezen | Document parsen | Pagina/content ophalen en operatoren decoderen | Eigen operatorverwerking | OPDL schrijven |
| --- | ---: | ---: | ---: | ---: | ---: |
| Vector | 0,16 | 0,23 | 11,94 | 0,59 | 0,22 ms |
| Hybride | 0,35 | 0,39 | 11,89 | 0,57 | 0,34 ms |

Een tweede, kleinere diagnostische run met vijf paren splitste de derde kolom:
content ophalen kostte 0,16/0,14 ms en `lopdf::Content::decode` 15,41/13,89 ms
voor vector/hybride. Ruwe samples staan in
[`bench-many-detailed-2026-10-03.json`](bench-many-detailed-2026-10-03.json).
De vijf-runwaarden dienen alleen om de bottleneck te lokaliseren; ze zijn geen
A/B-snelheidsbewijs. De parser decodeert nu alle 6.005 PDF-operatoren naar
algemene `lopdf::Object`-waarden voordat de eigen, beperkte interpreter ze
verwerkt. Een toekomstige snellere decoder moet dezelfde PDF-grenzen en
foutafwijzing behouden en met alle pixel- en regressiefixtures worden vergeleken.

Reproduceer met `rgba/buffer_verify.py` en `--bench-many-calls 100
--bench-many-runs 15 --bench-memory --bench-json UITVOER.json`, plus de in de
README genoemde DynLex 0.0.1- en LLVM20-toolchainargumenten.
