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
niet geïnterpoleerde RGB8-XObjects. De oude
OPDL-v2-route blijft maximaal 1024×1024; de geordende OPDL-v3-route accepteert
maximaal 4096 pixels per zijde en 6 miljoen paginapixels en zet geërfde
MediaBox/CropBox en kwartslagrotatie om naar zichtbare paginacoördinaten.
Een afbeelding mag de hele pagina vullen of een kleiner pixeluitgelijnd
rechthoekig deel. De bron mag een andere pixelmaat hebben: bij verkleinen
wordt de pure Rust-bibliotheek `pic-scale` 0.7.12 met Catmull–Rom gebruikt,
bij vergroten de dichtstbijzijnde bronpixel. DynLex tekent het voorbereide
RGB-beeld daarna in de geordende paginascene. De bron is begrensd op 4096
pixels per zijde en 6 miljoen pixels totaal; de geordende zichtbare pagina
heeft dezelfde zijde- en pixellimieten. Dit is nog geen volledig in DynLex uitgevoerde
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
Voor enkelvoudige FlateDecode op DeviceRGB8 ondersteunt de adapter ook
`/DecodeParms` met `/Predictor 15`, `/Colors 3`, acht bits per component
en `/Columns` gelijk aan de bronbreedte. Elke PNG-rij heeft een expliciete
filterbyte (None, Sub, Up, Average of Paeth); de decompressor begrenst de
uitvoer op `hoogte × (3 × breedte + 1)` bytes en de Rust-decoder controleert
filter en exacte rijlengtes. Andere predictors, parameters of filterketens,
alfamaskers, andere beeldfilters, ongeldige paginageometrie en ongedekte
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
buiten de pagina valt terug op Hayro; fractionele asgerichte plaatsing is
inmiddels als afzonderlijke OPDL-v5-route getest, zie hieronder.
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
Shear, willekeurige beeld-CTM, scenes boven de byte-/opdrachtlimiet en andere
filters zijn nog niet bewezen.
Een pagina met geërfde `/Rotate 90` is nu bytegelijk via DynLex;
een ongeldige Flate-stroom, niet-ondersteunde decodeparameters en een opgeblazen beeldstroom
worden niet aan DynLex doorgegeven.
Zeven aanvullende geschaalde beeld-PDF's testen vergroting, verkleining,
ongelijke schaalfactoren, raw/Flate en vectoroverlay; een 2048×64-beeldstrook
wordt op een 768×192-pagina naar 512×16 verkleind. Alle RGBA-bytes zijn
identiek aan Hayro. Een achtste A2-achtige 1684×1191-pagina zet de strook
rechts van x=1024 en legt daar een rode rechthoek overheen; ook alle
8.022.576 RGBA-bytes zijn gelijk aan Hayro. `/Interpolate true`, een bron
breder dan 4096 pixels, een pagina met meer dan 6 miljoen pixels of een zijde
boven 4096 pixels worden vóór de DynLex-aanroep geweigerd. Vier extra kleine
PDF's testen een geschaald beeld dat gedeeltelijk buiten de CropBox ligt,
vectorfill en asgerichte strokecropping, met `/Rotate 0/90/180/270` geërfd uit
de paginaboom. Een A1-achtige pagina met `/Rotate 90`, verschoven CropBox,
geschaald beeld en fill is op 1984×1484 zichtbare pixels eveneens bytegelijk
aan Hayro. Ongeldige rotatie, CropBox buiten MediaBox en geërfde `/UserUnit`
blijven buiten de deelset. Andere PDF-afbeeldingsfilters en kleurmodellen
ontbreken nog.

Vier aanvullende PDF's met FlateDecode en PNG-predictor 15 zijn in de native
route RGBA-bytegelijk aan Hayro: raster, hybride, geroteerd/gecropt en een
3843×68 RGB-bronstrook op een 1684×1191-pagina. De fixturegenerator wisselt
alle vijf PNG-rijfilters af. Verkeerde Predictor-, Columns- en Colors-waarden,
een onbekend rijfilter en afgekorte rijen worden geweigerd. Dit brengt de
echte combinatie-PDF dichterbij, maar die gebruikt ook fractionele/globale
CTM-transformaties, duizenden Bézier-curven, clipping en Indexed-kleurbeelden;
de volledige pagina valt nog terecht terug op Hayro.

De geordende route kan nu positieve asgerichte PDF-matrices door geneste
`q`/`Q`-grafische toestanden samenstellen en herstellen (maximaal zestien
niveaus). Dezelfde toestand transformeert beeldplaatsingen en rechthoekige
fills; kleur, lijnkleur, lijnbreedtestatus en clip worden bij `Q` hersteld.
Vier aanvullende PDF-fixtures met een globale `0.1 0 0 0.1 0 0 cm`, lokale
beeldmatrix, vectoroverlay en herstel van matrix/kleur zijn bytegelijk aan
Hayro. Een scheve of omgekeerde matrix, plaatsing buiten de paginalimiet en
een niet-ondersteunde CTM worden geweigerd. Dit is nog geen algemene affiene
beeldrasterisatie: ook na deze stap valt de complete echte combinatie-PDF op
Hayro terug.

