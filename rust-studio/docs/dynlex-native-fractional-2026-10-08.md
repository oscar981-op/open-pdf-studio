# Bewezen fractionele sampling aansluiten op de native route

Vervolg: de hieronder als onbewezen beschreven algemene tegelroute is inmiddels
uit de strikte app-route gehaald na 21 onjuiste bitmaps in 40 auditgevallen.
Zie [tegelroute-audit en strikte afwijzing](dynlex-zoom-guard-2026-10-08.md).
De historische metingen hieronder blijven ongewijzigd.

De bestaande documentbrede DynLex-decodecache bedient nu ook de eerder afzonderlijk
bewezen volledige-MediaBox-sampling. De adapter probeert eerst de bestaande
uitgelijnde route en daarna de begrensde fractionele downsampling. DynLex
composeert de geordende OPDL-scene. Er wordt geen `FractionalProbe`-cache aan de
app toegevoegd en geen Hayro/Vello-bitmap als vervanging gebruikt.

## Werkmap en scope

Bij aanvang stonden **andere ongecommitte wijzigingen** in `prepared.rs`,
`jpeg_zoom.rs`, `native-app/src/main.rs`, `opdl-adapter/src/lib.rs` en
`raster_source.rs`. Die voegden onder meer grotere zoomtegels en thumbnailwerk
toe. Deze wijzigingen zijn behouden. Deze run voegt in `prepared.rs` uitsluitend
de aanroep van de bewezen fractionele berekening toe, naast afzonderlijke tests,
audittools en meetbestanden. Er is geen gezamenlijke commit gemaakt die het
andere werk stilzwijgend zou meenemen.

De nieuwe exacte route behoudt haar grenzen: een enkele opaque DeviceRGB8-JPEG
vult de MediaBox, downsampling in beide assen, maximaal 4096 per tussen-/doelzijde
en zes miljoen pixels. CropBox en kwartslagen zijn ondersteund in de gemeten
gevallen. De cache is nog steeds één mutex-beveiligde RGB-bronbuffer van maximaal
120 MB per document. Ongevalideerde algemene image placement, upsampling,
transparantie, fonttekst en andere paints worden hiermee niet bewezen.

**De reeds aanwezige grotere/algemene zoomroute is hiermee niet als exact
gecertificeerd.** Die gebruikt andere berekeningen en haar tests staan nog
gemiddelde kanaalfouten toe. De hieronder gemeten pariteit betreft de expliciet
bewezen route en fixtures; zij bewijst geen algemene 800%- of tegelpariteit.
Er is geen volledige PDF-dekking of algemene snelheidsclaim.

## Brede samplingaudit

`scripts/audit_fractional_sampling.py` maakt deterministisch een 1024×1024 JPEG
met hoogfrequente kleuren en acht echte PDF-varianten: vier kwartslagen met en
zonder verschoven CropBox, geërfde resources/geometrie. Twaalf schalen omvatten
waarden net onder/boven 100%, 87%, 115% en 156,25%, met meerdere 256-pixelkolommen.
Alle uitvoer wordt volledig met Hayro vergeleken; de binaries en JPEG zijn gehasht.
De tool weigert resultaten als een binary tijdens de audit is vervangen.

| Route | Voor aansluiting | Na aansluiting |
|---|---:|---:|
| Afzonderlijke strip-f32-proef | 96/96 exact | 96/96 exact |
| Native strikte DynLex-route | 83/96 exact | 96/96 exact |

De dertien eerdere native afwijkingen betroffen samen 49.915 RGBA-kanalen,
met maximaal 250 verschil in één kanaal. Het zijn dus niet alleen onzichtbare
één-byteverschillen. De nieuwe route gebruikt het juiste ceil-tussenrooster en
de eerder onderzochte f32-samplingconventie, met herstart op 256 kolommen.
De MIT/Apache-2.0 bronvermelding uit de vorige proef blijft gelden; er is geen
nieuwe Vello-runtime-aanroep. JPEG-decode/resampling gebeurt in Rust; DynLex
composeert. Zijn toolchain gebruikt nog LLVM/C++/libc.

Bestanden: `dynlex-wide-sampling-before-2026-10-08.json` en
`dynlex-wide-sampling-after-2026-10-08.json`. De JPEG-generator gebruikt Pillow
en legt de JPEG-hash vast; dit is een testdependency, niet de native renderer.

## Volledige externe pagina en cache

