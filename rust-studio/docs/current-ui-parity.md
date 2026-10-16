# Doel: huidige Open PDF Studio-interface exact overzetten

De native Format-tab en het eigenschappenpaneel kunnen nu de kleur van een
geselecteerde rechthoek, ellips, lijn, pijl, polyline, zeshoek of tekst wijzigen.
Een kleurklik gebruikt één undo-bare `DocumentCommand`-transactie en bewaart
de geometrie; een klik op de bestaande kleur maakt geen nieuwe revisie. Een
geïsoleerde release-GUI/HTTP-proef tekende een ellips op
`simple-fill-stroke.pdf`, koos Blue in de Format-tab en controleerde via MCP
`#e74c3c`→`#0066cc`, onveranderde afmetingen, undo en redo. De zichtbare
nieuwe kleur en eigenschappen staan in `/tmp/opds-format-color-after-click.png`.
Dit is één native RGB-kleur per annotatie; afzonderlijke stroke/fill,
opaciteit en volledige Tauri-Format-pariteit ontbreken nog.

`app_move_annotation` verbindt nu een externe verplaatsing van één annotatie
met dezelfde geometrie en versiegebonden `DocumentCommand` als GUI-slepen.
Een geïsoleerde release-GUI/HTTP-proef tekende een ellips op
`simple-fill-stroke.pdf`, verplaatste haar exact 12×8 PDF-punten, verwierp
een stale verzoek en controleerde undo/redo. De PDF en selectiegrepen bleven
zichtbaar in `/tmp/opds-mcp-move-annotation-2026-10-16.png`. Dit dekt nog
geen externe eigenschappenwijziging. `app_resize_annotation` wijzigt nu
rechthoek, ellips, zeshoek en tekstvak binnen de pagina met dezelfde
versioned replacement als de GUI-grepen; puntgeometrie blijft uitgesloten.
App-, argument- en geïsoleerde HTTP-proeven controleren stale/replay,
annulering, randweigering en undo/redo. Een afzonderlijke release-GUI/HTTP-
proef tekende een ellips, wees stale resize af, vergrootte haar met 12×8
PDF-punten en verplaatste haar 4×5 punten. Undo/redo bracht de volledige
vorm terug en weer vooruit; de PDF, miniatuur en selectiegrepen bleven
zichtbaar in `/tmp/opds-mcp-resize-annotation-2026-10-16.png`.

`app_fit_view` bedient nu de echte Fit Width/Fit Page-acties van de Rust-UI
met actuele venster- en paneelafmetingen. De client geeft de exact gelezen
documentversie en `expectedZoom` mee; een stale, geannuleerde of onveranderde
fit wordt geweigerd. Een geïsoleerde release-GUI/HTTP-proef op een echte
300×1200-punts PDF ging van 52,5% pagina-fit naar 132% breedte-fit en terug,
met generatie 1→2→3; een oud verzoek werd geweigerd. De pagina en miniatuur
bleven zichtbaar in `/tmp/opds-mcp-fit-view-2026-10-16.png`. Dit bevestigt
alleen de native fit-bediening en geen DynLex-ondersteuning voor deze PDF.

`app_set_tool` kan nu de twaalf werkelijk aangesloten Rust-gereedschappen
selecteren via dezelfde actie als het lint en de linker paletstrook. De client
leest `tool` en `toolEpoch` met `app_get_current_tool` en stuurt beide als
verwachte staat, naast de exacte documentversie. Een onbestaande tool,
modaliteit, stale opdracht of replay verandert de actieve tool niet; de epoch
weert ook een oud verzoek nadat de gebruiker intussen heen en terug wisselde.
Een geïsoleerde release-GUI/HTTP-proef wisselde select→ellipse→select→ellipse,
weigerde de oude opdracht na de cyclus en tekende daadwerkelijk een ellips op
`simple-fill-stroke.pdf`; MCP telde één native ellips en de Rust-UI toonde haar.
Opname: `/tmp/opds-mcp-set-tool-ellipse-epoch-2026-10-16.png`. Dit maakt de
overige Tauri-tools nog niet beschikbaar.

`app_go_to_page` navigeert nu de live Rust-UI via dezelfde `GoToPage`-actie
als de paginabediening. De 1-gebaseerde doelpagina, de exact gelezen
documentversie en de verwachte huidige pagina zijn verplicht. Een ongeldige
pagina, geannuleerd verzoek of stale/herhaalde opdracht verandert de pagina
niet; een geslaagd antwoord meldt de nieuwe generatie en alleen dat de
render is ingepland. Een app-proef met een echte tweepagina-PDF controleert
de overgang 1→2, miniatuurfokus, annulering en replay. Een geïsoleerde
release-GUI/HTTP-proef op twee samengevoegde testpagina's controleerde een
ongeldige pagina (geen verandering), de overgang 1→2 en stale replay. De
screenshot `/tmp/opds-mcp-page-nav-2026-10-16.png` toont pagina 2/2, de
geselecteerde tweede miniatuur en de zichtbare PDF.

Op 16 oktober kreeg de native statusbalk rechtsonder een zichtbare diagonale
resizegreep. Zij gebruikt Iced's venster-resizeactie en verdwijnt bij een
gemaximaliseerd venster of actieve modaliteit. Een geïsoleerde release-GUI-proef
onder Xephyr/Metacity sleepte deze greep echt: het venster ging van 900×650
naar 980×706, de greep bleef in de nieuwe hoek en de PDF bleef zichtbaar.
Opname: `/tmp/opds-resize-grip-after-drag.png`. Dit verifieert die interactie,
niet de pixelpariteit van de hele statusbalk.

De opt-in lokale MCP-server heeft nu zes echte wijzigingsacties:
`app_delete_annotation`, `app_move_annotation`, `app_resize_annotation`,
`app_clear_annotations`,
`app_undo` en `app_redo`. De client moet document-ID,
rendergeneratie en annotatierevisie meesturen; de Iced-UI voert ze via dezelfde
`DocumentCommand` uit als de lokale UI. Stale of geannuleerde verzoeken
veranderen het document niet. Een geïsoleerde HTTP-proef en app-proeven
controleren verwijdering, undo/redo, stale resultaat, replay en selectie.
Een aparte release-GUI/HTTP-proef tekende een ellips met ID `0`, weigerde een
stale verwijdering, verwijderde haar op de actuele versie en herstelde haar
met de echte titelbalk-Undo; het aantal ging 1→1→0→1 en de pagina bleef
zichtbaar.
Een nieuwe geïsoleerde proef op de releasebinary tekende opnieuw een ellips en
riep de drie MCP-mutaties op: stale delete/undo/redo lieten de lijst ongemoeid,
geldige delete→undo→redo bracht het aantal van 1→0→1→0. De PDF bleef in de
Rust-UI zichtbaar.
Andere externe mutaties en de Tauri-plugin-API ontbreken nog.
De nieuwe clear-actie vereist expliciet `page` of `all`, verwerpt meer dan
4096 getroffen annotaties en gebruikt één undo-bare documenttransactie.
Een app-proef verifieert annulering, stale/replay, pagina-/documentclear en undo;
een geïsoleerde HTTP-proef verifieert stale clear, geldige clear en undo.

`app_select_annotation` en `app_clear_selection` bedienen nu de echte
Rust-selectie in de actieve Iced-UI. Ze vereisen dezelfde actuele documentversie
maar wijzigen geen PDF-inhoud of annotatierevisie. Selecteren accepteert alleen
een bestaande native annotatie op de zichtbare pagina. Een app-proef en een
geïsoleerde HTTP-proef controleren selectie, wissen en stale-weigering.
Een release-GUI/HTTP-proef tekende een ellips, wist en herstelde de selectie
via MCP en toonde de blauwe selectiegrepen en eigenschappen rechts; de
annotatierevisie bleef gelijk. De opname staat in
`/tmp/opds-mcp-selection-after.png`.

`app_select_native_renderer` bedient nu dezelfde vijf rendererkeuzes als de
selector linksonder, mits de gekozen optionele engine beschikbaar is. De
aanroep vereist de actuele documentversie én de eerder gelezen renderer;
een herhaalde of stale opdracht verandert niets. De bevestiging betekent
alleen dat de nieuwe render is gestart. Een geïsoleerde release-GUI/HTTP-proef
op `simple-fill-stroke.pdf` wisselde Hayro→DynLex→Hayro, weigerde stale en
replay, en liet de PDF zichtbaar. De originele 154,7%-fit bleek niet in
DynLex' bewezen schaalset te vallen. Bij zo'n rendererwissel probeert de UI
nu één keer 100% met **dezelfde strikte DynLex-kernel**; zij toont geen oude
Hayro-bitmap als eindresultaat. De screenshot na deze proef is
`/tmp/opds-dynlex-switch-recovery-2026-10-16.png`: de pagina staat op 100%
met `Renderer: DynLex`. Een app-proef controleert exact dezelfde RGBA-pixels
als de verse Hayro-referentie op die ondersteunde schaal, en controleert dat
een latere onbewezen zoom alsnog zonder oude bitmap wordt afgewezen. Dit is
geen algemene zoom-, snelheids- of volledige PDF-dekkingsclaim.

