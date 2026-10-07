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
| Eigen Rust | `opds-rust-kernel`, onafhankelijke RGBA-schilderlus | Experimenteel: één pagina op 100%. OPGM-v3 ondersteunt maximaal 64 pixeluitgelijnde dekkende fills en asgerichte 1px-strepen op kwartslaggeroteerde pagina's. De aparte OPDL-v3-route ondersteunt tot 256 geordende, dekkende RGB-beelden met dezelfde bewezen fills/stroken, ook op gedraaide/gecropte pagina's. Antialias-maskers en halftransparantie vallen terug. Geen DynLex-, Hayro- of PDFium-rasterizer in dit pad. |

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
na een backendwissel. De app herstelt de opgeslagen pagina-scrollpositie nadat
de nieuwe bitmap is geplaatst; anders klemde Iced de tijdelijke lege
viewport op x=0 en verschoof de pagina bij de eerste wissel 512 px.
De eigen Rust-route gebruikt de bestaande begrensde
PDF→OPGM-v3- of PDF→OPDL-v3-parser, maar heeft een zelfstandige scènevalidator
en schilderlus. Voor OPDL-v3 valideert zij de complete opdrachttabel,
contigue RGB-offsets en alle beeldlengtes vóór de eerste uitvoerallocatie;
v5/v6-maskers worden niet stilzwijgend als dekkende beelden getekend.
Het document-openpad en de paginametadata gebruiken momenteel nog Hayro;
de optionele renderkeuzes openen daardoor nog geen PDF die Hayro zelf weigert.
PDFium laadt het document bij elke render opnieuw, terwijl Hayro zijn
geparseerde document hergebruikt. Voor een prestatieclaim zijn dus eerst
een documentcache en gepaarde end-to-end tijd-/RSS-metingen nodig.
Een gecontroleerde PDF met rechthoek en asstreek is via Eigen Rust op alle
vier kwartslagen RGBA-bytegelijk aan Hayro.
Een afzonderlijke raster-PDF en een hybride PDF met twee geordende beelden
zijn via de eigen Rust-kernel volledig RGBA-bytegelijk aan Hayro. Dit blijft
een kleine fixturedeelset. In het 1200×832-releasevenster is dezelfde
hybride PDF op 100% achtereenvolgens met Hayro, DynLex, PDFium, Eigen Rust
en weer Hayro getoond. De status meldde telkens de werkelijk gekozen
renderer; de zichtbare 256×192-paginapixels hadden vijfmaal dezelfde hash,
en de pagina bleef na de viewportcorrectie op x=224–479 staan.
De vier backend-proberoutes leveren op drie 256×192-fixtures (vector,
raster en hybride) ieder dezelfde RGBA-hash, zonder fallback. Op een
150×190-beeldpagina met 90°/CropBox zijn Hayro, DynLex en Eigen Rust
bytegelijk; PDFium wijkt daar op 14.071 pixels/38.415 kleurkanalen af.
Dit is geen afgewezen render en vraagt apart onderzoek naar
beeldresampling en paginacoördinaten. PDFium is op de drie eenvoudige
fixtures wel bytegelijk; dit zegt niets over
algemene PDF- of UI-pixelpariteit. De native interface wijkt nog zichtbaar af
van de oorspronkelijke app. De bredere annotatielaag, tekst, curven,
transparantie en veel andere PDF-objecten zijn nog niet volledig gedekt.

DynLex wordt met LLVM/C++ en libc gecompileerd. Een configuratie met PDFium
bevat bovendien de C++-PDFium-library. De appintegratie en de eigen
Rust-renderer zijn in Rust geschreven; een letterlijke alles-in-Rust-build
kan PDFium of de huidige DynLex-toolchain niet omvatten.

Voor een reproduceerbare routeproef op dezelfde PDF biedt
`opds-dynlex-backend` de `backend_probe` met `--hayro`, `--dynlex`,
`--pdfium` en `--own-rust`. De probe meldt de werkelijk gebruikte renderer,
zodat een Hayro-fallback niet voor een kernelresultaat wordt aangezien.
`scripts/bench_four_backends.py` wisselt de procesvolgorde per ronde, bewaart
ruwe wandtijd/RSS-samples, vergelijkt alle RGBA-kanalen en schrijft JSON.
De eerste 15 warme procesrondes per fixture staan in
`docs/four-backend-bench-2026-10-07.json`. Mediane wandtijd in milliseconden
voor Hayro/DynLex/PDFium/Eigen Rust was 8,40/7,48/9,26/8,01 op de
vectorfixture, 7,61/8,41/9,93/7,28 op raster en 7,67/9,12/9,56/7,67 op
hybride. Alle drie waren bytegelijk en zonder fallback. De 90°/CropBox-
beeldfixture gaf 11,30/11,28/11,74/9,76 ms, maar PDFium had daar 38.415
afwijkende kleurkanalen. Piek-RSS-medians en alle samples staan in de JSON.
Deze meting omvat processtart, bestand lezen, Hayro-openpad, renderen en
RGBA-bestand schrijven met warme OS-cache. Zij bewijst geen in-app-snelheid of
algemeen prestatievoordeel; de vier fixtures zijn klein en gecontroleerd.