`scripts/verify_native_fractional_jpeg.py` controleert het ongewijzigde openbare
Albany-contract, pagina 1/49, via `--dynlex-strict` op dezelfde twintig schalen
als de eerdere losse proef: 13%–301%, inclusief 115%, 87% en 156,25%.
Alle 91.783.472 RGBA-bytes zijn exact. Resultaat:
`dynlex-native-fractional-public-2026-10-08.json`.

Twee nieuwe native regressietests controleren volledige RGBA-pariteit op vier
rotaties × vijf fractionele schalen en gelijktijdige zoomaanvragen. In beide
gevallen blijft het aantal brondecodes één. De grotere corpusdekking is hiermee
niet gewijzigd: nog dezelfde gescande pagina, geen nieuwe fonttekstpagina.

## Replay

Vanuit `rust-studio`:

```sh
cargo test -p opds-dynlex-backend --features kernel --test jpeg_fractional_native
python3 scripts/audit_fractional_sampling.py \
  --candidate target/release/fractional_jpeg_probe \
  --backend target/release/backend_probe --output /tmp/wide-sampling.json
python3 scripts/verify_native_fractional_jpeg.py \
  --backend target/release/backend_probe \
  --manifest docs/public-pdf-corpus-2026-10-08.json \
  --output /tmp/native-fractional.json
```

De audits geven geen snelheidsscore aan onjuiste pixels. Geheugen- en cachegrenzen
blijven essentieel; alleen tegels uitsturen maakt een volledig gealloceerde
pagina nog geen viewport-compositor.


## Appcontrole, regressies en meting

De nieuwe release is in een nieuw 1200×832 X11-venster gestart op het originele
Albany-document, met DynLex geselecteerd. Ctrl+scroll leverde zichtbaar 132%
(werkelijke schaal 1,3225; 809×1047) en terug 100%; een tweede inzoom staat weer
op 132% met `Renderer: DynLex`. De 1,3225-route is daarnaast apart volledig met
Hayro vergeleken: alle 3.388.092 RGBA-bytes exact. Dit is geen kwantitatieve
GUI-latentie- of zoomfocusmeting. Screenshot buiten Git:
`~/.cache/opds-benchmarks/internet-2026-10-08/previews/dynlex-native-fractional132.png`.
De nieuwe instantie heeft PID 126780 en log `/tmp/opds-wide-ui.log`; bestaande
gebruikersvensters en PDF-bestanden zijn niet gesloten of gewijzigd.

253 workspace-tests met alle features, standaardcheck, releasebuild,
formatteringscontrole en Python-compile slagen. Een aanvankelijk verouderde
adapter-rlib gaf Rust E0460 (`lopdf`-versie/fingerprint); na herbouw slaagt de
volledige verifier opnieuw: 47 PDF-pagina’s / 4.964.352 pixels en drie v4-PDF’s /
147.456 pixels exact, met bestaande foutafwijzingen.

Na afloop van builds en tests: vijftien afwisselende gepaarde procesreplays,
met eerste ronde en twee warme herhalingen. Iedere bitmap wordt buiten de timer
bytegewijs gecontroleerd. Warme rendermedianen (ms):

| Zoom | DynLex | Huidige Hayro-route |
|---|---:|---:|
| 87% | 64,13 | 342,25 |
| 115% | 69,09 | 337,37 |
| 156,25% | 84,87 | 341,17 |

DynLex wint op iedere schaal 15/15 gepaarde warme medianen. Eerste 87%-render
met lege enginecache: 275,58/349,53 ms. Proces-piek-RSS: 125,28/208,38 MiB.
De DynLex-broncache behoudt 100.980.000 RGB-bytes en één decode. Dit vergelijkt
huidige app-routes: Hayro maakt per render een nieuwe RenderCache, DynLex
hergebruikt de bron. Geen gelijke interne caches, koude OS-cache, kernel-only-
of GUI-latentiemeting, en geen algemene snelheidsclaim.

Ruwe gegevens: `dynlex-native-fractional-bench-2026-10-08.json`.
Replay:

```sh
python3 scripts/bench_jpeg_zoom.py --probe target/release/jpeg_zoom_probe \
  --render-probe target/release/backend_probe \
  --manifest docs/public-pdf-corpus-2026-10-08.json \
  --scales 0.87,1.15,1.5625 --rounds 2 \
  --output /tmp/native-fractional-bench.json
```

Volgende stap: controleer de nog aanwezige algemene/grote tegelroute afzonderlijk
op correcte sampling, AA, geheugen en strict rejection; accepteer gemiddelde
kanaalfout niet als pixelpariteit. Behoud de exacte volledige-beeldroute en
cache. Daarna verbreed volledige echte vector-/tekstpagina’s, niet alleen
zoomstanden van dezelfde scan.
