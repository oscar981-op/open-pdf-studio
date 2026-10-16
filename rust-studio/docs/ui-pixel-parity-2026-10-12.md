# UI-vergelijking met de Tauri-app, 12 oktober 2026

## Geselecteerde vormkleur, 16 oktober

De native Format-tab verschijnt nu ook bij een geselecteerde vorm. De actieve
kleurswatch en het eigenschappenpaneel tonen de werkelijke annotatiekleur; de
swatches wijzigen haar via een undo-bare documentedit. Een geïsoleerde
release-opname na een echte klik op Blue staat in
`/tmp/opds-format-color-after-click.png`. Er is hiervoor nog geen gepaarde
Tauri-opname of genormaliseerde pixelscore. Stroke, fill en opaciteit zijn in
Tauri afzonderlijke eigenschappen; de Rust-vorm gebruikt voorlopig één kleur.

## Fit Page zonder extra marge, 14 oktober

Tauri's `pdf-viewport.js::computeFitZoom` gebruikt bij Fit Page standaard
`padding = 0`. De Rust-berekening trok daar bovenop 48 px van zowel de
beschikbare breedte als hoogte af. Die extra aftrek is verwijderd; de
bestaande scrollbare pagina behoudt haar panruimte. In een afzonderlijke
1920×1040-Rust-opname van `simple-fill-stroke.pdf` met PDFium en de Drawing-tab
vult de pagina nu de beschikbare hoogte bij 436% (`/tmp/opds-oct14-fit-zero.png`).
Alle 114 actieve app-tests slagen. De Tauri-opname op dezelfde venstermaat
heeft een extra standaard-app-waarschuwingsbalk en daardoor een andere
werkhoogte; deze twee opnamen leveren geen genormaliseerde pixelpariteitsscore.
De wijziging geeft nog geen DynLex-viewport/1600%-ondersteuning.

## Uitgevouwen Drawing op volledig scherm, 13 oktober

Beide bestaande vensters stonden op 1920×1040. De Tauri-ribbon is vastgelegd in
`/tmp/opds-tauri-fullscreen-2026-10-13.png`; de actuele native release is met
`OPDS_UI_SCREENSHOT_SIZE=1920x1040` en
`OPDS_UI_SCREENSHOT_DRAWING_TAB=1` vastgelegd in
`/tmp/opds-rust-fullscreen-final2.png`. De ribbonhelften staan naast elkaar in
`/tmp/opds-drawing-fullscreen-final-comparison.png`. Selection, Snippet, Scale,
Annotate, Clipboard en Collection gebruiken nu weer grote of gelabelde
knopgroepen zoals de Tauri-ribbon; de groepsgrenzen liggen vrijwel op dezelfde
plaatsen. Draw en Modify blijven compacte icoonroosters. De vensters hebben
verschillende geselecteerde gereedschappen en pagina-zooms, dus de ruwe
pixelafwijking van deze opname is geen genormaliseerde totaalscore. De
uitgeschakelde Drawing-acties en het symboolpalet verschillen functioneel en
visueel nog duidelijk.

## Live Annotation-vergelijking, 13 oktober

De originele Tauri-app is in een afzonderlijk venster geopend; de bestaande
Rust-gebruikerssessie is ongemoeid gelaten. Op dezelfde lege 1200×832-
venstergrootte staan de acht Annotation-groepen en hun iconen in de native
release. De onbewerkte vergelijking van
`/tmp/opds-original-annotation-window.png` met
`/tmp/opds-rust-annotation-compare.rgba` telt 15.339/114.000 afwijkende
ribbonpixels (13,455%) en 223.307/998.400 over het hele venster. De originele
app toont hier ook een standaard-app-waarschuwingsbalk die de native app niet
heeft; daarom is de hele-venstermeting geen zuivere ribbonmeting. De
uitgeschakelde iconen/labels en de tabselectie zijn nog zichtbaar anders.
De meeste Annotation-acties zijn in Rust bewust uitgeschakeld zolang hun
documenttransactie ontbreekt; gelijke knoppen alleen bewijzen geen pariteit.
De optionele `OPDS_UI_SCREENSHOT_ANNOTATION_TAB=1`-modus legt voortaan deze
native tab in een eigen testvenster vast.

