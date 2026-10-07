# Vier PDF-renderkeuzes in de native Rust-app

De Settings-ribbon heeft nu vier expliciete voorkeuren. De statusregel noemt
steeds de renderer die de getoonde pagina werkelijk heeft geproduceerd. Bij
een niet-ondersteunde pagina of ontbrekende dynamische library gebruikt de
router de bestaande Hayro-renderer en meldt hij de fallback.

| Keuze | Implementatie | Huidige grens |
| --- | --- | --- |
| Hayro | `opds-engine-probe`, pure Rust | Standaard en veilige fallback. |
| DynLex | `opds-dynlex-backend`, optionele in-process kernel | Strikt bewezen PDF-/annotatiesubsets; overige inhoud via Hayro. |
| PDFium | `opds-pdfium-backend`, Rust-aanroep van de publieke C-ABI | Optioneel; vereist een door de gebruiker vertrouwde `libpdfium.so`. Geen JavaScript of zelfgeschreven C++-glue. PDFium zelf is C++. Interactieve widget-/popup-annotaties vallen niet onder `FPDF_ANNOT`. |
| Eigen Rust | `opds-rust-kernel`, onafhankelijke RGBA-schilderlus | Experimenteel: één ongeroteerde of kwartslaggeroteerde pagina op 100%, met maximaal 64 pixeluitgelijnde dekkende rechthoekfills en asgerichte 1px-strepen uit de streng gevalideerde OPGM-v3-scène. Geen DynLex-, Hayro- of PDFium-rasterizer in dit pad. |

De beperking van PDFiums `FPDF_ANNOT`-vlag voor interactieve widgets en
popups staat in [de publieke PDFium-C-API](https://pdfium.googlesource.com/pdfium/+/refs/heads/main/public/fpdfview.h).

De standaardbuild blijft Hayro. Bouw alle optionele keuzes met:

```sh
cd rust-studio
cargo run --release -p opds-native-app \
  --features dynlex-experimental,pdfium-experimental,own-rust-experimental \
  -- pad/naar/document.pdf
```

`OPDS_RENDER_BACKEND` accepteert bij de start `dynlex`, `pdfium` of
`own-rust`; zonder variabele wordt Hayro gekozen. PDFium wordt alleen
klikbaar als de library daadwerkelijk is geladen. Geef zo nodig een expliciet
pad met `OPDS_PDFIUM_LIBRARY=/absoluut/pad/libpdfium.so`. Het project bundelt
geen PDFium-binary. De PDFium Rust-grens serialiseert C-ABI-aanroepen,
valideert invoer- en bitmaplimieten, en zet PDFiums BGRA-uitvoer om naar RGBA.

De vier routes delen de bestaande cachegeneratie en negeren late renderresultaten
na een backendwissel. De eigen Rust-route gebruikt de bestaande begrensde
PDF→OPGM-v3-parser, maar heeft een zelfstandige scènevalidator en schilderlus.
Het document-openpad en de paginametadata gebruiken momenteel nog Hayro;
de optionele renderkeuzes openen daardoor nog geen PDF die Hayro zelf weigert.
PDFium laadt het document bij elke render opnieuw, terwijl Hayro zijn
geparseerde document hergebruikt. Voor een prestatieclaim zijn dus eerst
een documentcache en gepaarde end-to-end tijd-/RSS-metingen nodig.
Een gecontroleerde PDF met rechthoek en asstreek is op alle vier kwartslagen
RGBA-bytegelijk aan Hayro.
PDFium is op dezelfde fixture eveneens bytegelijk; dit zegt niets over
algemene PDF- of UI-pixelpariteit. De native interface wijkt nog zichtbaar af
van de oorspronkelijke app. De bredere annotatielaag, tekst, curven,
transparantie en veel andere PDF-objecten zijn nog niet volledig gedekt.

DynLex wordt met LLVM/C++ en libc gecompileerd. Een configuratie met PDFium
bevat bovendien de C++-PDFium-library. De appintegratie en de eigen
Rust-renderer zijn in Rust geschreven; een letterlijke alles-in-Rust-build
kan PDFium of de huidige DynLex-toolchain niet omvatten.
