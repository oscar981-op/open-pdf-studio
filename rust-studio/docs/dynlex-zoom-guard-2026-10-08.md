# Algemene JPEG-tegelroute: exacte uitvoer of expliciete afwijzing

De brede geometrische `RasterZoom`-kandidaat was ongecommit aangesloten op de
strikte native DynLex-route. Een volledige pixel-audit vond **21 onjuiste
bitmaps in 40 gevallen**, met samen 276.114 afwijkende RGBA-kanalen en maximaal
255 kanaalfout. Een gemiddelde fout onder vier is daarom geen acceptatiecriterium.
De kandidaat staat nu uitsluitend achter de expliciete proefmodus
`fractional_jpeg_probe ... unverified-tiles`; de app publiceert deze uitvoer niet.

## Audit en grenzen

`scripts/audit_unverified_jpeg_zoom.py` genereert een 1024×1024 hoogfrequente
RGB-JPEG in PDF, vier kwartslagen, verschoven CropBox `[7 11 567 389]`, volledige
of gedeeltelijke plaatsing en vijf schalen: 87%, 115%, 156,25%, 317% en 500%.
Hiermee worden ook meerdere 1024-pixeltegels en 256-kolomstrips geraakt. De
vergelijking controleert iedere RGBA-byte, niet een gemiddelde. Bronnen en
binaries zijn gehasht; een tijdens de audit vervangen binary wordt geweigerd.

| Strikte native route | Exact | Unsupported zonder bitmap | Onjuiste bitmap |
|---|---:|---:|---:|
| Voor afscherming | 19 | 0 | 21 |
| Na afscherming | 12 | 28 | 0 |

Zeven eerder exacte gevallen vallen ook terug naar **unsupported**, omdat de
algemene geometrieklasse nog niet bewezen is. Er is geen vorm- of
fixture-specifieke uitzonderingslijst. De oude bewezen uitgelijnde route blijft
bestaan, gevolgd door de volledige-MediaBox strip-f32-downsampling. De overige
gevallen worden afgewezen, ook als een losse fixture toevallig exact was.

De expliciete algemene tegelproef blijft reproduceerbaar: zelfstandig geeft
zij 17 exacte en 23 afwijkende bitmaps op hetzelfde corpus. Dat is niet dezelfde
route als de oude native dispatcher, die eerst de bewezen scenevarianten
probeerde. Alle kandidaatcode blijft beschikbaar; er wordt geen Hayro/Vello
renderer vanuit de kandidaat aangeroepen.

De native JPEG-route controleert nu vóór brondecode maximaal 4096 pixels per
zijde en zes miljoen uitvoerpixels. De tussenroostergrenzen blijven eveneens
actief. Een gerichte test bewijst afwijzing boven de pixel-/zijdegrens met nul
brondecodes en nul behouden RGB-bytes. De gedeelde documentcache blijft één
bron van maximaal 120 MB, met geserialiseerde gelijktijdige zoomaanvragen.
De losse kandidaat heeft zijn eerdere ruimere grenzen, maar is geen
gecertificeerde native viewport-compositor.

Bestanden:
- `dynlex-unverified-zoom-before-2026-10-08.json`
- `dynlex-unverified-zoom-after-2026-10-08.json`
- `dynlex-guard-wide-sampling-2026-10-08.json`: beide routes 96/96 exact
  (afzonderlijke strip-f32-proef en native, acht PDF-varianten × twaalf schalen).
- `dynlex-guard-public-2026-10-08.json`: volledige originele Albany-pagina 1,
  twintig schalen van 13%–301%, alle 91.783.472 RGBA-bytes exact.

Er is hiermee geen nieuwe externe tekst-/vectorpagina ondersteund en geen
nieuwe snelheidsclaim. De eerder gemeten scanprestaties zijn historische
app-route-metingen met verschillende interne cachelevensduur, geen bewijs
voor algemene PDF-snelheid.

## Native UI

Een oude bitmap blijft alleen als tijdelijke preview staan zolang een nieuwe
render onderweg is. Na een definitieve strikte DynLex-afwijzing wordt hij
gewist. Anders zou een opgeschaalde oude pagina ten onrechte als gevraagde
zoom kunnen blijven staan. Thumbnailprioriteit en gedeelde scene/decodecache
uit het andere werk zijn behouden.

