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

## Begrensde contourvakken in de experimentele kernel

De OPM2-DynLex-kernel berekent nu eenmaal per gevalideerde contour een
`min/max`-vak en slaat de vier polygoonclipstappen over voor pixels die
helemaal buiten dat vak liggen. De maximaal acht vakken nemen samen 256
bytes in. De Rust-grens en het OPM2-formaat zijn ongewijzigd. Twee echte
CID-glyphs, losse/geneste contouren, de grens van acht contouren, een
paginavullende contour met gat en een kwartslag blijven volledig bytegelijk
aan de oude kernel en Hayro.
De schuine ruit houdt exact dezelfde 72 diagnostische randverschillen; de
optimalisatie is geen oplossing voor f32-strippariteit.

`bench_glyph_multi_bbox.py` verifieert de complete buffer bij elk van 30
afwisselende warme procesparen, met honderd kernelcalls per proces. Op de
twee echte CID-glyphs waren de medianen oud/nieuw respectievelijk
**1,935/0,543 ms** en **2,636/0,541 ms per kernelcall**; nieuw won 30/30
paren in beide gevallen. Mediaan piek-RSS bleef ongeveer 2,1 MiB. De ruwe
samples staan in `rgba/glyph-multi-bbox-bench-2026-10-05.json`. Dit meet geen
PDF-parsing, validatie, compositing of app-weergave en toont dus geen
algemene PDF-snelheidswinst.

De oude bron kan uit commit `4d21eac1` worden gebouwd en de nieuwe bron
uit de huidige branch met het hierboven getoonde `build_glyph_multi.py`-
commando. Kopieer voor de oude build de huidige `build_glyph_multi.py` en
`renderer.dl` naar een aparte map en zet daarin
`git show 4d21eac1:rust-studio/dynlex-engine-poc/rgba/glyph_multi_kernel.dl`
als `glyph_multi_kernel.dl`; compileer beide libraries en link dezelfde
`glyph_multi_native_probe.rs` afzonderlijk tegen oud en nieuw. Maak de
scenes via `stage_type0_text --isolate-multi ... 2 26` en `... 2 264`.
Voor het gepaarde protocol:

```sh
python3 dynlex-engine-poc/rgba/bench_glyph_multi_bbox.py \
  --old /tmp/opds-glyph-multi-old-probe \
  --new /tmp/opds-glyph-multi-bbox-probe \
  --scene /tmp/opds-glyph-26.opm --scene /tmp/opds-glyph-264.opm \
  --rounds 30 --repeats 100
```

## Afzonderlijke f32-stripkernel voor rechte glyphpaden

`glyph_strip_scalar.rs` en `glyph_strip_kernel.dl` rekenen dezelfde
4×4-tegelwinding met f32-waarden en fused multiply-add. De wiskunde is
afgeleid uit de Apache-2.0/MIT-gelicentieerde `vello_common` 0.0.8-bron;
beide proeven gebruiken **geen Vello-runtime**. DynLex 0.0.1 ondersteunt
`a 32 bit float`, maar zijn `!=`-vergelijking is ordered en vangt NaN niet:
de kernel behandelt die situatie expliciet met de false-tak van `=`.
Rust valideert OPM2 voordat de begrensde C-ABI wordt aangeroepen. Deze
route is uitsluitend experimenteel en nog niet verbonden met de app.

De DynLex-stripkernel is op de twee echte CID-glyphs, losse/geneste
contouren, acht contouren, volle pagina met gat, 90°-vorm én de schuine
ruit met gat telkens **49.152/49.152 bytes** gelijk aan Hayro. De vorige
area-kernel had op die ruit 72 afwijkende randpixels. De eerste stripproef
gaf op 200 plus 500 willekeurige ruiten met gaten 666/700 volledige
Hayro-pariteit. Na strikte lijn/tegeltoewijzing zijn dit **678/700** scènes:
de overige 22 hebben samen 29 pixels met één byte verschil. Iedere
afwijking is met exacte rationale clipping geverifieerd als
een halve 8-bit-stap. De volledige buffers van de zelfstandige Rust- en
DynLex-stripproef waren op alle 700 scènes onderling bytegelijk. Een derde
seed gaf 975/1000 scènes volledig gelijk, met 37 halve-byte-randpixels in
25 overige scènes. Ruwe gevallen staan in de drie
`rgba/glyph-strip-dynlex-random-*-2026-10-05.json`-rapporten.
Dit is vooruitgang voor rechte outline-segmenten, geen bewijs voor algemene
glyphs, curves, tekstlagen of alle PDF-randen. De normale app blijft tekst
aan Hayro geven totdat ook de resterende halve-stapgevallen en PDF-
compositing veilig zijn.

