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

De eerdere testshell is vervangen door de port in `hakan_ui.rs`. Deze gebruikt echte upstream ribbonhelpers en SVG-assets, compacte quick actions, gegroepeerde tools, paginalijst en documenteigenschappen. Zie [bronverantwoording](ui-reference.md). Openen staat in een afzonderlijk dialoogvenster; de ribbon bevat geen padveld meer. De native overlay kan rechthoek en markering tekenen, selecteren, verplaatsen, met acht grepen vergroten/verkleinen en verwijderen, met undo/redo in de Rust-documentkern. De Annotation-ribbon toont nu dezelfde acht groepen en PDF-iconen als `CommentTab.jsx`; alleen Highlight is daarin functioneel. Tekstinvoer, opslag en printen ontbreken. [De scherm- en snelheidsvergelijking](ui-performance-2026-10-01.md) toont de resterende verschillen.