File → **Export XFDF annotations** schrijft nu een nieuw, niet bestaand
`.xfdf`-bestand voor dezelfde begrensde native annotatiesubset als de importer.
Niet getrouw weer te geven vormen, zoals polygonen of gecentreerde/rechts
uitgelijnde tekst, stoppen de volledige export zonder bestand. De serializer
begrenst de uitvoer tot 4 MiB en 4096 annotaties. Een roundtriptest controleert
rechthoek, ellips, lijn, open pijl, polyline en XML-escaped tekst; een
geïsoleerde release-GUI-proef importeerde twee vormen en exporteerde daarna
een XFDF met `<square>` en `<circle>`, terwijl pagina en miniatuur zichtbaar
bleven. De bestaande-bestandsproef toont dat de export niet overschrijft.
Volledige Tauri-XFDF-pariteit, waaronder lagen en stempels, ontbreekt nog.

Op 15 oktober kan File → **Import XFDF annotations** een begrensde subset van
Tauri-XFDF in het actieve Rust-document laden: rechthoek, ellips, lijn,
open pijl, tekst en inkt/polyline. De importer leest hoogstens 4 MiB en 4096
annotaties, valideert eerst het hele bestand en voegt alles toe als één
versiegebonden undo/redo-stap. Onbekende of niet getrouwe markup wordt geheel
geweigerd; stempels, lagen, opaciteit en veel Tauri-eigenschappen ontbreken
nog. Een geïsoleerde release-GUI-klik op een PDF met een rood vlak liet de
bronpagina en miniatuur zichtbaar, tekende twee geïmporteerde vormen, en de
live lokale MCP-lijst ging van nul naar twee annotaties. De volledige
workspace-testset slaagt. De modale Iced-laag behoudt nu een stabiele
widgetwortel en de actuele pagina heeft een eigen kleine miniatuurhandle;
hierdoor bleven pagina en miniatuur ook na Import of Cancel zichtbaar in de
geïsoleerde GUI-proef. Dit is geen algemene XFDF- of UI-pariteitsclaim.

Op 15 oktober ondersteunt File → **Export pages as images** PNG, JPEG en TIFF met
drie paginakeuzes:
actuele pagina, alle pagina's en een eigen bereik zoals `1-3,5`. Een losse
pagina gaat naar een nieuw `.png`-, `.jpg`- of `.tiff`-bestand; alle/eigen pagina's gaan
naar een **nieuwe** uitvoermap met genummerde bestandsnamen. Het bereik wordt
gevalideerd, gesorteerd en gededupliceerd. De PDF met native annotaties wordt
voor een reeks eenmaal voorbereid; pagina's worden achtereenvolgens met de
expliciet gekozen renderer op 72, 150 (standaard) of 300 DPI gerenderd. Een
bewaarde PNG bevat de bijbehorende pHYs-resolutie in pixels per meter,
zoals de Tauri-export; de drie DPI-keuzes worden hierop getest. JPEG gebruikt
kwaliteit 92, componeert transparantie op wit en bewaart de gekozen DPI in
JFIF. De JPEG-kwaliteit is nu net als in Tauri instelbaar van 10 tot 100%,
standaard 92%; de uitgeschakelde of ingeschakelde annotatie-optie bepaalt of
native annotaties in de uitvoer komen. Tests controleren de kwaliteitsgrenzen,
verschillende JPEG-uitvoer bij 10 en 100%, dimensies, bronbeeldnabijheid,
DPI, pagina 2 met annotatie en het ontbreken van een bestand na strikte
DynLex-afwijzing. TIFF bewaart de volledige RGBA-pixels zonder verlies en
schrijft de DPI als TIFF-resolutietags. Een gedecodeerde pixelproef vergelijkt
72, 150 en 300 DPI volledig met verse geselecteerde-rendereruitvoer, inclusief
de alfakanaalmarkering; bestaande doelen blijven ongewijzigd en een strikte
DynLex-afwijzing maakt geen TIFF. Een geïsoleerde release-GUI-klik schreef een
echte 533×400-TIFF op 150 DPI; Pillow las de resolutie terug als 150×150.
De huidige Tauri-code in `js/pdf/exporter.js` vraagt bij de TIFF-keuze nog
`image/png` aan en `js/pdf/image-dpi.js` ondersteunt TIFF niet. Daarom is voor
TIFF geen bytepariteit met die route geclaimd.
Een geïsoleerde release-GUI-proef selecteerde JPEG in de formaatkeuze en
exporteerde een nieuwe 533×400-JPEG op 150 DPI. De PDF-renderpixelbron is
dezelfde; JPEG-compressie is verliesgevend en geen byte-exacte Tauri-pariteit.
Twee aanvullende geïsoleerde GUI-proeven sleepten de echte kwaliteitsschuif
naar 10 en 100% en schreven respectievelijk 6960 en 10146 bytes op dezelfde
533×400-bronpagina. Een aparte GUI-klik zette **Include annotations** uit;
de renderproef vergelijkt een JPEG van pagina 2 met en zonder ellipsannotatie.
Dit bewijst de GUI-aansluiting, niet dat Chromium en de
Rust JPEG-encoder identieke gecomprimeerde bytes maken.
Een bestaand
bestand of een bestaande uitvoermap wordt niet overschreven. Bij een
renderfout worden de bestanden die deze opdracht in de nieuwe map maakte
best effort opgeruimd; een strikte DynLex-afwijzing kiest geen andere renderer
en levert geen bitmap op.

**Cancel** tijdens de beelduitvoer signaleert nu de renderopdracht. Zij stopt
vóór de volgende pagina of vóór het bewaren van een al gerenderde pagina;
een render die op dat moment loopt kan nog even doorgaan. De nieuwe uitvoermap
en de daarin door de opdracht gemaakte bestanden worden bij annulering
opgeruimd. Een geïsoleerde GUI-proef met een 1000-pagina-PDF klikte op
**All pages**, startte de PNG-export en annuleerde toen 89 pagina's waren
geschreven. De map was daarna weg en de statusbalk meldde `Export cancelled`.
De eenpagina-route controleert annulering vóór bewaren. Een volgende
geïsoleerde release-GUI-proef toonde tijdens dezelfde 1000-pagina-export een
voortgangsbalk met paginateller en annuleerde na 70 gemaakte pagina's; ook
hier verdween de uitvoermap. De UI krijgt maximaal circa 100 tussentijdse
meldingen per opdracht, zodat een groot aantal pagina's de eventwachtrij niet
overspoelt. Een lopende paginarender wordt nog niet onmiddellijk onderbroken.

Volledige gedecodeerde RGBA-proeven vergelijken alle drie DPI's met verse
renders, inclusief een ellipsannotatie. Een tweepagina-proef vergelijkt alle
pixels van beide batchbestanden en controleert dat een alleen op pagina 2
geplaatste annotatie daar verschijnt. Een geïsoleerde release-GUI-proef in
Xephyr klikte **All pages** op een PDF met twee verschillende pagina's en
schreef een nieuwe map met twee 533×400-RGBA-PNG's; **Custom range** met `2`
schreef in een andere nieuwe map uitsluitend pagina 2. Een eerdere geïsoleerde
GUI-proef exporteerde 300 DPI (1066×800) en de losse pagina 2. Alle proef-
vensters en uitvoer lagen onder `/tmp`; bestaande gebruikersvensters bleven
onaangeroerd. Dit is nog geen volledige Tauri-exportpariteit: 600 DPI en
de Tauri-mapkiezer ontbreken. De huidige
Hayro-route begrenst de schaal op 8×, minder dan 600/72.

Op 15 oktober is de **Extensions**-route ook met echte GUI-invoer in een
afzonderlijk 1200×832-Xephyr-venster gecontroleerd. File → Extensions
opende de HTML/JavaScript-editor; de ID, naam en het uitvoerpad zijn via
het venster ingevuld. Twee via de editor gewijzigde bronbestanden zijn met
de knop **Build .oppx** in een nieuw pakket geschreven. `unzip -t` slaagde
en `index.html` en `index.js` bevatten de aangebrachte wijzigingen. File →
Open Extension Package opende hetzelfde pakket opnieuw en toonde beide
wijzigingen in de editor. Dit bewijst bronbewerking, pakketbouw en import
via de GUI, niet het uitvoeren van JavaScript of Tauri-plugin-API-pariteit;
de Rust-app activeert het pakket nog niet. De proef gebruikte alleen een
eigen testvenster en `/tmp`-uitvoer.

