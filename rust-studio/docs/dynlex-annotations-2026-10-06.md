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
2px-rand. De native gele Highlight-appearance met `/ca 0.35` levert
premultiplied RGBA `[89,79,23,89]`. DynLex mengt die kleur met de
onderliggende pagina. Rust deelt het vlak voor Vello's 4×4-tegelgrenzen op
in maximaal negen niet-overlappende stroken: gedeeltelijke randtegels
gebruiken de gemaskeerde `/256`-afronding, de binnenkant de directe
source-over-afronding. De invoer is tot 4096 px per zijde,
6 miljoen pixels, 64 annotaties, 576 strookopdrachten en 13.844 OPAN-bytes
begrensd. De Rust-grens
controleert paginaformaat, ondoorzichtige bronpixels, gehele
rechthoekcoördinaten, grenzen en aantallen vóór de DynLex-ABI; de ABI
weigert null/te korte buffers en ongeldige totale lengtes.

Een gecontroleerde PDF vergelijkt de
DynLex-composiet rechtstreeks met Hayro na Save As: afzonderlijke rode Square,
afzonderlijke gele Highlight, twee gescheiden annotaties en overlappende
**dekkende Square**-appearances zijn volledig RGBA-bytegelijk. De dekkende
overlap is gecontroleerd op witte en gekleurde bronpagina's, beide
tekenvolgordes, een 90°/CropBox-pagina en 32 deterministische kleurparen.
Het experiment weigert tekst in deze OPAN-route en fractionele coördinaten
zonder een gedeeltelijke bitmap terug te geven. De nieuwe strokencompositie
vergelijkt **128 gecontroleerde PDF's** met een volledig dekkend gekleurd
bronvlak, wisselende Highlight-positie en -grootte, een tweede gekleurd
bronvlak in de helft van de gevallen, soms een tweede losse Highlight, en
periodiek `/Rotate 90` plus CropBox. Elke volledige RGBA-pagina is bytegelijk
aan Hayro na Save As. Voor een niet-witte ondergrond vraagt de app asynchroon
een transparante bronlaag op, gekoppeld aan dezelfde paginabitmap en hetzelfde
renderticket. DynLex wordt pas gebruikt wanneer **alle** pixels in het
Highlight-vlak in die bronlaag alpha 255 hebben en RGB-bytegewijs overeenkomen
met de zichtbare ondergrond. Deze extra render wordt uitsluitend voor de
begrensde PDF-subset met eenvoudige fills/strokes aangevraagd en daarna
gecacht. Een app-test controleert de overgang van fallback naar DynLex na
binnenkomst van die bronlaag. De bestaande gedeeltelijk gevulde
fill/stroke-fixture en een bron met `/ca 0.5` blijven fallback: de laatste
week zonder deze controle op 3.456 kanaalbytes af.
Een dekkende blauwe Square over gekleurde pagina-inhoud blijft eveneens
bytegelijk. Daarnaast zijn **64 gecontroleerde overlappende PDF's** met
Square/Highlight en Highlight/Highlight, wisselende kleuren, posities,
tekenvolgorde, meestal twee of drie vormen en 90°/CropBox over de hele pagina
bytegelijk aan Hayro na Save As. Een geïsoleerde `/AP`-proef verklaart
waarom een gewone RGBA-hermenging eerder acht randbytes miste: dezelfde
tussenlaagkleur kan op een gedeeltelijk bedekte tegel een andere
afrondingsweg doorlopen. De OPAN-stroken bewaren dat onderscheid in de
tekenvolgorde. Een extra 90°/CropBox-geval met 64 grotendeels overlappende
Square/Highlight-vormen bleef eveneens volledig bytegelijk. Dit bewijst
geen algemene PDF-transparantie of willekeurige
appearance-overlap; bij afwijzing blijft de annotatie via Iced zichtbaar.
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
de witte viewerachtergrond toegevoegd. Bij een afzonderlijke bronlaagtaak
vergelijkt Rust de transparante bron over wit bytegewijs met de getoonde
bronbitmap, zodat een verkeerd of verouderd resultaat niet als ondergrond
dient. De route accepteert
alleen één kleine ongefilterde contentstream, lege resources, maximaal 64
eenvoudige kleur-, rechthoek-, lijn- en fill/stroke-operators, geen bestaande
`/Annots`, 100% weergave en een pagina van maximaal 256 px breed. Eén
broncontentlaag wordt per pagina/bitmap gecacht en blijft bij
annotatiebewerkingen herbruikbaar. Lege en volledig dekkende bronnen blijven
op hun snellere eerder bewezen paden; andere graphics-state, fonts,
XObjects, clips en filters blijven fallback. Bestaat een Text Box al bij een
nieuwe paginarender, dan levert één asynchrone transparante Hayro-render zowel
de premultiplied bronlaag als de zichtbare pagina: Rust legt de witte
papierachtergrond pas na het renderen onder de bronlaag. Dit is op de
gecontroleerde gemengde vector-PDF bytegelijk aan de afzonderlijke witte
Hayro-render en aan de opgeslagen Text Box-appearance. De paginacache bewaart
beide lagen onder dezelfde geheugenlimiet. Wordt een Text Box pas ná de
zichtbare paginarender toegevoegd, dan start een afzonderlijke asynchrone
bronlaagtaak; totdat die klaar is blijft de Iced-laag zichtbaar. De taak
draagt documenttoken, backendgeneratie, renderticket en basisbitmap mee.
Verouderde resultaten worden geweigerd en de in-flight-status wordt bij een
nieuwe bitmap of backendwissel gewist. De bron wordt in dit latere geval nog
een tweede keer gerenderd. Dit is **geen volledig DynLex-PDF-pad en geen
algemene prestatieclaim**.

