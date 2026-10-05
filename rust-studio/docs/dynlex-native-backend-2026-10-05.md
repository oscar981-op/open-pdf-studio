# DynLex als optionele kernel in de native Rust-app

De native app gebruikt standaard Hayro. De Rust-crate
`opds-dynlex-backend` biedt een in-process, optionele DynLex-bufferkernel.
Er is geen JavaScript of zelfgeschreven C++-brug. De kernelcompile gebruikt
DynLex 0.0.1 en LLVM20 en levert een statische library; die toolchain
bevat C++-code. PDFium zit **nog niet** in deze native app. Wanneer die
later als tweede alternatieve backend wordt toegevoegd, moet de
aanroep uit Rust komen en blijft PDFium zelf een C++-library.

De feature `dynlex-experimental` bouwt de aparte kernel mee:

```sh
cd rust-studio
cargo run --release --offline -p opds-native-app \
  --features dynlex-experimental -- 'pad/naar/document.pdf'
```

Zet `OPDS_RENDER_BACKEND=dynlex` voor het starten om DynLex te kiezen.
Zonder die variabele kiest de app Hayro. De huidige OPGM-v3-adapter kan
uitsluitend één pagina op schaal 1,0 verwerken, met 1–64 dekkende
pixeluitgelijnde rechthoekfills/1px asgerichte strokes, beperkte q/Q,
gehele translatie en rechthoekige clips. Bij elke niet-ondersteunde PDF,
pagina of schaal rendert Hayro. Dit voorkomt dat een nog onvolledige
DynLex-kernel PDF-inhoud stil weglaat. Er is nog geen zichtbare
backendkeuze in de UI, geen PDFium-backend in de Rust-app en geen
100% PDF-objectdekking.

De native app roept de kernel alleen via de veilige Rust-router aan.
Deze valideert de gegenereerde OPGM-v3-scene opnieuw vóór de C-ABI,
begrenst de uitvoer tot 6 miljoen pixels en gebruikt de bestaande
Hayro-renderer als fallback. De standaardbuild compileert de DynLex-
toolchain niet; `build.rs` doet dat alleen bij de feature `kernel`.
Toolchainpaden zijn via `OPDS_DYNLEX_COMPILER`, `OPDS_LLVM_OPT` en
`OPDS_LLVM_LLC` instelbaar, met de gedocumenteerde lokale cache als
standaard.

Verificatie: een echte fill/stroke-PDF is in de Rust-router bytegelijk
aan Hayro; een tekst-PDF volgt de fallback en blijft identiek aan Hayro.
`rgba/verify_stroke_geometry.py` vergeleek dertien gecontroleerde PDF's,
inclusief rotatie/CropBox/A1, byte voor byte met Hayro en de onafhankelijke
referentie. Deze tests bewijzen slechts de beschreven deelset.