Op 15 oktober is Drawing → Modify → **Rotate** voor native lijnen, pijlen en
polylijnen aangesloten: één klik draait hun punten 90° met de klok mee rond
het Tauri-middelpunt. De bounds inclusief pijlvleugels worden opnieuw
berekend; een draaiing buiten de PDF-pagina wordt zonder modelwijziging
geweigerd. De actie gebruikt de versiegebonden `DocumentCommand` en is één
undo/redo-stap. Geometrie- en app-actietests controleren pijl/polyline,
paginarand, undo en redo. Andere vormen blijven uitgeschakeld zolang het
native model hun Tauri-rotatievlag niet kan bewaren.
Een afzonderlijke release-GUI-proef op `simple-fill-stroke.pdf` klikte na
het tekenen van een pijl op de echte Rotate-knop. `app_get_annotation`
bevestigde dat de eindpunten van ongeveer `(101,495;31,677)` en
`(179,071;80,162)` naar `(164,525;17,131)` en `(116,040;94,707)` gingen:
een kwartslag om het middelpunt. De proef gebruikte een apart Xephyr-venster.

Op 15 oktober zijn Drawing → Modify → **Mirror Horizontally** en **Mirror
Vertically** aangesloten voor native lijnen, pijlen en polylijnen. De
spiegelas volgt de extremen van de punten zoals in Tauri; de pagina- en
pijlpuntgeometrie worden opnieuw begrensd en ongeldige resultaten wijzigen
het document niet. Elke geslaagde spiegeling loopt via `DocumentCommand` en
is één undo/redo-stap. Geometrie- en app-actietests controleren een pijl en
een polyline, inclusief undo/redo. Voor tekst, stempels en andere vormen
blijven de knoppen uitgeschakeld: hun Tauri-flags of vrije puntgeometrie
bestaan nog niet in het native model.
In een afzonderlijke release-GUI-proef op `simple-fill-stroke.pdf` is met de
linker Arrow-tool een pijl getekend. Een echte klik op beide Modify-knoppen
verwisselde achtereenvolgens de X- en Y-coördinaten van de eindpunten rond
hun middelpunt; `app_get_annotation` bevestigde beide veranderingen. De
proef draaide in een eigen Xephyr-venster met aparte state-map.

Op 14 oktober is **Copy** in de Drawing-groep Modify aangesloten op de
geselecteerde native annotatie. Net als Tauri's `duplicateAnnotation()` maakt
de knop een nieuwe annotatie, 20 schermpixels verschoven bij de actuele zoom.
Rechthoeken, ellipsen, polygonen, tekst, lijnen, pijlen en polylijnen behouden
hun type en interne puntgeometrie. Als de voorkeursverschuiving de paginarand
raakt, gebruikt Rust de andere richting; als er geen ruimte is, blijft het
document ongewijzigd. De toevoeging loopt via dezelfde versiegebonden
`DocumentCommand` als de overige edits, is één undo/redo-stap en selecteert de
kopie. Een app-test controleert knopactie, selectie en undo/redo. In een
afzonderlijk Xephyr-venster op `tiny.pdf` is op 198% een rechthoek getekend,
waarna een echte klik op Drawing → Modify → Copy via de lokale MCP-leeslijst
precies twee rechthoeken gaf: de kopie verschoof 10,101 PDF-punt, gelijk aan
20 schermpixels. De titelbalkknoppen Undo en Redo brachten de telling
achtereenvolgens naar één en twee terug. De proef gebruikte een aparte
state-map en raakte geen bestaand gebruikersvenster.
De Drawing-groep Clipboard heeft nu ook werkende **Cut**, **Copy**, **Paste** en
**Paste in Place** voor één geselecteerde native annotatie. Copy bewaart een
interne kopie zonder documentwijziging; opeenvolgende Paste-acties verschuiven
de bron telkens 20 schermpixels verder, terwijl Paste in Place de oorspronkelijke
PDF-coördinaten gebruikt. Bij een andere paginagrootte wordt een ongeldige
plaatsing geweigerd. Elke geslaagde Paste is één versiegebonden undo/redo-stap.
Cut verwijdert pas na een geslaagde, versiegebonden modeltransactie en bewaart
dan de bron op het interne klembord; Undo herstelt de annotatie. Ctrl+X,
Ctrl+C, Ctrl+V en Ctrl+Shift+V volgen dezelfde acties buiten tekstvelden en
dialogen. Een app-test controleert de offsetreeks, coördinaten, selectie en
undo/redo. Een afzonderlijke echte Xephyr-GUI-proef op
`simple-fill-stroke.pdf` tekende een rechthoek en gebruikte de drie
sneltoetsen; de live MCP-leeslijst telde daarna drie rechthoeken, waarvan de
eerste kopie 12,9293 PDF-punt verschoof bij 155% zoom en de tweede exact op
de broncoördinaten stond. Meervoudige selectie en het systeemklembord
ontbreken nog.
Een tweede geïsoleerde release-GUI-proef op dezelfde fixture gebruikte
Ctrl+X, Ctrl+Z en Ctrl+V: de live annotatietelling liep van 1 naar 0, 1 en 2.
Daarmee is de Cut-actie samen met Undo en opnieuw plakken via de echte
toetsenroute gecontroleerd; de bestaande gebruikersvensters bleven ongemoeid.

Op 14 oktober opent **New** in de Home-ribbon en het File-menu nu een
dialoog; Ctrl+N volgt dezelfde route. Die maakt blanco pagina's in het
geheugen met dezelfde vijftien papierkeuzes als de Tauri-dialoog (A0–A6,
B3–B5, Letter, Legal, Tabloid, Ledger en Custom), portret/landschap en
1–999 pagina's. Custom accepteert 10–5000 mm per zijde. Standaard is één
A4-portretpagina; ongeldige invoer laat het document onaangetast.
Het actieve document blijft staan tot de nieuwe PDF volledig is voorbereid.
Bij niet-opgeslagen annotaties vraagt New eerst om bevestiging. De blanco
pagina rendert via de geselecteerde renderer; een DynLex-proef op 100% is
bytegelijk aan Hayro. Voor Save/Save As wordt een nieuw uitvoerpad gevraagd;
een succesvolle eerste opslag geeft de tab het gekozen pad en zet dat in de
recente bestanden. Een ongeslagen New verschijnt niet in de MRU of in de
reader-position-bestanden. Een geïsoleerde 1200×832-GUI-klik op de
releasebinary toonde de nieuwe blanco pagina en één tab zonder een bestaand
gebruikersvenster te veranderen. Een tweede geïsoleerde Xephyr-proef op de
releasebinary opende de nieuwe dialoog via de Home-knop, koos landschap,
wijzigde het aantal naar drie en klikte Create PDF. De live MCP-paginatelling
was `3`; drie miniaturen en `1 / 3` waren zichtbaar, met een liggende
297,0 × 209,9 mm-pagina. De eigenschappen tonen voor een nog niet opgeslagen
New-document geen fictief bronpad. De invoer- en begrenzingsgevallen zijn daarnaast
met Rust-tests gevalideerd. Tauri-frame- en titelbloktemplates en
meerdocumenttabs ontbreken nog.

Op 14 oktober werkt **Polygon** in beide indelingen van de linker
tekenpaletstrook. Slepen maakt een native zeskant volgens Tauri's standaard
`buildPolygonPath` (bovenste hoek eerst), met zichtbare voorvertoning,
randselectie, formaatwijziging en undo/redo. Save As schrijft een echte
PDF-`/Polygon` met zes `/Vertices` en een gesloten stroked appearance;
de native MCP-leeslijst meldt `type=polygon` en `sides=6`. Een geïsoleerde
release-GUI-proef op `simple-fill-stroke.pdf` klikte het paletpictogram en
sleepte op de pagina: de live tool werd `polygon`, de annotatielijst kreeg
één zeskant en de opname `/tmp/opds-polygon-after.png` toont de vorm.
Model-, PDF-export- en app-tests slagen. Aanpasbaar aantal zijden, vulkleur,
lijnstijl en andere Tauri-voorkeuren ontbreken nog; dit is geen volledige
Polygon-pariteit.
Een vervolgaudit vond dat de eerste PDF-appearance de gecentreerde
2-puntslijn bij de bovenste en onderste hoek afknipte: 16 pixels verschil
op 100%. De `/Rect` en appearance-`/BBox` krijgen nu 2 punten ruimte rondom
de geometrie. De opgeslagen PDF en een direct getekende referentie zijn
voor 0/90/180/270 graden op 50/100/200% bytegelijk; de zes `/Vertices`
blijven op hun oorspronkelijke paginacoördinaten staan.

