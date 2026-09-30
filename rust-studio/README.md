# Open PDF Studio — native Rust subproject

Een afzonderlijke herbouw naast de bestaande applicatie. Branch: `feat/rust-native`, gestart vanaf commit `2b2b3bb6` van de bestaande app. Het subproject heeft een eigen Cargo-workspace en lockfile; bouwen hiervan haalt geen Tauri-, JavaScript- of PDFium-afhankelijkheden binnen.

## Doel en huidige status

De applicatie, interface, PDF-engine en applicatiebibliotheken worden Rust. Open CAD Studio van Hakan Seven dient als **UI-referentie**, niet als vervangende PDF-engine. De bestaande app blijft de functionele referentie voor de migratie.

**Dit is de projectbasis, geen werkende PDF-editor.** Er is nog geen native venster, PDF-parser of renderer aangesloten. De eerste crate `opds-native-core` bevat documentgebonden render-tickets, pagina-selectie en cursorvast zoomen, met regressietests. Deze crate heeft geen externe dependencies en verbiedt eigen unsafe-code.

```sh
cd rust-studio
cargo test --workspace --locked
cargo tree --workspace --locked
```

De desktop-GUI is nog niet gekozen/geïntegreerd: Iced is de eerste kandidaat vanwege de UI-referentie. Hayro is de eerste kandidaat voor een PDF-engine. Beide moeten eerst op hun volledige, platform- en feature-specifieke afhankelijkheden worden gecontroleerd.

- [Architectuur en dependencybeleid](docs/architecture.md)
- [UI-referentie van Hakan](docs/ui-reference.md)
- [Migratie en acceptatie](docs/migration.md)

## Lokale werkmappen

- Deze branch: `/home/aia02/Documenten/GitHub/open-pdf-studio-rust`
- Bestaande app/PR #510: `/home/aia02/Documenten/GitHub/open-pdf-studio`
- Referentieclone: `/home/aia02/Documenten/GitHub/OpenCADStudio`

De referentieclone is een aparte repository, geen submodule of vendored broncode.
