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
andere vector-kleurruimtes en willekeurige paden zijn in deze OPGM-v3-route
nog niet ondersteund.
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
latere deelstappen hieronder ondersteunen de beeldtransformaties en een
begrensde Indexed-vorm. De volledige pagina valt nog terecht terug op Hayro.

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

Een niet-rechthoekig clipvak, `/Interpolate true`, scheve of omgekeerde
beeldmatrix en andere onbewezen PDF-states blijven op Hayro terugvallen. De echte
combinatie-PDF bevat bovendien duizenden curven en clippaden; deze proef
bewijst geen volledige objectdekking.

Op de gecontroleerde fractionele 3843×68-strook (15 afwisselende warme
procesparen) waren DynLex/Hayro mediane wandtijd 58,99/79,20 ms en mediane
piek-RSS 20.100/23.232 KiB; DynLex won 15/15 paren. De volledige
8.022.576-byte RGBA-uitvoer was in ieder paar bytegelijk. De benchmark meet
PDF-parsing, beeldvoorbereiding, processtart, renderen en raw-bestandsschrijven
met warme OS-cache. Ruwe samples staan in
`dynlex-engine-poc/rgba/native-fractional-strip-bench-2026-10-05.json`.
Dit is geen meting van de volledige echte combinatie-PDF of van andere
PDF-objecten.

Voor afbeeldings-XObjects ondersteunt de geordende native route nu ook een
begrensde `/Indexed /DeviceRGB`-deelset: 8-bit indices, hoogstens 256
paletkleuren, een letterlijke of hexadecimale PDF-lookupstring met exact
`3 × (hival + 1)` bytes, optioneel één FlateDecode-filter en geen predictor.
De pure-Rust-adapter valideert iedere index vóór omzetting naar RGB; daarna
gebruikt hij dezelfde schaal-, fractionele dekkings- en laagvolgorderoute.
De twee Indexed-objecten in de echte combinatie-PDF hebben precies deze
dictionaryvorm (1429×305 en 1428×232, hival 255, 768 paletbytes), maar
de **volledige pagina** blijft op Hayro terugvallen vanwege de overige
onondersteunde PDF-operators. Vier gecontroleerde Indexed-PDF's zijn
RGBA-bytegelijk aan Hayro: raw raster, Flate/hybride, kwartslag met CropBox
en een 1428×232-bron met 256-kleurenpalet op een grote pagina. Een te hoge
index, te kort palet en een expliciete `/Decode` worden afgewezen.
`dynlex-engine-poc/rgba/indexed_image_fixture.py` genereert deze fixtures.
Predictors op Indexed-beelden, niet-DeviceRGB-basispaletten, streamlookups,
maskers en transparantie zijn niet ondersteund.
Op de grote gecontroleerde Indexed-fixture waren 15 warme procesparen
DynLex/Hayro mediaan 50,51/54,32 ms en mediane piek-RSS 20.104/22.880 KiB;
DynLex won 10/15 paren. Alle 8.022.576 RGBA-bytes waren steeds gelijk.
De ruwe samples staan in
`dynlex-engine-poc/rgba/native-indexed-large-bench-2026-10-05.json`.
Door de spreiding is dit geen overtuigend of algemeen snelheidsvoordeel.

De geordende route snijdt nu een RGB- of Indexed-beeldpatch aan een bewezen
pixeluitgelijnd rechthoekig PDF-clipvak (`re W n`). De adapter zet het
clipvak eerst via MediaBox/CropBox en paginarotatie naar schermcoördinaten
om en snijdt daarna de RGB- of binaire-RGBA-patch uit, vóór de DynLex-ABI.
Opeenvolgende clipvakken worden doorsneden; `q/Q` bewaart en herstelt hun
toestand. Vijf gecontroleerde PDF's met fractionele beeldranden, vectorlaag
onder het beeld, dubbele clip, `q/Q` en 90°/CropBox zijn RGBA-bytegelijk
aan Hayro. Een volledig weggeclipt beeld levert nu een lege geordende actie op;
een daarna zichtbaar beeld behoudt zijn laagvolgorde. Drie fixtures voor lege
aligned/fractionele clips en herstel via `Q` zijn pixelgelijk aan Hayro.