Op 14 oktober werkt ook **Circle** op de Drawing-ribbon. De cirkel sleept,
zoals Tauri's `shape-tool.js`, vanuit het beginpunt met de grootste horizontale
of verticale afstand als zijde van een vierkant in PDF-paginacoördinaten.
Voorvertoning en definitieve annotatie gebruiken dezelfde begrensde
geometrie; aan de paginarand blijft zij binnen de pagina. De linker
paletknop met ellipspictogram kiest nu, net als Tauri's `ToolPalette.jsx`,
deze Circle-modus. De afzonderlijke Drawing-knop **Ellipse** blijft een vrije
ovaal. Beide worden als native ellips/PDF-`/Circle` bewaard. Een geïsoleerde
echte GUI-klik op de Circle-knop en een sleep op `tiny.pdf` leverden via de
live MCP-documentstaat `tool=circle` en één annotatie met gelijke breedte en
hoogte (25,2525 PDF-punt). Kleur-, vul- en lijndiktevoorkeuren van Tauri
ontbreken nog; dit is geen volledige tekenpariteit.

Op 14 oktober is ook **Arrow** in beide linker paletindelingen en de
Drawing-ribbon aangesloten. Het is een eigen bewerkbare native annotatie met
open pijlpunt, eindpuntgrepen en undo/redo; Save As schrijft een PDF `/Line`
met `/LE [ /None /OpenArrow ]` en een eigen appearance. Dezelfde geometrie
wordt voor hit-test, live overlay en export gebruikt. PDF-appearance en
directe PDF-stroken zijn op 0/90/180/270 graden pixelgelijk in de test; een
geïsoleerde echte GUI-klik en sleep op `tiny.pdf` leverde een `arrow` op in de
live documentstaat en een PDF met `/OpenArrow`. Pijlkleur, lijndikte en
puntstijl zijn nog vast, dus dit is geen volledige Tauri-Arrow-pariteit.

Op 14 oktober is **Ellipse** in beide indelingen van de linker tekenstrook
en op de Drawing-ribbon een echt gereedschap geworden. Een muissleep maakt
een eigen elliptische
annotatie met voorvertoning; selecteren, verplaatsen, formaat wijzigen en
undo/redo gebruiken de bestaande versiegebonden documenttransacties. Save As
schrijft een PDF `/Circle`-annotatie met een vierdelige Bézier-appearance,
ook op kwartslags gedraaide pagina's. De MCP-leeslijst noemt het type
`ellipse`. De onderliggende model-, app- en PDF-uitvoertests slagen.
Een geïsoleerde echte X11-GUI-proef op de releasebinary (Xephyr, 1200×832,
`tiny.pdf`) heeft de Ellipse-knop aangeklikt en over de pagina gesleept:
de live MCP-documentstaat ging van nul naar één `ellipse`-annotatie met
positieve breedte en hoogte. De titelbalkknop Undo bracht dat aantal weer
naar nul en de live edit-state meldde `canRedo=true`. Dit raakt geen bestaand
gebruikersvenster.
Een volledige Tauri-pixelvergelijking ontbreekt nog.

**Freehand** in beide strookindelingen en op de Drawing-ribbon is nu ook
actief: slepen maakt een bewerkbare native polyline, met voortgaande
puntreductie en een harde grens
van 512 punten per streek. De bestaande polyline-export schrijft haar naar
PDF. Gewone langere streken met meer dan 64 punten blijven nu intact;
zeer lange of fijne streken verliezen door de grens nog steeds detail.
Een 120-puntslijn is na PDF-export op 0/90/180/270 graden en 50/100/200%
volledig pixelgelijk aan een onafhankelijk PDF-pad. De geometrische
annotatiemarge is van 2 naar 4 PDF-punten vergroot: de oude marge knipte
bij die lange lijn op 100% twaalf randpixels af. Dit is nog geen
volledige Tauri-Freehand-pariteit. De overige grijze tekengereedschappen
zijn nog niet gemigreerd.
Een afzonderlijke Xephyr-GUI-proef selecteerde Freehand en sleepte zeven
punten over `tiny.pdf`; de live annotatielijst meldde één `polyline` met
`pointCount=7`. Een gesimuleerde Ctrl+Z zonder vensterbeheerder leverde in
die proef geen undo-event op; de titelbalk-Undo is apart met Ellipse getest.

Op 14 oktober is de native **Save**-route aangesloten op de titelbalk,
File-pagina en Ctrl+S; Ctrl+Shift+S opent Save As. De eerste Save vraagt via
Save As een nieuw doelpad. Daarna schrijft Save naar datzelfde exportbestand
alleen als de huidige bytes nog overeenkomen met de vorige uitvoer; een
extern gewijzigd doel wordt geweigerd met de aanwijzing Save As te gebruiken.
De uitvoer wordt via een tijdelijk bestand in dezelfde map vervangen. Save
behoudt de Unix-modusbits en controleert het doel nogmaals vlak voor
vervanging; symlinks en alleen-lezen-doelen worden afgewezen. Dit is geen
atomair slot tegen gelijktijdige externe schrijvers. Save overschrijft nooit
stilzwijgend de geopende bron-PDF. De bestaande Save As-
beperkingen voor ondersteunde native annotaties blijven gelden. De
bestandsguard, de eerste Save-route en 113 app-tests zijn gecontroleerd.
Een geïsoleerde Xephyr-GUI-proef klikte Ellipse, tekende op een kopie van
`tiny.pdf`, klikte Save As en daarna na een tweede ellips op de titelbalkknop
Save. De eerste uitvoer werd aangemaakt, de tweede wijzigde de uitvoerbytes,
de bron bleef bytegelijk en `pdfinfo` kon het resultaat als één pagina lezen;
de uitvoer bevat twee PDF-`/Circle`-annotaties.
Een tweede geïsoleerde GUI-proef wijzigde het opgeslagen doelbestand extern
en klikte na een nieuwe ellips opnieuw Save: de externe bytes bleven
ongewijzigd. Deze proeven verifiëren de knoppen en de native export voor dit
testbestand; andere annotaties en Tauri-pariteit blijven afzonderlijke
controles.

De gedockte **tekengereedschappenstrook links** volgt nu bij 1920×1040 de
eenkolomsindeling van Tauri; bij 1200×832 blijft zij tweekoloms. De
Fit Width-/Fit Page-berekening en centrering gebruiken de werkelijke breedte.
Een nieuwe 1920×1040-Rust-opname in `/tmp/opds-oct14-full-palette.png` toont
alle twintig pictogrammen in de oorspronkelijke volgorde. De
**symbolenbibliotheek rechts** blijft daarentegen onvolledig: de Rust-app
toont nog een statische gedeeltelijke categorielijst. De View-knop
Toolpalette en het paneelkruis sluiten/openen haar nu echt; Fit Width en
centrering rekenen de vrijgekomen 220 px mee. Zoeken, uitklappen,
instellingen en symboolplaatsing ontbreken nog. Die lijst is geen
functiepariteit met Tauri's `SymbolPalette.jsx`.

De optionele PDFium-route is op 14 oktober vanuit de Rust-probe uitgevoerd:
`simple-fill-stroke.pdf` gaf 256×192 pixels en dezelfde volledige
RGBA-SHA-256 als Hayro (`32847d8d…`). Een geïsoleerde Rust-UI-opname op
1200×832 met `OPDS_RENDER_BACKEND=pdfium` en de lokale `libpdfium.so` toont
de pagina en `Kernel PDFium` / `Renderer: PDFium` in de statusbalk
(`/tmp/opds-oct14-pdfium-ui.png`). Dit is een proef op één fixture, geen
algemene PDFium-/DynLex-pariteits- of snelheidsclaim.

De rendererkeuze bevat nu een optionele, expliciet als Hayro-gebaseerd
gelabelde PdfCraft-route naast Hayro, DynLex, PDFium en Eigen Rust.
De geselecteerde hoofdweergave blijft strikt op die route; de
vijfwegsproef, lokale bibliotheekvereisten en meetgrenzen staan in
[`pdfcraft-kernel-comparison-2026-10-13.md`](pdfcraft-kernel-comparison-2026-10-13.md).

Op 13 oktober heeft `DynlexDocument` een brongebonden, begrensde
`try_render_vector_viewport`-ingang voor de al bewezen ondoorzichtige,
pixeluitgelijnde vector-subset. Een 257×191-tegel op 1600% is bytegelijk
aan een Hayro-oracle van dezelfde verven op een tweemaal vergrote en
bijgesneden PDF, gerenderd op 800% binnen Hayro's publieke schaallimiet.
Ongeldige viewportgeometrie geeft een fout en een rasterpagina krijgt via
deze ingang geen bitmap; alle 127 backend-libtests slagen. Dit is een
kernel-API-proef, **nog geen 1600%-weergave in de Rust-UI** en geen bewijs
voor tekst, hybride pagina's of algemene vectorinhoud.

