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
native annotaties binnen deze bewezen subset vallen en er geen tekstbewerking
loopt. Bij een geselecteerde pixeluitgelijnde Rectangle/Highlight met even breedte en
hoogte tekent een tweede begrensde OPAN-pass ook het blauwe kader en de acht
6×6-formaatgrepen via DynLex. De app bewaart alleen het geselecteerde
eindbeeld in de annotatiecache. Bij mouse-down op een annotatie of greep
schakelt zij tijdelijk naar het Iced-canvas voor de levende verplaats- of
vergrotingspreview; mouse-up maakt de DynLex-selectie opnieuw. Andere zoom,
fractionele coördinaten, oneven afmetingen en Text Box-selectie/-bewerking blijven
veilig bij Iced. De cache vergelijkt basisbitmap, pagina en de werkelijke
annotatie-inhoud; een edit maakt een nieuwe bitmap en verwijdert het oude
selectiebeeld. De statusregel meldt PDF- en annotatierenderer afzonderlijk.
Voor deze selectie gebruikt een afzonderlijke OPAN-v3-scène een 1,5px-rand:
beide randpixels krijgen 191/255-dekking. Drie gevalideerde 256-byte-tabellen
vertalen de bestaande sRGB-pixels naar de lineair gemengde blauwe rand; DynLex
kiest de tabelwaarden per pixel. Alleen deze selectie-scène heeft de extra
768 bytes, en de C-ABI controleert versie en exacte scenelengte. Een
geselecteerde rode Rectangle krijgt uitsluitend in de viewerpreview een
1px naar buiten verplaatst 2px-vormkader, zodat hij onder de selectie dezelfde
positie heeft als in Iced. Het model en de printbare PDF-appearance veranderen
niet. Bij een geïsoleerde 60×40-rechthoek in het 1200×832-releasevenster waren
alle **44.928 zichtbare paginapixels** na uitlijnen van de vensters exact
gelijk tussen DynLex-selectie en Iced. Dit geldt nog niet voor geselecteerde
Highlights: de PDF-getrouwe DynLex-Highlight en de Iced-preview gebruiken
verschillende compositie, wat op een gecontroleerde witte pagina 2.084
pixelverschillen opleverde, ook doordat een tweede niet-geselecteerde Square
in de Iced-preview een anders gepositioneerde rand heeft. Dit is een gemeten
UI-limiet, geen verschil in Save As-PDF-pariteit. Selecties die een rand over
de paginagrens zouden tekenen blijven op Iced.

De 1200×832-releaseapp is met echte XTest-muisinvoer getest op
`simple-fill-stroke.pdf`: rechthoek tekenen, een 40×30-rect selecteren,
10 px verplaatsen en de zuidoostgreep 10×10 px vergroten. De status wisselde
bij beide gestes van `Annotations: DynLex` naar `Annotations: Iced` en na
mouse-up terug naar DynLex, zonder oude selectiebitmap. Een 57×49-rect
bleef correct op Iced. Ctrl+scroll zoomde 100→132→100%, met Hayro/Iced-
fallback tijdens de afwijkende zoom. Screenshots staan tijdelijk onder
`/tmp/opds-dynlex-{even-selected,moving,moved,resizing,resized,ctrl-scroll-in,ctrl-scroll-back}.png`.
De latere directe selectiepariteit staat tijdelijk in
`/tmp/opds-selection-aa2-{dynlex,iced}.png`; deze vergelijking corrigeert de
512px horizontale verschuiving die de bestaande backendwissel in het venster
veroorzaakte.
De Highlight-vergelijking staat tijdelijk in
`/tmp/opds-selection-highlight-{dynlex,iced}.png`; de volgende stap is een
afzonderlijke UI-kleurmenging voor Highlights en gecentreerde strokepreview
voor overige rechthoeken, terwijl Save As de bestaande PDF-appearance houdt.
Een gerichte app-unit-test controleert selectiecache en invalidatie na
verplaatsing. De native computergebruik-koppeling bood geen vensters aan;
deze UI-proef gebruikte XTest rechtstreeks op het X11-venster.

