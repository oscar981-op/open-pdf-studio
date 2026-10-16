# MCP en API-migratie, 12 oktober 2026

`app_move_annotation` verplaatst nu één native annotatie op de getoonde pagina
met `dx`/`dy` in PDF-punten. De externe actie en GUI-sleep gebruiken dezelfde
begrensde geometriefunctie; lijn-, pijl- en polylijnpunten schuiven met de
werkelijke, aan de pagina begrensde delta mee. De exacte documentversie en
annotatie-ID zijn verplicht. Ongeldige, onbekende, stale, geannuleerde en
onveranderde acties laten de annotatie ongemoeid. De vervanging gaat via
`DocumentCommand` en is in één stap undo-/redo-baar. Een geïsoleerde
release-GUI/HTTP-proef tekende een ellips met ID `0`, verwierp een stale
verplaatsing en wijzigde de positie exact met 12×8 punten; undo/redo gaf
revisies 2→3→4 en de pagina bleef zichtbaar. Opname:
`/tmp/opds-mcp-move-annotation-2026-10-16.png`. De externe actie biedt
ook formaatwijziging voor vlakke vormen en tekst; vrije puntgeometrie en
eigenschapswijziging zijn nog niet extern beschikbaar.

`app_resize_annotation` gebruikt `x`, `y`, `width` en `height` in PDF-punten
voor rechthoek, ellips, zeshoek en tekstvak op de getoonde pagina. De GUI-
sleepgrepen en externe actie delen dezelfde paginabegrensde vervanging via
`DocumentCommand`. Lijnen, pijlen en polylijnen worden geweigerd omdat alleen
hun bounds wijzigen de opgeslagen punten niet zou meeschalen. Een exacte
documentversie is verplicht; annulering, stale/replay, onbekende ID, ongeldige
of ongewijzigde rechthoek veranderen niets. App-, argument- en geïsoleerde
HTTP-proeven controleren resize, undo en afwijzing. Een afzonderlijke
release-GUI/HTTP-proef op `simple-fill-stroke.pdf` tekende een ellips,
verwierp een stale resize, wijzigde haar positie en formaat met 4×5 en
12×8 PDF-punten en controleerde undo/redo (revisies 2→3→4). De PDF,
miniatuur en selectiegrepen bleven zichtbaar in
`/tmp/opds-mcp-resize-annotation-2026-10-16.png`.

`app_fit_view` past nu de live Rust-pagina aan de beschikbare vensterbreedte
of volledige pagina aan. De client stuurt `mode:"width"` of `mode:"page"`,
de exacte documentversie en `expectedZoom` uit `app_get_native_view_state`.
De UI gebruikt haar eigen `FitView`-handeling en rekent met de huidige
venster- en paneelgeometrie; een stale versie/zoom, ongeldige modus, annulering
of zoom zonder verandering wordt geweigerd. Het antwoord vermeldt de nieuwe
zoom en dat de render is ingepland, niet dat de gekozen kernel de pagina
kan tekenen. App- en argumentproeven controleren deze grenzen.

`app_set_tool` bedient nu de twaalf daadwerkelijk aangesloten native tools:
`select`, `hand`, `box`, `circle`, `ellipse`, `polygon`, `freehand`, `line`,
`arrow`, `polyline`, `highlight` en `textbox`. De client leest de huidige
`tool` en `toolEpoch` via `app_get_current_tool` en stuurt die als
`expectedTool` en `expectedToolEpoch`, samen met de exacte documentversie.
Een stale tool/epoch, replay, modaliteit of onbekende tool wordt geweigerd.
Een afzonderlijke release-GUI/HTTP-proef selecteerde `ellipse` en tekende
daadwerkelijk één native ellips; een verzoek van vóór een volledige
select→ellipse→select-cyclus werd nog steeds geweigerd. Opname:
`/tmp/opds-mcp-set-tool-ellipse-epoch-2026-10-16.png`. Andere in Tauri
geregistreerde gereedschappen ontbreken nog.

