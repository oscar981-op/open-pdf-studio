# Begrensde fontloze vectorpagina's in de tekstworker

De Linux-tekstfrontend eiste eerder altijd een `/Font`-resource, ook wanneer
een pagina nooit `Tf` gebruikte. Een ontbrekende `/Font` wordt nu als lege
fontverzameling behandeld. Een daadwerkelijk geselecteerde fontnaam moet nog
steeds in de gevalideerde resources bestaan; ontbrekende of misvormde
fontverwijzingen blijven zonder bitmap afgewezen. Alle bestaande operator-,
pagina-, worker- en IPC-grenzen gelden ongewijzigd.

`scripts/audit_fontless_vector.py` maakt 24 volledige PDF-pagina's met twee
geordende, overlappende gekleurde rechthoekvullingen binnen marked content,
zonder fontresource. Vier kwartslagen, twee CropBoxes en 25/100/200% zijn
via `backend_probe --dynlex-strict` **24/24 volledig bytegelijk** aan Hayro:
7.441.152 RGBA-bytes. Een `Tf` zonder fontresource wordt geweigerd zonder
uitvoerbestand. De originele PDF's, pixelhashes en binaire hash staan buiten
Git onder `~/.cache/opds-benchmarks/fontless-vector-2026-10-11/final/`.
De release-helper heeft SHA-256
`0fba67019c3fd5be787a48684f4b30512b443422488153457a64fbc040faa8bd`.
De herhaalde strikte audit van alle zeven gepinde externe PDF's blijft
**195/1.119 exact, 924 unsupported en nul verkeerde bitmaps/procesfouten**;
60 exacte gevallen zijn lege NPS-pagina's. Het volledige ruwe rapport staat
onder `~/.cache/opds-benchmarks/fontless-vector-2026-10-11/public-full.json`.

Dit bewijst slechts deze strikte rechthoeksubset. Op 13 oktober is de
dispatcher aangepast: wanneer het generieke DynLex-pad na geldige
voorbereiding `unsupported` teruggeeft, mag de begrensde DynLex-tekstworker
de **volledige originele pagina** onafhankelijk proberen. Een succesvolle
generieke render keert meteen terug; een generieke fout wordt niet
gemaskeerd. Dit is geen Hayro-fallback. Voor één ongemarkeerde pagina met
twee overlappende opaak gekleurde rechthoeken, vier kwartslagen, twee
CropBoxes en 25/100/200% waren daarna 24/24 volledige RGBA-buffers
bytegelijk aan Hayro (7.441.152 bytes). De beperkte zeven-PDF-audit bleef
195/1.119 exact, 924 unsupported en nul foutieve bitmaps/procesfouten;
ruwe resultaten staan buiten Git in
`/tmp/opds-public-after-internal-worker-2026-10-13.json`.
De echte zware
Woodson-tekeningpagina 5 heeft geen `/Font`; de begrensde route gaat nu
voorbij de gebruikte smoothness-graphicsstate maar weigert nog steeds de
`cm` gevolgd door lijn-/strookpaden zonder bitmap. De Caltrans
XS16-010-tekening heeft 1.382.396 gedecodeerde
contentbytes en ruim 129.000 operatoren met veel `cm`, stroken en paden;
de huidige 1-MB-content- en 16.384-operatorgrenzen blijven staan. Deze
limieten verhogen zonder een begrensde path/stroke/graphicsstate-scene zou
geen correcte tekening opleveren. Er is dus nog geen nieuwe volledige zware
praktijktekening ondersteund en geen snelheidsclaim.

De werkelijk gebruikte `/GS0` op Woodson-pagina 5 heeft verder alleen
`/SM 0.02`: deze smoothness-parameter beïnvloedt volgens de
[Adobe PDF Reference 1.7](https://opensource.adobe.com/dc-acrobat-sdk-docs/pdfstandards/pdfreference1.7old.pdf)
de weergavekwaliteit van kleurgradiënten. De tekstworker verwerpt zulke
shadings nog steeds en behandelt uitsluitend eindige `/SM`-waarden tussen
0 en 1 als inert voor zijn huidige opaque-paints. Vier rotaties, twee
CropBoxes en drie schalen van dezelfde fontloze, overlappende
rechthoekpagina met actieve `/GS0 gs` zijn 24/24 bytegelijk met Hayro
(7.441.152 RGBA-bytes). Negatieve, te grote of niet-numerieke `/SM` worden
afgewezen. Reproduceer met `scripts/audit_fontless_vector.py --smoothness`
en de overige opties hieronder; het ruwe rapport staat buiten Git onder
`~/.cache/opds-benchmarks/fontless-vector-2026-10-11/smoothness/`.
Woodson-pagina 5 blijft zonder bitmap afgewezen: na `/SM` begint het echte
contentstream met een translatie-`cm`, `m/l` en `S`; de worker staat `cm`
alleen direct vóór een ondersteunde beeld-`Do` toe. Dit is geen
nieuwe volledige praktijkpagina.
De volledige zeven-PDF-audit na deze wijziging blijft **195/1.119 exact,
924 unsupported, nul fouten**; ruwe uitkomst:
`~/.cache/opds-benchmarks/fontless-vector-2026-10-11/smoothness-public.json`.
De oorspronkelijke hybride `Tekst.pdf`-pagina 5 blijft op zes schalen
bytegelijk; workspace-allfeaturetests, formattering, Python-compile en
`git diff --check` slagen. Er is geen nieuwe eerste-zoomversnelling gemeten.

Reproduceer de gerichte pixelproef met de gebundelde Pythonomgeving die
`pypdf` bevat:

```sh
python3 scripts/audit_fontless_vector.py \
  --probe target/release/backend_probe \
  --output-dir /pad/buiten/git/fontless-vector
```

De volgende inhoudelijke stap voor Woodson is de daadwerkelijke
ExtGState-inhoud en pad-/strokevolgorde begrensd analyseren, daarna een
volledige originele pagina op meerdere schalen bytegewijs bewijzen. Deze
fontresourcecorrectie is geen document- of glyphwhitelist.