Een bestaande normale Form-`/AP` kan nu als **ondergrond** voor nieuwe native
DynLex-annotaties dienen. De classifier accepteert hoogstens 64 bestaande
annotaties en vereist bij elk een `/AP/N`-stream; de eenvoudige
pagina-content en resourcegrenzen blijven gelden. De app gebruikt alleen de
premultiplied route nadat Hayro's transparante pagina **inclusief bestaande
appearances** de zichtbare witte pagina byte voor byte reconstrueert. Een
90°/CropBox-proef met een bestaande rode Square-`/AP`, twee nieuwe brede
Unicode-Text Boxes en een Highlight is na Save As over de volledige RGBA-pagina
bytegelijk aan Hayro. Een lege bronpagina met bestaande Square-`/AP` en nieuwe
Text Boxes/Highlight is eveneens bytegelijk. Dezelfde bron zonder `/AP` wordt
afgewezen, omdat de
previewfallback niet in de transparante bronlaag zit. De bestaande `/AP`
wordt hier **nog door Hayro**, niet DynLex, gerenderd; dit is dus een bewezen
laagvolgorde-/overgangspad en geen voltooide DynLex-importrenderer. Directe
Algemene DynLex-rendering van geïmporteerde `/AP`, annotaties zonder `/AP`, meer
kleurmodellen, transparantiegroepen en subpixelplaatsing blijven open.
De huidige app bevat nog geen PDFium-backend; Hayro blijft de fallback.

Er is daarnaast nu een **afzonderlijke read-only proef**
`render_single_square_appearance_direct` voor precies één geïmporteerde
`/Square` met een door de native Save As geschreven, ongefilterde dekkende
2pt-Form-appearance. Zij controleert de operatorvolgorde, kleur, BBox en
kwartslagmatrix, laat Hayro de al geparste pagina zonder annotaties transparant
tekenen en zet de Square via de DynLex
OPAN-kernel erboven. Het gehele resultaat moet bytegelijk zijn aan Hayro's
oorspronkelijke pagina **inclusief** appearance voordat de proef een bitmap
teruggeeft. Op gewone en verschoven/gecropte pagina's met `/Rotate`
0/90/180/270 zijn de resultaten volledig gelijk. Een onbekende appearance
of misvormde rotatie wordt geweigerd. De pariteitsoracle van deze read-only
proef is geen onderdeel van de app-router en de route is geen algemene
`/AP`-interpretatie: Hayro blijft voor de
pagina-inhoud én als pariteitsoracle nodig. Ook dit is dus nog geen volledige
DynLex-annotatielaag.

Dezelfde read-only route accepteert nu ook **tot 64** van deze gevalideerde
Square-appearances in de volgorde van de PDF-`/Annots`-array. Drie gekleurde
Squares, deels overlappend boven een gekleurde pagina, zijn in beide
tekenvolgordes en op alle vier kwartslagen met verschoven CropBox over de
volledige pagina bytegelijk aan Hayro. Eén ontbrekende `/AP` in de reeks
weigert de gehele proef; de oude enkelvoudige API weigert meerdere
annotaties. De read-only API gebruikt twee Hayro-renders (kale pagina en
oracle); hij blijft een correctheidsproef, geen prestatiepad.

`render_native_shape_appearances_direct` laat in dezelfde proef ook de
native `/Highlight`-Form met gevalideerde `/GS0`-opacity 0,35,
`/QuadPoints`, kleur en vulpad toe, gemengd met Squares in de PDF-volgorde.
Voor gekleurde pagina-inhoud controleert de OPAN-compositor Hayro's
transparante contentlaag en eist volledige dekking onder de Highlight.
Geïsoleerde Highlight, beide volgordes Square/Highlight en twee deels
overlappende Highlights op een gekleurde bron zijn op gewone en 90°/CropBox-
pagina's volledig RGBA-bytegelijk aan de opgeslagen PDF in Hayro. Onbekende
appearance-resources of gewijzigde grafische staat worden geweigerd. De
oude Square-only-API blijft Highlights weigeren. Dit is geen algemene
transparantiegroep- of annotation-support.

