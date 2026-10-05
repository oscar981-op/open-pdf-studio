# Experimentele Type0-tekststaging

`stage_type0_text` is een afzonderlijke read-only Rust-proef. Hij verandert de
normale PDF→OPDL-adapter, DynLex-renderer en native app niet. Een pagina met
ingebedde Type0/Identity-H + CIDFontType2-fonts kan worden onderzocht op
code→CID→GID, CID-breedtes en de plaatsing van `Tj`-glyphs. De embedded
FontFile2 wordt met een limiet van 8 MB via pure-Rust FlateDecode uitgepakt
en door `ttf-parser` gevalideerd. Alleen een identieke of ontbrekende
`CIDToGIDMap` is in deze proef toegestaan; een mapstream wordt afgewezen.
De ontbrekende map wordt hier als Identity behandeld en moet vóór
productiegebruik op meer corpus worden getoetst.

De proef verwerkt `Tf` ook buiten `BT`, zoals de echte technische tekening
doet. Voor glyphplaatsing verwerkt zij `q/Q`, `cm`, `Tm`, `Tc`, `Tz`, `Ts`,
de horizontale CID-breedtes `W`/`DW` en `Tj`. `TJ`, `Td`, andere fonttypen,
niet-nul woordspatiëring en onzichtbare/niet-gevulde tekstrendermodi worden
niet geëxporteerd. Een gemengde pagina mag een samenvatting tonen, maar
schrijft **geen** gedeeltelijke glyph-run als een tekst-show-opdracht is
overgeslagen. De PDF-invoer is tot 16 MB begrensd; `lopdf`-parsing en
pagina-contentdecompressie hebben in deze proef nog geen volledige
onbetrouwbare-invoer-garantie. Gebruik het commando daarom alleen voor
vertrouwde onderzoeksbestanden.

Optioneel schrijft het commando TGR1: 16-byte header (`TGR1`, versie 1,
recordgrootte 84, paginanummer, recordaantal) gevolgd door maximaal 65.536
records. Elk record bevat een resource-naam van maximaal 31 bytes, CID,
GID en een 2×3-`f64`-transformatie van ruwe TrueType-glyphcoördinaten naar
PDF-gebruikerscoördinaten. De fontbytes zelf blijven in de bron-PDF; een
latere renderer moet ze opnieuw aan de resource-naam koppelen en controleren.
De validator controleert versie, exacte lengte, naam/padding en eindige
begrensde transformaties. De tests weigeren afkapping, corrupte header,
recordgrootte, naam en NaN-coördinaten.

Reproduceren vanuit `rust-studio`:

```sh
cargo build --release --offline -p opds-opdl-adapter --bin stage_type0_text
target/release/stage_type0_text \
  '../test pdf-bestanden/Originele bestanden/Technische tekening.pdf' \
  2 /tmp/opds-technical-page2.tgr
target/release/stage_type0_text --validate /tmp/opds-technical-page2.tgr
```

Op de vier echte A1-pagina's werden respectievelijk 2.145, 2.566, 2.522 en
1.918 glyphrecords geëxporteerd; alle Type0-`Tj`-opdrachten van die pagina's
werden gestaged. Pagina 2 heeft 315 `Tj`-opdrachten en een TGR1-buffer van
215.560 bytes. Dit controleert structurele mapping en berekende
gebruikerscoördinaten; er is **nog geen** glyph-outline-rendering,
Hayro-pixelpariteit of prestatiewinst. De eerste echte DynLex-tekstproef
moet de TGR1-records met de originele fontprogramma's koppelen, glyph-
outlines begrensd verwerken, PDF-rotatie/CropBox en clips meenemen en de
pixels tegen Hayro vergelijken. Onbewezen PDF's blijven fallback.

