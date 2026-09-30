# Open PDF Studio — native Rust subproject

Een afzonderlijke herbouw naast de bestaande applicatie. Branch: `feat/rust-native`, gestart vanaf commit `2b2b3bb6` van de bestaande app. Het subproject heeft een eigen Cargo-workspace en lockfile; bouwen hiervan haalt geen Tauri-, JavaScript- of PDFium-afhankelijkheden binnen.

## Doel en huidige status

De applicatie, interface, PDF-engine en applicatiebibliotheken worden Rust. De **huidige Open PDF Studio-UI** is de exacte doelreferentie. Open CAD Studio van Hakan Seven dient als technische referentie voor native Iced-UI, niet als visueel ontwerp of vervangende PDF-engine. De bestaande app blijft de functionele referentie voor de migratie.

**Dit is de projectbasis, geen werkende PDF-editor.** Er is nog geen native venster, PDF-parser of renderer aangesloten. De eerste crate `opds-native-core` bevat documentgebonden render-tickets, pagina-selectie, cursorvast zoomen, annotaties en undo/redo, met regressietests. Deze crate heeft geen externe dependencies en verbiedt eigen unsafe-code. Een herbruikbare Rust-PDF-renderer (`opds-engine-probe` als library met CLI-harnas) rendert lokale PDF’s voor de enginekeuze.

```sh
cd rust-studio
cargo test --workspace --locked
cargo tree --workspace --locked
python3 scripts/audit_dependencies.py
python3 scripts/compare.py
```

De desktop-GUI is nog niet gekozen/geïntegreerd: Iced is de eerste kandidaat. Hayro 0.7.1 is als afzonderlijke Rust-library aangesloten en op een eerste corpus gemeten; visuele verschillen, featurepariteit en platformafhankelijkheden moeten verder worden onderzocht.

- [Architectuur en dependencybeleid](docs/architecture.md)
- [Exacte UI-pariteit met huidige app](docs/current-ui-parity.md)
- [Technische UI-referentie van Hakan](docs/ui-reference.md)
- [Migratie en acceptatie](docs/migration.md)
- [Eerste engineproef en meetgrenzen](docs/phase1-2026-09-30.md)

## Lokale werkmappen

- Deze branch: `/home/aia02/Documenten/GitHub/open-pdf-studio-rust`
- Bestaande app/PR #510: `/home/aia02/Documenten/GitHub/open-pdf-studio`
- Referentieclone: `/home/aia02/Documenten/GitHub/OpenCADStudio`

De referentieclone is een aparte repository, geen submodule of vendored broncode.