De opt-in app-router gebruikt nu dezelfde strikte parsing via
`render_native_shape_appearances_for_app` op **100% zoom**. Hij verwijdert de
gevalideerde `/Annots` niet uit de bron-PDF: Hayro rendert de al geparste
pagina eenmaal transparant met `render_annotations=false`. Vervolgens tekent
DynLex de geïmporteerde Square/Highlight-appearances met OPAN en wordt wit
papier toegevoegd.
Er is hierbij geen tweede Hayro-oracle per app-render; die blijft wel in de
read-only regressieproef. Op de acht gekleurde/geroteerde Square-PDF's en de
acht Square/Highlight-PDF's is de app-route over de hele pagina bytegelijk
aan Hayro's opgeslagen appearance-render; beschadigde `/QuadPoints` vallen
terug. De statusregel onderscheidt PDF-inhoud (Hayro) en geïmporteerde
appearances (DynLex) van de eventuele bewerkbare native annotaties. Dit pad
leest de annotatiestaat met lopdf bij een niet-gecachete pagina, maar herparst
de pagina-inhoud niet. Op één gecontroleerde 300×400-fixture waren 30
afwisselende warme procesparen bytegelijk. Met dezelfde `/usr/bin/time`-wrapper
voor beide processen was de mediaan voor geselecteerde route/Hayro
9,988/9,216 ms, het gepaarde mediaanverschil +0,973 ms en won de geselecteerde
route 6/30 keer. De mediane piek-RSS was 7.268/7.160 KiB. De OS-cache is niet
gewist en dit is geen algemene prestatiewaarde. Reproduceren met
`dynlex-engine-poc/rgba/benchmark_native_backend.py --selected`; de
fixture ontstaat uit de app-test met `OPDS_IMPORTED_AP_FIXTURE_DIR`. Andere
schalen en niet-bewezen `/AP`-vormen blijven veilig Hayro.

De opt-in router kan nu ook **geïmporteerde Square en Highlight zonder `/AP`**
als een geordende OPAN-lijst tekenen. Dit volgt de bestaande
`engine-probe`-previewfallback: een Square wordt als dekkende rand met
expliciete 1–20px breedte getekend, een rechthoekige Highlight als dekkende
vulling. Het zijn previewpixels; Save As en Print blijven de oorspronkelijke
annotatiedictionaries behouden. Rust valideert de zichtbare CropBox,
kwartslagrotatie, gehele rechthoek- en quadcoördinaten, kleuren en
annotatievolgorde voordat de DynLex-ABI wordt aangeroepen. Twaalf echte
PDF-fixtures (vier rotaties × drie lijnbreedtes) met overlappende Square en
Highlight op een blauwe paginabron zijn over de volledige pagina bytegelijk
aan de bestaande Hayro-preview. Een schuine quad, gemengde `/AP`- en
`/AP`-loze reeks, fractionele plaatsing en andere annotatietypen vallen nog
terug. De statusregel zegt nu `Imported: DynLex`, ongeacht of de geïmporteerde
vorm een `/AP` heeft. Dit is geen volledige PDF-annotatie-interpretatie en
bewijst geen snelheidswinst. Op de 90°/CropBox-fixture met 3px-Square waren
30 gepaarde warme procesruns inclusief dezelfde `/usr/bin/time`-wrapper
bytegelijk: geselecteerde route/Hayro 7,257/6,823 ms mediaan, gepaard
verschil +0,395 ms, geselecteerde route 10/30 keer sneller. Mediane piek-RSS
was 6.500/6.140 KiB. De OS-cache is niet gewist; het is één kleine fixture.