Bij een strikte DynLex-afwijzing blijft de pagina leeg en verschijnt nu een
expliciete handmatige knop voor een andere renderer. De knop gebruikt PDFium
als die beschikbaar is en anders Hayro; hij voert dezelfde selectieactie uit
als de rendererkeuze linksonder. Dit is geen automatische DynLex-fallback.
De fouttekst wordt binnen het smalle middenpaneel afgebroken. Het aparte
testvenster op de ongewijzigde `Tekst.pdf` pagina 1 staat in
`/tmp/opds-dynlex-unsupported-action-v2.png`.

De bestaande Tauri-app blijft de referentie. De twee eerder gemaakte
1200×832-opnamen van dezelfde `simple-fill-stroke.pdf` zijn opnieuw zonder
maskers vergeleken met `scripts/compare_ui_pixels.py`. Dit is een **baseline
van vóór de wijzigingen hieronder**, geen score voor de huidige Rust-code.
Pixelgelijkheid betekent identieke RGBA-waarden op dezelfde coördinaat;
deze schermvergelijking zegt niets over PDF-pixelpariteit.

| Gebied | Verschillende pixels, oude baseline | Zichtbare oorzaak |
| --- | ---: | --- |
| Hele venster | 536.581/998.400 (53,744%) | Meerdere panelen en positie wijken af |
| Titelbalk | 35.998/36.000 | Andere iconen, knopmaten, achtergrond en tekst |
| Lint | 16.509/114.000 | Groeps-/iconafstanden en selectie |
| Linker iconenstrook | 18.704/18.900 | Ontbreekt in Rust |
| Tekenwerkbalk | 53.550/53.550 | Ontbreekt in Rust |
| Gereedschapspalet rechts | 138.600/138.600 | Ontbreekt in Rust |
| Eigenschappenpaneel | 86.658/189.000 | Breedte, secties en inhoud verschillen |
| Statusbalk | 26.400/26.400 | Indeling en bediening verschillen |

De bestaande beelden zijn `/tmp/opds-tauri-parity-reference.png` en
`/tmp/opds-native-parity-after.png`; het reproduceerbare baseline-JSON en
de rode diff staan onder `/tmp/opds-tauri-rust-ui-baseline.json` en
`/tmp/opds-tauri-rust-ui-diff.png`. De opnamen hebben dezelfde PDF, schaal,
venstermaat en scherm-DPI.

De Rust-app heeft nu een optionele eigen screenshotmodus via
`OPDS_UI_SCREENSHOT_RGBA=/tmp/opds-rust-main.rgba`. Zij schrijft ook een
`.size`-bestand en sluit alleen haar eigen, nieuwe testvenster. Met
`OPDS_UI_SCREENSHOT_FILE_MENU=1` start zij direct op de File-pagina.
`compare_ui_pixels.py` leest PNG of deze RGBA+size-vorm. De ongewijzigde
Tauri-hoofdopname is tegen de **actuele** Rust-release op dezelfde PDF en
1200×832 vergeleken. Na de titelbalkwijziging waren er 506.359/998.400
verschillende pixels (50,718%); de titelbalk 5.776/36.000 (16,044%). Voor
de wijziging waren dat respectievelijk 536.581 (53,744%) en 35.998 (99,994%). De
29-pixel-Tauri-kleurverloopstroken plus de 1-pixel-onderrand zijn op een
lege titelpixel nu exact; resterende titelbalkverschillen zitten onder meer
in iconen, tekst en uitlijning. Ruwe actuele resultaten en diff:
`/tmp/opds-tauri-rust-ui-stripes.json` en
`/tmp/opds-tauri-rust-ui-stripes-diff.png`.

