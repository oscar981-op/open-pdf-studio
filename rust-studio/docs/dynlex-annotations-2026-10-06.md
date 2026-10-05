# DynLex-annotatielaag: eerste begrensde compositorproef

De gebruiker wil ook **de volledige annotatielaag** in DynLex. De huidige
native app houdt annotaties en PDF-bitmap bewust gescheiden: bewerkbare
`Rectangle`, `Highlight` en `Text Box` worden standaard in
`annotation_layer.rs` door Iced getekend. De beperkte DynLex-route hieronder
kan enkele niet-geselecteerde vormen op de pagina compositeren. Hayro rendert
bestaande PDF-appearances; de engine-probe heeft
alleen voor geïmporteerde Square en Highlight zonder `/AP` een eigen
previewfallback. PDF Save As schrijft de native annotaties als printbare
`/AP`-streams. Deze paden zijn nog niet door één DynLex-annotatierenderer
vervangen.

De opt-in `opds-dynlex-backend::render_native_annotation_overlay` maakt nu
een **afzonderlijke in-process proef**: hij zet een reeds opaak gerenderde
pagina plus pixeluitgelijnde native rechthoeken en Highlights om in een
gevalideerde OPDL-v2-scene. De bestaande DynLex-bufferkernel componeert de
annotaties in model-/laagvolgorde. Square wordt als vier stroken van 2 px
getekend; Highlight als één geel vlak met de 8-bit-dekkingsstap 90/255 die
bij de PDF-appearance met `/ca 0.35` hoort. De invoer is tot 1024×1024,
64 annotaties en 3.538.984 scenebytes begrensd. De Rust-grens controleert
paginaformaat, ondoorzichtige bronpixels, gehele rechthoekcoördinaten,
grenzen, aantallen en alle OPDL-opdrachten vóór de DynLex-ABI.

Een gecontroleerde PDF vergelijkt de
DynLex-composiet rechtstreeks met Hayro na Save As: afzonderlijke rode Square,
afzonderlijke gele Highlight en twee **niet-overlappende** annotaties zijn
volledig RGBA-bytegelijk. Het experiment weigerde tekst, fractionele
coördinaten en overlappende annotaties zonder een gedeeltelijke bitmap terug
te geven. Bij een overlappende Square/Highlight weken acht kanaalbytes op
de doorsnijding van de PDF-appearance-randen één stap af; daarom is die
combinatie nog fallback. Een Highlight over gekleurde PDF-pagina-inhoud week
op een gecontroleerde fixture 3.000 kanaalbytes af; ook die combinatie valt
terug. De proef accepteert Highlight alleen als **alle** onderliggende
RGB-pixels binnen de rechthoek wit zijn. Een directe 8-bit-beeldpatch met de
alternatieve Vello-/256-menging week op de witte achtergrond af en is niet
ingeschakeld. Een dekkende blauwe Square **over gekleurde pagina-inhoud** was
wel bytegelijk. De annotatie blijft bij afwijzing zichtbaar via Iced.

De opt-in app toont de DynLex-composiet als een gecachte pagina-afbeelding
wanneer de gebruiker DynLex heeft gekozen, de pagina op 100% staat, alle
native annotaties binnen deze bewezen subset vallen en er geen annotatie
geselecteerd of in tekstbewerking is. Een selectie schakelt terug naar het
bestaande Iced-canvas zodat slepen, formaatgrepen en inline tekstbewerking
beschikbaar blijven. De cache vergelijkt basisbitmap, pagina en de werkelijke
annotatie-inhoud; een edit maakt een nieuwe bitmap. De statusregel meldt
PDF- en annotatierenderer afzonderlijk. Een gerichte app-unit-test controleert
cachehergebruik en invalidatie na een annotatieverplaatsing. De 1200×832-
releaseapp is op een echte PDF geopend en als X11-venster vastgelegd; de
computergebruik-koppeling bood geen native app aan en synthetische XTest-
muisklikken veranderden het ribbon niet. Een betrouwbare end-to-end
muistest voor annotatie-aanmaken/selecteren ontbreekt daarom nog.

De PDF-importroute gebruikt deze compositor nog niet. Hierna moeten
geïmporteerde `/AP`-streams, annotaties
zonder `/AP`, tekst/free-text, meerdere kleurmodellen, transparantiegroepen,
rotatie/CropBox en subpixelplaatsing met echte PDF's worden geverifieerd.
De huidige app bevat nog geen PDFium-backend; Hayro blijft de fallback.

Gerichte reproductie vanuit `rust-studio`:

```sh
cargo test --offline -p opds-native-app --features dynlex-experimental \
  dynlex_native_rectangle_and_highlight_match_saved_pdf_appearances
cargo test --offline -p opds-native-app --features dynlex-experimental \
  dynlex_annotation_overlay_tests
cargo test --offline -p opds-native-app --features dynlex-experimental \
  dynlex_highlight_over_colored_page_falls_back
```

De andere OPDL-v2/v4-verifiers moeten bij verdere wijzigingen blijven slagen.
Er is geen algemene PDF-objectdekking of prestatievoordeel bewezen.