`app_go_to_page` stuurt nu een lees-/navigatieactie naar de live Iced-UI.
De client leest de actuele documentversie en `currentPage` uit
`app_get_page_count` of `app_get_native_view_state`, en stuurt
`documentId`, `generation`, `annotationRevision`, `expectedPage` en de
1-gebaseerde doel-`page`. De UI gebruikt dezelfde `GoToPage`-handeling als
haar eigen paginabediening. Ongeldige pagina's, annulering, stale versies en
replay wijzigen de staat niet. Het antwoord bevestigt een ingeplande render,
geen ondersteunde/afgeronde bitmap. Een app-proef gebruikt een tweepagina-PDF.
Een geïsoleerde release-GUI/HTTP-proef navigeerde 1→2 op twee samengevoegde
testpagina's, verwierp pagina 3 en replay, en toonde de tweede pagina en
miniatuur in `/tmp/opds-mcp-page-nav-2026-10-16.png`.

Op 15–16 oktober zijn zes **externe mutaties** gekoppeld:
`app_delete_annotation`, `app_move_annotation`, `app_clear_annotations`,
`app_resize_annotation`, `app_undo` en `app_redo`. De client leest eerst `documentId`, `generation` en
`annotationRevision` via `app_get_native_edit_state`, en stuurt die waarden
met de annotatie-ID bij verwijderen naar `tools/call`. De lokale HTTP-worker geeft de opdracht
door aan de Iced-UI; alleen de UI voert de overeenkomstige `DocumentCommand` uit.
Een verouderde document-ID, paginageneratie of annotatierevisie geeft
`{ok:false}` zonder wijziging. Het antwoord volgt op publicatie van de nieuwe
snapshot. Een geannuleerd wachtend verzoek wordt niet meer uitgevoerd. Bij
een time-out kan de uitkomst onzeker zijn als de UI al met de transactie
bezig was: de client moet de edit-state opnieuw lezen vóór een herhaling.
Een geïsoleerde lokale HTTP-proef controleert stale-weigering en geslaagde
verwijdering; een app-proef controleert ook annulering, selectie, replay en
undo. Een tweede proef op de releasebinary in een apart Xephyr-venster tekende
via de echte GUI een ellips met native ID `0`; een stale HTTP-verwijdering
liet de lijst op één, de juiste versie verwijderde haar en een klik op de
titelbalk-Undo bracht haar terug. De pagina bleef zichtbaar. Daarmee is ook
vastgesteld dat `0` een geldige annotatie-ID is. Een tweede geïsoleerde HTTP-proef
controleert stale en geldige undo/redo na verwijdering. `app_clear_annotations`
vereist expliciet `scope:"page"` met een 1-gebaseerd paginanummer of
`scope:"all"` zonder pagina. De UI behandelt dit als één undo-bare transactie;
meer dan 4096 getroffen annotaties, een lege scope, een ongeldige pagina en
een stale versie worden geweigerd. App- en geïsoleerde lokale HTTP-proeven
controleren stale clear, geldige clear en undo. Andere externe mutaties
ontbreken nog. De latere alinea's hieronder
beschrijven het oorspronkelijke lees-only stadium en zijn daardoor deels
historisch.

Een afzonderlijke release-GUI/HTTP-proef tekende een ellips in een geïsoleerd
Xephyr-venster en riep de mutaties tegen de actieve UI op. Stale delete, undo
en redo lieten de annotatielijst ongemoeid; actuele delete→undo→redo gaf
1→0→1→0 annotaties en revisie 1→2→3→4. De PDF bleef zichtbaar. De volledige
workspace-testset, release-build, fmt en diffcheck slagen.

De bestaande Tauri-toolnamen `app_select_annotation` en
`app_clear_selection` zijn nu verbonden met de live Rust-selectie. Beide
vereisen de actuele document-ID, generatie en annotatierevisie; een selectie
van een onbekende ID of een annotatie op een andere pagina wordt geweigerd.
Selectie verandert geen PDF-inhoud of annotatierevisie. App- en geïsoleerde
HTTP-proeven controleren de zichtbare selectie, wissen en stale-weigering.
Een release-GUI/HTTP-proef tekende een ellips, maakte de selectie leeg en
selecteerde ID `0` opnieuw. Stale en onbekende ID's werden geweigerd, de
annotatierevisie bleef `1` en de blauwe grepen en eigenschappen waren in de
Iced-UI zichtbaar (`/tmp/opds-mcp-selection-after.png`).

