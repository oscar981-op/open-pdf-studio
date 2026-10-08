# JPEG-zoom: tussenrooster en tweede samplingstadium

De eerder afgewezen **125%**-weergave van pagina 1 van het originele openbare
Albany-contract is nu volledig bytegelijk aan Hayro: **765×990, 3.029.400
RGBA-bytes**, zonder vervangende renderer. De historische mislukte kandidaat
blijft vastgelegd in `dynlex-jpeg-zoom-125-rejected-2026-10-08.json`.

De oorzaak was het verschil tussen het interne resamplingrooster en de
uiteindelijke pagina. Hayro 0.7.1 `renderer.rs::draw_image` gebruikt de lengte
van de beeldtransformatie als f32, vermenigvuldigt met de bronafmetingen en
neemt ceil. Bij Albany 125% resulteert dit in 765×991 tussenpixels, terwijl de
bedoelde beeldplaatsing 765×990 is. Rechtstreeks resamplen naar 765×990 leverde
102.935 verkeerde pixels op.

De onafhankelijke Rust-adapter berekent nu dit tussenrooster, resamplet de
**originele** JPEG-decode met pic-scale en projecteert pixelcentra via
nearest-neighbour en begrensde pad-sampling naar de uiteindelijke plaatsing.
Bij ongewijzigde breedte kan hij volledige rijen kopiëren. De RGB-uitvoer gaat
daarna door de bestaande gevalideerde OPDL-v3-scene naar DynLex-compositie.
Dit is nieuwe Rust-beeldvoorbereiding binnen de DynLex-route, geen verborgen
Hayro/Vello-rasterizer en geen nieuwe JPEG-codec of samplingcode in DynLex zelf.
De lokale oraclebron is alleen voor onderzoek gebruikt; geen nieuwe dependency.

## Veilige scope

- 125% is alleen toegevoegd voor beelden die in **beide** assen worden verkleind
  ten opzichte van de JPEG-bron. Ook daar moeten plaatsing en CropBox op exacte
  pixelranden uitkomen. De bestaande 25/50/100/200%-route blijft beschikbaar.
- Vergroting van kleine beelden op 125% heeft nog samplingfaseverschillen.
  Een gecontroleerde 32×24-bron geplaatst als 64×48 op (16,16) week af; de nieuwe
  schaal blijft voor deze upsamplingroute expliciet unsupported.
- Willekeurige Ctrl+scroll-schalen zoals 115% en fractionele plaatsingsranden
  blijven unsupported. Dit is geen algemene vrije zoom of volledige PDF-dekking.
- Het tussenrooster krijgt een eigen grens: maximaal 4096 per zijde en zes
  miljoen pixels (18 MB RGB), ook als afronding een extra rij/kolom toevoegt.
  De doelbitmap, bronpixel- en scenelimieten blijven gelden.
- De documentbrede decodecache blijft één buffer van maximaal 120 MB. Albany
  decodeert één keer per replay; er is geen decodecache per zoomniveau.
  Resampling-, doel- en scene-buffers tellen aanvullend mee in proces-RSS.

## Pixelcontrole

`verify_public_jpeg.py` controleert nu de hele originele pagina op
25/50/100/125/200%. Werkelijke fonttekst op pagina 6 en 30%/115% leveren nog
geen DynLex-bitmap. De nieuwe golden staat in
`dynlex-jpeg-sampling-parity-2026-10-08.json`; oudere golden- en auditbestanden
blijven historische meetpunten.

36 extra gegenereerde PDF's combineren vier rotaties, drie x- en drie
y-posities, verschoven CropBox en een 81×81-JPEG geplaatst op 36×36 punten.
Bij 125% is het doel 45×45, maar het tussenrooster 46×46. Iedere RGBA-byte is
vergeleken met Hayro. De bestaande tests voor diverse beeldmarges, cachehergebruik,
paginawissel, concurrentie en ongeldige invoer blijven actief.