Op 13 oktober is de eerste paginaweergave bij openen aangepast aan de
beschikbare documentruimte. De Rust-app gebruikt nu dezelfde Fit Page-
berekening als haar zichtbare knop, rekening houdend met paneel- en
ribbonbreedtes. Een opgeslagen zoom blijft voor de andere renderers behouden;
DynLex probeert bij openen eerst de begrensde fit-schaal. Als juist die
schaal buiten zijn bewezen subset valt, probeert de strikte DynLex-route
dezelfde pagina op 100%; beide pogingen blijven native en een dubbele
mislukking geeft geen bitmap. Op de onveranderde `simple-fill-stroke.pdf`
is 136% bij 1200×832 niet ondersteund, terwijl 100% wel exact rendert;
de app toont daarom de pagina op 100% met de juiste zoomstatus. De bij het
openen voorbereide DynLex-documentinstantie blijft nu aan de documenttab
gekoppeld, zodat volgende zooms en miniaturen dezelfde begrensde bron- en
scenecaches kunnen gebruiken. Dit is geen gemeten versnelling van de eerste
nieuwe zoom. De originele
`Tekst.pdf` pagina 5 op de nieuwe automatische 58%- en 94%-schalen is in
de strikte DynLex-route zichtbaar; aparte volledige RGBA-vergelijkingen op
0,5846 en 0,938 leverden respectievelijk 684.864 en 1.761.048 gelijke bytes
met Hayro. Dit bewijst alleen deze pagina en schalen. Niet-ondersteunde
pagina's blijven zonder bitmap. De reeds draaiende gebruikersvensters
gebruiken hun bestaande binary en zijn niet herstart.

Op 13 oktober is de **uitgevouwen Drawing-ribbon op 1920×1040** in de
draaiende Tauri- en Rust-vensters vergeleken. Rust had Selection, Snippet,
Scale en Annotate tot icoonroosters samengedrukt terwijl Tauri daar grote
knoppen of gelabelde stapels toont. De native layout gebruikt nu dezelfde
groepsvormen en benadert de oorspronkelijke horizontale groepsgrenzen; de
Draw-groep houdt het compacte icoonrooster. De reproducibele Rust-proef gebruikt
`OPDS_UI_SCREENSHOT_RGBA` met `OPDS_UI_SCREENSHOT_SIZE=1920x1040` en
`OPDS_UI_SCREENSHOT_DRAWING_TAB=1`. Uitgeschakelde knoppen blijven zichtbaar
maar zijn geen functiepariteit. Vooral Clipboard, symboolpalet, documentzoom
en meerdere Drawing-acties verschillen nog. De reeds draaiende Rust-vensters
zijn niet herstart en tonen dus nog hun vorige binary.

De Annotation-ribbon heeft nu werkende **Clear Page**- en **Clear All**-
knoppen voor native Rust-annotaties. Een bevestigingsdialoog bewaart de
documentversie van het verzoek; een tussentijdse pagina- of annotatiewijziging
verwerpt de verouderde bevestiging. Elke verwijderactie is één atomaire
undo/redo-stap, met behoud van annotatie-ID's. De knoppen zijn uitgeschakeld
als er in hun bereik geen native annotaties zijn. Dit wist nog geen
geïmporteerde PDF-annotaties en bewijst geen volledige Annotation-pariteit.

De nieuwste ongemaskeerde 1200×832-pixelvergelijking, live File-menu-metingen
en resterende structurele afwijkingen staan in
[`ui-pixel-parity-2026-10-12.md`](ui-pixel-parity-2026-10-12.md). De
titelbalk is aantoonbaar dichter bij de Tauri-referentie, maar volledige
pixelpariteit is nog niet bewezen.

De bestaande Open PDF Studio-versie op branch `feat/pdf-export-snippets-pdfjs6` is de **visuele en functionele referentie**. Op verzoek van de gebruiker is de UI-code van Hakan's OpenCADStudio nu de implementatiebasis. De ribbonhelpers en iconen worden daadwerkelijk overgenomen en aan PDF-commands gekoppeld.

De eerste afgebakende Rust-MCP-server en de nog ontbrekende Tauri-commands en
plugin-API staan in [de MCP/API-migratienotitie](mcp-api-migration-2026-10-12.md).

De File-pagina toont nu echt eerder geopende PDF's (maximaal 20, nieuwste
eerst) in plaats van één statische bestandsnaam. Het zoekveld filtert die
lijst; een gekozen bestand loopt via dezelfde Open-route met controle op
onopgeslagen wijzigingen. Succesvol laden voegt een bestand toe, een mislukte
laadpoging niet. De lijst wordt begrensd en atomair bewaard in
`$XDG_STATE_HOME/open-pdf-studio-rust/recent-files.json` of onder
`~/.local/state`; screenshotproeven lezen of schrijven deze gebruikersstaat
niet. Pinnen, ontpinnen en individuele items verwijderen werken nu in dezelfde
File-lijst. De bewaarde stringlijst van de eerste Rust-versie wordt gelezen
als ongepinde items en bij een volgende wijziging omgezet. De lijst houdt
maximaal 20 items; een vastgezet item wordt bij verdringing overgeslagen.
De beide koppen voor vastgezette en overige recente bestanden zijn nu echt
afzonderlijk in- en uitklapbaar, zonder de bewaarde lijst te wijzigen. De
overige Tauri-File-acties ontbreken nog.

## Huidige desktopstructuur (broncode-inventaris)

| Gebied | Huidige bron | Rust-equivalent en controle |
| --- | --- | --- |
| Titelbalk en snelle acties | `open-pdf-studio/js/solid/components/TitleBar.jsx`, `styles/titlebar.css` | Zelfde volgorde, hoogtes, iconen, hover en toetsenbordacties |
| Lint en tabbladen | `components/ribbon/Ribbon.jsx`, `HomeTab.jsx`, `DrawingTab.jsx`, `CommentTab.jsx`, `ViewTab.jsx`, `OrganizeTab.jsx`, `HelpTab.jsx`, contexttabs Format/Arrange/Image; `styles/ribbon.css` | Zelfde tabs, groepen, knoppen, ingeklapte toestand en contextzichtbaarheid |
| Meldingen en documenttabs | `App.jsx`, `DocumentTabs.jsx`, `styles/document-tabs.css` | Zelfde documentnaam, wijzigingsmarkering, tabwissel, sluiting en lege toestand |
| Links | `left-panel/LeftPanel.jsx`, pagina's/bookmarks/annotaties/lagen/bijlagen/velden/handtekeningen enz. | Zelfde tabindeling, breedte, scroll en inhoud |
| Midden | `App.jsx` (`pdf-container`, canvas/text/form/annotation layers), `styles/layout.css` | Zelfde paginapositie, schaduw, zoom, selectie, grips en overlays |
| Rechts/docking | `App.jsx` dock targets en panelen, `styles/properties-panel.css` | Zelfde positie, resize, zichtbaarheid en eigenschappen |
| Onder | `StatusBar.jsx`, `styles/status-bar.css` | Zelfde pagina- en zoomcontrols, status/metingen en view modes |
| Modals en menu's | `components/app-menu/`, `styles/dialogs.css`, `styles/app-menu.css` | Zelfde keuzes, labels en foutmeldingen |
| Thema en lokalisatie | `styles/themes.css`, `styles/base.css`, `js/i18n/locales/` | Zelfde kleuren, fonts, maatvoering, talen en dark/light gedrag |

Deze lijst is een startinventaris. Voor elk scherm komt er een visuele vergelijking op gelijke venstergrootte/DPI én een interactietest. Een identieke opmaak zonder dezelfde functie telt niet als afgerond; een werkende functie met zichtbaar afwijkende UI ook niet.

## Volgorde

Eerst documentmodel, PDF-engine en rendercontract. Daarna een native shell die de huidige titelbalk, lint, tabs, panelen en statusbalk reproduceert. Vervolgens functie voor functie koppelen, van openen/zoomen en pagina's naar annotaties, tekst, export, print en integraties. De bestaande app blijft beschikbaar als testreferentie tot de pariteit bewezen is.

## Overname OpenCADStudio (1 oktober 2026)