Vervolgens zijn de 30px-linker iconenstrook, 170px-miniaturenpaneel,
84px-tekenwerkbalk, 220px-symboolpalet en 300px-eigenschappenpaneel op de
Tauri-posities gezet. De PDF opent in het midden van de resterende viewport;
de scrolloffset behoudt de panmarge. De echte native selecteerbare
annotatiegereedschappen zitten ook in de tekenwerkbalk. Daarna is de
statusbalk 22 px hoog gemaakt en kregen die balk en de linker strook de
Tauri-achtergrondkleur. De laatste release-opname heeft
**202.156/998.400 afwijkende pixels (20,248%)**. Hierbij heeft Rust een
300px-eigenschappenpaneel met bestandspad, documentgegevens en secties op de
Tauri-plaatsen. De vorige tussenstap zonder die inhoud scoorde 199.974
(20,029%); de vollediger inhoud vergroot hier dus de ruwe pixelafwijking
licht. De werkruimte in de huidige meting is 138.597/756.000 afwijkend;
het pagina-/tekengebied blijft 7.863/248.850 (3,160%). Onbewerkte meting
en diff: `/tmp/opds-tauri-rust-ui-props2.json` en
`/tmp/opds-tauri-rust-ui-props2-diff.png`. Deze opname gebruikt hetzelfde
absolute PDF-pad als de referentie. De symboolcategorieën zijn op dit
moment slechts visuele labels; het Tauri-palet kan zoeken en symbolen plaatsen,
de Rust-variant nog niet. Daardoor is dit nadrukkelijk geen pixelperfecte
of functioneel volledige UI.

De linker iconenstrook en de tekenwerkbalk gebruiken nu de oorspronkelijke
Tauri-SVG-paden uit `leftPanelIcons.js`, `ToolPalette.jsx` en `ribbonIcons.js`,
als lokale SVG-assets in de Rust-app. Hun maat en groepsvolgorde volgen de
32px-linkertabs en het 80px-palet met twee kolommen; de 4px-resizegreep tussen
miniaturen en palet is zichtbaar. Alleen al bestaande native acties zijn
klikbaar; de overige iconen hebben een `Not yet available`-tooltip. De
herhaalde volledige 1200×832-meting heeft nu **201.875/998.400** afwijkende
pixels (20,220%), waarvan 2.893/18.900 in de linker strook en 9.259/53.550
in de tekenwerkbalk. De verandering is klein in ruwe pixeltelling, maar
vervangt de generieke tekens door brongetrouwe iconen. Het nieuwe rapport en
de diff staan onder `/tmp/opds-rust-tool-icons24.json` en
`/tmp/opds-rust-tool-icons24-diff.png`.

De miniatuur is vervolgens op de Tauri-bronafmetingen afgestemd: de
landschappagina krijgt maximaal 140 px breedte, de afbeelding begint bovenin
de actieve kaart en de kaart gebruikt de oorspronkelijke donkere actieve
achtergrond en oranje 2px-rand. De rijhoogte is 144 px, zodat de volgende
miniatuur niet door een te hoge kaart wordt weggedrukt. De miniaturenregio
daalde van 34.680 in de vorige opname naar 23.629/107.100 afwijkende
pixels. De tussentijdse hele-vensterscore in
`/tmp/opds-rust-thumbs-shrink.json` gebruikte een relatief PDF-pad en is
daarom niet direct vergelijkbaar met de absolute-pad-Tauri-referentie;
de regiometing van de miniatuur blijft bruikbaar. De afbeelding staat in
`/tmp/opds-rust-thumbs-shrink.png`.
Dit is alleen visuele afstemming; de Tauri-functie voor pagina's toevoegen
en miniaturen herordenen is in Rust niet geïmplementeerd.