Een enkel gesloten `m/l/l/l/h`-pad wordt nu alleen als rechthoek geaccepteerd
wanneer het precies de vier verschillende hoeken bevat en elke zijde
asgericht is. `W` en `W*` hergebruiken dan dezelfde begrensde rechthoekige
cliproute, inclusief fractionele rand en paginaoriëntatie. Vier gerichte
PDF-fixtures zijn pixelgelijk aan Hayro; open en kruisende paden blijven op
Hayro terugvallen. Twee extra deterministische reeksen van elk
1.000 `m/l/h`-rechthoek-PDF's met wisselende kwartslag en `W`/`W*` gaven
949/949 en 959/959 ondersteunde resultaten bytegelijk aan Hayro; 51 en 41
ambigue randen vielen terug. Herhaal met de bovenstaande
`fractional_clip_verify.py`-commando's plus `--polygon-rect`. Dit is geen
algemene polygonale clipping.
`dynlex-engine-poc/rgba/clipped_image_fixture.py` maakt de fixtures.

Een afzonderlijke, nog smalle route accepteert nu één gesloten, eenvoudig
`m/l/.../h W/W* n`-clippad met drie tot acht verschillende hoekpunten rond
een beeldlaag. De Rust-adapter begrenst alle coördinaten tot ±4096, wijst
kruisingen/zelfaanrakingen en meer dan één holle hoek af, rekent de
polygonale overlap per pixel analytisch uit en beperkt de beeldpatch tot
65.536 pixels. Hij schrijft de dekking als geordende OPDL-v6 RGBA-patch;
DynLex mengt die met de onderliggende kleur. Bij één eenvoudig contour zijn
`W` en `W*` gelijk. Zeven gecontroleerde PDF's met convexe en éénhoekig
holle vormen, `W`/`W*`, 90°/CropBox en `q/Q`-herstel met een latere zichtbare
afbeelding zijn bytegelijk aan Hayro. Bij dekking
binnen 0,002 van een halve 8-bit-stap valt de hele pagina terug op Hayro:
de f32-randberekening kan daar één kleurwaarde anders afronden. Twee
deterministische reeksen van elk 1.000 wisselende drie-/vierhoeken gaven
418/418 en 446/446 ondersteunde pagina's exact gelijk aan Hayro; 582 en
554 vielen terug. Met kwartpixelverschuiving waren 339/339 ondersteunde
pagina's exact en 661 fallback. Reproduceer met:

```sh
python3 dynlex-engine-poc/rgba/polygon_clip_verify.py \
  --probe target/release/backend_probe --cases 1000 --seed 20261005
python3 dynlex-engine-poc/rgba/polygon_clip_verify.py \
  --probe target/release/backend_probe --cases 1000 --seed 20261006
python3 dynlex-engine-poc/rgba/polygon_clip_verify.py \
  --probe target/release/backend_probe --cases 1000 --quarter
```

De uitbreiding met één holle hoek is in vijf extra deterministische reeksen
van elk 1.000 gevallen vergeleken. Ondersteunde gevallen waren
453/453 (gehele coördinaten), 278/278 (kwartpixels), 538/538 (gewone
decimale PDF-coördinaten), 440/440 (convexe decimalen) en 553/553
(decimale coördinaten onder een globale 0,1-CTM), steeds exact gelijk aan
Hayro; de overige gevallen vielen terug. Reproduceer onder meer met:

```sh
python3 dynlex-engine-poc/rgba/polygon_clip_verify.py \
  --probe target/release/backend_probe --cases 1000 --concave --decimal
python3 dynlex-engine-poc/rgba/polygon_clip_verify.py \
  --probe target/release/backend_probe --cases 1000 --concave --decimal \
  --global-ctm --seed 20261007
```

Het echte `W*`-pad bij contentoperator 582 van `Combinatie Raster, vector,
tekening images.pdf` heeft zes hoekpunten, één holle hoek en staat onder
`0.1 0 0 0.1 0 0 cm`. Een geïsoleerde 1684×1191-PDF met precies die
coördinaten is nu door DynLex gerenderd: alle 8.022.576 RGBA-bytes zijn
gelijk aan Hayro. De dichtstbijzijnde halve dekkingsstap in die fixture is
95,49665 van 255, dus ongeveer 0,00335 van de afrondingsgrens. De
veiligheidsmarge is van 0,01 naar 0,002 verkleind; exacte halve stappen
blijven fallback. Dat is empirische uitbreiding, geen formeel bewijs voor
alle PDF-geometrie. Op zeven extra deterministische reeksen van 2.000
polygonen waren alle 9.588 geaccepteerde gevallen bytegelijk aan Hayro,
waaronder vier reeksen met onafhankelijk verschoven decimale hoekpunten
onder een globale 0,1-CTM. Herhaal bijvoorbeeld:

```sh
python3 dynlex-engine-poc/rgba/polygon_clip_verify.py \
  --probe target/release/backend_probe --cases 2000 --concave --decimal \
  --jitter-vertices --global-ctm --seed 20261009
```

De andere elf complexe `W*`-paden en de duizenden curven/tekstoperatoren
zijn niet bewezen; de hele echte pagina blijft Hayro gebruiken. De read-only
`inspect_pdf --clip-ops`-inventaris toont voor `W*`-operator 338 vier gesloten
contouren met 108, 68, 99 en 56 hoekpunten; elf andere complexe `W*`-paden
hebben één contour met 8–48 hoekpunten. De echte pagina valt nog buiten de
bewezen subset. Zelfkruisende/open paden, fills/strokes onder de polygonale
clip en onzekere randdekking blijven fallback.

De eerste begrensde `W*`-meercontourproef accepteerde twee tot vier expliciet
gesloten `m/l/h`-contouren. Iedere contour was strikt convex met 3–32 hoekpunten en
coördinaten binnen ±4096; een contour mag geheel in een andere liggen of
er geheel los van staan. Rakende/kruisende grenzen, zelfkruising, vijfde
contour en onzekere 8-bit-dekking geven Hayro-fallback. De adapter valideert
iedere contour, bouwt de containmentdiepte op en telt per pixel de
analytische oppervlakken op met afwisselend teken. Pas daarna rondt hij de
even-odd-dekking eenmaal af en mengt de beeldlaag via de bestaande begrensde
OPDL-v6-patch. Een enkel contour met hoogstens één holle hoek behoudt zijn
bestaande aparte subset.

Gerichte echte PDF-fixtures voor twee gaten, twee disjuncte vlakken, vier
contouren met twee gaten, een genest eiland en een twaalfhoekig buitenpad
zijn RGBA-bytegelijk aan Hayro; rotatie/CropBox, blauwe onderlaag,
omgekeerde contourvolgorde en `q/Q`-herstel zijn meegetest. Vijf layouts
in twee deterministische reeksen van 400 PDF's gaven samen 2.703/2.703
geaccepteerde DynLex-resultaten bytegelijk aan Hayro. De overige 1.297
gevallen vielen terug, meestal door ambigue dekking nabij een halve
8-bit-stap. Herhalen vanuit `rust-studio`:

```sh
for layout in one-hole two-holes disjoint four ring12; do
  python3 dynlex-engine-poc/rgba/nested_clip_verify.py --probe target/release/backend_probe --cases 400 --seed 20261005 --layout "$layout"
  python3 dynlex-engine-poc/rgba/nested_clip_verify.py --probe target/release/backend_probe --cases 400 --seed 394727 --layout "$layout"
done
```

`W` met meerdere contouren, overlappende/open grenzen, een vijfde contour
en een driehoekig gat met twijfelachtige 8-bit-randdekking worden geweigerd.
De latere eenvoudige-polygoonproef verhoogt de limiet tot 128 hoekpunten per
contour en accepteert ook meerdere holle hoeken, mits elke contour eenvoudig
is en de contourgrenzen elkaar niet raken of kruisen. Voor de nestingtest
gebruikt zij punt-in-polygoon in plaats van de convexe-halfvlaktest; de
analytische oppervlakken blijven per containmentdiepte getekend. Een
67-punts trapvorm met gat, een holle contour met gat op normale en
90°/CropBox-pagina's en 430/430 geaccepteerde willekeurige holle gevallen
waren bytegelijk aan Hayro. Nog 370 gevallen vielen veilig terug. Reproduceer
de twee differentiële reeksen met `nested_clip_verify.py --layout concave-hole`
en seeds `20261005` en `394727`, elk met `--cases 400`.

`inspect_pdf --page 1 --dump-clip 338` exporteert het echte pad naar
`dynlex-engine-poc/rgba/real-clip-338.content`. Alle vier contouren zijn
eenvoudig maar hol; drie zijn onderling los, de vierde ligt in een andere.
`clipped_image_fixture.py` bouwt hieruit een geïsoleerde beeld-PDF, ook met
90°/CropBox. Bij het uitschakelen van de conservatieve halve-8-bit-randcontrole
weken 48 RGBA-kanaalbytes af van Hayro. Met die controle actief wordt dit
echte corpuspad nog geweigerd en veilig door Hayro getekend. Dit is dus geen
bewijs van volledige even-odd-clippariteit; onzekere randdekking, complexe
vectorinhoud, tekst en curven blijven fallback.

