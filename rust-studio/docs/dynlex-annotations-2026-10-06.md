# DynLex-annotatielaag: eerste begrensde compositorproef

De gebruiker wil ook **de volledige annotatielaag** in DynLex. De huidige
native app houdt annotaties en PDF-bitmap bewust gescheiden: bewerkbare
`Rectangle`, `Highlight` en `Text Box` worden standaard in
`annotation_layer.rs` door Iced getekend. De beperkte DynLex-route hieronder
kan enkele niet-geselecteerde vormen op de pagina compositeren. Hayro rendert
bestaande PDF-appearances; de engine-probe heeft
alleen voor geïmporteerde Square en Highlight zonder `/AP` een eigen
previewfallback. PDF Save As schrijft de native annotaties als printbare
`/AP`-streams. Deze paden zijn nog niet door één DynLex-annotatierenderer
vervangen.

De opt-in `opds-dynlex-backend::render_native_annotation_overlay` gebruikt nu
een **afzonderlijke in-process DynLex-kernel**. Een versieerbare OPAN-v1-lijst
bevat alleen gevalideerde pixeluitgelijnde native Square- en
Highlight-opdrachten. Rust maakt één RGBA-uitvoer van de reeds gerenderde
pagina; DynLex tekent de vormen daarin direct in model-/laagvolgorde. Een
volledige tweede RGB-paginalaag in OPDL is niet meer nodig. Square is een
2px-rand, Highlight één geel vlak met de 8-bit-dekkingsstap 90/255 die bij
de PDF-appearance met `/ca 0.35` hoort. De invoer is tot 4096 px per zijde,
6 miljoen pixels, 64 annotaties en 1556 OPAN-bytes begrensd. De Rust-grens
controleert paginaformaat, ondoorzichtige bronpixels, gehele
rechthoekcoördinaten, grenzen en aantallen vóór de DynLex-ABI; de ABI
weigert null/te korte buffers en ongeldige totale lengtes.

Een gecontroleerde PDF vergelijkt de
DynLex-composiet rechtstreeks met Hayro na Save As: afzonderlijke rode Square,
afzonderlijke gele Highlight en twee **niet-overlappende** annotaties zijn
volledig RGBA-bytegelijk. Het experiment weigerde tekst, fractionele
coördinaten en overlappende annotaties zonder een gedeeltelijke bitmap terug
te geven. Bij een overlappende Square/Highlight weken acht kanaalbytes op
de doorsnijding van de PDF-appearance-randen één stap af; daarom is die
combinatie nog fallback. Een Highlight over gekleurde PDF-pagina-inhoud week
op een gecontroleerde fixture 3.000 kanaalbytes af; ook die combinatie valt
terug. De proef accepteert Highlight alleen als **alle** onderliggende
RGB-pixels binnen de rechthoek wit zijn. Een directe 8-bit-beeldpatch met de
alternatieve Vello-/256-menging week op de witte achtergrond af en is niet
ingeschakeld. Een dekkende blauwe Square **over gekleurde pagina-inhoud** was
wel bytegelijk. De annotatie blijft bij afwijzing zichtbaar via Iced.
Een extra gecontroleerde PDF met verschoven CropBox en `/Rotate 90` toont
beide annotaties op een zichtbare 1100×1000-pagina: de volledige RGBA-uitvoer
is na Save As bytegelijk aan Hayro. Daarmee is de oude 1024px-grens voor
deze annotatieroute opgeheven. Dit bewijst nog geen algemene grote-pagina-
prestatie of subpixelplaatsing.

De opt-in app toont de DynLex-composiet als een gecachte pagina-afbeelding
wanneer de gebruiker DynLex heeft gekozen, de pagina op 100% staat, alle
native annotaties binnen deze bewezen subset vallen en er geen annotatie
geselecteerd of in tekstbewerking is. Een selectie schakelt terug naar het
bestaande Iced-canvas zodat slepen, formaatgrepen en inline tekstbewerking
beschikbaar blijven. De cache vergelijkt basisbitmap, pagina en de werkelijke
annotatie-inhoud; een edit maakt een nieuwe bitmap. De statusregel meldt
PDF- en annotatierenderer afzonderlijk. Een gerichte app-unit-test controleert
cachehergebruik en invalidatie na een annotatieverplaatsing. De 1200×832-
releaseapp is op een echte PDF geopend en als X11-venster vastgelegd; de
computergebruik-koppeling bood geen native app aan en synthetische XTest-
muisklikken veranderden het ribbon niet. Een betrouwbare end-to-end
muistest voor annotatie-aanmaken/selecteren ontbreekt daarom nog.

De PDF-importroute gebruikt deze compositor nog niet. Hierna moeten
geïmporteerde `/AP`-streams, annotaties
zonder `/AP`, tekst/free-text, meerdere kleurmodellen, transparantiegroepen,
rotatie/CropBox en subpixelplaatsing met echte PDF's worden geverifieerd.
De huidige app bevat nog geen PDFium-backend; Hayro blijft de fallback.

