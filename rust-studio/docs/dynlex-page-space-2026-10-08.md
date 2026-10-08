# Fractionele paginageometrie: OPGX-v1 proef

`crates/dynlex-backend/src/page_space.rs` introduceert een zelfstandige,
begrensde paginacoördinatenproef. Dit is **nog geen nieuwe PDF-adapterroute**:
het bestand interpreteert geen PDF-operators en wordt niet door de native
viewer aangeroepen. Het complexe corpus blijft onondersteund.

Het vaste 80-byte little-endian contract bevat `OPGX`, versie 1, vier f64
MediaBox-coördinaten, vier f64 CropBox-coördinaten, u32 kwartslag en f32 schaal.
De caller moet geërfde PDF-state en originele numerieke waarden correct oplossen;
het contract zelf bevat expliciet opgeloste waarden, geen objectreferenties.
Coördinaten zijn eindig en begrensd tot ±16.384; zoom 0,01–8; uitvoer maximaal
4.096 per zijde en zes miljoen pixels. CropBox wordt met MediaBox doorsneden.
Lege of sub-point-zichtbare pagina’s worden afgewezen, niet naar A4 gerepareerd.
UserUnit anders dan 1, algemene affine CTM, fonts, clippingpaden en paints maken
geen deel uit van deze geometrieheader.

## Numerieke bevinding

De bestaande Hayro 0.7.1-oracle gebruikt **floor**, niet ceil, voor de
bitmapafmetingen. Eerst worden zichtbare afmetingen naar f32 teruggebracht;
`width * scale` gebeurt eveneens in f32. De punttransformatie gebruikt daarna
f64 met die afgeronde basisafmetingen. Daardoor is alleen het omzetten van
595,32×841,92 naar afgeronde gehele OPDL-pagina-afmetingen niet correct: naast
de bitmapgrootte moet ook de y-inversie/rotatie precies worden vastgelegd.

De proef reproduceert deze conventie expliciet zonder Hayro/Vello-aanroep in
de implementatie. Bronnen zijn de lokaal vastgepinde Cargo-dependencies:
`hayro-0.7.1/src/lib.rs::render` en
`hayro-syntax-0.7.2/src/page.rs::{base_dimensions,initial_transform}`.
Er is geen algemene AA- of PDF-conformiteitsclaim.

`aligned_rect` verlaagt een rechthoek alleen als iedere getransformeerde rand
exact geheel en volledig binnen de bitmap is. Fractionele inktranden worden
geweigerd; ze worden niet heimelijk afgerond of afgeknipt. De huidige DynLex
OPDL-kernel kan de bewezen uitkomst vervolgens tekenen.

## Verificatie

Vier gegenereerde PDF-layouts dekken fractionele MediaBox/CropBox, negatieve
pagina-oorsprong, CropBox buiten MediaBox, geërfde geometrie en een A4-achtige
595,32×841,92-pagina. Alle kwartslagen en 25/100/200/400% worden geprobeerd.
Vier schaal/page-combinaties overschrijden de pixelgrens; van de resterende 60
zijn de bitmapafmetingen gelijk aan Hayro. **36** exact uitgelijnde resultaten
zijn over de volledige RGBA-buffer bytegelijk tussen de bestaande DynLex-kernel
en Hayro; **24** gevallen met fractionele inktranden worden expliciet afgewezen.
Dit bewijst een coördinatenroute voor deze gevallen, geen nieuw ondersteunde
hele praktijkpagina en geen snelheidwinst.

Gerichte tests, workspace-all-features-tests, de standaard native check en formattering slagen.
Roundtrip, alle truncatielengtes, extra bytes, NaN, ongeldige rotatie/schaal,
lege doorsneden en pixelgrenzen worden gecontroleerd. Mutatie van iedere byte
in het contract veroorzaakt geen panic. Bounds worden vóór rasterallocatie
gecontroleerd. De native viewer en zijn strict-no-fallbackgedrag zijn ongewijzigd.

Reproduceer:

```sh
cargo test -p opds-dynlex-backend --features kernel page_space:: -- --nocapture
```

De volgende stap is integratie met een algemene geordende scene: originele
PDF-numerieke precisie, overerfde geometry/resources en paint-state oplossen,
met expliciete dekking voor fractionele inktranden. Sluit deze header niet
simpelweg aan op de oude integeradapter: die kan de vereiste AA/clip niet tekenen.

## Precieze PDF-geometrie uitlezen

De afzonderlijke `PageSpace::from_pdf`-proef leest nu de geselecteerde pagina en dichtstbijzijnde geërfde MediaBox/CropBox/Rotate met de pure-Rust `hayro-syntax`-parser. Dit is uitsluitend syntaxis/metadata, zonder Hayro/Vello-interpreter of rasterisatie. Originele numerieke arraybytes worden strikt als precies vier directe PDF-getallen gelezen zodat lopdf-f32-vernauwing wordt vermeden.

Grenzen: 16 MB bron, maximaal 4.096 pagina’s, 16 ouderknooppunten, expliciete cyclusdetectie, maximaal 4.096 arraybytes en 64 bytes per getal. Ontbrekende MediaBox, extra/ongeldige arrayoperanden, UserUnit, ongeldige rotatie, lege crop en ongeldige pagina/schaal worden afgewezen. De ouder wordt met een nieuwe xref-context opgehaald; de bestaande page-tree-parsercontext heeft die ouder bij het afdalen al bezocht.

Tests verifiëren originele f64-decimalen, dichtstbijzijnde overerving, negatieve kwartslag, pagina-/schaalgrenzen, corrupte geometrie en een cyclische ouder. De bestaande 36 uitgelijnde DynLex-pixelproeven blijven behouden. **Geen koppeling aan native acceptatie of nieuwe corpusdekking:** de volledige praktijk-PDF’s uit de publieke benchmark blijven buiten DynLex.