Een aparte numerieke proef (`rgba/real_clip_338_diag.py`) lokaliseert het
verschil. `lopdf` bewaart decimale PDF-operanden als `f32`; Hayro leest voor
dit pad de oorspronkelijke decimale waarde met hogere precisie en zet pas de
uiteindelijke schermpunten om naar `f32`. Op de geïsoleerde PDF levert de
analytische proef met volledige `f64`-coördinaten 14 afwijkende pixels op,
met al bij inlezen vernauwde coördinaten 8, en met precieze PDF-getallen plus
`f32`-schermpunten 0. De laatste variant geeft ook op 90°/CropBox 0
afwijkende pixels. Herhaal vanuit `rust-studio` na de backend-probebuild:

```sh
python3 dynlex-engine-poc/rgba/real_clip_338_diag.py \
  --pdf crates/dynlex-backend/tests/fixtures/clipped-real-shape-338.pdf \
  --probe target/release/backend_probe --mode f64-input-f32-display --expect-mismatches 0
python3 dynlex-engine-poc/rgba/real_clip_338_diag.py \
  --pdf crates/dynlex-backend/tests/fixtures/clipped-real-shape-338-rotate-crop.pdf \
  --probe target/release/backend_probe --mode f64-input-f32-display \
  --rotate-crop --expect-mismatches 0
```

De experimentele adapter bewaart daarom voor een strikt canonieke
één-operator-per-regel-contentstream de originele `m`, `l` en `cm`-getallen
als `f64`, na afstemming met de door `lopdf` geparseerde operatoren. Andere
syntaxis blijft op het bestaande beperkte decodepad. De PDF→scherm-punten
worden vóór oppervlakte-integratie naar `f32` gekwantiseerd; de adapter
valideert de eenvoudige contouren en nesting daarna opnieuw om een door
afronding gewijzigde topologie veilig af te wijzen. De conservatieve
grens rond halve 8-bit-dekking blijft verplicht: zonder deze grens weken twee
van 400 willekeurige een-gat-PDF's bij seed `394727` af, ondanks precieze
coördinaten. Met de grens waren alle 4.800 gevallen uit de zes meercontour-
layouts en drie extra reeksen van elk 1.000 enkelcontourgevallen bij
geaccepteerde DynLex-uitvoer bytegelijk aan Hayro. De echte operator 338
raakt nog steeds de onzekere grens en blijft fallback.

Fractionele rechthoekranden krijgen een aparte geordende OPDL-v6-opdracht
(`kind=8`) met RGBA-patch en een 8-bit-dekkingswaarde per pixel. De
pure-Rust-adapter berekent de overlap van het pixelvak met het via
MediaBox/CropBox/rotatie getransformeerde PDF-clipvak. DynLex mengt de
beeldkleur in tekenvolgorde met de bestaande pagina; de 8-bit-bron- en
achtergrondbijdrage worden afzonderlijk afgerond zoals in de gecontroleerde
Hayro/Vello-uitvoer. De Rust-grens valideert versie, opdrachtsoort,
beeldgrenzen, byte-offsets en lengte voordat de ABI wordt aangeroepen.
`W` en `W*` geven bij één rechthoek hetzelfde clipresultaat. Zeven extra
gecontroleerde PDF's testen fractionele randen, overlap met een eerdere
vectorlaag, fractionele beeldrand, de vier kwartslagen, CropBox en een globale
`0.1`-CTM; zij zijn RGBA-bytegelijk aan Hayro. De vroegere eenvoudige
éénstapsmengregel week op 56 pixels van een geroteerde fixture één
kleurwaarde af en is vóór activering vervangen. Fractionele vectorfills en
strokes onder zo'n clip, algemene polygonale clips en transparantie blijven
op Hayro terugvallen.

Sommige decimale randen liggen zo dicht bij een halve 8-bit-dekkingsstap dat
de f32-PDF-padtransformatie de afronding één waarde kan veranderen. Zulke
ambiguë randen worden expliciet afgewezen en via Hayro gerenderd. Twee
deterministische differentiële reeksen van elk 1.000 fractionele clip-PDF's
gaven respectievelijk 948/948 en 959/959 bytegelijke DynLex-resultaten;
52 en 41 gevallen vielen terug. Dit is een begrensde steekproef, geen bewijs
voor willekeurige PDF-clippaden. Herhaal vanuit `rust-studio` met:

```sh
python3 dynlex-engine-poc/rgba/fractional_clip_verify.py \
  --probe target/release/backend_probe --cases 1000 --seed 20261005
python3 dynlex-engine-poc/rgba/fractional_clip_verify.py \
  --probe target/release/backend_probe --cases 1000 --seed 20261006
```

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
