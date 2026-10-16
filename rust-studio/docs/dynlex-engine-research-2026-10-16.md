# DynLex: bronbuffer, andere engines en eerste-render-tijd (16 oktober 2026)

De actuele DynLex-kernel is **niet aantoonbaar sneller in het algemeen**. Op de
originele `Tekst.pdf`, pagina 5 op 100%, waren vijftien afwisselende gepaarde
nieuwe processen volledig RGBA-gelijk. DynLex had 123,958 ms mediane wandtijd
en 19.216 KiB piek-RSS; Hayro 58,434 ms en 19.704 KiB. DynLex won 0/15 paren.
Dit omvat processtart, lezen, voorbereiding, renderen en het schrijven van RGBA;
het meet geen warme zoom in de app. De losse workerfase besteedde circa 16 ms
aan `lopdf`-laden en circa 40 ms aan JPEG-decode, resampling en compositie.
De eerste nieuwe zoom blijft dus een eigen prestatieprobleem.

De documentgebonden rastervoorbereiding maakte een onnodige volledige
`bytes.to_vec()`-kopie voordat `PageSpace` dezelfde PDF-bron las. De route vanuit
`DynlexDocument` deelt nu de bestaande `Arc<Vec<u8>>`; de publieke
`PreparedPage::from_pdf_page(&[u8], ...)` blijft de bron zelf bezitten als zij
die voor de geometrieparser nodig heeft. De strikte acceptatie en het
bitmapresultaat zijn ongewijzigd.

Een gecontroleerde A/B-proef bouwde dezelfde huidige code tweemaal, met als
enige verschil wel of geen bronkopie in `PageSpace`. Een geldige 7.148.200-byte
PDF was gemaakt van de bestaande `full-page-raster.pdf` met één niet-gerefereerd
groot streamobject, zodat de zichtbare pagina en de decode identiek bleven.
Twintig afwisselende procesparen op pagina 1/100% leverden voor kopiëren
39,046 ms en 26.288 KiB mediaan op; voor delen 37,532 ms en 26.292 KiB.
De gedeelde bron won slechts 10/20 paren. Iedere volledige RGBA-buffer was
onderling en met Hayro bytegelijk. Deze proef **bewijst geen meetbare
end-to-end snelheids- of RSS-winst**; zij rechtvaardigt uitsluitend het
wegnemen van een onnodige allocatie. Ruwe samples:
[`dynlex-source-arc-ab-2026-10-16.json`](dynlex-source-arc-ab-2026-10-16.json)
en [`dynlex-tekst-page5-speed-2026-10-16.json`](dynlex-tekst-page5-speed-2026-10-16.json).
De synthetische grote bron is geen representatief praktijkcorpus.

Vergelijking met primaire engine-documentatie:

| Engine | Relevante aanpak | DynLex-status |
| --- | --- | --- |
| [MuPDF DisplayList](https://mupdf.readthedocs.io/en/latest/reference/javascript/types/DisplayList.html) | Voorbereide tekenopdrachten op meerdere resoluties opnieuw afspelen | Brongebonden voorbereide pagina's en een begrensde LRU bestaan al; de tekst/hybride worker moet nieuwe zooms vaak nog apart voorbereiden. |
| [PDF.js PDFPageProxy](https://mozilla.github.io/pdf.js/api/draft/module-pdfjsLib-PDFPageProxy.html) en [RenderTask](https://mozilla.github.io/pdf.js/api/draft/module-pdfjsLib-RenderTask.html) | Operatorlijst, opruimen en annuleerbare renderopdrachten | Het native venster weigert stale resultaten, maar DynLex zelf kan een lopende paginarender nog niet fijnmazig stoppen. |
| [PDFium progressive API](https://pdfium.googlesource.com/pdfium/+/refs/heads/main/public/fpdf_progressive.h) | Rendering pauzeren, hervatten en expliciet sluiten | Een mogelijke ontwerpaanwijzing voor begrensde Rust-tegels en controlepunten; geen PDFium-code of automatische fallback in DynLex. |

De volgende prestatiewijziging vraagt een brongebonden, begrensde
beeld-/tekstvoorbereiding die op nieuwe zooms kan worden hergebruikt, plus
controlepunten voor annulering. Daarvoor zijn eerst volledige pixelproeven op
hybride pagina's, rotatie/CropBox, meerdere beelden en transparantie nodig.
Vervolgens moeten eerste en warme zoom, piek-RSS en cacheverbruik op hetzelfde
echte corpus gepaard worden gemeten. `unsupported` blijft zonder bitmap en
telt niet als snelle render.

Verificatie na de wijziging: `cargo test --workspace --all-features --offline`
slaagt (native app: 150 geslaagd, 2 bestaande ignored); release-app en
`backend_probe` bouwen, `cargo fmt --all -- --check` en `git diff --check`
slagen. De geïsoleerde GUI-resizeproef staat in
[`current-ui-parity.md`](current-ui-parity.md).
