# PDF-tekstinventaris voor de DynLex-kernel

`inspect_pdf --font-ops` leest per pagina de fontresources inclusief
geërfde resources en de `FontDescriptor` van een Type0-descendant. Het toont
subtype, encoding, ToUnicode, ingebed fontbestand en aantallen tekst-show-
operators/stringbytes per geselecteerd font. De opdracht is read-only en
laat de bestaande renderroute ongemoeid. Zij leest content van de pagina,
niet recursief van Form-XObjects; codebytes zijn geen Unicode-codepunten.

Reproduceren vanuit `rust-studio`:

```sh
cargo build --release --offline -p opds-opdl-adapter --bin inspect_pdf
target/release/inspect_pdf \
  '../test pdf-bestanden/Originele bestanden/Technische tekening.pdf' \
  --page 2 --font-ops
target/release/inspect_pdf \
  '../test pdf-bestanden/Originele bestanden/Tekst.pdf' \
  --page 1 --font-ops
target/release/inspect_pdf \
  '../test pdf-bestanden/Originele bestanden/Text pdf gecombineerd.pdf' \
  --page 1 --font-ops
```

| Pagina | Fonts | Tekstgebruik |
| --- | --- | --- |
| `Technische tekening.pdf` pagina 2, 90° A1 | Drie ingebedde Type0-fonts, `Identity-H`, met ToUnicode | 315 `Tj`-opdrachten, 5.132 codebytes; F1 draagt 309 opdrachten |
| `Tekst.pdf` pagina 1 | Ingebedde Type1 en TrueType, beide met ToUnicode | 49 `Tj`/`TJ`-opdrachten; F1 bevat 578 stringdelen in `TJ` |
| `Text pdf gecombineerd.pdf` pagina 1 | WinAnsi TrueType: één ingebed en één niet ingebed, geen ToUnicode | 25 `TJ`-opdrachten, 111 codebytes; tekst wordt met veel `W*`-clips gecombineerd |

Dit corpus vraagt zowel samengestelde CID-code-naar-glyph-mapping als
eenvoudige WinAnsi-fonts, positionering van `TJ`, fontfallback en tekst onder
clips. Een eerste experimentele tekstroute moet het PDF-fontobject en de
originele codebytes begrensd stagen, glyphselectie en plaatsing in Rust
valideren en een versieerbare DynLex-scene produceren. Type0/Identity-H met
ingebedde TrueType-outlines is de relevante eerste corpusvorm; op echte
pagina's blijven alle niet bewezen tekststaten, fonts en clips bij Hayro.
Een raster-, vector- of hybride prestatieclaim vergt daarna dezelfde pagina,
resolutie en cachetoestand, bytevergelijking en per-proces tijd/geheugen.
Deze inventaris bewijst nog geen DynLex-tekstweergave of volledige
PDF-objectdekking.
