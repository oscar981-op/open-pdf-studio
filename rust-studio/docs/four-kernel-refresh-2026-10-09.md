# Vier PDF-kernels opnieuw gemeten — 9 oktober 2026

De actuele release-`backend_probe` en `opds-text-worker` zijn gebouwd met alle
rendererfeatures. Deze meting gebruikt **strikte** backendkeuze: DynLex en Eigen
Rust geven bij unsupported inhoud geen Hayro-bitmap. De bestaande publieke set
omvat 10 geselecteerde pagina's op 25/100/200% (30 gevallen). De tweede set
omvat 9 originele openbare tekeningen en 9 afzonderlijke kopieën met **door ons
toegevoegde testcommentaren**, elk op een echt tekenblad bij 25/100% (36
gevallen). De commentaren zijn geen originele reviews van de bronbeheerders.

| Kernel | Publiek gerenderd | Tekeningen gerenderd | Samen | Exact gelijk aan Hayro binnen eigen gerenderde gevallen |
| --- | ---: | ---: | ---: | ---: |
| Hayro | 28/30 | 36/36 | 64/66 | referentie |
| PDFium | 28/30 | 36/36 | 64/66 | 0/61 vergelijkbare afmetingen |
| DynLex strikt | 6/30 | 0/36 | 6/66 | 6/6 |
| Eigen Rust strikt | 0/30 | 0/36 | 0/66 | geen praktijkresultaat |

Alle overige gevallen werden afgewezen, zonder procesfout. Twee NPS-bladen op
200% overschrijden ook bij Hayro/PDFium de applicatiebitmapgrens. Bij drie
TfL-kaartgevallen heeft PDFium andere afmetingen dan Hayro; die zijn niet als
gelijke pixelrasters vergeleken. PDFium en Hayro gebruiken verschillende
rasterisatie; Hayro is hier een vergelijkingsreferentie, geen bewijs dat PDFium
onjuist rendert. Alle zes DynLex-resultaten zijn pagina 1 (JPEG-scan) en pagina
6 (tekst) van hetzelfde Albany-contract op drie schalen. Een kleine
`simple-fill-stroke.pdf`-controle buiten de praktijkscore rendert met **alle
vier** routes identiek, 196.608 RGBA-bytes. De nul bij Eigen Rust op de echte
set betekent beperkte inhoudsdekking, geen ontbrekende build.

## Voorbeelden op 100%

Mediaan over drie nieuwe processen, in milliseconden; tussen haakjes de mediaan
van proces-piek-RSS in MiB. Een streep betekent strikte afwijzing, niet nul
milliseconden.

| Inhoud | Hayro | PDFium | DynLex | Eigen Rust |
| --- | ---: | ---: | ---: | ---: |
| Albany-contract p1, JPEG-scan | 407,12 (201,5) | 286,82 (26,4) | 326,99 (119,5) | — |
| Albany-contract p6, tekst | 24,74 (13,9) | 19,95 (13,3) | 128,89 (19,3) | — |
| Albany-bouwtekening p3, zware scan | 894,32 (438,8) | 653,02 (93,0) | — | — |
| NPS-schematisch p3, groot vectorblad | 150,84 (69,8) | 141,99 (68,6) | — | — |
| NPS-schematisch p71, vectorplattegrond | 127,38 (70,2) | 162,59 (70,4) | — | — |
| Woodson p5, vectorcontouren | 117,72 (44,5) | 134,90 (41,1) | — | — |
| Yosemite p3, schema | 487,75 (262,7) | 142,73 (41,1) | — | — |
| Caltrans XS16-010, zware vector/tekst | 92,45 (17,0) | 138,19 (20,6) | — | — |

PDFium heeft een lagere mediaan in 20/28 publieke pagina/zoomgevallen en 6/36
tekeninggevallen, of 85/192 afzonderlijke gepaarde procesrondes. De verschillen
in pixels en ondersteunde inhoud maken dit geen algemene snelheidsranglijst.
DynLex is bij de ondersteunde scan in deze procesmeting sneller dan Hayro, maar
bij de ondersteunde tekst fors langzamer. Alle 36 tekeninggevallen, inclusief
testannotaties, blijven voor DynLex en Eigen Rust buiten hun bewezen subset.

## Meetwijze en grenzen

Een ongetimede preflight verifieert de werkelijk gebruikte backend, dimensies,
outputlengte en SHA-256. Daarna volgen drie processen per gerenderde route, met
afwisselende backendvolgorde en warme OS-bestandscache. Elke herhaling moet
dezelfde bitmaphash geven. De wandtijd omvat processtart, lezen, parsen,
renderen en wegschrijven van RGBA; `/usr/bin/time` meet piek-RSS van dat
proces. Afwijzingen krijgen geen tijdscore. De PDFium-bibliotheek is dynamisch
via Rust-FFI geladen; de tekstworker staat naast de probe. Deze meting geeft
**geen** interactieve eerste-zoom-, GUI-, koude OS-cache- of gelijkgetrokken
interne cachevergelijking. Met name de eerste DynLex-tekstrender start een
afzonderlijke begrensde worker; warme sceneherhalingen in één proces zijn hier
niet gemeten. De app en geopende gebruikersdocumenten zijn niet veranderd.

De release-probe heeft SHA-256
`bf15720c072f32d71c2fa560d930fced27384815831e80184b9cea0b48149874`;
de PDFium-library
`cb59522bf0c48a6408fd2adc8c87120a1418bacbda193f898e8cdccda7d5ca27`.
Alle oorspronkelijke PDF's zijn vóór de runs tegen de gepinde manifesthashes
gecontroleerd. Volledige per-case-data en samples staan buiten Git in:

- `~/.cache/opds-benchmarks/internet-2026-10-08/four-kernel-new-2026-10-09.json`
- `~/.cache/opds-benchmarks/drawings-2026-10-09/four-kernel-new-2026-10-09.json`
- `~/.cache/opds-benchmarks/internet-2026-10-08/four-kernel-control-new-2026-10-09.json`

Reproduceer met `scripts/bench_public_corpus.py` en
`scripts/bench_annotated_drawings.py`, elk met `--rounds 3`, de eigen gepinde
`--manifest`, `--probe target/release/backend_probe` en de lokale
`--pdfium-library`. De oudere nulmeting blijft in
`four-kernel-public-bench-2026-10-08.md`; vergelijk geen ruwe tijdverschillen
tussen die runs alsof de machine- en cacheruis nul zijn.
