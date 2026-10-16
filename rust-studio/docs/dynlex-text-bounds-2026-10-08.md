# Minder werk per tekstglyph, met onveranderde pixels

De native DynLex-tekstroute is versneld zonder extra bitmapcache of andere
renderer. Op de volledige originele Albany-pagina 6 daalt warme replay op
100% van **39,05 naar 11,80 ms**. Hayro blijft op die pagina sneller:
**6,06 ms**. Dit is een verbetering tegenover de vorige DynLex-binary op één
tekstpagina, geen algemene PDF-, zoom- of GUI-snelheidsclaim.

## Gemeten oorzaak en wijziging

De DynLex stripkernel beperkte de rasterberekening al tot de 4×4-tegels rond
een glyph. De voorbereide Rust-paginacompositor scande daarna alsnog alle
49.152 dekkingsbytes van ieder 256×192-glyphmasker, ook bij een kleine letter.
Op Albany-pagina 6 gebeurt dat 658 keer: 657 zichtbare glyphs plus een
rechthoekpaint. De kernel wiste bovendien het volledige masker met een
expliciete scalaire lus.

Twee algemene wijzigingen:

1. `glyph_run_core.rs` scant alleen het conservatieve schrijfgebied van de
   stripkernel. Dit gebruikt dezelfde f64→f32-conversie, 4×4-tegelverdeling en
   één-tegelmarge als `glyph_strip_kernel.dl`. Het is geen afgeronde
   mathematische inkt-bounding-box. Alle aangetroffen niet-nul-dekking wordt
   nog steeds tegen de werkelijke paginagrens gecontroleerd. Een uitlopende
   paint blijft een fout zonder gedeeltelijke bitmap.
2. `glyph_strip_kernel.dl` wist exact dezelfde volledige 49.152-byte-uitvoer
   met `libc::memset`, zoals andere bestaande DynLex-kernels al doen.
   Rasterformules, f32/FMA-bewerkingsvolgorde, contourvolgorde en compositor
   zijn ongewijzigd. Werkbuffers worden volledig gewist; geen oude pixels
   blijven als impliciete cache behouden.

Er is geen nieuwe acceptatiesubset, glyphspecialcase, font-/documentwhitelist,
resolutieafronding of Hayro/Vello-runtime-rendering toegevoegd. Scene-IPC,
private ABI-validatie, kleur/paintvolgorde en alle geheugenlimieten blijven
hetzelfde. DynLex gebruikt LLVM/C++ en libc; dit maakt de toolchain niet
letterlijk volledig Rust.

`text_profile_probe` meet een resource-begrensde workervoorbereiding apart
van tien replays en controleert iedere uitvoer tegen een apart gemaakte
Hayro-referentie. `PreparedWindingPage::profile_replay` registreert raster,
bounds/command, compositor en wit papier. De gewone approute leest geen klok
per glyph. De profieltijden sluiten PDF-voorbereiding en bufferallocatie uit;
de gemeten totale replay bevat allocatie wel. Voorbereiding omvat workerstart,
parsing, fonts, IPC en hervalidatie samen; deze deelkosten zijn nog niet apart
geïnstrumenteerd.

De diagnostische coverage-scan daalde van circa 18 ms naar 1–2 ms. Na `memset`
bedroeg de gemeten rasterfase op 100% circa 9,79 ms, scan/command 1,55 ms,
compositie 0,32 ms en papier 0,95 ms. Deze losse profielen hebben timer-overhead,
zijn niet gepaard en zijn geen vervanging voor de hieronder gemeten route.
De eerste historische profielrun had bovendien een hoge rastertijd op 100%;
de raw samples blijven intact. Geen snelheidsratio op dat uitschieterprofiel.

Profielrapporten: `dynlex-text-bounds-profile-{before,after,memset}-2026-10-08.json`.
`after` is de tussenstap met alleen de beperkte scan; `memset` is de eindversie.

## Gepaard oud / nieuw / Hayro

Vijftien afwisselende procesdrietallen per schaal, oorspronkelijke Albany-pagina
6. Iedere executable doet één initiële render en twee warme herhalingen;
**iedere uitvoer wordt bytegewijs gecontroleerd buiten de timer**. Beide
DynLex-versies hergebruiken dezelfde pagina-/schaalscene (één voorbereiding),
Hayro gebruikt de huidige app-route met een nieuwe RenderCache per render.
OS-cache warm. GNU time registreert piek-RSS inclusief helper en verificatie.
Geen gelijke interne caches, koude OS-cache, kernel-only- of GUI-meting.
Tijdens deze eindmeting liepen geen eigen builds/tests/audits parallel.

| Schaal | Warm oud DynLex | Warm nieuw DynLex | Warm Hayro | Nieuw wint van oud / Hayro |
| --- | ---: | ---: | ---: | ---: |
| 25% | 35,61 ms | 8,02 ms | 2,91 ms | 15/15 / 0/15 |
| 100% | 39,05 ms | 11,80 ms | 6,06 ms | 15/15 / 0/15 |
| 200% | 54,11 ms | 25,86 ms | 15,61 ms | 15/15 / 0/15 |