De eerdere testshell is vervangen door de port in `hakan_ui.rs`. Deze gebruikt echte upstream ribbonhelpers en SVG-assets, compacte quick actions, gegroepeerde tools, paginalijst en documenteigenschappen. Zie [bronverantwoording](ui-reference.md). Openen staat in een afzonderlijk dialoogvenster; de ribbon bevat geen padveld meer. De native overlay kan rechthoek en markering tekenen, selecteren, verplaatsen, met acht grepen vergroten/verkleinen en verwijderen, met undo/redo in de Rust-documentkern. De Annotation-ribbon toont nu dezelfde acht groepen en PDF-iconen als `CommentTab.jsx`; Highlight en meerregelige Text Box zijn daarin functioneel. Een tekstvak wordt inline op de pagina ingevoerd en bewerkt, met een Format-contexttab, zeven werkende tekstkleuren, een lettergroottekeuze van 8–72 pt en links/midden/rechts-uitlijning. De contexttab blijft bij een geselecteerd native tekstvak beschikbaar; kleur, lettergrootte en uitlijning wijzigen die annotatie direct. Alle drie lopen door de Rust-overlay, undo/redo en de printbare PDF-appearance. Sinds de native `inline_text.rs`-laag worden regels, caret en selectie ook tijdens midden/rechts-uitgelijnde invoer op hun werkelijke plaats geschilderd. Muisklik en slepen worden via UTF-8-veilige glyphposities naar de Iced-teksteditor vertaald. Screenshots op 1200×832: oorspronkelijke app `/tmp/opds-original-text-dialog.png`; Rust live midden `/tmp/opds-live-center-unicode.png`, selectie `/tmp/opds-live-center-selection.png`, live rechts `/tmp/opds-live-right-click-insert.png` en gewone pagina `/tmp/opds-live-normal-center-editor.png`. De oorspronkelijke TextAnnotationDialog.jsx heeft drie uitlijningsknoppen. De Rust-contexttab heeft nu dezelfde keuzes maar mist lettertype en overige oorspronkelijke groepen; de editor- en panelopmaak zijn nog niet pixelgelijk. Een gedeelde Rust-tekstlayout breekt woorden en lange tokens nu af tijdens live invoer, in de overlay en in de PDF. Vergelijk de geroteerde schermopname `/tmp/opds-wrap-center-editor.png` met `/tmp/opds-wrap-center-poppler.png` en de normale pagina `/tmp/opds-wrap-normal-editor.png` op 1200×832. IME-compositie en rechts-naar-links-schrift moeten nog apart worden gevalideerd; de synthetische Ctrl+Shift+U-proef met IBus leverde in dit Iced-venster alleen letterlijke tekens op en is geen betrouwbare IME-test. Een eerste Save As-pad schrijft annotaties in Rust; zie [PDF-opslag](pdf-save-2026-10-01.md) voor de bewuste beperkingen. [De scherm- en snelheidsvergelijking](ui-performance-2026-10-01.md) toont de resterende verschillen.

Op 2 oktober is de Format-stijlgalerij in `hakan_ui.rs` in dezelfde volgorde en ongeveer dezelfde compacte breedte als `FormatTab.jsx` gezet: zeven werkende kleurstijlen en drie zichtbaar uitgeschakelde Cloudy-stijlen. De oorspronkelijke galerij gebruikt ook een extra uitklapknop en andere rand-/hoverdetails; die ontbreken nog. Deze wijziging is gebouwd en met workspace-tests gecontroleerd, maar een screenshot van de nieuwe Format-tab kon in deze run niet via de beschikbare native UI-automatisering worden gemaakt. Verificatie in het venster blijft nodig.

IME-onderzoek: Iced 0.14 ontvangt preedit via `iced_widget::text_editor`, waarna `iced_winit` de compositietekst als apart venster-overlay op de cursorpositie van die editor tekent. Bij rechts- en middenuitlijning is de onderliggende editor tekst transparant en links uitgelijnd; de eigen Rust-canvas tekent de juiste tekst/caret, maar kan de positie van deze Iced-preedit-overlay niet overschrijven. Sinds commit-in-voorbereiding geeft een omhullende Rust-widget bij een Iced-redraw de IME-cursorpositie van de visueel uitgelijnde, eventueel afgebroken regel door. Een Unicode-regressietest controleert Center en Right. Een echte IME-compositietest en een screenshot van de preedit ontbreken nog; de synthetische Ctrl+Shift+U-proef is daarvoor onvoldoende. In deze run bood de native computerkoppeling geen vensters aan, hoewel het venster onder X11 draaide.

Op 2 oktober is het linkerpaneel van paginalabels naar echte paginaminiaturen gebracht. De actieve pagina hergebruikt de getekende bitmap; nabije pagina’s worden één voor één op de achtergrond gerenderd. De cache bevat maximaal twaalf miniaturen en een foutpagina wordt niet eindeloos opnieuw geprobeerd. De schaal mag tot 0,01 zodat ook A1-tekeningen passen. De toenmalige proef van `Technische tekening.pdf` leverde bij schaal 0,05369 een miniatuur van 127×90 px in 28 ms (`/tmp/opds-a1-thumbnail-fitted.png`). De huidige maximale maat is verderop bijgewerkt.

De miniaturenplanner volgt ook de scrollpositie van het linkerpaneel. Een vaste kaartstap koppelt de zichtbare rij aan paginanummers; de scheduler haalt die pagina en maximaal twee buren op, naast de actieve pagina. Ook bij ver scrollen blijft slechts één renderopdracht tegelijk actief. De actuele kaartstap is 164 px.

De miniaturenlijst bouwt maximaal twaalf zichtbare kaarten in plaats van één widget per PDF-pagina. Boven en onder het venster houden vaste spacers de totale scrollhoogte constant; paginawisseling scrollt de selectie in beeld. Een regressietest controleert de venstergrenzen voor elke focuspagina in een document van honderd pagina’s. De actuele plaatsing is op 7 oktober in een echt venster visueel gecontroleerd; verdere scrollinteractie en selectie blijven open.

Het Open-venster heeft nu een volledig Rust-bestandsbrowser voor lokale mappen en PDF-bestanden, met oudermap, vernieuwen en optionele handmatige padinvoer. De lijst wordt op de achtergrond gelezen en late resultaten van eerdere navigatie worden weggegooid. Een regressietest controleert dat mappen boven bestanden staan en `.PDF`/`.pdf` worden getoond terwijl andere bestanden verborgen blijven. De oorspronkelijke systeembestandskiezer, snelkoppelingen, symlinknavigatie en de precieze opmaak zijn nog niet overgezet; echte UI-invoer wacht op een werkende native vensterkoppeling.

De Rust-bestandsbrowser heeft nu een lokaal zoekveld en een gevirtualiseerde lijst: hoogstens twintig bestandsrijen zijn tegelijk Iced-widgets, ook als de map veel PDF’s bevat. Scrollspacers bewaren de lengte van de lijst; een nieuwe map of zoekterm zet de scrollpositie terug. De filter is hoofdletterongevoelig, met regressietests voor map-/PDF-resultaten en vensters op positie 200 en 999 in een lijst van duizend. De originele systeemkiezer en UI-interactie zijn nog niet visueel getest.

Op 7 oktober zijn de te grote linker miniaturen van maximaal 128×170 naar 96×128 px teruggebracht. De kaartstap is 164 px en de virtuele scrollberekening gebruikt dezelfde maat; de actieve kaart heeft een smalle accentlijn in plaats van een volledig oranje vlak. De grote PDF-pagina behoudt haar resolutie. Bij aanwezigheid van de optionele PDFium-library worden alleen navigatieminiaturen daarmee gemaakt; de geselecteerde renderer blijft verantwoordelijk voor de grote pagina. Miniaturen blijven bij rendererwissel in cache. Op de echte zevenpagina-bouwtekening kostte een miniatuur bij 96×62 px via Hayro circa 0,95 s en via PDFium circa 0,22 s aan rendertijd; dit is een lokale procesproef, geen algemene snelheidsclaim. In een echt 1200×832-venster passen nu ongeveer vier compacte kaarten waar eerder drie hoge kaarten zichtbaar waren. Ctrl+scroll boven de pagina zoomt rond de aanwijzer; gewoon scrollen verandert de zoom niet. Fit Width en Fit Page zijn nu gekoppeld aan de documentruimte en het werkelijke vensterformaat. De overige knoppen, zijpanelen en documentopmaak zijn nog niet gelijk aan de oorspronkelijke app; vooral View, Drawing en Edit & Combine PDFs vragen nog functie-voor-functie werk.

Op 8 oktober is zoomen responsiever gemaakt: de reeds gerenderde pagina schaalt onmiddellijk als tijdelijk voorbeeld; na 160 ms zonder nieuwe zoomactie begint één scherpe PDF-render op de laatste schaal. Een lopende oude render wordt niet als eindresultaat getoond. Op de echte Barn Relocation-PDF veranderde bij een Ctrl+scroll-proef de zichtbare paginagrootte binnen circa 0,34 s; de scherpere Hayro-uitvoer verving het voorbeeld later. Dat verkort de waargenomen wachttijd, niet de onderliggende rendertijd. Ook de Reader Mode-scrollhoogte volgt de tijdelijke schaal.

De vier renderkeuzes staan bovendien in een **Kernel**-keuzelijst linksonder in de statusbalk; alleen beschikbare optionele kernels verschijnen daar. De ernaast getoonde rendererstatus blijft de werkelijk gebruikte backend of Hayro-fallback aangeven.