Een gemengde pagina kan nu native Square/Highlight-`/AP`-Forms en de
begrensde `/AP`-loze previewvormen in **één OPAN-commandolijst en één
DynLex-kernelaanroep** tekenen. De parser valideert eerst alle annotaties;
één ongeldige dictionary maakt de hele route onbruikbaar. De bestaande
preview schildert eerst alle Form-appearances en daarna zijn fallbackvormen,
ook wanneer hun entries in `/Annots` zijn verweven. De gemengde DynLex-route
neemt precies die zichtbare volgorde over en bewaart de onderlinge volgorde
binnen beide groepen. Acht gecontroleerde PDF's (vier kwartslagen × beide
`/Annots`-volgordes) met gekleurde bron, dekkende en transparante overlap
zijn over de volledige RGBA-pagina bytegelijk aan de bestaande preview.
Een beschadigde Highlight-quad dwingt volledige Hayro-fallback af. Dit is
nog geen algemene PDF-annotatievolgorde of willekeurige Form-rendering.
De app-router gebruikt inmiddels deze gezamenlijke parser ook voor pagina's
met alleen `/AP` of alleen `/AP`-loze vormen, zodat hij per niet-gecachete
pagina niet eerst drie afzonderlijke lopdf-parses hoeft te proberen.
Op de gemengde 90°/CropBox-fixture zijn 30 afwisselende warme procesparen met
dezelfde `/usr/bin/time`-wrapper en volledige bytecontrole gemeten:
geselecteerde route/Hayro 13,071/11,957 ms mediaan, gepaard verschil
+1,267 ms, geselecteerde route 8/30 keer sneller. Piek-RSS was
7.218/7.064 KiB mediaan. Een diagnostische A/B tegen de voorafgaande
drie-parser-route op exact dezelfde PDF gaf -0,855 ms gepaard mediaanverschil
voor de gezamenlijke parser (21/30 keer sneller), maar de totale route is
nog niet sneller dan Hayro. De OS-cache is niet gewist; één kleine fixture
bewijst geen algemene prestatie.

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
De app kan native Text Boxes en Square/Highlight-vormen in de echte
annotatie-/ID-volgorde compositeren. Opeenvolgende vormen worden samen als
één OPAN-batch getekend; elke Text Box krijgt zijn eigen begrensde OPGS-run.
Tot 64 annotaties zijn toegestaan, zolang iedere individuele glyphrun en
vorm binnen de bewezen rendergrenzen past. Een volledig dekkende PDF-fill,
een bewezen lege pagina of een kleine gedeeltelijk dekkende bronpagina zijn
geschikt. Square- en Highlight-vormen en Text Boxes mogen elkaar overlappen.
Bij een latere Highlight op een volledig dekkende bron
verifieert een nieuwe Rust-API de **oorspronkelijke** volledig dekkende
PDF-bronlaag; zij vergelijkt die dus niet foutief met de reeds door glyphs
of vormen gewijzigde bitmap. Een geïsoleerde Save As-PDF met blauwe `ΩΩΩ`
en kruisende rode Square of gele Highlight is in beide volgordes volledig
RGBA-bytegelijk aan Hayro. App-tests bewijzen ook twee overlappende Text Boxes
met verschillende kleur, grootte en uitlijning, plus vormen ertussen: op
dekkende, lege en gedeeltelijk geverfde pagina's volledig bytegelijk aan
Save As/Hayro. Een afzonderlijke PDF-proef met 90°-rotatie en verschoven
CropBox vergelijkt twee Text Boxes met een Highlight ertussen eveneens
bytegelijk. Een
verouderde of deels transparante bronlaag wordt geweigerd voor de dekkende route.
OPAN-v2 kan nu dezelfde vormopdrachten op een gevalideerde premultiplied
PDF-bronlaag compositeren en werkt de alfawaarde per vorm bij. De app accepteert
hiervoor een bewezen lege pagina of een kleine gedeeltelijk dekkende PDF met
een asynchroon geladen content layer die de zichtbare pagina byte voor byte
reconstrueert. Text Boxes en Square/Highlight worden vóór of na elkaar in de
annotatievolgorde getekend; pas daarna wordt wit papier toegevoegd. Echte
Save As-proeven voor leeg en gedeeltelijk gevuld, beide vormkleuren en beide
volgordes zijn RGBA-bytegelijk aan Hayro. De app-cachetest controleert ook
Highlight vóór en na Text Box op een gedeeltelijk geverfde PDF, met behoud van
de bestaande stale-result-afwijzing. Niet-gevalideerde contentlagen blijven
Iced-fallback.