De extra controle kwam uit een directe Vello CPU-oracle op een afwijkende
ruit: het Vello-alfa was 42, gelijk aan Hayro, terwijl de eerste DynLex-
proef 43 gaf. Vello registreerde in die tegel slechts één lijn. De eerste
proef nam door een overlappende bounding box nog lijnen mee die de tegel
alleen op zijn grens raakten; na een halfopen lijn/tegeltest is deze pixel
exact. Een verticale lijn op een tegelgrens wordt aan de tegel rechts
toegewezen. De proef bevat nog geen volledige Vello-tegelbinning, dus de
overblijvende halve-byteverschillen zijn niet stilzwijgend weggewerkt.
De read-only `glyph_vello_probe` en `verify_glyph_vello_oracle.py` maken de
directe Vello/Hayro/DynLex-vergelijking van die 49.152 bytes reproduceerbaar;
de app en kernel roepen deze Vello-proef niet aan.

Dertig afwisselend gepaarde warme processen met honderd kernelcalls per
proces op de twee echte glyphs gaven vorige area-/eerste stripkernel
**0,552/0,314 ms** en **0,554/0,341 ms per call**; strip won 30/30 paren
bij beide. Dit sluit PDF-parsing, Rust-validatie, compositing en processtart
uit. De ruwe samples staan in `rgba/glyph-strip-bench-2026-10-05.json`.
Een nieuw gepaard A/B van eerste/strikt gebinde stripkernel gaf
**0,334/0,334 ms** en **0,349/0,374 ms** per call, met 16/30 en 11/30
winstparen; de nauwkeuriger tegeltest heeft dus geen bewezen snelheidswinst.
Ruwe samples: `rgba/glyph-strip-line-bench-2026-10-05.json`.
Reproduceren vanuit `rust-studio`:

```sh
python3 dynlex-engine-poc/rgba/build_glyph_strip.py \
  --compiler ~/.cache/opds-toolchains/dynlex-0.0.1/dynlex/bin/dynlex \
  --opt ~/.cache/opds-toolchains/llvm-20/root/usr/bin/opt-20 \
  --llc ~/.cache/opds-toolchains/llvm-20/root/usr/bin/llc-20 \
  --output-dir /tmp/opds-glyph-strip-build
rustc --edition=2024 -O dynlex-engine-poc/rgba/glyph_strip_native_probe.rs \
  -L native=/tmp/opds-glyph-strip-build \
  -L native=/tmp/opds-curve-pixel-build \
  -o /tmp/opds-glyph-strip-native-probe
python3 dynlex-engine-poc/rgba/verify_glyph_multi_geometry.py \
  --probe /tmp/opds-glyph-strip-native-probe \
  --hayro target/release/opds-engine-probe --expect-diagonal-exact
python3 dynlex-engine-poc/rgba/verify_glyph_strip_random.py \
  --probe /tmp/opds-glyph-strip-native-probe \
  --compare-probe /tmp/opds-glyph-strip-scalar \
  --hayro target/release/opds-engine-probe --cases 500 --seed 20261006
cargo build --offline --release -p opds-engine-probe --bin glyph_vello_probe
python3 dynlex-engine-poc/rgba/verify_glyph_vello_oracle.py \
  --vello target/release/glyph_vello_probe \
  --hayro target/release/opds-engine-probe \
  --dynlex /tmp/opds-glyph-strip-native-probe
```