Echte Save As-/AP-vergelijkingen zijn bytegelijk voor een gedeeltelijk
gekleurde rechthoek, een rechthoek die de glyphs kruist, twee overlappende
vectorvlakken, een lijn/gevulde vorm via de app-cache en een 90°-pagina met
verschoven CropBox. De broncontent-alpha onderscheidt een lege pagina
(alpha 0), een volledig dekkende pagina (255) en gedeeltelijke vectorinhoud.
De app kan nu daarnaast precies één Text Box samen met meerdere native
vormannotaties compositeren: DynLex tekent eerst de tekst
op de bewezen bronlaag en vervolgens de vormen. Een echte gemengde PDF met
Text Box, blauwe Square en gele Highlight is over de hele pagina bytegelijk
aan Hayro na Save As. Square- en Highlight-vormen mogen elkaar onder de
bovenstaande grenzen overlappen. Voor een gecontroleerde bronpagina die
volledig door één dekkende PDF-fill is geverfd, kan nu ook één Text Box met
Square of Highlight overlappen. DynLex schildert de vormen vóór en na de
glyphrun in de echte annotatie-/ID-volgorde. Bij een latere Highlight
verifieert een nieuwe Rust-API de **oorspronkelijke** volledig dekkende
PDF-bronlaag; zij vergelijkt die dus niet foutief met de reeds door glyphs
of vormen gewijzigde bitmap. Een geïsoleerde Save As-PDF met blauwe `ΩΩΩ`
en kruisende rode Square of gele Highlight is in beide volgordes volledig
RGBA-bytegelijk aan Hayro. De app-cachetest bewijst ook vorm-vorm-tekst en
tekst-vorm-vorm in beide volgordes op een gekleurde 256×192-pagina. Een
verouderde of deels transparante bronlaag wordt geweigerd voor de dekkende route.
OPAN-v2 kan nu dezelfde vormopdrachten op een gevalideerde premultiplied
PDF-bronlaag compositeren en werkt de alfawaarde per vorm bij. De app accepteert
hiervoor een bewezen lege pagina of een kleine gedeeltelijk dekkende PDF met
een asynchroon geladen content layer die de zichtbare pagina byte voor byte
reconstrueert. Text Box en Square/Highlight worden vóór of na elkaar in de
annotatievolgorde getekend; pas daarna wordt wit papier toegevoegd. Echte
Save As-proeven voor leeg en gedeeltelijk gevuld, beide vormkleuren en beide
volgordes zijn RGBA-bytegelijk aan Hayro. De app-cachetest controleert ook
Highlight vóór en na Text Box op een gedeeltelijk geverfde PDF, met behoud van
de bestaande stale-result-afwijzing. Niet-gevalideerde contentlagen blijven
Iced-fallback. De annotaties moeten ook ieder afzonderlijk binnen hun bestaande
bewezen rendergrenzen vallen. Voor grotere en complexere pagina's en
achteraf toegevoegde Text Boxes zijn bredere bronlaagstaging en
cache-integratie nodig. Volledige Text Box- en annotatiedekking zijn nog
niet bewezen.

Gerichte reproductie vanuit `rust-studio`:

```sh
cargo test --offline -p opds-native-app --features dynlex-experimental \
  dynlex_native_rectangle_and_highlight_match_saved_pdf_appearances
cargo test --offline -p opds-native-app --features dynlex-experimental \
  dynlex_annotation_overlay_tests
cargo test --offline -p opds-native-app --features dynlex-experimental \
  dynlex_highlight_premultiplied_composition_matches_colored_and_layered_pages
cargo test --offline -p opds-native-app --features dynlex-experimental \
  dynlex_overlapping_highlights_and_squares_match_saved_appearances
cargo test --offline -p opds-native-app --features dynlex-experimental \
  colored_highlight_waits_for_opaque_content_proof
cargo test --offline -p opds-native-app --features dynlex-experimental \
  dynlex_glyph_and_shape_order_matches_saved_appearances
cargo test --offline -p opds-native-app --features dynlex-experimental \
  dynlex_premultiplied_text_and_shape_order_matches_saved_appearances
```

De andere OPDL-v2/v4-verifiers moeten bij verdere wijzigingen blijven slagen.
Er is geen algemene PDF-objectdekking of prestatievoordeel bewezen.