**Verplaatsbare glyphtegel (OPGC-v2).** Een afzonderlijke Text Box mag nu op
een andere plek dan de linkerbovenhoek staan. Rust stageert haar glyphs in een
lokale tegel van maximaal 256×192 pixels; OPGC-v2 draagt de gehele
pagina-oorsprong van die tegel mee aan DynLex. De oude OPGC-v1-records blijven
geldig. De pagina mag maximaal 4096 pixels per zijde en 6 miljoen pixels
hebben, mits de PDF-bronlaag afzonderlijk bytegewijs is gevalideerd. De
schermcoördinaten worden vóór het aftrekken van de tegeloorsprong naar `f32`
afgerond; anders gaf een 90°/CropBox-proef één afwijkende 8-bit-kleurbyte.
De annotaties blijven in ID-/laagvolgorde getekend. De staging verdeelt een
Text Box die breder of hoger is dan één tegel op glyphgrenzen over opeenvolgende
OPGS-runs. Iedere glyph blijft heel en de tekenvolgorde blijft gelijk; een
glyph die zelf niet in 256×192 past, valt terug. De compositor ontvangt
steeds de volledige Text Box als clip, maar toetst ook de daadwerkelijke
glyphdekking binnen dat vak. OPGS met één glyph is daarom geldig. De
app-route geeft de paginabuffer aan de volgende tegel door zonder per tegel
een volledige RGBA-kopie te maken.

**Heropende native FreeText-`/AP`.** De opt-in backend leest opgeslagen
Open PDF Studio-FreeText-annotaties rechtstreeks uit de PDF en stageert hun
ingebedde Type0/Identity-H-Liberation-Sans-glyphs als OPGS. De Rust-grens
controleert de exacte FontFile2-bytes, CID-breedtes, Form-BBox, clip,
operatorvolgorde, grafische toestand en glyphgrenzen vóór de in-process
DynLex-aanroep. De pagina-inhoud komt als transparante Hayro-laag binnen;
DynLex schildert de glyphs daarop en daarna wordt het witte papier toegevoegd.
Er wordt geen tijdelijke afgevlakte PDF geschreven. Zestien opgeslagen
FreeText-pagina's met Unicode, spaties, meerdere regels en links/midden/rechts
op gewone, 0/90/180/270°-gecropte en een A1-achtige pagina zijn volledig
RGBA-bytegelijk aan Hayro. Een onbekende annotatie-state of extra Form-operator
valt terug zonder gedeeltelijke DynLex-tekening. De route verdeelt een
opgeslagen FreeText nu in maximaal zestien geordende 256×192-glyphtegels,
met maximaal 32 glyphs per tegel en 128 per annotatie. Alle glyphs en tegels
worden vóór het tekenen gevalideerd; het exacte ingebedde font blijft vereist.
Acht extra volledige PDF-proeven met een brede run en een hoge negenregelige
Text Box, op gewone en 90°/CropBox-pagina's, zijn RGBA-bytegelijk aan Hayro.
Daarbij zijn beide laagvolgordes met een Square over de brede tekst getest.
Twee gevallen met 150 zichtbare glyphs vallen volledig op Hayro terug.

De geïmporteerde annotatieroute parseert de PDF nu eenmaal, stageert alle
ondersteunde `/AP`-Forms vóór het tekenen en verwerkt FreeText, Square,
Highlight en de begrensde dekkende Stamp in PDF-volgorde. Opeenvolgende
vormopdrachten delen een OPAN-aanroep; een FreeText-run onderbreekt de batch
en gebruikt OPGS zonder een volledige paginakopie. Square/Highlight zonder
`/AP` blijven, net als in Hayro's preview, na de Forms staan. Twaalf extra
Save As-/PDF-mengproeven met twee overlappende teksten, kruisende vormen,
Stamp, een ontbrekende Square-`/AP`, 90°/CropBox en gekleurde onderlaag zijn
volledig RGBA-bytegelijk aan Hayro. Onbekende annotatie-inhoud laat de hele
pagina veilig op Hayro terugvallen. Algemene tekstfonts, grotere of gesplitste
geïmporteerde glyphruns boven deze grenzen en overige `/AP`-inhoud zijn nog
niet ondersteund.