Het File-menu is daarnaast live vergeleken met de lokale Solid/Tauri-frontend.
De oorspronkelijke bron heeft een titelbalk van 30 px, een File-zijbalk van
260 px, een File-terugknop van 45 px, menurijen van 44 px,
Open-subnavigatie van 200 px en standaard het paneel `Recent Files`.
De eerste Rust-versie bedekte de titelbalk volledig, was 275 px breed en
toonde een eigen, inhoudelijk afwijkende Open-pagina. Rust houdt de titelbalk
nu zichtbaar en volgt de 260/200 px-structuur, de Open-selectie, de
menugroepen en de recente-bestanden-indeling. De Tauri-bron is
`open-pdf-studio/js/solid/components/app-menu/AppMenu.jsx`, `OpenPanel.jsx`
en `styles/app-menu.css`; ook de referentie-webtab gebruikt die componenten.
De taal van de live referentie was Nederlands, die van de Rust-shell Engels.

De File-pagina toont nu een echte bewaarde MRU en een werkend zoekveld. De
nieuwe 1200×832-testopname met één geopende fixture staat in
`/tmp/opds-rust-recent-current.png`; het venster sloot zichzelf na de opname.
De rij opent via dezelfde documentroute en respecteert de waarschuwing voor
onopgeslagen werk. De screenshotmodus raakt de bewaarde lijst niet; die
testopname toont dus alleen het daarin geopende fixturebestand. De rij heeft
nu werkende knoppen voor vastzetten, losmaken en individueel verwijderen; de
bewaarde lijst ondersteunt ook het oude stringformaat. Beide recente-bestanden-
koppen klappen nu op klik afzonderlijk in en uit, net als in Tauri. Een
eerlijke gepaarde File-menu-pixelmeting tegen
de Tauri-referentie met dezelfde recente-bestandenstaat ontbreekt nog.

De titelbalk heeft nu de ontbrekende snelacties in dezelfde volgorde als de
Tauri-bron: Open, Save, Save As, Print, Raster PDF, Preferences, Undo/Redo en
vorige/volgende weergave. Window-controls zijn 40×30 px volgens
`styles/titlebar.css`. Acties die nog geen native workflow hebben blijven
zichtbaar maar uitgeschakeld; hun aanwezigheid is geen functiepariteit.
Het titelbalklogo wordt nu rechtstreeks uit dezelfde 128×128 `public/icon.png`
als in Tauri naar 18 px geschaald. De eerdere 16×16-afgeleide werd eerst
naar 18 px vergroot en zag er daardoor grof uit. Het venstericoon gebruikte
al dezelfde originele PNG. Een apart 1200×832-testvenster leverde een nieuwe
opname; in de linkerbovenhoek van 31×30 px daalde de afwijking tegen de
bestaande Tauri-referentie van 335 naar 285 pixels. Er blijven dus zichtbare
verschillen. De volledige actuele debug-opname op dezelfde PDF en
1200×832 (`/tmp/opds-logo-current.rgba`) meet 172.468/998.400
afwijkende pixels (17,274%), waarvan
5.726/36.000 in de titelbalk. Dit is geen volledige titelbalk- of
UI-pixelpariteit.
De specifieke hover-achtergronden van minimaliseren, maximaliseren en sluiten
waren al in een afzonderlijk testvenster gecontroleerd. Een nieuwe opname
van de File-pagina staat in `/tmp/opds-rust-file-final.png`. Zij is visueel
naast het live Solid/Tauri-menu gelegd. Exacte File-menu-pixelstatistiek is
nog niet mogelijk omdat de live referentieopname niet als lokaal bestand is
vastgelegd. Pixel-perfecte titelbalk- of File-menu-pariteit is niet geclaimd.

Openstaand voor echte pixelpariteit: een actuele Tauri-opname van de
File-pagina en twee opnamen op dezelfde taal; daarna per regio de
geometrie, kleuren, iconen, fonts en geselecteerde staat bijstellen. Vooral
de linker iconenstrook, symboleninhoud, eigenschappenpaneel en statusbalk
verschillen nog. De File-pagina's
Sessions, URL, Places, import/export en overige native workflows ontbreken
functioneel nog; tijdelijk uitgeschakelde knoppen mogen niet als werkend
worden behandeld. Gebruikersvensters en onopgeslagen werk zijn niet gesloten
of herstart.