De native XFDF-import gebruikt inmiddels dezelfde versiegebonden
`DocumentCommand`-ingang met een atomaire, tot 4096 annotaties begrensde
`AddMany`-edit. De volledige batch wordt vóór mutatie gevalideerd en undo/redo
behandelt haar als één stap. Een stale document-ID/generatie/revisie wordt
geweigerd. Dit is een lokale UI-actie; een externe import-API ontbreekt.

Een in het geheugen gemaakte native New-pagina meldt in `app_list_tabs`
`isUntitled:true`, `fileName:"Untitled.pdf"` en `filePath:null`. Pas na
succesvolle Save As wordt de gekozen naam en het pad gepubliceerd. Dit is
uitsluitend leesstatus; `app_new_blank_pdf` is nog geen aangeboden MCP-tool.

`app_get_native_view_state` meldt nu ook de breedte en hoogte van de actieve
pagina in PDF-punten. De maat wordt samen met de document-ID, viewgeneratie
en annotatierevisie gepubliceerd; verouderde paginamaten kunnen de huidige
snapshot niet overschrijven. Bij ontbrekende of ongeldige geometrie staan
de twee velden op `null`. Dit zijn bronpaginamaten, geen schermafmetingen
of toezegging dat DynLex de pagina op de gekozen zoom kan renderen.
Een geïsoleerde release-app met `tiny.pdf` meldde via de lokale HTTP-tool
`200 × 300` PDF-punten, gelijk aan `pdfinfo`. De 126 actieve app-tests slagen.

Op 14 oktober meldt `app_get_native_renderers` de daadwerkelijk gekozen
Rust-renderer en de actuele beschikbaarheid van alle vijf UI-opties. Het is
een leesfunctie. `app_select_native_renderer` kan nu via de Iced-UI dezelfde
keuze als de selector linksonder uitvoeren. De client stuurt de actuele
documentversie én `expectedRenderer` mee; herhalen met de oude selectie
wordt geweigerd. Het antwoord meldt dat renderen is gestart, niet dat de
gekozen kernel de pagina ondersteunt of al een bitmap heeft geleverd.
DynLex blijft strikt zonder automatische andere renderer. PDFium- en
PdfCraft-beschikbaarheid hangt van hun optionele externe componenten af.

De gedeelde `DocumentCommand` ondersteunt inmiddels ook atomaire
`Clear { page }`-transacties met één undo/redo-stap. De Rust-Annotation-ribbon
gebruikt deze na bevestiging tegen de opgeslagen documentversie; een late
bevestiging verandert niets. `Restore` is uitsluitend interne undo-data en
wordt als nieuwe opdracht geweigerd. Clear is nu ook via
`app_clear_annotations` beschikbaar met expliciete scope en versiecontrole.

De Tauri-referentie heeft ongeveer 75 geregistreerde MCP-tools in
`open-pdf-studio/src-tauri/src/mcp_tool_meta.rs`, 71 Tauri-commands in
`src-tauri/src/lib.rs` en een aparte JavaScript-plugin-API. De Rust-kern
heeft een eerste gedeelde, versiegebonden annotatiecommandolaag, maar de
Rust-app koppelt voorlopig verwijderen, clear en undo/redo als externe mutaties. De aanwezigheid van een knop of
MCP-toolnaam is daarom geen functiepariteit.

## Eerste werkende grens

De native app heeft nu een optionele lokale HTTP-server op `POST /mcp` met
JSON-RPC 2.0. `--mcp-server` activeert hem, `--mcp-port` kiest de poort
(standaard 9223). Een release-build vereist bovendien `OPS_ENABLE_MCP=1`.
De listener bindt uitsluitend op `127.0.0.1` en weigert een vreemde `Host`
of `Origin`. Aanvragen zijn beperkt tot 64 KiB; een te lezen PDF tot 64 MiB
en een corpuslijst tot 256 PDF's. De server draait naast het Iced-venster,
zonder Tauri, WebView, JavaScript of PDF-renderfallback.

`initialize` en `tools/list` volgen de bestaande Tauri-vorm. De lijst bevat
elf werkelijk geïmplementeerde leesfuncties, vier documentmutaties, twee
annotatieselectieacties, een rendererkeuze, paginanavigatie en toolselectie:

| Tool | Rust-bron | Huidige grens |
| --- | --- | --- |
| `list_test_pdfs` | lokale corpusmap en `lopdf`-paginatelling | geen recurse; grote/onleesbare PDF krijgt telling 0 |
| `get_pdf_metadata` | `lopdf`-structuur en PDF-stringdecode | Tauri-vorm met versie, producent, maker en directe page MediaBox/Rotate; geen XMP of geërfde geometrieclaim |
| `app_get_page_count` | dezelfde actieve `DocumentView` als de Iced-UI | Tauri-vorm met actuele pagina en paginatelling; geen actief document geeft `{ok:false}` |
| `app_get_current_tool` | de actuele `AnnotationTool` en wissel-epoch van de Iced-UI | Appniveau, ook zonder geopend document; meldt `tool` en `toolEpoch` voor de twaalf native tools. De getypeerde annotatie na Circle en Ellipse heet in de native lijst in beide gevallen `ellipse`, zoals beide Tauri-knoppen een `circle`-annotatie schrijven. |
| `app_set_tool` | dezelfde Iced-UI-actie als lint en linker paletstrook | accepteert alleen de twaalf geïmplementeerde tools, exacte documentversie, `expectedTool` en `expectedToolEpoch`; herhaling en een oude opdracht na heen-en-terug wisselen worden geweigerd |
| `app_get_native_renderers` | de actuele Rust-rendererkeuze en beschikbaarheidscontroles van dezelfde vijf UI-opties | appniveau; PDFium/PdfCraft kunnen ontbreken als hun externe bibliotheek/CLI niet beschikbaar is; alleen lezen, geen rendererwissel |
| `app_go_to_page` | dezelfde Iced-UI-actie als de paginabediening | exacte documentversie plus verwachte huidige en gewenste 1-gebaseerde pagina; antwoord meldt ingeplande render, niet pixelondersteuning |
| `app_list_tabs` | dezelfde actieve `DocumentModel`-snapshot plus live pad en wijzigingsstatus | Tauri-veldvorm voor één native documenttab; zonder document lege lijst en `activeIndex:-1`; meerdere tabs ontbreken in Rust |
| `app_get_native_view_state` | versiegebonden `DocumentView`-snapshot uit dezelfde UI-documentstaat | huidige 1-gebaseerde pagina, schaal en offset in Rust-kerncoördinaten; **geen** Tauri-scherm-/canvasgeometrie of equivalent van `app_get_viewport_state` |
| `app_get_native_selection` | actuele UI-selectie, gecontroleerd tegen hetzelfde versiegebonden `DocumentModel`-snapshot | geselecteerde native annotatie-ID of `null`; geen Tauri-alias of eigenschappen, die komen via `app_get_annotation` |
| `app_get_native_edit_state` | `can_undo`/`can_redo` uit het actieve `DocumentModel` plus live wijzigingsstatus van de UI | versiegebonden leesactie; geen Tauri-command |
| `app_list_annotations` | snapshot van dezelfde `DocumentModel` als de Iced-UI | maximaal 4096 annotaties; optionele 1-gebaseerde paginafilter; geen markup-laagfilter of volledig Tauri-annotatieschema |
| `app_get_annotation` | dezelfde versiegebonden `DocumentModel`-snapshot | exacte ID als string; alle eigenschappen van de vier native annotatietypen, waaronder volledige tekst, uitlijning en polylinepunten; gezamenlijke detailgrens 4 MiB, geen Tauri-only eigenschappen of gridkoppeling |
| `app_delete_annotation` | de live Iced-UI en dezelfde `DocumentCommand`-ingang | vereist exacte `documentId`, `generation`, `annotationRevision` en annotatie-`id`; verwijdert één native annotatie als undo-bare transactie en weigert stale versies |
| `app_clear_annotations` | de live Iced-UI en `DocumentCommand::Edit(Edit::Clear)` | vereist exacte documentversie en `scope:"page"` met 1-gebaseerde `page`, of `scope:"all"` zonder pagina; maximaal 4096 getroffen annotaties in één undo-bare transactie; lege scope en stale versies doen niets |
| `app_undo` | de live Iced-UI en `DocumentCommand::Undo` | vereist exacte `documentId`, `generation` en `annotationRevision`; verwerpt stale of niet-beschikbare undo |
| `app_redo` | de live Iced-UI en `DocumentCommand::Redo` | vereist exacte `documentId`, `generation` en `annotationRevision`; verwerpt stale of niet-beschikbare redo |
| `app_select_annotation` | de zichtbare annotatieselectie van de Iced-UI | vereist exacte documentversie en ID van een native annotatie op de zichtbare pagina; wijzigt geen PDF-inhoud |
| `app_clear_selection` | dezelfde Iced-UI-selectie | vereist exacte documentversie; wijzigt geen PDF-inhoud |
| `app_select_native_renderer` | dezelfde Iced-UI-actie als de keuze linksonder | vereist exacte documentversie, `expectedRenderer` en beschikbare `renderer`; het antwoord bevestigt uitsluitend de selectie en een geplande render, niet pixelondersteuning; replay met de oude renderer wordt geweigerd |

