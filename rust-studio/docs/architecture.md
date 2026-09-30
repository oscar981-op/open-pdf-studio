# Architectuur en afhankelijkheden

## Uitgangspunt

Gebruikerskeuze: ook PDF-engine en afhankelijkheden in Rust. Geen Tauri/WebView, JavaScript-runtime, PDFium, MuPDF, Tesseract, FreeType/HarfBuzz, libjpeg-turbo of OpenSSL als ingebouwde applicatie-engine. Een Rust-wrapper om een C/C++-bibliotheek voldoet niet.

Er is een noodzakelijke technische grens: besturingssysteem, drivers, venstersysteem en de implementatie van delen van de Rust-standaardbibliotheek zijn niet per definitie Rust. **Zonder zo'n grens kunnen we een desktopapp niet eerlijk '100% Rust inclusief alles' noemen.** Het project houdt deze OS/ABI-grenzen zichtbaar en sluit C/C++-PDF-engines en C-buildstappen uit de actieve Linux-appgrafiek uit.

## Lagen

| Laag | Verantwoordelijkheid | Status |
| --- | --- | --- |
| `opds-native-core` | Documentidentiteit, viewport, afwijzen verouderde resultaten, annotaties en undo/redo | Eerste implementatie |
| PDF-engine | Parser, fonts, transparantie, kleuren, raster/vector, tekstextractie | Hayro-parser/renderer op native viewer aangesloten; tekstextractie ontbreekt |
| Documentopslag | PDF-objectmutaties, vectorfragmenten, formulieren, atomair opslaan | Te ontwerpen; rendering is geen schrijfmotor |
| Desktop-UI | Iced, ribbon, documenttabs, dockpanelen, viewport | Eerste native venster met PDF-weergave; pariteit ontbreekt |
| Integraties | Print, clipboard, bestanddialoog, keyring, toegankelijkheid | Platformgrens nog vastleggen |
| MCP | Getypeerde documentcommands, dezelfde kern als UI | Te porteren |

Gebruik stabiele documentidentiteiten, geen actieve-tab-globals voor async werk. Een renderresultaat draagt de document-ID en generatie waarop het is gestart. Pagina-, zoom- en inhoudswijzigingen maken eerdere resultaten ongeldig. Document-ID's mogen tijdens de sessie niet opnieuw worden gebruikt. Bestand lezen, PDF-parsing en rendering vinden in de native viewer buiten de UI-update plaats. De paginacache heeft een budget van 32 MiB; taakcoalescing/annuleren en een expliciete renderwachtrij ontbreken nog.

## Eerste audit van de huidige broncode

| Onderdeel | Bevinding | Consequentie |
| --- | --- | --- |
| `open-pdf-render/Cargo.toml` | `turbojpeg` met `cmake` ondanks omschrijving 'Pure Rust' | Niet ongewijzigd koppelen; JPEG-pad vervangen |
| `open-pdf-cad/src/pdfium_ffi.rs` en `extract.rs` | Dynamische PDFium-bindingen | PDF-extractie opnieuw op Rust-engine bouwen |
| `src-tauri/Cargo.toml` | Tauri, pdfium-render, tesseract-rs | Geen dependency van het nieuwe workspace maken |
| Bestaande Rust-geometrie/CAD-code | Gedeeltelijk los te maken | Per module en dependencygraph beoordelen |
| OpenCADStudio | Iced/wgpu voor desktop, o.a. `meshopt` in dependencylijst | Alleen UI-referentie; Cargo.toml niet overnemen |

## Dependency-toelating

Voor iedere nieuwe library:

1. Exacte release/revisie en features vastleggen in Cargo.lock en een auditnotitie.
2. `cargo tree` en `cargo metadata` voor Linux, Windows en macOS controleren, inclusief build-dependencies en feature-unificatie.
3. Broncode/buildscripts bekijken op native compilatie, `links`, FFI, externe tools, dynamisch laden en downloads van binaries. Een ontbrekend `-sys`-suffix bewijst niets.
4. Rust-implementaties van codecs, font shaping/rasterisering, compressie, crypto en TLS selecteren; ongewenste defaults uitzetten.
5. OS/driverbindings apart inventariseren. Alleen expliciet afgesproken systeemgrenzen toelaten.
6. Herhalen bij dependencyupdates; onbekend is niet goedgekeurd.

De huidige kern heeft nul third-party dependencies. De actieve Linux-GUI/enginegrafiek wordt met `scripts/audit_dependencies.py` op bekende native links en verboden crates gecontroleerd. Dat bewijst niet dat alle transitieve broncode handmatig is beoordeeld of dat Windows/macOS dezelfde grafiek hebben.

## Enginekandidaten

[Hayro](https://github.com/LaurenzV/hayro) is een Rust PDF-interpreter/renderer, maar noemt zichzelf experimenteel. Het biedt geen automatische garantie op alle bestaande editorfuncties, performance of foutloze weergave. Eerste proef: representatief lokaal corpus renderen en vergelijken met bestaande referentiebeelden. Geen C/C++-fallback in de nieuwe app; unsupported functies zichtbaar rapporteren.

De bestaande eigen renderer is een tweede kandidaat na verwijdering van zijn native decoder. Kies pas na dezelfde corpus-, geheugen- en snelheidsmetingen. Geen aanname dat Rust alleen de applicatie sneller maakt.