De File-knop **About** en de About-knop op de Help-tab openen nu hetzelfde
native dialoog met de werkelijke Rust-pakketversie, licentie en bronproject.
Kruisje, Close en Esc sluiten het dialoog zonder documentwijziging. Een apart
1200×832-testvenster leverde `/tmp/opds-rust-about.png`; de test sloot alleen
dat venster. De bronlinks zijn daar voorlopig tekst en geen werkende externe
acties. Een Tauri-opname van dit dialoog en gepaarde pixelmeting ontbreken,
dus dit is functievoortgang, geen About-pixelpariteit.

De referentie-DOM bevestigde verder een documenttabbalk van 28 px,
statusbalk van 22 px en eigenschappenpaneel van 300 px. Rust gebruikt nu de
300px-paneelbreedte en toont de secties, maar de metadatawaarden zijn behalve
de PDF-versie nog placeholders; lettertypen, regelafstanden en kolomposities
zijn nog niet gelijk.
De Rust-statusbalk wijkt nog voor 8.925/26.400 pixels af. De gecentreerde beginpositie is
gecontroleerd op de 1200×832-fixture; zoomfocus, miniaturen en opgeslagen
scrollpositie blijven afzonderlijke regressiepunten bij andere venstermaten.

De statusbalk heeft nu, naast de gevraagde rendererkeuze linksonder, de
werkelijke actieve tool en annotatietelling links, en eerste/vorige/volgende/
laatste pagina plus uit-/inzoomen als werkende knoppen in het midden. De
duplicaten Undo/Redo zijn uit de statusbalk verwijderd; ze staan al in de
titelbalk. De vaste zoomtekst is nog geen bewerkbaar Tauri-zoomveld en de
paginaweergavemodi ontbreken nog. Met exact hetzelfde absolute PDF-pad als
de Tauri-referentie wijkt de statusregio **9.332/26.400** pixels af
(eerder 8.925) en het volledige venster **191.231/998.400 (19,154%)**
(eerder 201.875). De totaalverbetering komt van de miniatuur; de extra
navigatie is functioneel nuttig maar vergroot de statusregioafwijking nog.
`/tmp/opds-rust-status-center-abs.json` bewaart de ruwe vergelijking.

De eigenschappenrijen gebruiken nu dezelfde **70px-labelkolom** als
`styles/properties-panel.css` van Tauri. Daardoor begint de echte
bestandspadwaarde op de juiste x-positie en loopt zij in vier in plaats
van vijf regels. Met ongewijzigde referentie en absoluut PDF-pad daalde
de afwijking van het eigenschappenpaneel van 55.163 naar
**51.242/189.000** pixels; het hele venster daalde naar
**187.310/998.400 (18,761%)**. Ruwe vergelijking:
`/tmp/opds-rust-prop-label70.json`. De inhoudelijke metadatavelden
blijven voorlopig ongeïmplementeerd; de kolomcorrectie bewijst alleen
visuele vooruitgang.

De Document-, Metadata- en Annotations-sectiekoppen van het Rust-paneel zijn
nu daadwerkelijk inklapbaar, met dezelfde pijlrichting en een hoverstatus
als de Tauri-secties. De sectiestaat blijft behouden bij openen/sluiten van
het paneel. Op de ongewijzigde 1200×832-referentie is dit vrijwel neutraal:
**187.317/998.400 (18,762%)** afwijkende pixels en
**51.249/189.000** in het eigenschappenpaneel. Dit is functiepariteit voor
deze drie sectieknoppen, nog geen metadata- of totale pixelpariteit.

