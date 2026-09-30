# Technische UI-referentie: Open CAD Studio van Hakan Seven

Gevonden via https://open-aec.com/en/open-cad-studio/.

- Repository: https://github.com/HakanSeven12/OpenCADStudio
- Lokale clone: `/home/aia02/Documenten/GitHub/OpenCADStudio`
- Bekeken commit: `2a15a370f6886d3e8695ab81c7d677e0cd484451`
- UI: Iced; GPU-rendering via wgpu.
- Referentiebeeld bekeken: `site/workspace.png`.
- Licentiebestand van referentie: GPL-3.0. Er is geen broncode of beeldmateriaal uit deze clone gekopieerd naar het Rust-subproject.

De huidige Open PDF Studio is de visuele doelreferentie; zie [current-ui-parity.md](current-ui-parity.md). Hieronder staan alleen ideeën voor de Rust-implementatie.

## Vertaling naar een PDF-editor

| Hakan-referentie | Locatie in clone | Toepassing in Open PDF Studio Rust |
| --- | --- | --- |
| Ribbon met gegroepeerde tools, compacte weergave | `src/ui/ribbon/mod.rs`, `collapse.rs`, `widgets.rs` | Tabs Bestand, Annoteren, Meten, Bewerken, Beeld; contextgroep voor selectie |
| Dockbare panelen, resize en automatisch inklappen | `src/ui/dock.rs` | Links pagina's/bookmarks/lagen; rechts eigenschappen/meting; ruimte voor canvas bewaren |
| Eigenschappenpaneel | `src/ui/properties.rs` | Getypeerde eigenschappen van PDF-annotaties en geselecteerde objecten |
| Centrale viewport | `src/scene/view/` | Pagina's op neutrale achtergrond, cursorvast zoomen, pan, selecteren; geen CAD-kernel overnemen |
| Meerdere documenten | `src/app/document.rs` | Documenttabs met eigen undo/redo, selectie, zoom en bewaartoestand |
| Statusbalk | `src/ui/statusbar/` | Pagina, schaal, coördinaten, zoom en renderstatus |
| Dialogen | `src/ui/window/` | Openen, exporteren, afdrukken en voorkeuren als latere stappen |

## Eerste native scherm, volgende implementatiestap

Maak een Iced-proef zodra dependencybeleid en systeemgrens zijn afgerond. Toon bovenaan compacte quick actions en linttabs, daaronder documenttabs; links pagina's, centraal canvas, rechts eigenschappen, onderaan status. Paneelbreedtes aanpasbaar; relevante PDF-handelingen moeten ook via toetsenbord bereikbaar zijn. Geen knoppen presenteren als functioneel zolang de documentcommand ontbreekt.

Eerste werkende route: PDF openen → pagina renderen → zoomen/pannen → tab wisselen → document sluiten. Daarna selectie en een echte annotatie met undo/redo en opslaan/heropenen. Niet meteen de volledige CAD-interface kopiëren.