De nieuwe fixture `rgb81-grid.jpg` is lokaal gegenereerd met Pillow: voor
x,y in 0..80 is RGB `((13*x+7*y)%256, (3*x+19*y)%256, (17*x+11*y)%256)`, JPEG
quality=90, subsampling=0, optimize=false, progressive=false. Geen extern
beeld of extractie van het Albany-document.

## Reproduceren

```sh
cargo test -p opds-dynlex-backend --features kernel --test jpeg_zoom
cargo build --release -p opds-dynlex-backend --all-features --bins
python3 scripts/verify_public_jpeg.py --probe target/release/backend_probe \
  --manifest docs/public-pdf-corpus-2026-10-08.json \
  --output /tmp/jpeg-sampling-parity.json
python3 scripts/bench_jpeg_zoom.py --probe target/release/jpeg_zoom_probe \
  --render-probe target/release/backend_probe \
  --manifest docs/public-pdf-corpus-2026-10-08.json \
  --scales 0.25,1,1.25,2 --rounds 2 \
  --output /tmp/jpeg-sampling-bench.json
```

De benchmark vergelijkt de huidige app-routes, niet gelijke interne caches.
Hayro maakt nog een RenderCache per render; DynLex hergebruikt oorspronkelijke
RGB. Elke uitvoer wordt bytegewijs gecontroleerd buiten de rendertimer. OS-cache
is warm; eerste render heeft een lege enginecache. Geen koude OS-cache of
GUI-latentiemeting en geen algemene snelheidsclaim. DynLex-toolchain blijft
LLVM/C++/libc gebruiken, terwijl app en nieuwe beeldvoorbereiding Rust zijn.

Vijftien afwisselende procesparen, twee warme herhalingen per schaal na de
eerste ronde, geven de volgende medianen (ms):

| Schaal | DynLex | Hayro |
|---|---:|---:|
| 25% | 39,09 | 326,42 |
| 100% | 73,28 | 362,38 |
| 125% | 59,99 | 345,62 |
| 200% | 87,72 | 362,49 |

DynLex wint op iedere schaal 15/15 gepaarde warme medianen. Eerste 25%-render:
249,68/327,29 ms. Proces-piek-RSS: 134,63/209,99 MiB. Iedere DynLex-replay
behoudt 100.980.000 bytes RGB en telt exact één decode. Ruwe samples:
`dynlex-jpeg-sampling-bench-2026-10-08.json`. Variatie tussen schalen is geen
bewijs van algemene efficiëntie; eerdere meetbestanden blijven ongewijzigd.

Validatie: 248 workspace-tests met alle features, standaard native check,
releasebuild, formattering en Python-compile slagen. Na het behouden van de
oude guard voor andere schalen zijn de gerichte zoomtests en externe pixelproef
opnieuw geslaagd. De volledige bestaande verifier blijft exact: 47 PDF's /
4.964.352 pixels en drie v4-PDF's / 147.456 pixels. De 30-case openbare audit
is deze run niet opnieuw uitgebreid: 125% is geen case in die historische
25/100/200%-selectie en levert geen nieuwe inhoudelijke pagina op.

Een nieuwe echte 1200×832 native release-instantie is geopend op het originele
Albany-contract. De Home-knop Zoom In brengt pagina 1 van 100% naar 125%,
met zichtbare scan en status `Renderer: DynLex`, 765×990 pixels. Screenshot
buiten Git: `~/.cache/opds-benchmarks/internet-2026-10-08/previews/
dynlex-native-jpeg-sampling125.png`. PDF's en bestaande gebruikersvensters
zijn niet gewijzigd of gesloten. Dit is geen GUI-latentiemeting of bewijs
van pixelperfecte Tauri-UI-pariteit.

Fixture SHA-256: `b6d52aeb1c9861fe31334c9570e07ada32a31293946b1dae446526d62892afec`.
Volgende stap: een aparte begrensde volledige-pagina-samplingproef met
fractionele doelranden, floor-pagina-afmetingen en een correcte inverse
schermtransformatie bij bijvoorbeeld 115%, 87% en 156,25%. Niet stilzwijgend
afronden, geen geschaalde oude bitmap als scherpe uitvoer, en geen Hayro-fallback.