De toolresultaten hebben dezelfde MCP-tekstblokvorm als Tauri. Niet
geïmplementeerde tools verschijnen niet in `tools/list` en een poging tot
aanroepen geeft een JSON-RPC-fout. Het actieve-document-snapshot draagt een
document-ID, rendergeneratie en afzonderlijke annotatierevisie en weigert
een achteruitgaande versie van hetzelfde document. Annotatie-edits verhogen
die revisie zonder een lopende PDF-paginarender ongeldig te maken. De
annotatielijst neemt type, ID, pagina, begrenzende
rechthoek, kleur en beschikbare lijn-/tekstvelden uit het native model. Een
te grote lijst geeft een fout in plaats van een afgekapt succes.
Detailinformatie boven de 4 MiB-grens geeft eveneens een fout; een
lijstsamenvatting blijft beschikbaar. Onbekende annotatie-ID's geven
`{ok:false}`. Een losse MCP-client kan één native annotatie verwijderen en de
actuele documentgeschiedenis één stap ongedaan maken of opnieuw toepassen naast
deze leesfuncties. De
volledige Tauri-toolset en plugin-API zijn
uitdrukkelijk niet gemigreerd.

De gedeelde `DocumentModel` heeft nu een getypeerde `DocumentCommand`-ingang
voor edit, undo en redo. Een opdracht draagt het document-ID, de
rendergeneratie en de annotatierevisie van het gelezen model; een verouderde
opdracht wordt vóór elke wijziging geweigerd. De Rust-UI leidt nu ook haar
synchrone toevoeg-, verplaats-, formaat- en tekstwijzigingen via deze ingang,
naast verwijderen, undo en redo. De UI neemt de modelversie direct vóór de
lokale edit. De eerste externe verwijdering gebruikt de door de client
meegegeven versie; ook externe undo/redo vereisen de exacte actuele versie.
Andere MCP-mutaties zijn nog niet gekoppeld. Een regressietest
controleert dat late opdrachten na paginawissel, annotatie-edit of
documentwissel de inhoud en undo/redo-geschiedenis niet wijzigen.
Selectiegebaren voor verplaatsen, schalen en lijneindpunten dragen nu ook de
documentversie van het indrukken van de muisknop tot de uiteindelijke edit.
Een tussentijdse annotatiewijziging verwerpt het oude voorbeeld en de oude
verplaatsing; een nieuw gebaar op de actuele versie blijft werken. Ook nieuwe
rechthoeken, lijnen en polylijnen dragen die begindocumentversie. Een tekstvak
houdt dezelfde versie vast tot bevestiging van de invoer; een tussentijdse
documentwijziging kan daardoor geen verouderde tekst toevoegen of vervangen.
De canvasvoorbeelden worden bij een versiewissel gewist en niet op het nieuwe
document getekend. Een regressieproef controleert de oude en actuele
versie voor verplaatsen, toevoegen en vertraagde tekstbevestiging.

De nieuwe transportafhankelijkheden zijn `axum` 0.8.9 en `tokio` 1.53.1,
beide met beperkte features. De Linux dependency-gate passeert zonder
nieuwe native linktargets of verboden PDF-/WebView-crates. De nieuwe
`httparse`-buildscriptcontrole leest alleen de `rustc`-versie en zet
Rust-SIMD-configuratie; zij compileert geen C/C++-bron. Deze gate is geen
volledige handmatige audit van iedere transitieve crate of van andere
platformen.

## Vervolgvolgorde