## Geïsoleerde native Text Box-appearance

`flatten_native_free_text_appearance` is nu een **read-only onderzoeksbrug**
in de DynLex-backend. Zij accepteert één door Open PDF Studio opgeslagen
`/FreeText`-annotatie met een ongecomprimeerde Form `/AP`, ingebed `/OPDS0`
Type0/Identity-H-font, één lege ongefilterde oorspronkelijke paginastream en geen
rotatie/CropBox. Onbekende appearance-operators, extra annotaties, andere
fonts en geërfde paginatransformaties worden geweigerd; ontbrekende glyphs
moeten door de opvolgende tekststaging worden afgewezen.
De bron-PDF blijft ongewijzigd; een tijdelijke kopie zet de Form-content op
de pagina met de annotatieverschuiving. Een oude `/Prev`-verwijzing uit een
incrementele brontrailer wordt bij deze volledige proefopslag verwijderd.

Een door de app opgeslagen gecentreerde blauwe Text Box met drie `Ω`-glyphs
uit ingebed Liberation Sans levert na deze normalisatie dezelfde **240.000
RGBA-bytes** als de oorspronkelijke `/AP` in Hayro. De bestaande
`stage_type0_text` leest drie TGR1-records uit de genormaliseerde PDF,
zonder overgeslagen tekstshows. De zelfstandige DynLex-glyphstripkernel
rendert een OPGS-scene met de oorspronkelijke glyphvolgorde en blauwe
8-bit-kleur. In het 200×192 zichtbare proefgebied zijn **153.600/153.600
RGBA-bytes** gelijk aan de werkelijke opgeslagen `/AP` in Hayro. De
read-only Vello-oracle is eveneens bytegelijk aan de DynLex-proef.
`stage_type0_text` bewaart hiervoor uitsluitend de canonieke 3-decimale
DeviceRGB-kleuren die de native Save As uit 8-bit RGB schrijft; overige
PDF-kleurprecisie wordt niet stilzwijgend afgerond.

Reproduceren vanuit `rust-studio`, na het bouwen van de zelfstandige
glyphstrip-probe volgens `docs/dynlex-text-stage-2026-10-05.md`:

```sh
OPDS_AP_FIXTURE_DIR=/tmp/opds-ap-stage cargo test --offline -p opds-native-app \
  --features dynlex-experimental native_text_appearance_can_be_staged_as_page_content
cargo build --release --offline -p opds-opdl-adapter --bin stage_type0_text
python3 dynlex-engine-poc/rgba/verify_native_text_appearance.py \
  --saved /tmp/opds-ap-stage/saved-freetext.pdf \
  --flattened /tmp/opds-ap-stage/flattened-freetext.pdf \
  --stage target/release/stage_type0_text \
  --dynlex /tmp/opds-glyph-run-probe \
  --hayro target/release/opds-engine-probe \
  --vello target/release/glyph_run_vello_oracle
```

Dezelfde OPGS-validatie en DynLex-glyphstrip zijn nu ook via
`opds_dynlex_backend::render_validated_glyph_scene` **in-process** beschikbaar
voor de opt-in Rust-backend. De proef-CLI gebruikt dezelfde gedeelde
Rust-code. Een vastgelegde 2480-byte OPGS-scene van de opgeslagen blauwe
`ΩΩΩ`-appearance levert in een backendtest exact de eerder onafhankelijk met
Hayro bevestigde 256×192-RGBA-hash; afgekorte scenes worden geweigerd.
De kernel wordt met de bestaande DynLex/LLVM-toolchain als statische library
meegebouwd, zonder extern renderproces in deze API.

