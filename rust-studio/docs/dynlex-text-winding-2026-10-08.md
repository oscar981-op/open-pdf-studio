# Zelfstandige DynLex-proef rendert volledige externe tekstpagina

De aparte `truetype_page_probe --experimental-winding` tekent de volledige,
ongewijzigde pagina 6 van het openbare Albany-contract: 657 zichtbare glyphs,
798 tekencodes, twee ingebedde TrueType-fonts en één brede rechthoek. Op 25%,
100% en 200% zijn alle **9.815.256 RGBA-bytes** exact gelijk aan Hayro.
Geen glyph is weggelaten; er wordt geen Hayro/Vello-bitmap gebruikt.

**Nog geen native ondersteuning:** alleen de expliciete onderzoeksmodus
accepteert de nieuwe contouren. `PreparedPage`, de normale OPGS/OPM2-validator
en app-dispatch zijn niet verruimd. De gewone DynLex-app blijft deze tekstpagina
weigeren. De lopdf-onderzoeksfrontend heeft nog geen volledig object-/recursie-
allocatiebudget voor onbetrouwbare documenten. Native integratie is de volgende
stap, geen voltooid onderdeel van dit resultaat. Geen nieuwe snelheidsclaim.

## Oorzaak van de blokkade en aparte grens

De eerdere afwijzing bij paint 8 / TT0 / GID 992 kwam van de OPM2-eis dat
contouren eenvoudig en onderling niet-kruisend zijn. De originele door skrifa
geleverde quadratic-outline heeft ook bij een onafhankelijke dichte evaluatie
met 64 én 128 stappen per quadratic één stabiele kruising. Dit wijst niet op
een foutief bronfont: een nonzero-vulregel kan zulke omtrekken beschrijven.
Er is daarom geen apostrof-uitzondering en de oude validator is niet verwijderd.

De afzonderlijke **OPNW-v1** onderzoeksgrens accepteert gesloten nonzero-
contouren die mogen kruisen, overlappen of degenereren. Het contract heeft:

- vier bytes `OPNW`, daarna little-endian u32 versie 1, breedte 256, hoogte 192,
  contouraantal;
- per contour een u32 puntaantal gevolgd door gesloten f64 x/y-paren;
- 1–8 contouren, 4–256 punten per contour en maximaal 256 punten samen;
- maximaal 4.148 invoerbytes, exacte lengte, eindige coördinaten binnen de tile,
  expliciete sluiting en geen extra bytes;
- uitsluitend nonzero-vulling, geen even-odd-keuze of impliciete flags.

Rust valideert alle grenzen vóór de C-ABI, bouwt een 8-byte-uitgelijnde private
buffer en roept de **bestaande zelfstandige DynLex strip-windingkernel** aan.
Die loopt over begrensde contouren/edges en schrijft precies 49.152
dekkingsbytes. Geometrie beïnvloedt de dekking, niet bufferindexering of
onbegrensde subdivisie. De interne historische ABI-magic verandert niet;
OPNW wordt uitsluitend via de nieuwe onderzoeksvalidator toegelaten. De
native OPM2-route blijft kruisende invoer afwijzen, ook in een regressietest.

De DynLex-compositor schildert de glyphs in oorspronkelijke paintvolgorde op
het begrensde premultiplied paginabuffer. De laatste witte-papiercompositie
blijft Rust. Brede rechthoeken worden uitsluitend in de onderzoeksmodus over
niet-overlappende tiles met gehele pixelgrenzen verdeeld. Fractionele
buitenranden blijven behouden; een tile introduceert geen extra subpixelrand.

## Eén resterende pixel: oorsprong van 4×4-strips

De eerste volledige kandidaat had op 100% één afwijkende pixel op (310,127):
RGB 46 in plaats van 47. Op 25% en 200% was hij al exact. De glyphtegel werd
op een willekeurige gehele oorsprong gezet; dat veranderde de f32-bewerkings-
volgorde van de 4×4-striprasterisatie.

De onderzoeksmodus rondt nu de tile-oorsprong naar beneden af tot een veelvoud
van vier, terwijl de glyphpunten eerst in globale schermruimte tot f32 worden
gekwantiseerd. Hiermee blijven stripfase en het globale paginaraster gelijk.
Dit is een algemene tileconventie, geen correctie van die ene pixel. De
gewone stager blijft ongewijzigd. Historische foutdata staan in
`dynlex-text-winding-before-2026-10-08.json`.

## Bewijs en grenzen van het bewijs