Een kleine diagnostische, afwisselend gepaarde releaseproef op de gemengde
90°/CropBox-pagina met twee FreeText-Forms en twee vormen gaf vóór de
glyphbegrenzing bij twintig warme in-process renders een mediaan van
**4,74 ms** voor Hayro-paginainhoud plus DynLex-annotaties tegen **0,41 ms**
voor Hayro's volledige pagina. Profielmetingen wezen naar de glyphpaintfase,
niet naar PDF-parsing. De DynLex-stripkernel wist daarom eerst zijn volledige
uitvoerbuffer en doorloopt vervolgens alleen 4×4-tegels rond de vooraf door
Rust gevalideerde glyphgrenzen, met één tegel marge. Een uit dezelfde native
Unicode-fixture geëxtraheerd OPM2-glyph gaf in dertig afwisselende A/B-paren,
elk met honderd herhaalde calls per proces, **531,15→72,25 µs** mediaan per
oude/nieuwe kernelcall; nieuw won 30/30 paren. Vijfhonderd deterministische
convexe OPM2-omtrekken en vijfhonderd geneste contouren met gaten leverden
exact dezelfde 49.152 coveragebytes bij beide kernels. Op de volledige
gemengde PDF-proef zonder profieluitvoer werd de nieuwe route **1,54 ms**
tegen **0,35 ms** Hayro-mediaan (twintig warme paren): nog steeds trager.
De oude en nieuwe volledige-routeproeven zijn niet rechtstreeks gepaard;
OS-cache en `ParsedPdf` bleven warm. Dit is geen algemene prestatie- of
RSS-meting. De ruwe gepaarde kernelmetingen en maximale proces-RSS staan in
`dynlex-engine-poc/rgba/glyph-strip-tile-bounds-bench-2026-10-06.json`;
de invoer staat in `rgba/fixtures/glyph-strip-tile-bounds.opm`; de oude
kernelbron is die van commit `4ea5f67e`. `bench_glyph_multi_bbox.py` voert de
afwisselende probeparen uit. Reproduceer
de volledige route met
`OPDS_BENCH_IMPORTED_ANNOTATIONS=1 cargo test --release --offline -p
opds-native-app --features dynlex-experimental
reimported_native_freetext_appearance_uses_dynlex_glyphs -- --nocapture`.

Negen volledige Save As-/Hayro-proeven zijn RGBA-bytegelijk:
595×842 met twee posities, 842×595 met 90° en verschoven CropBox op twee
posities, een synthetische A1-achtige 2384×1684/90°-pagina, twee brede
Text Boxes over meerdere tegels (waarvan één 90°/CropBox) en een hoge
vierregelige 72pt-Text Box over meerdere tegelrijen, en een lege bronpagina
met bestaande Square-`/AP`. Iedere proef bevat
Unicode-Text Boxes en een Highlight in annotatievolgorde. Het tekstpad
beperkt invoer nog tot 64 tekens, vier regels, 64 zichtbare glyphs en de
bewezen ingebedde fontglyphs; complexe bron-PDF's en de echte technische
A1-tekening blijven Iced/Hayro-fallback. Volledige Text Box- en
annotatiedekking zijn nog niet bewezen.

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
cargo test --offline -p opds-native-app --features dynlex-experimental \
  large_page_tiled_text_boxes_match_saved_pdf
