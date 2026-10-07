# Zoom- en kerneldiagnose, 8 oktober 2026

De native app toont bij zoomen direct de vorige bitmap op de nieuwe grootte.
Na 160 ms zonder volgende zoomactie start één scherpe paginarender; oude
resultaten worden genegeerd. Een cachetreffer voor dezelfde pagina/schaal
gebruikt de bestaande bitmap meteen. Dit is een interactieverbetering, geen
versnelling van de PDF-render zelf. In een echt 1200×832-venster op pagina 1
van de zevenpagina-Barn Relocation-PDF was de grotere, tijdelijk zachte pagina
zichtbaar binnen circa 0,34 s na Ctrl+scroll; bij twee snelle zoomstappen
verscheen de eindstand ook voordat de scherpe render klaar was.

`four-backend-zoom-quarter-2026-10-08.json` bevat twee warme procesrondes op
zes PDF's bij schaal 0,25. Iedere ronde rendert pagina 1 met Hayro, DynLex,
PDFium en Eigen Rust in afwisselende volgorde, vergelijkt alle RGBA-kanalen
met Hayro en bewaart wandtijd en piek-RSS. Dit is een korte diagnose, geen
betrouwbare algemene snelheidsrangschikking of koude-startmeting.

| PDF | Hayro mediaan | PDFium mediaan | PDFium afwijkende kanalen | DynLex/Eigen Rust |
| --- | ---: | ---: | ---: | --- |
| Rasterfixture | 19,3 ms | 8,3 ms | 8.588 | beide Hayro-fallback |
| Vectorfixture | 8,1 ms | 7,1 ms | 68 | beide Hayro-fallback |
| Hybride fixture | 6,6 ms | 9,0 ms | 865 | beide Hayro-fallback |
| `Tekst.pdf` | 48,7 ms | 52,7 ms | 29.800 | beide Hayro-fallback |
| `Combinatie Raster, vector, tekening images.pdf` | 763,7 ms | 120,1 ms | 62.494 | beide Hayro-fallback |
| Barn Relocation | 978,1 ms | 276,8 ms | 133.591 | beide Hayro-fallback |

Ten tijde van deze meting weigerden DynLex en Eigen Rust schaal 0,25 volledig;
de gelijke pixels in die kolommen zijn **Hayro**, niet hun eigen kernels.
PDFium rendert sommige complexe pagina's sneller maar is hier niet bytegelijk
aan Hayro. De app laat de gebruiker linksonder een kernel kiezen en meldt
de werkelijk gebruikte renderer/fallback apart.

In het echte 1200×832-venster zijn op de onderste statusbalk achtereenvolgens
Hayro, DynLex, PDFium en Eigen Rust gekozen. Op de kleine ondersteunde
fill/stroke-PDF meldde de status ook telkens die werkelijk gebruikte backend.

Een faseproef met `OPDS_AP_PROFILE=1` op dezelfde schaal mat voor Barn
Relocation 23,2 ms lezen, 0,2 ms Hayro-openen en 942,4 ms renderen; voor de
gemengde PDF 0,9/0,5/738,2 ms. Zoomen wordt dus vooral door pagina-inhoud
vertraagd, niet door het PDF-openpad of het schrijven van de bitmap. Een
octree of tegelraster alleen is waarschijnlijk onvoldoende wanneer ieder
tegel opnieuw alle contentoperatoren moet doorlopen. De kansrijke richting
is een eenmalig gevalideerde, geordende display-list per pagina met
begrensde ruimtelijke index, gevolgd door viewport-tegels en een cache van
reeds gerenderde schalen. Daarbij moeten clips, transparantie, fonts,
annotaties en laagvolgorde correct blijven. De huidige DynLex-subset en de
Hayro-fallback blijven actief totdat echte raster-, vector- en hybride
PDF's op meerdere zoomniveaus pixelpariteit aantonen.

Reproduceren vanuit `rust-studio/`:

```sh
python3 scripts/bench_four_backends.py --probe target/release/backend_probe \
  --pdfium-library /pad/naar/libpdfium.so --rounds 2 --scale 0.25 \
  --output docs/four-backend-zoom-quarter-2026-10-08.json PDF1.pdf PDF2.pdf
```

De daaropvolgende beperkte scene-cache en vector-zoom/viewport-API staan in
[dynlex-prepared-scene-2026-10-08.md](dynlex-prepared-scene-2026-10-08.md).
De bovenstaande ruwe benchmark is niet opnieuw geïnterpreteerd als resultaat
van die nieuwe code; raster-/hybride zoom blijft daar buiten de subset.
