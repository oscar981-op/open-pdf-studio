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
één of meer ongefilterde of enkelvoudig met FlateDecode gecomprimeerde,
niet geïnterpoleerde RGB8-XObjects op een ongeroteerde pagina tot 1024×1024.
Een afbeelding mag de hele pagina vullen of een kleiner pixeluitgelijnd
rechthoekig deel. De bron mag een andere pixelmaat hebben: bij verkleinen
wordt de pure Rust-bibliotheek `pic-scale` 0.7.12 met Catmull–Rom gebruikt,
bij vergroten de dichtstbijzijnde bronpixel. DynLex tekent het voorbereide
RGB-beeld daarna in de geordende paginascene. De bron is begrensd op 4096
pixels per zijde en 6 miljoen pixels totaal; de zichtbare pagina blijft in
deze route maximaal 1024×1024. Dit is nog geen volledig in DynLex uitgevoerde
resampling.
Buiten de afbeeldingen is de pagina wit. Overlap volgt de PDF-tekenvolgorde.
Bewezen rechthoekfills en 1px-strokes zijn toegestaan. Als een afbeelding ná
vectorinhoud komt, gebruikt de native route een nieuwe experimentele
OPDL-v3-scene met geordende 24-byte opdrachten en een aansluitende RGB-blob.
Een afbeeldingsopdracht (kind 6) verwijst met een gecontroleerde byte-offset
naar zijn pixeldata. De Rust-grens eist aaneengesloten offsets, exacte
record- en beeldlengtes, maximaal 256 afbeeldingen en maximaal 3.538.984
scenebytes voordat DynLex wordt aangeroepen. OPDL-v1/v2 blijven ongewijzigd.
Afbeeldingen worden via hun PDF-resource-naam opgezocht; de naam is dus niet
beperkt tot `/Im0` of `/Im1`. De snelle contentdecoder verwerkt eenvoudige
ASCII-namen, terwijl de algemene lopdf-decoder andere geldige PDF-syntaxis
afhandelt. Onbekende resources worden geweigerd.
Rust valideert de complete OPDL-scene opnieuw voor de DynLex-ABI; alleen dan
draait de hybride v2-modus of de geordende v3-modus. FlateDecode
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
buiten de pagina of op een fractionele positie valt terug op Hayro.
Vier andere PDF's overlappen een tweede
72×56-beeld met het eerste, weer in raster/hybride en raw/Flate, telkens
RGBA-bytegelijk aan Hayro. Twee extra PDF's plaatsen de eerste of tweede
afbeelding ná vectorinhoud en zijn pixelgelijk aan Hayro via OPDL-v3.
Een nieuwe 12-image-PDF met verschillende namen, gedeeltelijke overlap,
afwisselende raw/Flate-stromen en tussenliggende vectorfills is ook
RGBA-bytegelijk. Een derde herhaalde afbeelding is eveneens gecontroleerd.
De Rust-FFI-grens weigert afgekorte of extra beeldbytes, ongeldige offsets,
opdrachtsoorten, grenzen en een scene met 257 beeldopdrachten.
De generator is daarnaast rechtstreeks met 256 en 257 verschillende
XObjects uitgevoerd: bij 256 was de 196.608-byte RGBA-uitvoer identiek aan
Hayro (SHA-256 `18fc2be16dcb4f52a338295da2cfbe99bc2771f0f371a2549da3d4d7ccb7f485`);
bij 257 schreef de DynLex-probe geen uitvoer. De native app gebruikt voor
dat laatste geval de Hayro-fallback.
Rotatie, shear, scenes boven de byte-/opdrachtlimiet en andere
filters zijn nog niet bewezen.
Een pagina met geërfde `/Rotate 90` volgt aantoonbaar de Hayro-fallback;
een ongeldige Flate-stroom, decodeparameters en een opgeblazen beeldstroom
worden niet aan DynLex doorgegeven.
Zeven aanvullende geschaalde beeld-PDF's testen vergroting, verkleining,
ongelijke schaalfactoren, raw/Flate en vectoroverlay; een 2048×64-beeldstrook
wordt op een 768×192-pagina naar 512×16 verkleind. Alle RGBA-bytes zijn
identiek aan Hayro. `/Interpolate true` en een bron breder dan 4096 pixels
worden vóór de DynLex-aanroep geweigerd. Dit dekt nog geen gedraaide/gecropte
beeldpagina of andere PDF-afbeeldingsfilters en kleurmodellen.

De 12-image-fixture is ook met een gepaarde procesbenchmark gemeten. Vanaf
`rust-studio`:

```sh
cargo build --release -p opds-dynlex-backend --features kernel --bin backend_probe --offline
python3 dynlex-engine-poc/rgba/benchmark_native_backend.py \
  crates/dynlex-backend/tests/fixtures/many-images-mixed-flate.pdf \
  --probe target/release/backend_probe --runs 30
```

`backend_probe` schrijft in beide modi dezelfde ruwe RGBA-bytes; `--hayro`
kiest de referentierenderer. Het script wisselt de procesvolgorde per ronde,
controleert bij ieder paar de SHA-256 en meet wandtijd inclusief PDF-parse,
renderen, raw-bestand schrijven en processtart, plus piek-RSS per proces.
Op deze kleine 256×192-fixture waren de warme medianen DynLex/Hayro
8,16/8,04 ms en 4.688/6.610 KiB; DynLex won 18 van 30 paren. Het gepaarde
mediane tijdverschil was -0,30 ms. De ruwe samples staan in
`dynlex-engine-poc/rgba/native-many-image-bench-2026-10-05.json`.
De OS-cache werd niet gewist en de timings liggen dicht bij elkaar: dit is
geen overtuigend of algemeen snelheidsvoordeel.

Op de aparte 2048×64→512×16-strook (15 gepaarde warme procesruns) waren de
medianen DynLex/Hayro 9,32/10,39 ms en piek-RSS 5.560/7.408 KiB;
DynLex won 13 van 15 paren. Dit omvat ook PDF-parsing, pure-Rust-beeldschaling,
processtart en RGBA-bestand schrijven. Ruwe samples:
`dynlex-engine-poc/rgba/native-scaled-strip-bench-2026-10-05.json`.
De kleine gecontroleerde pagina rechtvaardigt geen algemene snelheidsclaim.