```

De andere OPDL-v2/v4-verifiers moeten bij verdere wijzigingen blijven slagen.
Er is geen algemene PDF-objectdekking of prestatievoordeel bewezen.

**Geïmporteerde dekkende Stamp-Form.** De opt-in DynLex-annotatieroute
accepteert nu ook een `/Stamp` met precies één ongefilterde `/AP/N`-Form:
geen resources, Matrix of Group, een BBox gelijk aan de gehele `/Rect`, en
uitsluitend `q rg re f Q` met een pixeluitgelijnde, volledig dekkende
DeviceRGB-rechthoek. De Form wordt als OPAN-v2-vulopdracht in dezelfde
gevalideerde annotatiescene gezet als Square/Highlight. Een echte PDF-proef
vergelijkt de volledige pagina op 0/90/180/270 graden en verschoven CropBox
RGBA-bytegewijs met Hayro; op 0 graden is ook Stamp gevolgd door Square in
annotatievolgorde gecontroleerd. Een extra tekenoperator veroorzaakt veilige
Hayro-fallback zonder gedeeltelijke DynLex-uitvoer. Dit ondersteunt nog geen
algemene Stamp-inhoud of tekst-/afbeeldings-Form.

**Gevalideerde meervoudige Form-vullingen.** Dezelfde geïmporteerde route
interpreteert nu een ongefilterde `/AP/N`-Form met één tot 32 opeenvolgende,
expliciet gekleurde dekkende DeviceRGB-`rg re f`-vullingen in één `q`/`Q`.
Elke vulling moet een gehele rechthoek binnen de Form-BBox zijn; de BBox
moet exact op de annotatie-`/Rect` passen. Een expliciete identiteits-`/Matrix`,
lege `/Resources` en `/FormType 1` zijn toegestaan. De Form-inhoud bepaalt de pixels,
onafhankelijk van het annotatiesubtype. Op 0/90/180/270°-pagina's met
verschoven CropBox zijn `Stamp`, `Circle`, `Ink` en `Widget` met twee
overlappende vullingen, plus een overlappende `Underline`-Form in beide
annotatievolgordes, over de volledige RGBA-pagina bytegelijk aan Hayro
(32 gecontroleerde PDF-uitvoeren). De route weigert `/OC`/`/AS`, een
vulling buiten de BBox, meer dan 32 vullingen, extra Form-operators en
onbekende resources. Algemene `/AP`-paden, transparantie, XObjects en
interactieve annotatiebewerking blijven open; dit is geen volledige
subtype- of Form-dekking.

**Gemengde Form-vulling en rand.** Dezelfde begrensde Form-parser herkent
naast `rg re f` nu ook een expliciete dekkende DeviceRGB-rand met
`RG 2 w re S`. Het 2pt-pad moet gehele coördinaten hebben, rondom ten
minste één pixel ruimte binnen de Form-BBox laten en minstens 2×2 pt zijn.
De DynLex-OPAN-scene bewaart de volgorde van maximaal 32 vullingen en randen;
een onbekende operator, andere lijndikte of pad buiten de BBox laat de hele
annotatiepagina op Hayro terugvallen. Acht gemengde fill/stroke-PDF's met
beide tekenvolgordes en 0/90/180/270° zijn volledig RGBA-bytegelijk aan
Hayro. Zestien extra PDF's controleren randen van 2×2 tot 38×28 pt op
dezelfde rotaties en een gekleurde onderlaag. Dit blijft een exacte 2pt-
rechthoekdeelset, geen algemene PDF-stroker of volledige annotatielaag.

**Begrensde Form-grafische toestand.** De parser bewaart nu afzonderlijke
DeviceRGB-vul- en lijnkleur en een expliciete 2pt-lijndikte, en herstelt die
over maximaal vier `q/Q`-niveaus. Meerdere `re`-paden mogen dezelfde staat
hergebruiken. `B` schildert eerst de vulling en dan de rand; voor een enkel
rechthoekpad zijn ook `F`, `f*`, `s`, `B*`, `b` en `b*` pixelgewijs bewezen.
Acht PDF-uitvoeren met staat-hergebruik, genest herstel en gecombineerd
schilderen plus 24 uitvoeringen van de equivalente rechthoekoperators zijn
op 0/90/180/270° volledig RGBA-bytegelijk aan Hayro. De inhoud blijft
maximaal 128 operators, 32 tekenopdrachten en 4096 Form-bytes; ontbrekende
kleur, een onafgemaakt pad, verkeerde nesting of onbekende operator laat de
hele pagina op Hayro terugvallen. Clipping, transparantie, afbeeldingen,
tekst en algemene vectorpaden in een Form zijn nog niet ondersteund.

**Eerste gevulde Form-paden.** Dezelfde opt-in importroute kan nu ook een
enkel `m/l/.../h f`-pad in een ongefilterde `/AP/N`-Form herkennen; `f` sluit
het pad ook impliciet en `F`/`f*` zijn voor één eenvoudige contour gelijk.
Rust valideert de volledige Form, lokale punten, BBox, maximaal 64 punten,
kwartpixelcoördinaten en één 256×192-tegel voordat een bestaande zelfstandige
DynLex-analytische-oppervlaktekernel het pad tekent. De padkleur is expliciet
DeviceRGB; de getekende volgorde met rechthoekvullingen en -randen blijft
behouden. Het resultaat gaat op een bestaande premultiplied paginalaag en
werkt op de vier kwartslagen met verschoven CropBox. Een onzekere rand ligt
dicht bij een halve 8-bit-dekkingsstap: zo'n pad wordt vóór enig schilderen
afgewezen, waarna de hele pagina de Hayro-route gebruikt. In 256
deterministische willekeurige driehoeken met gehele punten waren 39
geaccepteerde gevallen volledig bytegelijk aan Hayro; op een kwartpixelgrid
waren 234 van 256 geaccepteerde gevallen volledig bytegelijk. De andere
gevallen vielen terug. Gerichte proeven bevestigen ook laagvolgorde,
expliciete/impliciete sluiting, rotaties en foutafwijzing van o.a.
zelfkruising, onvolledige paden, curveoperatoren, stroke,
punten buiten de Form en coördinaten buiten het kwartpixelgrid. Dit is een
nauwe deelset en geen volledige vector- of annotatielaag. Een onafhankelijke
implementatie van Hayro/Vello's f32-randdekking blijft nodig om de
conservatieve terugval te verkleinen; deze route heeft nog geen
prestatievoordeel aangetoond.

**Begrensde Form-contourhiërarchie.** Dezelfde versieerbare OPM2-kernel
accepteert nu maximaal acht eenvoudige, niet-rakende `m/l/h`-contouren met
samen maximaal 256 punten inclusief sluitpunten in één `f`- of
`f*`-opdracht. De parser bewaart expliciete contourgrenzen en valideert
zelfkruising en onderlinge aanraking vóór vereenvoudiging. Een
containmentboom bepaalt per contour de ouder, vuldiepte en cumulatieve
winding. Onder `f*` wisselen buitenvlak en gat per niveau af; onder `f`
wordt alleen een grens met overgang tussen nul en niet-nul winding
getekend. Onzichtbare binnencontouren verdwijnen pas na topologievalidatie.
Gerichte PDF-proeven voor losse vlakken met tegengestelde richting,
omgekeerde contourvolgorde, meerdere gaten, een genest eiland, een
vierde binnengat, een hol buitenpad en acht losse vlakken zijn op
0/90/180/270° met verschoven CropBox en een gekleurde onderlaag volledig
RGBA-bytegelijk aan Hayro.
Een 2384×1684 zichtbare 90°-pagina rendert hetzelfde Form-gat via één
begrensde tegel eveneens bytegelijk.
Van 96 gevarieerde driehoekige gaten waren 88 geaccepteerde gevallen
bytegelijk; acht onzekere gevallen vielen volledig terug. Een tweede reeks
met twee gescheiden driehoekige gaten accepteerde 62/64 bytegelijke
gevallen; twee onzekere gevallen vielen terug. Negen contouren,
elkaar kruisende of rakende contouren, curven, polygonale strokes en
onbewezen randafronding blijven Hayro-fallback. Dit is een volgende stap
richting een volledige annotatielaag, geen algemene Form-renderer.
De conservatieve randcontrole slaat pixels buiten de afzonderlijke
contourgrenzen over; daarvoor is nog geen gepaarde prestatiewinst gemeten.
