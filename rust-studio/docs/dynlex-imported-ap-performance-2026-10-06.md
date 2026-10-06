# DynLex: grote geïmporteerde appearance op 6 oktober 2026

Een gecontroleerde PDF heeft één 90°-gedraaide pagina met verschoven CropBox
(2384×1684 uitvoerpixels) en vier `/AP/N`-Forms. Elke Form tekent een
rechthoekvulling, twee DeviceRGB8-beelden, een door `W*` geclipte
meercontourvulling en na `Q` een rand. De geselecteerde native route laat de
PDF-pagina-inhoud door Hayro renderen en componeert de geïmporteerde
annotaties door DynLex. De volledige 16.058.624 RGBA-bytes zijn gelijk aan
Hayro; SHA-256:
`ef76d54c335b7ad139881fcab8266f0edb058c7347025b2309d3ce8f6460f951`.

De beeldcompositor werkt nu op maximaal 700.000 pixels per geordende tegel,
in plaats van een RGB-kopie van de hele pagina. Ver uit elkaar liggende beelden
vormen aparte tegels. Geïmporteerde OPAN-vormgroepen gebruiken eveneens een
op een 4×4-grid uitgelijnde tegel; de OPM2-meercontourkernel ontvangt alleen
zijn 256×192-tegel. De witte papierlaag wordt één keer toegevoegd. Een
paginabrede grens blijft 4096 px per zijde en 6 miljoen pixels; maximaal acht
beelden en één miljoen RGB-bytes per pagina worden vóór rendering gecontroleerd.

De laatste vijftien afwisselend gepaarde warme procesruns op de grote PDF
gaven **DynLex geselecteerd 114,11 ms** versus **Hayro 97,92 ms** mediaan,
met gepaard verschil +12,89 ms en DynLex-winst in 2/15 paren. Mediane
piek-RSS was 39.444 versus 38.864 KiB. Op de kleine 240×160-variant waren
de medianen 8,01 versus 7,49 ms, gepaard verschil +0,36 ms en winst 5/15.
Beide routes schrijven dezelfde raw RGBA-uitvoer. Deze cijfers omvatten
processtart, PDF-parsing, rendering en bestandsuitvoer. De OS-cache is niet
gewist; dit is geen app-interactiemeting of algemene PDF-snelheidsclaim.
Ruwe samples staan in
[`bench-large-clipped-ap-final-2026-10-06.json`](../dynlex-engine-poc/rgba/bench-large-clipped-ap-final-2026-10-06.json)
en
[`bench-small-clipped-ap-final-2026-10-06.json`](../dynlex-engine-poc/rgba/bench-small-clipped-ap-final-2026-10-06.json).

Reproduceren vanuit `rust-studio` met de gedocumenteerde DynLex/LLVM-toolchain:

```sh
OPDS_LARGE_CLIPPED_AP_TEST_PDF=/tmp/opds-large-clipped-ap.pdf \
  cargo test -p opds-dynlex-backend --features kernel large_rotated_page_with_four
cargo build --release -p opds-dynlex-backend --bin backend_probe --features kernel --locked
python3 dynlex-engine-poc/rgba/benchmark_native_backend.py \
  /tmp/opds-large-clipped-ap.pdf --probe target/release/backend_probe \
  --selected --runs 15
OPDS_CLIPPED_POLYGON_AP_TEST_PDF=/tmp/opds-clipped-polygon-ap.pdf \
  cargo test -p opds-dynlex-backend --features kernel clipped_multi_contour_fill
python3 dynlex-engine-poc/rgba/benchmark_native_backend.py \
  /tmp/opds-clipped-polygon-ap.pdf --probe target/release/backend_probe \
  --selected --runs 15
```

De eerdere diagnostische zeven-paarstanden tijdens de optimalisatie waren
ongeveer 227 ms (hele-pagina RGB), 163 ms (papier eenmaal), 129 ms
(beeld- en vormtegels) en 112 ms (ook polygonen als tegel), telkens voor de
geselecteerde route. Deze standen komen uit opeenvolgende werkboomversies en
zijn geen strikte A/B tussen commitbare binaries. De laatste vijftien-paar
meting is het reproduceerbare eindpunt. Er is op deze fixture nog **geen
snelheidsvoordeel** ten opzichte van Hayro. De volgende prestatiestap moet
zich richten op het resterende parse-/compositiewerk met faseprofilering en
pixelcontrole.

## Faseprofilering en papiercompositie

`OPDS_AP_PROFILE=1` laat de geselecteerde annotatieroute en `backend_probe`
afzonderlijk tijden voor PDF-inlezen, Hayro-basisinhoud, DynLex-vormen,
polygonen, beelden, witte-papiercompositie en raw RGBA-uitvoer rapporteren.
De profiler schrijft alleen naar stderr. Op vijftien nieuwe gepaarde warme
procesruns van dezelfde grote PDF was de mediane geselecteerde **rendertijd
47,83 ms** tegenover **36,37 ms** voor Hayro. Bestandsuitvoer kostte in beide
gevallen circa **56 ms**; de vroegere totale wandtijd maskeerde daardoor een
deel van het renderingsverschil. De geselecteerde route leest geen tweede
Hayro-document in, maar stageert `/AP` met lopdf en rendert de pagina-inhoud
zonder annotaties via Hayro. De faseverdeling binnen 47,73 ms was circa
34,37 ms Hayro-basis, 4,01 ms DynLex-vormen, 0,94 ms polygonen, 0,10 ms
beelden, 2,28 ms staging en 5,13 ms witte-papiercompositie. Mediaantijden
van afzonderlijke fasen hoeven niet exact tot de mediaan van de totale tijd
op te tellen.

De grote witte-papierpass is nu in hoogstens vier niet-overlappende Rust-
segmenten parallel verwerkt; onder 500.000 pixels blijft de seriële route.
Een dertig-paar A/B tegen de vorige releasebinary gaf **8,28 → 5,36 ms**
mediaan voor die pass, met winst in 30/30 paren en bytegelijke RGBA-uitvoer.
De totale renderroute blijft trager; de nieuwe vijftien-paar meting gaf
**112,37/98,88 ms** proceswandtijd (geselecteerd/Hayro), gepaard verschil
**+11,58 ms**, en geselecteerd won slechts 1/15 paren. Op de kleine fixture
was het verschil +0,72 ms met 6/15 winst. Dit is geen algemene
snelheidsverbetering voor PDF's of de gehele app. De ruwe samples staan in
[`bench-paper-parallel-ab-2026-10-06.json`](../dynlex-engine-poc/rgba/bench-paper-parallel-ab-2026-10-06.json),
[`bench-large-clipped-ap-profiled-2026-10-06.json`](../dynlex-engine-poc/rgba/bench-large-clipped-ap-profiled-2026-10-06.json)
en
[`bench-small-clipped-ap-profiled-2026-10-06.json`](../dynlex-engine-poc/rgba/bench-small-clipped-ap-profiled-2026-10-06.json).

Herhaal de gefaseerde gepaarde meting met `--profile-phases` op
`benchmark_native_backend.py`; dit zet de omgevingsvariabele voor beide
probeprocessen en legt hun stderr-fasen samen met hashes en RSS in JSON vast.
De annotaties worden nog niet volledig door DynLex gerenderd: Hayro maakt
de basispagina, de PDF-subset en meerdere appearance-typen vallen terug.