Fractionele asgerichte beeldplaatsing heeft nu een afzonderlijke geordende
OPDL-v5-scene. Rust bereidt het RGB-beeld voor met dezelfde begrensde
Catmull–Rom-verkleining of dichtstbijzijnde-bronpixelvergroting als de
pixeluitgelijnde route. Voor de fractionele rand maakt Rust een RGBA-patch:
de alfawaarde is uitsluitend 0 of 255 volgens de door Hayro/Vello gebruikte
1/255-dekkingsdrempel. DynLex tekent opdracht `kind=7` alleen waar de
gevalideerde alfawaarde 255 is. De Rust-FFI-grens controleert versie 5,
opdrachtsoort, afbeeldingsgrenzen, aaneengesloten byte-offsets, exacte
RGB(A)-lengtes en uitsluitend binaire alpha vóór de kernel. De bestaande
experimentele OPDL-v4-strokelijst heeft een ander contract en blijft
ongewijzigd. Volledige PDF-transparantie of een `/SMask` is hiermee niet
ondersteund.

Elf gecontroleerde PDF's testen vergroting, verkleining, gemengde schaal,
laagvolgorde, RGB- en RGBA-opdrachten in één scene en `/Rotate 0/90/180/270`
met verschoven CropBox. Een synthetische 3843×68-Predictor-15-beeldstrook op
een A2-achtige pagina gebruikt geometrie van de echte combinatie-PDF; de
DynLex-uitvoer is op deze fixtures bytegelijk aan Hayro. Een deterministische
differentiële proef vergelijkt daarnaast 200 fractionele plaatsingen van een
4×4-kleurenraster op 256×192: 200/200 volledig RGBA-gelijk. Herhaal met:

```sh
python3 dynlex-engine-poc/rgba/fractional_image_verify.py \
  --probe target/release/backend_probe --cases 200 --seed 20261005
```

Een actief clipvak, `/Interpolate true`, scheve of omgekeerde beeldmatrix en
andere onbewezen PDF-states blijven op Hayro terugvallen. De echte
combinatie-PDF bevat bovendien duizenden curven, clippaden en Indexed-
kleurenbeelden; deze proef bewijst geen volledige objectdekking.

Op de gecontroleerde fractionele 3843×68-strook (15 afwisselende warme
procesparen) waren DynLex/Hayro mediane wandtijd 58,99/79,20 ms en mediane
piek-RSS 20.100/23.232 KiB; DynLex won 15/15 paren. De volledige
8.022.576-byte RGBA-uitvoer was in ieder paar bytegelijk. De benchmark meet
PDF-parsing, beeldvoorbereiding, processtart, renderen en raw-bestandsschrijven
met warme OS-cache. Ruwe samples staan in
`dynlex-engine-poc/rgba/native-fractional-strip-bench-2026-10-05.json`.
Dit is geen meting van de volledige echte combinatie-PDF of van andere
PDF-objecten.

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
Op de A2-achtige 1684×1191-pagina waren 15 warme procesparen
57,27/58,85 ms en 20.024/22.612 KiB; DynLex won slechts 9 van 15 paren.
De ruwe samples staan in
`dynlex-engine-poc/rgba/native-large-scaled-strip-bench-2026-10-05.json`.
Op de geroteerde/gecropte A1-achtige fixture waren de medianen 70,65/76,40 ms
en 27.700/30.640 KiB; DynLex won 14 van 15 paren. De 15 ruwe paren staan in
`dynlex-engine-poc/rgba/native-rotated-cropped-image-bench-2026-10-05.json`.
Ook dit meet slechts een gecontroleerde kleine inhoud op een groot canvas,
met warme cache en processtart, geen echte technische tekening.

De PNG-predictor-strook op 1684×1191 is met hetzelfde afwisselend gepaarde
protocol en 15 warme procesruns gemeten. DynLex/Hayro mediane wandtijd was
57,23/75,34 ms; het gepaarde mediane verschil was -17,43 ms en DynLex won
14 van 15 paren. Mediane piek-RSS was 20.028/23.240 KiB. Alle 8.022.576
RGBA-bytes hadden per run dezelfde SHA-256. Ruwe samples staan in
`dynlex-engine-poc/rgba/native-predictor-strip-bench-2026-10-05.json`.
Dit is een synthetische pagina met één geschaalde strook en vectoroverlay;
de echte combinatie-PDF en algemene PDF-objecten zijn hiermee niet gemeten.

De kleine 256×192-fixture met samengestelde CTM, beeld en vectorfill is
eveneens met 15 afwisselend gepaarde warme procesruns gemeten: DynLex/Hayro
6,30/7,21 ms mediaan en 4.480/6.432 KiB piek-RSS; DynLex won 13 van 15
paren. Alle 196.608 RGBA-bytes waren gelijk. Ruwe samples staan in
`dynlex-engine-poc/rgba/native-global-ctm-bench-2026-10-05.json`.
Processtart en warme OS-cache maken dit geen algemene snelheidsclaim.