| Schaal | Eerste render oud / nieuw / Hayro | Piek-RSS oud / nieuw / Hayro |
| --- | ---: | ---: |
| 25% | 129,51 / 101,33 / 4,54 ms | 14,95 / 14,80 / 9,92 MiB |
| 100% | 134,00 / 106,34 / 10,90 ms | 16,66 / 16,61 / 13,70 MiB |
| 200% | 149,02 / 125,40 / 21,91 ms | 29,37 / 29,50 / 25,91 MiB |

De nieuwe tekstversie is op deze schalen circa 4,44× / 3,31× / 2,09× sneller
dan de bewaarde oude DynLex-versie. Voorbereiding en glyphrasterisatie blijven
belangrijke kosten. Nog steeds geen snelheidsvoordeel tegenover Hayro op deze
tekstpagina. Ruwe procesmetingen, binary-/bronhashes en cachetellers staan in
`dynlex-text-bounds-bench-2026-10-08.json`.

De oude releaseprobes en helper zijn vóór wijziging bewaard buiten Git in
`~/.cache/opds-benchmarks/text-bounds-before-2026-10-08/`. De werkmap is
ongecommit gebleven; een Git-HEAD alleen beschrijft deze baseline dus niet.
De benchmark legt daarom concrete binaryhashes vast.

## Correctheid en grenzen

- Alle 49 originele Albany-pagina’s op 25/100/200% opnieuw via native strict:
  **78 exact, 69 unsupported, nul verkeerde bitmaps of procesfouten**.
  255.196.656 vergeleken RGBA-bytes; dezelfde acceptatie als vóór optimalisatie.
- Zestien volledige native tekst-/zoom-/kleur-/overlap-/rotatie-/CropBox-gevallen:
  **52.524.160 bytes exact**. Ontbrekende helper, crash en invalid IPC blijven
  afgewezen zonder output of fallback.
- Afzonderlijke windingaudit: 48 deterministische kruisende/overlappende
  contourgroepen, de volledige tekstpagina op drie schalen, acht kleur/
  rotatie/CropBox-fixtures en vijf ongeldige CLI-inputs blijven slagen.
- Nieuwe regressie vergelijkt de beperkte scan met de oude volledige scan
  rond f32-/4px-tegelgrenzen, aan de uiterste 256×192-randen en met een grote
  paint gevolgd door een kleine in dezelfde werkbuffer. Ook de diagnostische
  replay is exact gelijk aan de gewone route.
- **276 workspace-allfeaturetests** slagen met lokale PDFium-library;
  standaardcheck, release-app/helper/probes, fmt, Python-compile en diffcheck
  slagen. Cache/concurrency/page/zoom/fouthersteltests blijven intact.
- Volledige oude v2/v4-verifier: **47 PDF’s / 4.964.352 pixels plus drie v4 /
  147.456 pixels exact**, inclusief acht ongeldige scenes, 24 unsupported states
  en tien syntaxvarianten.

Rapporten: `dynlex-text-bounds-native-2026-10-08.json`,
`dynlex-text-bounds-winding-2026-10-08.json`,
`dynlex-text-bounds-albany-2026-10-08.json`.
Deze stap vergroot de PDF-objectdekking niet. De openbare 30-case-score uit de
vorige run blijft zes geaccepteerd en 24 unsupported; niet opnieuw als nieuwe
dekkingswinst presenteren.

De nieuwe native release is gebouwd. Bestaande gebruikers-/testvensters en
PDF’s zijn deze run niet gesloten of herstart. De eerdere UI-screenshot van
pagina 6 bewijst de integratie, niet de latentie van deze optimalisatie. Geen
nieuwe kwantitatieve UI-meting of pixelperfecte-UI-claim.

## Reproduceren en vervolg

```sh
cargo build --release -p opds-dynlex-backend --all-features \
  --bin text_profile_probe --bin jpeg_zoom_probe --bin backend_probe \
  --bin opds-text-worker --bin truetype_page_probe --bin winding_probe
python3 scripts/profile_text_replay.py /tmp/text-profile.json
python3 scripts/audit_native_text.py --output /tmp/native-text.json
python3 scripts/audit_text_winding.py --require-exact --output /tmp/winding.json
python3 scripts/audit_strict_dynlex_corpus.py --probe target/release/backend_probe \
  --output /tmp/albany.json /pad/naar/originele/albany-contract.pdf
python3 scripts/bench_text_bounds.py \
  --baseline ~/.cache/opds-benchmarks/text-bounds-before-2026-10-08 \
  --pairs 15 --output /tmp/text-bounds-bench.json
```

Vervolg: meet de resterende strip-/FMA-kosten en voorkom onnodige herhaalde
geometriebewerkingen zonder andere subpixelfase. Maak voorbereiding zichtbaar
per fase (oude vector-/rasterpoging, workerstart, font/scenevoorbereiding),
zonder onbetrouwbare tekstparsing naar het hoofdproces te verplaatsen. Werk
ook aan inhoudelijke dekking op echte font-/operatorafwijzingen, algemene
paden en annotaties. Geen eindeloze microbenchmarks als vervanging voor brede
praktijkdekking. Behoud strikte afwijzing en herhaal alle geaccepteerde corpus-
en contourpixels vóór verdere activering.
