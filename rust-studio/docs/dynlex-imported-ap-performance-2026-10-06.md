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