| Onderzoek | Resultaat |
|---|---|
| Originele Albany-pagina 6 op 25/100/200% | 3/3 gehele pagina's, 9.815.256 RGBA-bytes exact |
| Gekleurde tekst/overlap, vier kwartslagen, twee CropBoxes | 8/8 gehele PDF-pagina's, 16.019.456 RGBA-bytes exact |
| 48 deterministische kruisende/overlappende contourgroepen | 48/48 dekkingsbuffers exact, 2.359.296 bytes |
| Apostrof met eigen én oracle-afvlakking op drie schalen | Beide routes elk 49.152 dekkingsbytes per geval exact |
| Ongeldige CLI-invoer | 5/5 afgewezen zonder uitvoer |

De synthetische contourgroepen omvatten zelfkruisende vlindervormen en
contouren met gelijke/tegengestelde oriëntatie en overlap. Zij vergelijken de
DynLex-uitvoer met een afzonderlijke Vello CPU-testoracle. Deze oracle wordt
nooit door de kandidaat aangeroepen. De gehele PDF-vergelijking gebruikt Hayro
uitsluitend als externe testreferentie. De shader-/stripwiskunde en bestaande
flattener zijn eerder geïnformeerd door de MIT/Apache-2.0 Vello-bron; geen claim
van onafhankelijk bedachte numerieke conventies. Rust ttf-parser/skrifa
leveren fontgegevens, niet PDF-rasterisatie. DynLex gebruikt LLVM/C++ en libc.

Het rapport `dynlex-text-winding-2026-10-08.json` bevat hashes, elke testcase,
foutaantallen en de dichte
quadratic-kruisingdiagnose. Dit bewijst geen algemene font-, CMap-, clip-,
CTM-, transparantie- of PDF-dekking. De eerdere native corpusaudit blijft
3/30 ondersteund; deze afzonderlijke tekstproef verandert dat cijfer niet.

## Reproductie

```sh
cd rust-studio
cargo build --release -p opds-dynlex-backend --features kernel \
  --bin truetype_page_probe --bin winding_probe
# Gebruik de bestaande release testoracles en backend_probe.
python3 scripts/audit_text_winding.py --require-exact \
  --output docs/dynlex-text-winding-2026-10-08.json

target/release/truetype_page_probe \
  "$HOME/.cache/opds-benchmarks/internet-2026-10-08/albany-contract.pdf" \
  6 1 /tmp/albany-text.rgba --experimental-winding /tmp/albany-text-trace
```

`--require-exact` faalt bij een afgewezen verwacht geval of ook maar één
ongelijke byte, en schrijft eerst het diagnoserapport. De oude
`audit_truetype_text_stage.py` blijft de normale strenge afwijzing controleren.
Unit-tests controleren truncatie op iedere lengte, header-/puntlimieten,
NaN/Inf, open contouren, extra bytes, byte-mutaties zonder panic en behoud van
de oude OPM2-afwijzing. De ongewijzigde native app is niet herstart en
gebruikersvensters/PDF's zijn niet gewijzigd. Ander ongecommit werk is behouden.

## Volgende stap

Maak een begrensde voorbereide tekstscene met expliciete fonts/resourcekoppeling,
PageSpace, paintvolgorde en totale geheugenlimiet. Valideer onbetrouwbare
objecten en content vóór integratie. Hergebruik fonts/geometrie tussen zoom-
aanvragen zonder een nieuwe decoder per glyph; controleer cache/stale resultaten.
Integreer pas na dezelfde volledige pixeltests, plus unsupported-inputtests en
een echte native UI-proef op pagina 6. Meet prestaties daarna apart met
vermelde cachelevensduur. De huidige proef hercontroleert een hele paginabuffer
per glyph en is geen geoptimaliseerde tekstpipeline.

Validatie: 259 workspace-tests met alle features, standaard workspace-check,
release-probes, formattering, Python-compile en diffcheck geslaagd. De oude
strikte tekststage-audit is opnieuw geslaagd: exacte plaatsing maar nog steeds
afwijzing van dezelfde kruisende glyph zonder output in de standaardmodus.
De PNG-preview buiten Git staat in
`~/.cache/opds-benchmarks/internet-2026-10-08/previews/dynlex-text-research-page6.png`;
dit is uitvoer van de zelfstandige proef, geen screenshot van de native app.

De volledige oude renderer-verifier is opnieuw geslaagd: 47 PDF-pagina's /
4.964.352 pixels exact en drie v4-PDF's / 147.456 pixels exact, plus acht
ongeldige scenes, 24 unsupported PDF-states en tien syntaxisvarianten.
