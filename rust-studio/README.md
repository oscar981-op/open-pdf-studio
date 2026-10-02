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

De native shell gebruikt Iced 0.14 met `wgpu` voor de GPU en `tiny-skia` als softwareterugval. Hayro 0.7.1 is als afzonderlijke Rust-library aangesloten en op een eerste corpus gemeten. Het bestand wordt buiten de UI-update gelezen en één keer per open document geparseerd; renderaanroepen delen de originele bytes en parser. De renderer tekent geïmporteerde Square- en Highlight-annotaties zonder `/AP` zelf en cachet hun gegevens per pagina; annotaties met `/AP` blijven bij Hayro. Visuele verschillen, featurepariteit en platformafhankelijkheden moeten verder worden onderzocht.

```sh
cargo run --release --bin opds-native-app -- "/pad/naar/tekening.pdf"
```

Open toont nu een native Rust-bestandsbrowser met mappen, oudermap, vernieuwen en alleen PDF-bestanden. Het pad kan ook handmatig worden ingevoerd. Het lezen van mappen gebeurt via een achtergrondtaak; een laat resultaat van een vorige map wordt genegeerd. Het zoekveld filtert de huidige map zonder nieuwe schijfscan, en maximaal twintig zichtbare bestandsrijen worden als UI-widgets opgebouwd. Het linkerpaneel toont paginaminiaturen: de actieve pagina hergebruikt de bestaande bitmap; hoogstens één nabije pagina wordt tegelijk op de achtergrond op klein formaat gerenderd, met maximaal twaalf gecachte miniaturen. Verder weg gelegen pagina’s verschijnen eerst als placeholder en worden tijdens scrollen of na navigatie op de achtergrond geladen. Het paneel maakt maximaal twaalf zichtbare paginakaarten aan, ook bij zeer veel pagina’s; de scrollbar behoudt de volledige documenthoogte.
Gebruik voor interactie altijd de release-build: de debug-build rendert zware PDF's veel trager en vertraagt ook de ribbon. Onder Annotation kunnen Highlight en Text Box met slepen op de pagina worden geplaatst. Text Box toont een meerregelige inline editor op de pagina, met een eerste Format-contexttab voor zeven tekstkleuren en de 16 lettergroottes uit de oorspronkelijke tekstannotatiedialoog; een geselecteerd tekstvak kan via Properties opnieuw worden bewerkt. Tekstkleur, lettergrootte en links/midden/rechts-uitlijning zitten in het Rust-annotatiemodel en worden met undo/redo en PDF Save As bewaard. Tijdens invoer schildert een native Rust-laag de regels, caret en selectie op hun links/midden/rechts-uitgelijnde positie; Iced blijft toetsenbord- en Unicode-invoer afhandelen. IME-compositietekst is in deze laag nog niet visueel geverifieerd. Rectangle staat onder Drawing; Select staat onder Home en kan annotaties aanwijzen en verslepen; de acht grepen van een selectie wijzigen de grootte. Delete verwijdert de selectie. Het eigenschappenpaneel toont type, pagina, positie en afmetingen. De overlay, selectie, wijzigingen en undo/redo zijn Rust. Ctrl+scroll zoomt op de pagina rond de muispositie. Een gedeelde Rust-tekstlayout breekt woorden en lange tokens binnen de tekstvakbreedte af in live invoer, pagina-overlay en PDF-appearance; expliciete en zachte regels worden binnen het tekstvak afgeknipt. Andere tekstopmaak en fontfallback ontbreken nog. Save As schrijft de native annotaties met `lopdf` als incrementele PDF-update, met printbare appearances. De titelbalk-Print en Print in de voorlopige View-ribbon openen nu een eerste pure-Rust afdrukpad: uitvoer naar een nieuw PDF-bestand met alle of geselecteerde bronpagina's en native annotaties. Het printvenster heeft paginabereik (All, Current, Custom), een filter voor oorspronkelijke oneven/even pagina's, omgekeerde uitvoervolgorde en een bladerbaar voorbeeld uit de printbare PDF. Page Zoom (10–400%) schaalt de pagina-inhoud en annotaties rond het midden van het vel. Original/A4/Letter en Fit/Actual/Shrink/Custom zijn beschikbaar; preview en uitvoer gebruiken dezelfde PDF-transformatie. De Printer-sectie kan daarnaast een IPP Print-Job naar een tijdelijke, uitsluitend lokale Rust-testserver sturen en de ontvangen PDF opslaan. Er wordt geen fysieke of geïnstalleerde printer benaderd; apparaatselectie, Auto-Rotate en overige oorspronkelijke instellingen ontbreken nog. Deze eerste versie maakt uitsluitend een nieuw bestand. Printplaatsing en Save As van nieuwe native annotaties ondersteunen inmiddels 0/90/180/270°-rotatie en een verschoven CropBox binnen de MediaBox. Versleuteling en tekst zonder glyphs in Liberation Sans worden geweigerd.

- [Architectuur en dependencybeleid](docs/architecture.md)
- [Exacte UI-pariteit met huidige app](docs/current-ui-parity.md)
- [Overgenomen UI-code van Hakan](docs/ui-reference.md)
- [Migratie en acceptatie](docs/migration.md)
- [Eerste engineproef en meetgrenzen](docs/phase1-2026-09-30.md)
- [UI- en snelheidsvergelijking](docs/ui-performance-2026-10-01.md)
- [Eerste native Print-pad en vergelijking](docs/print-2026-10-01.md)
- [Preview van geïmporteerde annotaties zonder appearance](docs/imported-annotations-2026-10-01.md)

## Lokale werkmappen

- Deze branch: `/home/aia02/Documenten/GitHub/open-pdf-studio-rust`
- Bestaande app/PR #510: `/home/aia02/Documenten/GitHub/open-pdf-studio`
- Originele UI-bronclone: `/home/aia02/Documenten/GitHub/OpenCADStudio`

De originele clone blijft een aparte repository. De UI-bronnen en iconen van de vastgelegde commit zijn gekopieerd naar `vendor/opencadstudio`, met de oorspronkelijke licentie.