Dit bewijst één geïsoleerde Unicode-tekstfixture, **niet** de gehele Text
Box in de normale DynLex-app. De glyphkernel is nu in de backend aanwezig,
en de opt-in app kan nu één niet-geselecteerde native Text Box **rechtstreeks
uit het actuele annotatiemodel** naar OPGS stagen. De begrensde route gebruikt
dezelfde Liberation Sans-fontbytes en tekstafbreking als Save As; maximaal
vier expliciete of automatisch afgebroken visuele regels met samen 2–32
ondersteunde zichtbare glyphs die zonder clipping in een 256×192-tegel
past, gehele paginacoördinaten, 100% zoom en een volledig witte bronpagina
zonder bestaande `/Annots` worden geaccepteerd. De Rust-route controleert
expliciet dat de zichtbare CropBox na de geërfde /Rotate 0/90/180/270 bij de
bitmap past en dat /UserUnit 1 is. De DynLex-kernel schrijft de glyphtegel in-process;
de app kopieert het zichtbare deel in de witte paginabitmap en houdt de
Iced-laag voor selectie, grepen en inline bewerking. Een 200×300-pagina met
een gecentreerde blauwe `ΩΩΩ`-Text Box is over de **hele pagina** RGBA-
bytegelijk aan de echte Save As-/AP-uitvoer in Hayro. De aanvullende proef
vergelijkt achttien echte Save As-PDF's met uiteenlopende letters, Unicode,
groottes, kleuren, uitlijningen, spaties, lege regels en zachte/expliciete
regelafbreking; ieder door de begrensde route geaccepteerd geval is bytegelijk.
Vier extra echte PDF's met geërfde verschoven MediaBox, CropBox en alle
kwartslagen zijn volledig RGBA-bytegelijk met de opgeslagen /AP in Hayro.
Glyphs met uitsluitend rechte contouren gebruiken dezelfde
1000-unit pure-Rust fontuitlezing als Hayro: de ruwe TrueType-coördinaten
gaven bij `Hello` nog 33 afwijkende kleurbytes en zijn daarom niet gebruikt.
Een tekst die clipping vraagt, ontbrekende glyph of een vorm buiten de tegel valt
terug zonder gedeeltelijke DynLex-bitmap. De opgeslagen scènebytes en de
modelscène verschillen op lage floatbits, maar hun gerenderde pixels zijn
gelijk. Niet-ondersteunde glyphcontouren, clipping en gemengde lagen
vereisen verdere pariteitstests; dit is nog geen volledige Text Box-dekking.
Een afzonderlijke DynLex OPGC-v1-kernel schildert gevalideerde glyphs nu
rechtstreeks in tekenvolgorde over de bestaande bitmap. Een zeer smalle
PDF-inhoudscontrole accepteert daarvoor uitsluitend één expliciete dekkende
`rg re f`-rechthoek die de hele zichtbare CropBox omvat. De gekleurde
200×300-proef is dan volledig bytegelijk aan een opgeslagen PDF-appearance;
hermenging van de premultiplied glyphlaag wijkt daar op 207 kleurkanalen af.
Een lege pagina gebruikt juist de eerder bewezen geïsoleerde glyphtegel:
direct schilderen over de zichtbare witte bitmap wijkt daar af omdat wit
buiten de PDF-pagina-inhoudsgroep ligt. Een bestaande annotatie of onzekere
ondergrond valt terug op Iced.

Een experimentele route gebruikt nu Hayro's **transparante paginabitmap**
als invoer voor een tweede modus van de DynLex OPGC-kernel. DynLex werkt
premultiplied RGB en alpha per glyph in tekenvolgorde bij; pas daarna wordt
de witte viewerachtergrond toegevoegd. Rust vergelijkt eerst de transparante
bron over wit bytegewijs met de getoonde bronbitmap. Zo kan een verkeerd of
verouderd renderresultaat niet als ondergrond dienen. De route accepteert
alleen één kleine ongefilterde contentstream, lege resources, maximaal 64
eenvoudige kleur-, rechthoek-, lijn- en fill/stroke-operators, geen bestaande
`/Annots`, 100% weergave en een pagina van maximaal 256 px breed. Eén
broncontentlaag wordt per pagina/bitmap gecacht en blijft bij
annotatiebewerkingen herbruikbaar. Lege en volledig dekkende bronnen blijven
op hun snellere eerder bewezen paden; andere graphics-state, fonts,
XObjects, clips en filters blijven fallback. Bij de eerste cachemiss rendert
Hayro de transparante bron nogmaals. Dit is dus **geen volledig DynLex-PDF-pad
en geen prestatieclaim**.

Echte Save As-/AP-vergelijkingen zijn bytegelijk voor een gedeeltelijk
gekleurde rechthoek, een rechthoek die de glyphs kruist, twee overlappende
vectorvlakken, een lijn/gevulde vorm via de app-cache en een 90°-pagina met
verschoven CropBox. De broncontent-alpha onderscheidt een lege pagina
(alpha 0), een volledig dekkende pagina (255) en gedeeltelijke vectorinhoud.
Voor grotere en complexere pagina's moet deze laag in het asynchrone
renderresultaat worden opgebouwd zodat een eerste annotatie geen extra
synchrone Hayro-render veroorzaakt. Volledige Text Box- en
annotatiedekking zijn nog niet bewezen.

Gerichte reproductie vanuit `rust-studio`:

```sh
cargo test --offline -p opds-native-app --features dynlex-experimental \
  dynlex_native_rectangle_and_highlight_match_saved_pdf_appearances
cargo test --offline -p opds-native-app --features dynlex-experimental \
  dynlex_annotation_overlay_tests
cargo test --offline -p opds-native-app --features dynlex-experimental \
  dynlex_highlight_over_colored_page_falls_back
```

De andere OPDL-v2/v4-verifiers moeten bij verdere wijzigingen blijven slagen.
Er is geen algemene PDF-objectdekking of prestatievoordeel bewezen.
