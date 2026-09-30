# Open PDF Studio — native Rust subproject

Een afzonderlijke herbouw naast de bestaande applicatie. Branch: `feat/rust-native`, gestart vanaf commit `2b2b3bb6` van de bestaande app. Het subproject heeft een eigen Cargo-workspace en lockfile; bouwen hiervan haalt geen Tauri-, JavaScript- of PDFium-afhankelijkheden binnen.

## Doel en huidige status

De applicatie, interface, PDF-engine en applicatiebibliotheken worden Rust. De **huidige Open PDF Studio-UI** is de exacte doelreferentie. De UI-code van Hakan Seven's OpenCADStudio wordt rechtstreeks hergebruikt als basis: ribbonhelpers en SVG-iconen zijn aangesloten, de overige UI-bronnen zijn lokaal opgenomen voor verdere migratie. De bestaande app blijft de functionele referentie voor de migratie.

**Dit is een werkend eerste native venster, nog geen volledige PDF-editor.** Het venster kan een PDF openen, pagina's met Hayro renderen, door pagina's bladeren en in stappen in- en uitzoomen. De lint- en paneelindeling is nog een beginversie; visuele en functionele pariteit is niet bereikt. De eerste crate `opds-native-core` bevat documentgebonden render-tickets, pagina-selectie, cursorvast zoomen, annotaties en undo/redo, met regressietests. Deze crate heeft geen externe dependencies en verbiedt eigen unsafe-code. Een herbruikbare Rust-PDF-renderer (`opds-engine-probe` als library met CLI-harnas) rendert lokale PDF’s voor de enginekeuze.

```sh
cd rust-studio
cargo test --workspace --locked
cargo tree --workspace --locked
python3 scripts/audit_dependencies.py
python3 scripts/compare.py
```

De native shell gebruikt Iced 0.14 met `tiny-skia`, zonder de optionele wgpu-renderer op Linux. Hayro 0.7.1 is als afzonderlijke Rust-library aangesloten en op een eerste corpus gemeten. Het bestand wordt buiten de UI-update gelezen en één keer per open document geparseerd; renderaanroepen delen de originele bytes en parser. Visuele verschillen, featurepariteit en platformafhankelijkheden moeten verder worden onderzocht.

```sh
cargo run --bin opds-native-app -- "/pad/naar/tekening.pdf"
```

Open toont nu een afzonderlijk invoervenster voor het PDF-pad. Een bestandsbrowser ontbreekt nog.

- [Architectuur en dependencybeleid](docs/architecture.md)
- [Exacte UI-pariteit met huidige app](docs/current-ui-parity.md)
- [Overgenomen UI-code van Hakan](docs/ui-reference.md)
- [Migratie en acceptatie](docs/migration.md)
- [Eerste engineproef en meetgrenzen](docs/phase1-2026-09-30.md)

## Lokale werkmappen

- Deze branch: `/home/aia02/Documenten/GitHub/open-pdf-studio-rust`
- Bestaande app/PR #510: `/home/aia02/Documenten/GitHub/open-pdf-studio`
- Originele UI-bronclone: `/home/aia02/Documenten/GitHub/OpenCADStudio`

De originele clone blijft een aparte repository. De UI-bronnen en iconen van de vastgelegde commit zijn gekopieerd naar `vendor/opencadstudio`, met de oorspronkelijke licentie.