De gebruikte Identity-H-, CID-metric- en tekstmatrixregels zijn beschreven
in de [Adobe PDF 32000-1-referentie](https://developer.adobe.com/document-services/docs/assets/35e4369068f86065372c18787171a17e/PDF_ISO_32000-1.pdf), §9.4 en §9.7.

## Eerste echte glyph-dekkingsproef

`stage_type0_text --isolate INPUT.pdf PAGE auto OUTPUT.opln OUTPUT.pdf` kiest
uitsluitend een werkelijk gebruikt glyph met één gesloten contour van rechte
lijnsegmenten. De proef haalt de outline uit het ingebedde FontFile2 via
`ttf-parser`, past de TGR1-transformatie toe en centreert het glyph in een
256×192-tegel. Zij schrijft een OPLN-v1-buffer en vervangt **alleen in een
tijdelijke kopie** de geselecteerde PDF-pagina door dezelfde ene zwarte
CID-glyph. De originele pagina, bron-PDF en normale adapter wijzigen niet.
Meerdere contouren, quadratic/cubic-segmenten, te grote glyphs, ongeldige
recordindex en pagina's met overgeslagen tekstopdrachten worden geweigerd.

Op `Technische tekening.pdf`, pagina 2, is record 16 (GID 78, twaalf
lijnsegmenten) geselecteerd. De zelfstandige DynLex-OPLN-area-kernel geeft
voor het geïsoleerde glyph **49.152/49.152 dekkingsbytes** gelijk aan Hayro;
38 pixels bevatten inkt. De afzonderlijke Rust-area-proef geeft dezelfde
49.152 bytes. Record 0 met een onbewezen outline wordt zonder bestand
afgewezen. Reproduceren vanuit de repo, na het bouwen van de bestaande
`opds_dynlex_curve_pixel`-library volgens de POC-README:

```sh
cd rust-studio
cargo build --release --offline -p opds-opdl-adapter --bin stage_type0_text
cargo build --release --offline -p opds-engine-probe
rustc --edition=2024 -O dynlex-engine-poc/rgba/curve_raster_probe.rs \
  -L native=/tmp/opds-curve-pixel-build -o /tmp/opds-curve-raster-probe
python3 dynlex-engine-poc/rgba/verify_real_glyph.py \
  --stage target/release/stage_type0_text \
  --hayro target/release/opds-engine-probe \
  --dynlex /tmp/opds-curve-raster-probe \
  --pdf '../test pdf-bestanden/Originele bestanden/Technische tekening.pdf'
```

Dit bewijst één eenvoudig glyph bij één tegelgrootte. Het is nog geen
multi-contour-tekst, geen Bézier/TrueType-glyphdekking in het algemeen en geen
tekst in de native DynLex-route. De geïsoleerde PDF hergebruikt een bestaand
fontresource uit de technische PDF; resourcekoppeling, compositing,
clipping, kleur en laagvolgorde moeten vóór integratie expliciet worden
gevalideerd. De Hayro-vergelijking is een correcte maar smalle oracle;
prestatiewinst is niet gemeten.

## Tweecontourproef met echt CID-glyph

De optionele `--outline-inventory` toont daadwerkelijk gebruikte glyphs met
meerdere gesloten rechte contouren. Record 26/GID 76 heeft twee losse
contouren; record 264/GID 23 heeft een buitencontour en een gat. De aparte
`--isolate-multi`-route schrijft een OPM2-buffer en een tijdelijke PDF met
slechts dat glyph. OPM2 bevat versie, 256×192-tegelmaat en per contour een
expliciet puntental plus `f64`-punten. De experimentele Rust-grens controleert
exacte lengte, maximaal acht contouren/256 punten, gesloten eindige punten,
oriëntatie, zelfkruising en rakende/kruisende contouren voordat de bestaande
DynLex-pixeloppervlaktefunctie wordt aangeroepen. Rust combineert voor deze
proef de **onafgeronde** DynLex-oppervlakken volgens de getekende winding en
rondt één keer naar 8-bit af; dit is nog geen zelfstandige DynLex-OPM2-kernel.

Beide geïsoleerde echte glyphs geven elk **49.152/49.152 pixels** gelijk aan
Hayro: respectievelijk 18 en 14 zichtbare inktpixels. Afgekapt OPM2, extra
bytes, NaN, zelfkruising en contouraanraking worden geweigerd. Reproduceren
vanuit `rust-studio` met de bestaande statische DynLex-area-library:

```sh
cargo build --release --offline -p opds-opdl-adapter --bin stage_type0_text
cargo build --release --offline -p opds-engine-probe
rustc --edition=2024 -O dynlex-engine-poc/rgba/glyph_multi_probe.rs \
  -L native=/tmp/opds-curve-pixel-build -o /tmp/opds-glyph-multi-probe
python3 dynlex-engine-poc/rgba/verify_real_multi_glyph.py \
  --stage target/release/stage_type0_text \
  --hayro target/release/opds-engine-probe \
  --probe /tmp/opds-glyph-multi-probe \
  --pdf '../test pdf-bestanden/Originele bestanden/Technische tekening.pdf'
```

De proef gebruikt alleen zwarte geïsoleerde line-only-glyphs; samengestelde
tekstlagen, rotatie/CropBox op de uitvoertegel, kleur, clips, quadratic/cubic
TrueType-contouren en PDF-fontfallback zijn niet gevalideerd. De native app
blijft tekst via Hayro renderen. Een volgende stap is de getekende
meercontour-compositie binnen één begrensde DynLex-kernel, hieronder
beschreven; kleur, clips en lastigere glyphs blijven daarna open.

## Afzonderlijke DynLex-OPM2-bufferkernel

`glyph_multi_kernel.dl` doet nu zelf de vier halfvlakclips, getekende
contouroppervlakken, nonzero-compositie en één 8-bit-afronding in één
256×192-buffer. De Rust-probe valideert eerst OPM2, normaliseert naar
8-byte-uitgelijnde punten en vergelijkt de complete uitvoer met de vorige
DynLex-per-contour/Rust-compositie. De C-ABI weigert null- en te korte
buffers. Twee hergebruikte clipbuffers hebben elk 65.536 bytes capaciteit
voor maximaal 256 punten en vier clippassen; de productieroute gebruikt deze
kernel nog niet.

De twee echte CID-glyphs blijven ieder 49.152/49.152 bytegelijk aan Hayro.
Vier kleine PDF-fixtures voor losse contouren, een gat, een eiland in een
gat en een 90° gedraaid gat zijn ook elk 49.152/49.152 exact. Een schuine
ruit met gat heeft daarentegen **72 afwijkende randpixels**, maximaal één
8-bit-dekkingsstap per pixel. Dit is een geregistreerde diagnostische
mismatch; schuine vormen zijn dus niet algemeen bewezen en de gewone
PDF-adapter blijft op Hayro terugvallen. `diagnose_glyph_multi_edges.py`
knipt beide contouren met exacte rationale getallen tegen elk afwijkend
pixel. **Alle 72 verschillen liggen precies op een halve 8-bit-stap**
(`oppervlakte × 255 = geheel getal + 1/2`): Hayro kiest 46 keer een byte
hoger en 26 keer een byte lager dan de huidige DynLex-berekening. Dat wijst
op de f32-stripaccumulatie en omzetting in Vello CPU 0.0.8, maar bewijst
niet dat andere schuine contouren geen geometrische verschillen hebben.
Een proef met pixels rond de lokale oorsprong maakte de oppervlakteberekening
numeriek stabieler, maar liet 59 verschillen over; die variant is niet
geactiveerd. Reproduceren vanuit `rust-studio`:

```sh
python3 dynlex-engine-poc/rgba/diagnose_glyph_multi_edges.py \
  --probe /tmp/opds-glyph-multi-native-probe \
  --hayro target/release/opds-engine-probe
```

Een enkele warme in-process
diagnose met dertig kernelcalls op de echte glyphs gaf ongeveer 2,1/2,7 ms
per DynLex-call versus circa 19/20 ms voor de oude per-pixel-FFI/Rust-proef.
Dit is geen volledige PDF→RGBA-benchmark en geen algemene snelheidsclaim.

Reproduceren vanuit `rust-studio` na de OPLN-area-library uit de vorige
sectie:

```sh
python3 dynlex-engine-poc/rgba/build_glyph_multi.py \
  --compiler ~/.cache/opds-toolchains/dynlex-0.0.1/dynlex/bin/dynlex \
  --opt ~/.cache/opds-toolchains/llvm-20/root/usr/bin/opt-20 \
  --llc ~/.cache/opds-toolchains/llvm-20/root/usr/bin/llc-20 \
  --output-dir /tmp/opds-glyph-multi-build
rustc --edition=2024 -O dynlex-engine-poc/rgba/glyph_multi_native_probe.rs \
  -L native=/tmp/opds-glyph-multi-build \
  -L native=/tmp/opds-curve-pixel-build \
  -o /tmp/opds-glyph-multi-native-probe
python3 dynlex-engine-poc/rgba/verify_real_multi_glyph.py \
  --stage target/release/stage_type0_text \
  --hayro target/release/opds-engine-probe \
  --probe /tmp/opds-glyph-multi-native-probe \
  --pdf '../test pdf-bestanden/Originele bestanden/Technische tekening.pdf'
python3 dynlex-engine-poc/rgba/verify_glyph_multi_geometry.py \
  --probe /tmp/opds-glyph-multi-native-probe \
  --hayro target/release/opds-engine-probe
```
