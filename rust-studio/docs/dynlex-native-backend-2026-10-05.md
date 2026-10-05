# DynLex als optionele kernel in de native Rust-app

De native app gebruikt standaard Hayro. De Rust-crate
`opds-dynlex-backend` biedt een in-process, optionele DynLex-bufferkernel.
Er is geen JavaScript of zelfgeschreven C++-brug. De kernelcompile gebruikt
DynLex 0.0.1 en LLVM20 en levert een statische library; die toolchain
bevat C++-code. PDFium zit **nog niet** in deze native app. Wanneer die
later als tweede alternatieve backend wordt toegevoegd, moet de
aanroep uit Rust komen en blijft PDFium zelf een C++-library.

De feature `dynlex-experimental` bouwt de aparte kernel mee:

```sh
cd rust-studio
cargo run --release --offline -p opds-native-app \
  --features dynlex-experimental -- 'pad/naar/document.pdf'
```

Zet `OPDS_RENDER_BACKEND=dynlex` voor het starten om DynLex te kiezen.
Zonder die variabele kiest de app Hayro. In de **Settings**-ribbon staan
Hayro en DynLex als directe keuzes. Wisselen maakt pagina- en
miniaturencaches leeg en negeert oudere asynchrone renderresultaten.
DynLex is alleen klikbaar in een build met `dynlex-experimental`.
De actieve knop toont de voorkeursbackend; bij een niet-ondersteunde
pagina gebruikt de router Hayro zonder de selectie te veranderen.
De statusregel toont de renderer van het daadwerkelijk getoonde paginabeeld:
`Renderer: DynLex`, `Renderer: Hayro` of
`Renderer: Hayro (DynLex fallback)`. Een pagina uit de bitmapcache behoudt
de vastgelegde renderer; oude asynchrone resultaten worden na wisselen geweigerd.
De huidige OPGM-v3-adapter kan
uitsluitend één pagina op schaal 1,0 verwerken, met 1–64 dekkende
pixeluitgelijnde rechthoekfills/1px asgerichte strokes, beperkte q/Q,
gehele translatie, rechthoekige clips en DeviceRGB-kleuren met kanaalwaarden
van 0 tot 1. De RGB-waarden worden naar 8-bit afgerond; transparantie,
andere kleurruimtes en willekeurige paden zijn nog niet ondersteund.
Vul- en lijnkleur en de expliciete 1pt-lijnbreedte blijven tussen meerdere
tekenopdrachten behouden; `q/Q` bewaart en herstelt deze staat.
Een tweede, eveneens opt-in route hergebruikt de beperkte OPDL-v2-adapter:
één ongefilterde of enkelvoudig met FlateDecode gecomprimeerde, niet
geïnterpoleerde RGB8-XObject op een ongeroteerde pagina tot 1024×1024.
De afbeelding mag de hele pagina vullen of een kleiner pixeluitgelijnd
rechthoekig deel zonder resampling; de bronpixels moeten exact overeenkomen
met de positieve gehele breedte en hoogte in de PDF-plaatsingsmatrix.
Buiten de afbeelding is de pagina wit. Daarna zijn bewezen rechthoekfills
en 1px-strokes toegestaan; een afbeelding ná vectorinhoud wordt geweigerd.
Rust valideert de complete OPDL-scene opnieuw
voor de DynLex-ABI; alleen dan draait de hybride kernelmodus. FlateDecode
wordt met een harde limiet van exact de verwachte RGB-bytelengte uitgepakt.
De begrensde decompressor is de pure-Rust `miniz_oxide`-bibliotheek.
Onbekende afbeeldingsdictionarysleutels, waaronder `/DecodeParms` en
alfamaskers, andere beeldfilters, geërfde rotatie, CropBox en ongedekte
PDF-operatoren vallen
terug op Hayro. Dit is geen algemene PDF-afbeeldingsdecoder.
Bij elke niet-ondersteunde PDF,
pagina of schaal rendert Hayro. Dit voorkomt dat een nog onvolledige
DynLex-kernel PDF-inhoud stil weglaat. Er is nog geen
PDFium-backend in de Rust-app en geen
100% PDF-objectdekking.

De native app roept de kernel alleen via de veilige Rust-router aan.
Deze valideert de gegenereerde OPGM-v3- of RGB-OPDL-scene opnieuw vóór de C-ABI,
begrenst de uitvoer tot 6 miljoen pixels en gebruikt de bestaande
Hayro-renderer als fallback. De standaardbuild compileert de DynLex-
toolchain niet; `build.rs` doet dat alleen bij de feature `kernel`.
Toolchainpaden zijn via `OPDS_DYNLEX_COMPILER`, `OPDS_LLVM_OPT` en
`OPDS_LLVM_LLC` instelbaar, met de gedocumenteerde lokale cache als
standaard.

Verificatie: een echte fill/stroke-PDF is in de Rust-router bytegelijk
aan Hayro; een tekst-PDF volgt de fallback en blijft identiek aan Hayro.
In het draaiende 1200×832-venster zijn beide Settings-knoppen geklikt;
na wisselen rendert de pagina opnieuw. Zoom In bij DynLex-selectie
ging van 100% naar 125% en werd via de Hayro-fallback getoond.
Met de statusregel is in het 1200×832-venster ook gecontroleerd dat
`simple-fill-stroke.pdf` op 100% `Renderer: DynLex` toont, op 125%
`Renderer: Hayro (DynLex fallback)`, en dat `tekstrotatie-extern.pdf`
direct bij openen dezelfde fallback toont.
`rgba/verify_stroke_geometry.py` vergeleek dertien gecontroleerde PDF's,
inclusief rotatie/CropBox/A1, byte voor byte met Hayro en de onafhankelijke
referentie. Vier extra PDF's met fractionele RGB-fill en -stroke zijn op
normale, 90°-, 270°/CropBox- en A1/90°-pagina's eveneens bytegelijk;
waarden buiten 0–1 worden afgewezen. De native backendtest controleert
dat zo'n PDF daadwerkelijk via DynLex gaat. Deze tests bewijzen slechts
de beschreven deelset.
Twee aanvullende pagina's testen hergebruik en `q/Q`-herstel van vulkleur,
lijnkleur en lijnbreedte; de directe native backendtest is ook daarvoor
bytegelijk aan Hayro.
Vier echte RGB-image-PDF's, raster en met vectoroverlay, zowel ongefilterd
als FlateDecode, worden
in de native backend op 256×192 pixel voor pixel met Hayro vergeleken.
Vier extra PDF's plaatsen een 64×48-beeld op (31,27) binnen die pagina:
raster/hybride, raw/Flate, telkens RGBA-bytegelijk aan Hayro. Een plaatsing
buiten de pagina, een fractionele positie en beeldinhoud na vectorinhoud
vallen aantoonbaar terug op Hayro. Rotatie, shear, geschaalde bronpixels,
meerdere afbeeldingen en andere filters zijn nog niet bewezen.
Een pagina met geërfde `/Rotate 90` volgt aantoonbaar de Hayro-fallback;
een ongeldige Flate-stroom, decodeparameters en een opgeblazen beeldstroom
worden niet aan DynLex doorgegeven.
