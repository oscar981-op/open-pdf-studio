# Open PDF Studio — native Rust subproject

Een afzonderlijke herbouw naast de bestaande applicatie. Branch: `feat/rust-native`, gestart vanaf commit `2b2b3bb6` van de bestaande app. Het subproject heeft een eigen Cargo-workspace en lockfile; bouwen hiervan haalt geen Tauri-, JavaScript- of PDFium-afhankelijkheden binnen.

## Doel en huidige status

De applicatie, interface, PDF-engine en applicatiebibliotheken worden Rust. De **huidige Open PDF Studio-UI** is de exacte doelreferentie. De UI-code van Hakan Seven's OpenCADStudio wordt rechtstreeks hergebruikt als basis: ribbonhelpers en SVG-iconen zijn aangesloten, de overige UI-bronnen zijn lokaal opgenomen voor verdere migratie. De bestaande app blijft de functionele referentie voor de migratie.

**Dit is een werkend native venster, nog geen volledige PDF-editor.** Het venster kan een PDF openen, pagina's met Hayro renderen, door pagina's bladeren en via ribbon of Ctrl+scroll zoomen. De Home- en Annotation-ribbon en titelbalk volgen de afmetingen en icoonvolgorde van de bestaande app op een 1200 px breed venster; andere tabs, panelen en functies zijn nog niet visueel of functioneel gelijk. De eerste crate `opds-native-core` bevat documentgebonden render-tickets, pagina-selectie, zoom, annotaties en undo/redo, met regressietests. Deze crate heeft geen externe dependencies en verbiedt eigen unsafe-code. Een herbruikbare Rust-PDF-renderer (`opds-engine-probe` als library met CLI-harnas) rendert lokale PDF’s voor de enginekeuze.

```sh
cd rust-studio
cargo test --workspace --locked
cargo tree --workspace --locked
python3 scripts/audit_dependencies.py
python3 scripts/compare.py
```

De native shell gebruikt Iced 0.14 met `wgpu` voor de GPU en `tiny-skia` als softwareterugval. Hayro 0.7.1 is als afzonderlijke Rust-library aangesloten en op een eerste corpus gemeten. Het bestand wordt buiten de UI-update gelezen en één keer per open document geparseerd; renderaanroepen delen de originele bytes en parser. Visuele verschillen, featurepariteit en platformafhankelijkheden moeten verder worden onderzocht.

```sh
cargo run --release --bin opds-native-app -- "/pad/naar/tekening.pdf"
```

Open toont nu een afzonderlijk invoervenster voor het PDF-pad. Een bestandsbrowser ontbreekt nog.
Gebruik voor interactie altijd de release-build: de debug-build rendert zware PDF's veel trager en vertraagt ook de ribbon. Onder Annotation kunnen Highlight en Text Box met slepen op de pagina worden geplaatst. Text Box opent een invoervenster voor één regel tekst; een geselecteerd tekstvak kan via Properties worden bewerkt. Rectangle staat onder Drawing; Select staat onder Home en kan annotaties aanwijzen en verslepen; de acht grepen van een selectie wijzigen de grootte. Delete verwijdert de selectie. Het eigenschappenpaneel toont type, pagina, positie en afmetingen. De overlay, selectie, wijzigingen en undo/redo zijn Rust. Ctrl+scroll zoomt op de pagina rond de muispositie. Meerregelige tekst en opmaak ontbreken nog. Save As schrijft de native annotaties met `lopdf` als incrementele PDF-update, met printbare appearances. De titelbalk-Print en Print in de voorlopige View-ribbon openen nu een eerste pure-Rust afdrukpad: uitvoer naar een nieuw PDF-bestand met alle of geselecteerde bronpagina's en native annotaties. Het printvenster heeft paginabereik (All, Current, Custom), een filter voor oorspronkelijke oneven/even pagina's en een bladerbaar voorbeeld uit de printbare PDF. Er wordt nog geen printopdracht aan een printer verstuurd; schaling, papierkeuze en de overige instellingen uit het oorspronkelijke printvenster zijn nog niet overgezet. Deze eerste versie maakt uitsluitend een nieuw bestand en ondersteunt alleen niet-versleutelde, ongedraaide pagina's met een origin-zero MediaBox/CropBox en tekst met glyphs in het ingebedde Liberation Sans-lettertype.

- [Architectuur en dependencybeleid](docs/architecture.md)
- [Exacte UI-pariteit met huidige app](docs/current-ui-parity.md)
- [Overgenomen UI-code van Hakan](docs/ui-reference.md)
- [Migratie en acceptatie](docs/migration.md)
- [Eerste engineproef en meetgrenzen](docs/phase1-2026-09-30.md)
- [UI- en snelheidsvergelijking](docs/ui-performance-2026-10-01.md)
- [Eerste native Print-pad en vergelijking](docs/print-2026-10-01.md)

## Lokale werkmappen

- Deze branch: `/home/aia02/Documenten/GitHub/open-pdf-studio-rust`
- Bestaande app/PR #510: `/home/aia02/Documenten/GitHub/open-pdf-studio`
- Originele UI-bronclone: `/home/aia02/Documenten/GitHub/OpenCADStudio`

De originele clone blijft een aparte repository. De UI-bronnen en iconen van de vastgelegde commit zijn gekopieerd naar `vendor/opencadstudio`, met de oorspronkelijke licentie.