De volledige knopreferentie is de bestaande ribbon in `HomeTab.jsx`, `ViewTab.jsx`, `DrawingTab.jsx`, `CommentTab.jsx` en `OrganizeTab.jsx`. De native Home-tab heeft nu werkende navigatie/zoom inclusief Fit Width/Fit Page; New, IFC-report, To CAD, E-mail, Raster-PDF, Hand en Screenshot zijn nog zichtbaar maar uitgeschakeld. De native View-tab toont nu de hoofdgroepen van de bestaande app; Navigation, Properties, Annotations, Reader Mode, Fullscreen en Keystrokes zijn aangesloten. Drawing toont nu alle 64 knoppen en 54 originele SVG-pictogrammen uit de Tauri-Drawing-ribbon in dezelfde groepen en volgorde. Select, Hand, Deselect, Rectangle, Line, Polyline, Text Box/Label en Delete hebben native acties; de overige knoppen zijn herkenbaar zichtbaar maar uitgeschakeld. De brede ribbon is horizontaal scrollbaar. Dit is knopinventarisatie, geen functie- of pixelpariteit. Edit & Combine PDFs bevat nog niet de bestaande bewerk-, pagina-, combineer- en watermerkgroepen. Een visueel identieke knop zonder werkende actie telt niet als functiepariteit; deze hiaten blijven expliciet open.

Openen, sluiten en afsluiten hebben nu een native Rust-waarschuwing voor niet-opgeslagen annotaties of een nog geopende teksteditor. Save As bewaart precies de annotatiesnapshot die aan de PDF-export is meegegeven; bewerkingen tijdens de asynchrone opslag blijven daardoor terecht als gewijzigd gemarkeerd. Een laat Save As-resultaat van een eerder document maakt het nieuwe document niet schoon. Een lopende Save As blokkeert wisselen en afsluiten tot de uitvoer klaar is. De window-manager-close-request loopt ook door deze controle. De dialoog en de routes moeten nog via echte UI-invoer worden getest zodra native venstertoegang beschikbaar is.


Op 7 oktober heeft de native View-tab Page Display, Display, Panels, Appearance, Compare en Window. Navigation en Properties openen de bestaande panelen; Annotations toont een scrollbare lijst van native annotaties op de huidige pagina en selecteert een item. Fullscreen werkt via de ribbon en F11: een echt X11-venster wisselde van 1200×832 naar 1920×1080 en terug. Continuous/Book/Facing, weergaverotatie, Thin Lines, Element Visibility, Annotation Layers, Toolpalette, Theme, Compare en Keystrokes zijn zichtbaar maar uitgeschakeld. Dit is nog geen volledige knop- of pixelpariteit. De View-tab toont geen generieke Document-groep meer. Een native test controleert het wisselen tussen Properties en Annotations; 59 all-feature-tests slagen.

Reader Mode gebruikt nu hetzelfde per-PDF `.readerpos.json`-bestand als de bestaande app. Inschakelen schrijft meteen pagina, zoom en verticale scrollverhouding; uitschakelen verwijdert de sidecar. Bij openen wordt de bewaarde pagina direct op de bewaarde zoom gerenderd. Tabsluiting, de eigen venster-× en een venstermanager-sluiting bewaren de positie; voor de laatste route is ook een opslag bij normaal afbouwen van de Rust-app toegevoegd. Een tijdelijke kopie van een echte zevenpagina-PDF heropende in het venster op pagina 2 bij 30% zoom; de venstermanager-sluiting bewaarde vervolgens pagina 3. Ongeldige, te grote of onleesbare sidecars worden genegeerd. Doorlopende/boekweergave is nog niet aanwezig, dus de Rust-app herstelt scrollpositie alleen in Single Page. Dit is geen renderer- of algemene UI-pariteitsclaim.

De View-knop **Keystrokes** toont nu tijdelijke toetsenlabels rechtsonder binnen de PDF-werkruimte. Herhaalde toetsen krijgen een teller en maximaal vijf labels blijven tegelijk zichtbaar; één timer verwijdert ze na 2,2 seconden. Gewone invoer die door een tekstveld of dialoog wordt verwerkt, wordt niet getoond; Ctrl/Alt-sneltoetsen blijven zichtbaar. De toetsenlistener draait alleen wanneer de knop actief is. Een echte 1200×832-X11-proef op de zevenpagina-bouwtekening toonde `G` en daarna het verdwijnen ervan; een beeldvergelijking van het paginagebied vóór en tijdens het label had nul gewijzigde pixels. De overlay blijft in de widgetstructuur ook wanneer er geen labels zijn, zodat het verschijnen ervan de horizontale PDF-scrollpositie niet opnieuw initialiseert. De overige uitgeschakelde View- en Drawing-knoppen blijven openstaande UI-pariteit.

De Drawing-knop **Line** maakt nu een echte native lijnannotatie met twee PDF-pagina-eindpunten. De overlay toont de lijn, geselecteerde eindpunten en een live sleepvoorbeeld; selecteren, verplaatsen, Undo/Redo en Save As gebruiken hetzelfde annotatiemodel als Rectangle. Save As schrijft `/Subtype /Line`, `/L` en een expliciete `/AP`-lijn. Een onafhankelijke PDF-lijninhoud is bij 0/90/180/270° over de volledige RGBA-pagina pixelgelijk aan de opgeslagen annotatie. In een echt 1200×832-venster is de lijn op een PDF getekend en geselecteerd; Save As leverde een geldige eenpagina-PDF met `/Subtype /Line` en de verwachte eindpunten. Beide blauwe eindpuntgrepen zijn nu afzonderlijk versleepbaar op de pagina; de paginageometrie, hit-test en Save As volgen de gewijzigde eindpunten. Een live X11-proef verplaatste een eindpunt, liet Undo de oude positie terugzetten en Redo de nieuwe positie herstellen. Daarna schreef Save As de gewijzigde `/L[71 133 201 43]` in een geldige 256×192-PDF. Bestaande geïmporteerde Line-annotaties bewerken is nog niet ondersteund; deze knop bewijst geen volledige Drawing- of annotatiepariteit.

De Drawing-knop **Polyline** plaatst hoekpunten per muisklik en toont tijdens het plaatsen alle segmenten plus een lijn naar de aanwijzer. Dubbelklik, Enter of rechtermuisklik rondt af; Esc annuleert. Een pad bevat 2–512 punten; ongeldige of opeenvolgend te korte segmenten worden geweigerd. Het Rust-model gebruikt lijnspecifieke hit-testing, verplaatsen en Undo/Redo. Save As schrijft een echte `/Subtype /PolyLine`, `/Vertices` en een printbare `/AP`. De opgeslagen appearance is bij 0/90/180/270° over de volledige RGBA-pagina bytegelijk aan een onafhankelijk PDF-pad. In een echt 1200×832-venster zijn drie punten geplaatst, met dubbelklik afgerond, geselecteerd en verplaatst; Save As leverde een geldige 256×192-PDF met de verplaatste vertices. Afzonderlijke hoekpunten bewerken en geïmporteerde PolyLine-annotaties bewerken blijven open.

Op 8 oktober zijn twee gemelde fouten hersteld. Ctrl+scroll wordt aan de
buitenkant van de document-scrollcontainer afgehandeld; het muispunt wordt
op het event zelf gemeten in vaste viewportcoördinaten. Daarmee blijven
snelle events vóór de volgende layout hetzelfde PDF-punt volgen. De vorige
combinatie van bewaarde inhoudscoördinaten en viewportcoördinaten kon drift
geven. Ook verticaal is nu 512 px panruimte beschikbaar, zodat een kleine
pagina tijdens zoomen rond de aanwijzer kan bewegen. De eerste plaatsing
compenseert die marge; Reader Mode slaat alleen de document-scrollpositie
op. De weergavemaat komt uit pagina-afmetingen en zoom, onafhankelijk van
afronding in een tijdelijk of definitief raster.

Op 11 oktober is de geplande wachttijd voor de **eerste** Ctrl+scroll-stap
van 60 naar 24 ms verlaagd; wielstappen terwijl de timer loopt houden de
160-ms-rustperiode. Een reeds gecachte pagina-/schaalscene start nog steeds
direct. Dit verwijdert 36 ms bewust uitgestelde starttijd bij één nieuwe
zoomstap, maar versnelt de worker, JPEG-decode of rasterisatie zelf niet.
De bestaande tests voor herhaalde wielstappen, verouderde resultaten en
zoomfocus, de workspace-allfeaturetests en de releasebuild slagen. De al
draaiende vensters gebruiken nog de oudere binary;
er is voor deze wijziging geen gemeten GUI-responstijd of algemene
snelheidsclaim.

De miniaturenplanner vult de volledige virtuele reeks van maximaal twaalf
kaarten, eerst vanaf de zichtbare focus naar beneden en daarna naar boven.
Eerder vroeg hij maar twee buren op: onderaan konden de laatste zichtbare
pagina's daardoor nooit aan de beurt komen. Bij cacheverdringing gaan eerst
kaarten buiten de huidige reeks weg. In het echte 1200×832-venster op Barn
Relocation zijn na scrollen naar onderen ook pagina's 4, 5, 6 en 7 gevuld.

