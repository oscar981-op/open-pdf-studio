# Experimentele Type0-tekststaging

`stage_type0_text` is een afzonderlijke read-only Rust-proef. Hij verandert de
normale PDF→OPDL-adapter, DynLex-renderer en native app niet. Een pagina met
ingebedde Type0/Identity-H + CIDFontType2-fonts kan worden onderzocht op
code→CID→GID, CID-breedtes en de plaatsing van `Tj`-glyphs. De embedded
FontFile2 wordt met een limiet van 8 MB via pure-Rust FlateDecode uitgepakt
en door `ttf-parser` gevalideerd. Alleen een identieke of ontbrekende
`CIDToGIDMap` is in deze proef toegestaan; een mapstream wordt afgewezen.
De ontbrekende map wordt hier als Identity behandeld en moet vóór
productiegebruik op meer corpus worden getoetst.

De proef verwerkt `Tf` ook buiten `BT`, zoals de echte technische tekening
doet. Voor glyphplaatsing verwerkt zij `q/Q`, `cm`, `Tm`, `Tc`, `Tz`, `Ts`,
de horizontale CID-breedtes `W`/`DW` en `Tj`. `TJ`, `Td`, andere fonttypen,
niet-nul woordspatiëring en onzichtbare/niet-gevulde tekstrendermodi worden
niet geëxporteerd. Een gemengde pagina mag een samenvatting tonen, maar
schrijft **geen** gedeeltelijke glyph-run als een tekst-show-opdracht is
overgeslagen. De PDF-invoer is tot 16 MB begrensd; `lopdf`-parsing en
pagina-contentdecompressie hebben in deze proef nog geen volledige
onbetrouwbare-invoer-garantie. Gebruik het commando daarom alleen voor
vertrouwde onderzoeksbestanden.

Optioneel schrijft het commando TGR1: 16-byte header (`TGR1`, versie 1,
recordgrootte 84, paginanummer, recordaantal) gevolgd door maximaal 65.536
records. Elk record bevat een resource-naam van maximaal 31 bytes, CID,
GID en een 2×3-`f64`-transformatie van ruwe TrueType-glyphcoördinaten naar
PDF-gebruikerscoördinaten. De fontbytes zelf blijven in de bron-PDF; een
latere renderer moet ze opnieuw aan de resource-naam koppelen en controleren.
De validator controleert versie, exacte lengte, naam/padding en eindige
begrensde transformaties. De tests weigeren afkapping, corrupte header,
recordgrootte, naam en NaN-coördinaten.

Reproduceren vanuit `rust-studio`:

```sh
cargo build --release --offline -p opds-opdl-adapter --bin stage_type0_text
target/release/stage_type0_text \
  '../test pdf-bestanden/Originele bestanden/Technische tekening.pdf' \
  2 /tmp/opds-technical-page2.tgr
target/release/stage_type0_text --validate /tmp/opds-technical-page2.tgr
```

Op de vier echte A1-pagina's werden respectievelijk 2.145, 2.566, 2.522 en
1.918 glyphrecords geëxporteerd; alle Type0-`Tj`-opdrachten van die pagina's
werden gestaged. Pagina 2 heeft 315 `Tj`-opdrachten en een TGR1-buffer van
215.560 bytes. Dit controleert structurele mapping en berekende
gebruikerscoördinaten; er is **nog geen** glyph-outline-rendering,
Hayro-pixelpariteit of prestatiewinst. De eerste echte DynLex-tekstproef
moet de TGR1-records met de originele fontprogramma's koppelen, glyph-
outlines begrensd verwerken, PDF-rotatie/CropBox en clips meenemen en de
pixels tegen Hayro vergelijken. Onbewezen PDF's blijven fallback.

De gebruikte Identity-H-, CID-metric- en tekstmatrixregels zijn beschreven
in de [Adobe PDF 32000-1-referentie](https://developer.adobe.com/document-services/docs/assets/35e4369068f86065372c18787171a17e/PDF_ISO_32000-1.pdf), §9.4 en §9.7.