Het lege/foutdocumentgebied accepteert nu ook Ctrl+scroll. Tijdens de eerste
UI-proef bleek uitzoomen daar onmogelijk; de bestaande `ZoomArea` omhult nu
ook deze toestand. Zo kan de gebruiker terug naar een ondersteunde zoom
zonder rendererwissel of vervangende pixels. De regressietest controleert
wissen, zoomherstel, stale backend-resultaten en thumbnailprioriteit.

## Replay

Vanuit `rust-studio`, met de gedocumenteerde Rust/DynLex-toolchain:

```sh
cargo test -p opds-dynlex-backend --features kernel --test jpeg_zoom --test jpeg_fractional_native
cargo test -p opds-native-app --all-features
python3 scripts/audit_unverified_jpeg_zoom.py \
  --backend target/release/backend_probe \
  --candidate target/release/fractional_jpeg_probe \
  --require-exact --output /tmp/unverified-zoom-after.json
python3 scripts/audit_fractional_sampling.py \
  --candidate target/release/fractional_jpeg_probe \
  --backend target/release/backend_probe --output /tmp/guard-wide.json
python3 scripts/verify_native_fractional_jpeg.py \
  --backend target/release/backend_probe \
  --manifest docs/public-pdf-corpus-2026-10-08.json \
  --output /tmp/guard-public.json
```

Voor de bewust onjuiste kandidaat kan `unverified-tiles` afzonderlijk worden
uitgevoerd. Geef onjuiste/unsupported uitvoer geen snelheidsscore.

## Werkmap en vervolgstap

De werkmap bevat zowel eerder werk als deze integratie. Er is geen brede commit
of reset gedaan. De algemene RasterZoom-code en ruimere adapterproefgrenzen
zijn behouden; alleen de onbewezen aanroep uit de strikte dispatcher is
verplaatst. De native UI behoudt thumbnailprioriteit en andere wijzigingen.

Werk hierna aan inhoudelijke ondersteuning van een volledige externe
vector-/tekstpagina. De algemene tegelsampler kan later afzonderlijk worden
hersteld met correcte fractionele randen, beeldsampling en AA; activeer hem
pas na volledige pixelpariteit. DynLex-selectie blijft zonder Hayro-fallback.
JPEG-decode/resampling is Rust (zune-jpeg/pic-scale); DynLex composeert en zijn
toolchain gebruikt LLVM/C++ en libc. Geen claim dat de hele stack puur Rust is.

## Verificatie van de wijziging

254 workspace-tests met alle features slagen. Na de aanvullende UI-correcties
zijn de 78 native all-feature-tests opnieuw geslaagd, net als standaardcheck,
releasebuild, formattering, Python-compile en diffcheck. De volledige oude
verifier blijft exact: 47 PDF-pagina's / 4.964.352 pixels, acht ongeldige scenes,
24 unsupported states, tien syntaxisvarianten en drie v4-pagina's / 147.456 pixels.

In een nieuw 1200×832-X11-venster is de echte Albany-scan met DynLex geopend.
Ctrl+scroll 100% → 132% toont de scherpe DynLex-pagina. Verder inzoomen tot 405%
overschrijdt de bewezen pixelgrens: de pagina verdwijnt en meldt expliciet
`No fallback rendered`. De UI-herstelroute positioneert een nieuw of gecachet
paginabeeld bovenaan nadat de foutweergave leeg was; bij normale zoom met een
zichtbare pagina blijft het bestaande focusgedrag behouden. Er is geen
kwantitatieve GUI-latentie-/focusmeting of volledige UI-pariteit geclaimd.

De definitieve UI-build herstelt na 405% afwijzing met één Ctrl+scroll-uitactie
naar 306% (`3.0590228625390625`), toont weer `Renderer: DynLex` en zet de pagina
bovenaan in beeld. Deze werkelijke schaal is afzonderlijk gecontroleerd:
1872×2422 en alle 18.135.936 RGBA-bytes exact gelijk aan Hayro. Daarna is met
de Home-knop naar 100% teruggekeerd. De nieuwe release staat daar open, PID
137518, log `/tmp/opds-guard-ui-final.log`. Controleer de actuele PID/toestand
vóór een latere herstart. Alleen twee eigen tijdelijke testvensters zonder
annotaties zijn vervangen; vooraf bestaande gebruikersvensters/PDF's zijn
niet gesloten of gewijzigd.

Screenshots buiten Git:
- `~/.cache/opds-benchmarks/internet-2026-10-08/previews/dynlex-guard-final405-rejected.png`
- `~/.cache/opds-benchmarks/internet-2026-10-08/previews/dynlex-guard-final306-recovered.png`