Vervolgens is de eigenschappenindeling op de Tauri-CSS afgestemd: elke
sectie heeft 4px ruimte onder de inhoud, `On Page` toont het echte
modelaantal met paginanummer en de annotatiehint staat gecentreerd en
cursief buiten de inklapbare sectie. Op dezelfde referentie daalde de
afwijking naar **48.514/189.000** in het eigenschappenpaneel en
**184.582/998.400 (18,488%)** over het hele venster. Dit maakt de
sectieposities beter gelijk, maar zegt niets over ontbrekende PDF-metadata,
geïmporteerde annotatietellingen of de andere UI-delen.

De vijf metadatavelden Title/Author/Subject/Creator/Producer lezen nu de
PDF-Info-dictionary uit de al geopende `ParsedPdf`. PDFDocEncoding,
UTF-16BE en UTF-8 met BOM worden gedecodeerd; het effectieve PDF-versienummer
komt uit dezelfde parser, inclusief een eventuele catalogusoverride.
Ontbrekende of ongeldige waarden blijven `-`; velden boven 4096 bronbytes
worden niet in de UI geplaatst. Een gegenereerde PDF met UTF-16-titel,
Auteur, Creator en versie 1.7 passeert de engine-probe-test. De bestaande
pixelreferentie bevat geen Info-waarden, dus deze functionele correctie
verandert de 1200×832-score niet. XMP-metadata en geïmporteerde annotaties
zijn hiermee nog niet gelijkgetrokken met Tauri.

De tabstrook boven het lint gebruikt nu Tauri's verticale verloop van
`#44444C` naar `#3E3E46` in plaats van een effen achtergrond. Op dezelfde
referentie daalde de tabregio van 28.994 naar **18.876/33.600**
afwijkende pixels en het hele venster van 184.582 naar
**174.464/998.400 (17,474%)**. De resterende tabafwijking zit onder meer
in tabgeometrie, tekst en de onderste rand; de verbeterde score is geen
claim van pixelperfecte UI-pariteit.

De Properties-kop volgt nu de Tauri-bediening: een linker
paneel-inklapknop vervangt het misleidende sluitkruis. De kop heeft het
bronkleurverloop en laat de bestaande property-toggle intact. Ingeklapt
blijft een klikbare 28px-strook met werkelijk geroteerde tekst staan;
de fit-/centreerberekening houdt die 28px ruimte aan. Een afzonderlijke
testopname via `OPDS_UI_SCREENSHOT_COLLAPSED_PROPERTIES=1` toont de strook,
zonder een gebruikersvenster te wijzigen. Op de uitgeklapte referentie
daalde het eigenschappenpaneel van 48.514 naar **46.568/189.000**
afwijkende pixels en het venster naar **172.518/998.400 (17,279%)**.
Voor de ingeklapte staat ontbreekt nog een Tauri-pixelreferentie; die staat
daarom alleen functioneel en visueel, niet pixelexact, vast.

De drie vensterknoppen rechtsboven gebruiken nu de oorspronkelijke Tauri-SVG-
geometrie in plaats van Unicode-tekens. De middelste knop wisselt zijn
maximaliseer-/herstelicoon op basis van de door het venstersysteem gemelde
toestand, ook na een externe vensterwijziging. De sluitknop houdt de bestaande
waarschuwing voor onopgeslagen werk. De geïsoleerde 1200×832-opname na deze
wijziging wijkt **173.810/998.400 (17,409%)** pixels af; de titelbalk
**5.710/36.000 (15,861%)**. Deze opname gebruikt dezelfde absolute PDF,
taal en grootte als de Tauri-referentie. De kleine verandering in de
totaalscore bewijst geen algemene UI-pariteit.

Wanneer een native dialoog openstaat, zijn nu ook de vensterknoppen,
beschikbare snelle titelbalkacties en het slepen/dubbelklikken op de titel
uitgeschakeld, zoals bij Tauri's `dialogs-open`-toestand. Een geïsoleerde
About-opname bevestigt de gedimde venstericonen; de 103 app-tests slagen.
De gewone 1200×832-baseline verandert hierdoor niet.