1. Leg getypeerde, gevalideerde documentcommands in een gedeelde Rust-laag
   vast. Geef iedere opdracht een document-ID en generatie, zodat een late
   MCP- of API-reactie niet op een inmiddels ander document landt.
2. Breid de eerste gekoppelde leesactie uit naar tabbladen, schaal,
   viewport, annotaties en tekst. Test dezelfde uitkomst via UI en MCP.
3. Koppel mutaties via dezelfde undo/redo- en save-transacties als de UI.
   Valideer alle operanden vóór de eerste wijziging en test stale ID's.
4. Breng PDF-export, CAD, pluginhooks, account-API, print en assistentfuncties
   over, elk met hun eigen toestemming en begrensde externe effecten.
5. Voeg een strikte DynLex-screenshottool pas toe wanneer het volledige
   viewportcontract, PNG-framing en unsupported-zonder-bitmap bewezen zijn.

De bestaande Tauri-MCP-server blijft de functionele referentie. De Rust-server
is een eerste transport- en metadata-subset, geen vervanging van de hele API.

## Verificatie

De release-build weigerde `--mcp-server` zonder `OPS_ENABLE_MCP=1`. Met die
opt-in antwoordde `initialize` en `tools/list` via de lokale HTTP-poort;
een vreemde `Origin` kreeg HTTP 403 en een niet-aangeboden mutatietool een
JSON-RPC-fout. `get_pdf_metadata` las uit de echte `Tekst.pdf` vijf pagina's,
versie 1.7, maker `Writer` en de gedecodeerde LibreOffice-producent.
`app_get_page_count` gaf voor het daadwerkelijk geopende DynLex-testvenster
`{"ok":true,"pageCount":5,"currentPage":5}`. Het tijdelijke testvenster
werd via de normale sluitroute gesloten; het eerdere gebruikersvenster bleef
open. De workspace-all-featuretests, formattering, `git diff --check` en de
Linux dependency-gate slaagden. De latere annotatielijst is aanvullend via
modeltests op paginafilter, undo en verouderde generatie gecontroleerd. Een
geïsoleerde live HTTP-test van `app_list_annotations` met een echte
`DocumentModel` en paginafilter slaagde. Dezelfde lokale HTTP-test las na
een toolwissel `app_get_current_tool` als `polyline`. `app_list_tabs` gaf via
dezelfde lokale HTTP-route de actieve bestandsnaam, wijzigingsstatus en het
annotatieaantal. Een modeltest controleert ook dat een verouderde generatie
die tabstaat niet kan overschrijven. Volledige Tauri-contractvergelijking
volgt nog. Dit
bewijst alleen de genoemde leesfuncties, niet overige MCP- of API-pariteit.

De native view-leesactie volgt de echte `DocumentView`-zoom en offset in
dezelfde versiegebonden snapshot. Een modeltest controleert een 200%-zoom om
een ankerpunt en weigering van een oude generatie; de geïsoleerde HTTP-proef
leest de oorspronkelijke pagina, 100%-schaal en nul-offset. Het antwoord
geeft bewust geen PDF-paginaformaat, tegelstatus of schermafmetingen: die
gegevens worden nog niet via een betrouwbaar gedeeld UI-viewportcontract
gepubliceerd.

De native selectieleesactie publiceert de gekozen annotatie uit de live
UI-staat met dezelfde document-ID, generatie en annotatierevisie als het
model. Een onbekende ID wordt `null`; een oude modelsnapshot kan de keuze
niet overschrijven. De geïsoleerde lokale HTTP-proef las selectie-ID `87`
naast de bestaande annotatie- en viewportacties. Alle 105 actieve
app-tests slagen; één HTTP-proef wordt afzonderlijk als ignored test
uitgevoerd om de procesbrede snapshot niet met parallelle tests te delen.

De native edit-state-leesactie meldt bij hetzelfde document-ID, dezelfde
rendergeneratie en annotatierevisie of de huidige UI-staat gewijzigd is en of
de native undo-/redo-stap beschikbaar is. De modelproef controleert de
overgang van een toevoeging naar undo en verwerpt een oude snapshot; de
geïsoleerde HTTP-proef leest de gewijzigde toestand uit de live snapshot.
Er zijn nu 105 actieve app-tests plus de afzonderlijke HTTP-proef geslaagd.