In de laatste X11-regressie op de 256×192-fixture bleef het gekozen punt
onder de cursor bij in-/uitzoomen behouden. De rode rechthoek had vóór en
na de volledige zoomcyclus exact dezelfde schermbegrenzing
`[294,398]–[343,427]`. De extra wiel-events uit XTest zijn hierbij als echte
ontvangen events verwerkt; er wordt geen aanname gedaan dat één synthetische
press/release precies één event oplevert. Het oorspronkelijke gebruikersvenster
bleef tijdens de proef open. De annotatiecanvas-test borgt dat deze laag geen
eigen Ctrl+wheel-zoom meer publiceert. Alle 76 native tests en workspace-tests
met alle features slagen; de bestaande DynLex-verifier blijft bytegelijk op
47 PDF-pagina's/4.964.352 pixels plus drie v4-pagina's/147.456 pixels.


### 8 oktober: expliciet DynLex-only

De native DynLex-selectie gebruikt geen Hayro-fallback voor pagina's of
miniaturen. Unsupported pagina/zoom wordt zichtbaar gemeld; een oude bitmap
wordt niet als nieuw resultaat getoond. Ook de gemengde Hayro-body/DynLex-AP
route is uit de viewer gehaald. Backendwissels invalidëren miniaturen en late
asynchrone resultaten via generatiecontrole. Gecontroleerd in het echte venster
met een ondersteunde fill/stroke-PDF en een unsupported tekst-PDF; zie
`four-pdf-backends-2026-10-07.md`. Hayro blijft een handmatige keuze.

### 8 oktober: vergelijking met de draaiende Tauri-versie

De oorspronkelijke Tauri-debugapp is opnieuw naast een afzonderlijk native
releasevenster geopend, op `simple-fill-stroke.pdf`, 100% zoom, dezelfde X11/DPI
en 1200×832 venstergrootte. De bestaande gebruikers- en DynLex-demovensters
bleven intact. Deze vergelijking is nadrukkelijk geen bewijs van pixelpariteit
van de hele app.

In `hakan_ui.rs` zijn concrete chromeverschillen verkleind:

- De documentbalk is 28 px hoog, met `#2e2e36` achtergrond, een `#36363e`
  actieve tab en een 2 px `#d97706` onderlijn zoals `document-tabs.css`. Een lange
  naam duwt de sluitknop niet meer weg; de volledige naam staat in de tooltip.
  Een stip geeft de bestaande echte niet-opgeslagen annotatiestatus weer.
- Thumbnails heeft een vaste header en een werkende sluitknop; de titel scrolt
  niet meer met de pagina's weg. De achtergrond volgt `--theme-panel-bg`.
- Properties heeft compacte DOCUMENT/ANNOTATIONS-secties en uitgelijnde echte
  bestands-, pagina-, formaat-, zoom- en annotatiegegevens. Lange inhoud is
  scrollbaar. De header sluit het bestaande paneel via dezelfde View-actie.
- De ruimte om de PDF gebruikt nu `#e0e0e0`, dezelfde achtergrond die Tauri's
  `js/pdf/pdf-viewport.js` schildert. Het paginaraster en de zoomankers veranderen
  hierdoor niet.

Resterende zichtbare verschillen: de Tauri-referentie heeft een linker
navigatie-iconenstrook, aparte tekenpalette en symbolenpalette; die zijn nog
niet functioneel geport. Het native Properties-paneel is 230 px tegenover
300 px in de referentie. Compacte native thumbnails blijven op de door de
gebruiker gevraagde 96×128 px. PDF-plaatsing, lettermetriek, titlebar,
file-tabbreedte, meerdocumenttabs en verschillende ribbonfuncties zijn nog
ongelijk. De native tab gebruikt voorlopig maximaal 29 Unicode-tekens in de
zichtbare naam, geen pixelgemeten CSS-ellipsis. Er zijn geen werkende functies
geclaimd voor de ontbrekende palettes.

De 78 native all-feature-tests slagen en de releasebuild slaagt. Lokale
referentieopname: `/tmp/opds-tauri-parity-reference.png`; de afzonderlijke
native proef is vastgelegd in `/tmp/opds-native-parity-after.png`.

De schermproef bevestigt dezelfde horizontale chromegrenzen op y=30/58/152/180
(titel, tabs, ribbon, documentbalk) in beide 1200×832-opnamen. Drie vlakke
kleuren zijn daadwerkelijk uit de screenshots gemeten: PDF-achtergrond
`(224,224,224)`, documentbalk `(46,46,54)` en Properties `(68,68,76)` zijn nu
identiek. Dit zegt niets over alle overige pixels. Beide nieuwe header-×-knoppen
zijn via XTest aangeklikt en de panelen zijn via View teruggeopend. De lokale
vergelijking staat in `/tmp/opds-ui-parity-comparison-2026-10-08.png`.

## Slepen om te pannen (9 oktober)

De Home-knop **Hand** is nu functioneel: linkermuisknop ingedrukt houden en
slepen verplaatst de document-scrollpositie. Met de middelste muisknop werkt
dezelfde beweging bij ieder geselecteerd gereedschap. Loslaten of het
venster verlaten beëindigt de beweging. Select blijft annotaties selecteren;
linkerslepen in Select verschuift de pagina niet. De bestaande Ctrl+scroll-
zoom blijft afzonderlijk afgehandeld en pannen veroorzaakt geen nieuwe
PDF-renderaanvraag.

Een apart 1200×832-X11-venster met de originele Albany-pagina 30 en
Renderer: DynLex is met echte muisdruk-/beweeg-/loslaatevents getest:
middelste en Hand-linkerdrag verplaatsten de pagina, terwijl Select-linkerdrag
haar positie behield. Er is geen annotatie of PDF-bestand gewijzigd.
De 80 native all-feature-tests, releasebuild en formattering slagen. Dit
bewijst de geteste interactie, geen volledige UI-pixelpariteit of gemeten
panlatentie.

## Drawing-GUI-proef naast Tauri (13 oktober)

Beide apps zijn met hetzelfde ongewijzigde `simple-fill-stroke.pdf` in aparte
1200×832 X11-vensters bediend. De Tauri-Drawing-groepen zijn in Rust nu in
dezelfde volgorde en met overeenkomstige breedtegrenzen zichtbaar. De derde
rij pictogrammen is niet meer afgesneden. De groep Edit verschijnt alleen bij
een geselecteerde annotatie, zoals in Tauri. Bij geselecteerd gereedschap en
document verschillen nog 16.184 van 114.000 RGB-pixels in het ribbongebied
`y=58..153`; dit is dus nog geen pixelpariteit. Reproduceerbare opnamen staan
in `/tmp/tauri-drawing-baseline.png` en `/tmp/rust-drawing-widths.png`.

Via echte muis- en toetsgebeurtenissen is op beide apps een rechthoek
getekend en met Ctrl+Z/Ctrl+Y hersteld. Rust accepteert nu ook Ctrl+Shift+Z;
voorheen was de sneltoetslistener alleen actief wanneer de optionele
toetsaanslagen-overlay zichtbaar was. Delete geeft in beide apps eerst een
bevestiging. Rust weigert bevestiging wanneer documentversie of selectie
intussen veranderde; annuleren laat de annotatie intact. De Tauri-lijn vraagt
een startklik en eindklik; Rust accepteert nu dezelfde twee klikken, toont
een concept en wist dat met Escape of bij gereedschap-/pagina-/zoombinding.
Slepen blijft ook bruikbaar in Rust. De laatste release-GUI-proef zette via
twee klikken de native annotatieteller op 1; Ctrl+Z bracht hem terug naar 0.

Nog zichtbaar/functioneel verschillend: Tauri heeft bij Line een
`Continuous`-optie en veel uitgebreidere eigenschappen. Rust heeft slechts
9 van de 64 getoonde Drawing-knoppen aan een echte actie gekoppeld: Select,
Hand, Deselect, Line, Rectangle, Polyline, Text Box, Label en Delete. De
overige blijven uitgeschakeld; de 12 Edit-knoppen zijn alleen bij selectie
zichtbaar. De dialoogvorm, iconenpositie binnen sommige groepen,
standaardstijl en de rest van de UI zijn evenmin pixelgelijk.
De Arrow-knop is expliciet via beide GUI's geprobeerd: Tauri maakt met twee
klikken een pijl (annotatieteller 1), terwijl Rust "not yet available" toont
en bij het vorige gereedschap blijft. Dit is een functioneel resterend gat,
geen renderfout.

Aanvullende geïsoleerde 1200×832-GUI-proef: de release-Rust-app op hetzelfde
`simple-fill-stroke.pdf` schakelde via een echte muisklik naar Drawing. De
Line-knop werd aangeklikt en een lijn op de PDF gesleept; de teller werd 1.
Een klik op Undo bracht de teller terug naar 0, Redo naar 1. Opnamen staan in
`/tmp/opds-gui-smoke-drawing2.png`, `-line.png`, `-undo.png` en `-redo.png`.
Alleen het eigen proefvenster is daarna beëindigd; deze proef raakt geen
gebruikersdocument of bestaande appvensters en test geen andere knoppen.
