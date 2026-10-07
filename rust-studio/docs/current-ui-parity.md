# Doel: huidige Open PDF Studio-interface exact overzetten

De bestaande Open PDF Studio-versie op branch `feat/pdf-export-snippets-pdfjs6` is de **visuele en functionele referentie**. Op verzoek van de gebruiker is de UI-code van Hakan's OpenCADStudio nu de implementatiebasis. De ribbonhelpers en iconen worden daadwerkelijk overgenomen en aan PDF-commands gekoppeld.

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

De volledige knopreferentie is de bestaande ribbon in `HomeTab.jsx`, `ViewTab.jsx`, `DrawingTab.jsx`, `CommentTab.jsx` en `OrganizeTab.jsx`. De native Home-tab heeft nu werkende navigatie/zoom inclusief Fit Width/Fit Page; New, IFC-report, To CAD, E-mail, Raster-PDF, Hand en Screenshot zijn nog zichtbaar maar uitgeschakeld. De native View-tab toont nu de hoofdgroepen van de bestaande app; Navigation, Properties, Annotations, Reader Mode, Fullscreen en Keystrokes zijn aangesloten. Drawing heeft werkende Rectangle, Line en Polyline; Circle en de overige tekengereedschappen ontbreken. Edit & Combine PDFs bevat nog niet de bestaande bewerk-, pagina-, combineer- en watermerkgroepen. Een visueel identieke knop zonder werkende actie telt niet als functiepariteit; deze hiaten blijven expliciet open.

Openen, sluiten en afsluiten hebben nu een native Rust-waarschuwing voor niet-opgeslagen annotaties of een nog geopende teksteditor. Save As bewaart precies de annotatiesnapshot die aan de PDF-export is meegegeven; bewerkingen tijdens de asynchrone opslag blijven daardoor terecht als gewijzigd gemarkeerd. Een laat Save As-resultaat van een eerder document maakt het nieuwe document niet schoon. Een lopende Save As blokkeert wisselen en afsluiten tot de uitvoer klaar is. De window-manager-close-request loopt ook door deze controle. De dialoog en de routes moeten nog via echte UI-invoer worden getest zodra native venstertoegang beschikbaar is.


Op 7 oktober heeft de native View-tab Page Display, Display, Panels, Appearance, Compare en Window. Navigation en Properties openen de bestaande panelen; Annotations toont een scrollbare lijst van native annotaties op de huidige pagina en selecteert een item. Fullscreen werkt via de ribbon en F11: een echt X11-venster wisselde van 1200×832 naar 1920×1080 en terug. Continuous/Book/Facing, weergaverotatie, Thin Lines, Element Visibility, Annotation Layers, Toolpalette, Theme, Compare en Keystrokes zijn zichtbaar maar uitgeschakeld. Dit is nog geen volledige knop- of pixelpariteit. De View-tab toont geen generieke Document-groep meer. Een native test controleert het wisselen tussen Properties en Annotations; 59 all-feature-tests slagen.

Reader Mode gebruikt nu hetzelfde per-PDF `.readerpos.json`-bestand als de bestaande app. Inschakelen schrijft meteen pagina, zoom en verticale scrollverhouding; uitschakelen verwijdert de sidecar. Bij openen wordt de bewaarde pagina direct op de bewaarde zoom gerenderd. Tabsluiting, de eigen venster-× en een venstermanager-sluiting bewaren de positie; voor de laatste route is ook een opslag bij normaal afbouwen van de Rust-app toegevoegd. Een tijdelijke kopie van een echte zevenpagina-PDF heropende in het venster op pagina 2 bij 30% zoom; de venstermanager-sluiting bewaarde vervolgens pagina 3. Ongeldige, te grote of onleesbare sidecars worden genegeerd. Doorlopende/boekweergave is nog niet aanwezig, dus de Rust-app herstelt scrollpositie alleen in Single Page. Dit is geen renderer- of algemene UI-pariteitsclaim.

De View-knop **Keystrokes** toont nu tijdelijke toetsenlabels rechtsonder binnen de PDF-werkruimte. Herhaalde toetsen krijgen een teller en maximaal vijf labels blijven tegelijk zichtbaar; één timer verwijdert ze na 2,2 seconden. Gewone invoer die door een tekstveld of dialoog wordt verwerkt, wordt niet getoond; Ctrl/Alt-sneltoetsen blijven zichtbaar. De toetsenlistener draait alleen wanneer de knop actief is. Een echte 1200×832-X11-proef op de zevenpagina-bouwtekening toonde `G` en daarna het verdwijnen ervan; een beeldvergelijking van het paginagebied vóór en tijdens het label had nul gewijzigde pixels. De overlay blijft in de widgetstructuur ook wanneer er geen labels zijn, zodat het verschijnen ervan de horizontale PDF-scrollpositie niet opnieuw initialiseert. De overige uitgeschakelde View- en Drawing-knoppen blijven openstaande UI-pariteit.

De Drawing-knop **Line** maakt nu een echte native lijnannotatie met twee PDF-pagina-eindpunten. De overlay toont de lijn, geselecteerde eindpunten en een live sleepvoorbeeld; selecteren, verplaatsen, Undo/Redo en Save As gebruiken hetzelfde annotatiemodel als Rectangle. Save As schrijft `/Subtype /Line`, `/L` en een expliciete `/AP`-lijn. Een onafhankelijke PDF-lijninhoud is bij 0/90/180/270° over de volledige RGBA-pagina pixelgelijk aan de opgeslagen annotatie. In een echt 1200×832-venster is de lijn op een PDF getekend en geselecteerd; Save As leverde een geldige eenpagina-PDF met `/Subtype /Line` en de verwachte eindpunten. Beide blauwe eindpuntgrepen zijn nu afzonderlijk versleepbaar op de pagina; de paginageometrie, hit-test en Save As volgen de gewijzigde eindpunten. Een live X11-proef verplaatste een eindpunt, liet Undo de oude positie terugzetten en Redo de nieuwe positie herstellen. Daarna schreef Save As de gewijzigde `/L[71 133 201 43]` in een geldige 256×192-PDF. Bestaande geïmporteerde Line-annotaties bewerken is nog niet ondersteund; deze knop bewijst geen volledige Drawing- of annotatiepariteit.

De Drawing-knop **Polyline** plaatst hoekpunten per muisklik en toont tijdens het plaatsen alle segmenten plus een lijn naar de aanwijzer. Dubbelklik, Enter of rechtermuisklik rondt af; Esc annuleert. Een pad bevat 2–64 punten; ongeldige of opeenvolgend te korte segmenten worden geweigerd. Het Rust-model gebruikt lijnspecifieke hit-testing, verplaatsen en Undo/Redo. Save As schrijft een echte `/Subtype /PolyLine`, `/Vertices` en een printbare `/AP`. De opgeslagen appearance is bij 0/90/180/270° over de volledige RGBA-pagina bytegelijk aan een onafhankelijk PDF-pad. In een echt 1200×832-venster zijn drie punten geplaatst, met dubbelklik afgerond, geselecteerd en verplaatst; Save As leverde een geldige 256×192-PDF met de verplaatste vertices. Afzonderlijke hoekpunten bewerken en geïmporteerde PolyLine-annotaties bewerken blijven open.

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
