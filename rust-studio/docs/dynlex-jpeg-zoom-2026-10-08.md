# Originele JPEG-scene en gedeelde decode voor DynLex-zoom

Historisch verslag van 7bdf331f. Het vervolg repareert 125% voor begrensde
downsampling: [tweede samplingstadium](dynlex-jpeg-sampling-2026-10-08.md).

Het ongewijzigde openbare Albany-contract, pagina 1/49, rendert nu zelfstandig
via DynLex op **25%, 100% en 200%**. Alle 121.176 / 1.938.816 / 7.755.264 RGBA-bytes
zijn exact gelijk aan Hayro. Dit blijft een gescande pagina: de letters zijn
beeldpixels, geen nieuw ondersteunde fonttekst. Er is geen Hayro-fallback.

## Voorbereiding en geheugen

`RasterPlan` bewaart één oorspronkelijke DeviceRGB8/DCTDecode-image, zijn
plaatsing en geërfde paginageometrie. Een begrensde tokenizer leest originele
f64-getallen voor de beperkte q/Q/cm/g/Do-syntaxis. De precieze PageSpace-boxen
moeten exact overeenkomen met de integergeometrie; nabij-gehele waarden worden
niet stilzwijgend afgerond. Er is precies één beeldpaint, zonder andere paint,
clip of annotaties. De oude routes blijven beschikbaar voor hun bewezen subset.

Een `DynlexDocument` deelt **één** mutex-beveiligde RGB-decodecache over alle
paginascenes en gelijktijdige zoomaanvragen. Vóór een ander beeld wordt gedecodeerd
wordt de vorige buffer vrijgegeven. Een page-slot of in-flight caller kan dus
geen extra oude 120 MB-bronbuffer vasthouden. Voor Albany blijft precies
100.980.000 bytes RGB bewaard en is één decode uitgevoerd tijdens de hele
25/100/200%-replay. De gecomprimeerde image blijft in de begrensde paginascene.

Harde grenzen: bron-PDF 16 MB, maximaal vier bewaarde page-slots, maximaal 16 MB
gecomprimeerde image per slot (conservatief samen 64 MB), één RGB-cache maximaal
120 MB, JPEG-zijden 8192 en maximaal 40 miljoen bronpixels. Content maximaal
65.536 bytes / 1024 tokens / 128 bytes per token, q/Q-diepte 16. Tijdelijke
lopdf-/codec-/resamplingbuffers en uitvoer zijn **niet** inbegrepen in de 120 MB
cachegrens. Dit is geen claim dat het gehele proces maximaal 120 MB gebruikt.

Elke zoom resamplet met Rust pic-scale uit de oorspronkelijke JPEG-decode.
De 100%-bitmap wordt niet als bron voor correcte andere zoomniveaus gebruikt.
DynLex tekent de nieuwe geordende RGB-scene; geen Hayro/Vello-rasterizer in deze
route. De JPEG-codec blijft zune-jpeg in Rust, geen eigen DynLex-JPEG-decoder.
Schaal, plaatsing en CropBox moeten exact uitgelijnde pixelranden opleveren.
Rotatie/CropBox worden na resampling toegepast. Fractionele beeldranden,
beeld-viewport/tiling, meerdere images en vectoroverlay horen nog niet bij deze
nieuwe gecachte zoomroute. Bestaande 100%-hybridefuncties zijn niet verwijderd.

De uitvoer blijft maximaal 4096 per zijde / zes miljoen pixels. Voor precies
één OPDL-v3 RGB-record is de gevalideerde scenegrens verhoogd naar 18.000.064
bytes (64-byte header/record plus 18 MB RGB). Alle andere ordered scenes houden
hun eerdere 3.538.984-bytegrens. De C-ABI controleert voor de uitbreiding ook
versie/count/kind; Rust valideert vóór die grens de volledige lengtes, offsets,
rechthoeken en pixelgrens. De oude CLI en v1/v2-grenzen zijn niet verruimd.

## Meting op de volledige externe scan

Vijftien afwisselende gepaarde nieuwe processen; warme OS-cache. Elk proces opent
een document, rendert eerst 25/100/200%, daarna drie herhalingen van die reeks.
Elke uitvoer wordt buiten de rendertimer bytegewijs gecontroleerd. Geen bitmap-
zoomcache in deze proef. De huidige Hayro-app-route maakt per render een nieuwe
RenderCache; DynLex hergebruikt zijn originele RGB. Dit vergelijkt de huidige
app-routes, **niet gelijkgetrokken interne caches of uitsluitend kernels**.

| Meting | DynLex | Hayro |
|---|---:|---:|
| Eerste render 25%, lege enginecache | 242,93 ms | 325,25 ms |
| Herhaalde 25% | 35,23 ms | 317,46 ms |
| Herhaalde 100% | 70,94 ms | 347,60 ms |
| Herhaalde 200% | 84,70 ms | 350,77 ms |
| Proces-piek-RSS, mediaan | 134,67 MiB | 207,15 MiB |

Op elke schaal wint DynLex de mediane warme render in 15/15 procesparen.
RSS omvat de hele replay inclusief bytecontrole; eerste-render-tijd sluit
bestandslezen/openen uit (afzonderlijk opgeslagen). Geen koude OS-cache, geen
interactieve GUI-latentiemeting en geen algemene PDF-snelheidsclaim.
Ruwe samples: `dynlex-public-jpeg-zoom-bench-2026-10-08.json`.

## Reproduceren

```sh
cargo build --release -p opds-dynlex-backend --all-features --bins
python3 scripts/verify_public_jpeg.py --probe target/release/backend_probe \
  --manifest docs/public-pdf-corpus-2026-10-08.json \
  --output /tmp/jpeg-zoom-parity.json
python3 scripts/bench_jpeg_zoom.py --probe target/release/jpeg_zoom_probe \
  --render-probe target/release/backend_probe \
  --manifest docs/public-pdf-corpus-2026-10-08.json \
  --output /tmp/jpeg-zoom-bench.json
cargo test -p opds-dynlex-backend --features kernel --test jpeg_zoom
```

Gerichte tests controleren de volledige pixels bij vier rotaties, verschoven
CropBox, beeldmarges en vijf opeenvolgende schalen. De cachetest controleert
concurrent decodehergebruik en evictie tijdens vijf pagina's. Ongeldige zoom,
naargeheel afgeronde CTM-getallen, onbewezen paints/tekst, onbekende/extra
operanden, q/Q-fouten en grote/corrupte scene-records worden geweigerd. Echte
Albany-pagina 6 met fonttekst en schaal 0,3 blijven zonder vervangende bitmap.

De eerdere vier-kernelbenchmark en 1/30-audit blijven historische nulmetingen.
Dit is vooruitgang voor één volledige rasterpagina, geen volledige PDF-dekking.
DynLex gebruikt LLVM/C++ en libc in de toolchain; de app en nieuwe glue zijn Rust.

Validatie: 247 workspace-tests met alle features, standaard native check en
releasebuild slagen. Nieuwe audit van dezelfde 30 openbare pagina/zoomgevallen:
**3/30 gerenderd**, 27 unsupported, nul procesfouten/time-outs. Zie
`dynlex-public-jpeg-zoom-audit-2026-10-08.json`; alleen de twee extra zoomniveaus
van dezelfde scan zijn toegevoegd, geen nieuwe tekst- of vectorpagina.
Willekeurige Ctrl+scroll-schalen zoals 115% hebben nog fractionele randen en
blijven unsupported. De volgende stap is correcte bronresampling/coverage op
die fractionele schermgrenzen, met dezelfde strikte bytevergelijking.

Extra UI-/samplingcontrole vóór vrijgave vond op 125% (765×990) **102.935
afwijkende pixels / 241.881 RGBA-kanalen**, maximale kanaalfout 108. Die schaal
is daarom expliciet geweigerd in de nieuwe route. Diagnose in
`dynlex-jpeg-zoom-125-rejected-2026-10-08.json`. Lokale Hayro 0.7.1
`renderer.rs::draw_image` berekent het resample-rooster via f32-advances en
ceil: `f32(6600 * f32(990 / 6600)) = 990.000061`, dus 991 bronresamplerijen
voor 990 uitvoerrijen. De kandidaat resamplede direct naar 990. Dit wijst op
het ontbrekende tweede samplingstadium; dat is nog geen bewezen oplossing.
De nieuwe route controleert nu ook dat de tussenliggende f32/ceil-omvang met de
gevraagde pixelomvang overeenkomt. Alleen 25/50/100/200% zijn vrijgegeven; 50%
is eveneens op de hele externe pagina bytegelijk (484.704 bytes).

De oude volledige verifier blijft exact: 47 PDF-pagina's/4.964.352 pixels en
drie v4-pagina's/147.456 pixels. Na de extra samplingguard slaagden de gerichte
zoom/cachetests en externe pixel-/afwijzingsproef opnieuw. De nieuwe release
is geopend op Albany pagina 1/49, 100%, met `Renderer: DynLex` en echte
miniatuur. De oudere testprocessen zijn gesloten; PDF-bestanden zijn niet
gewijzigd. Deze UI-controle bewijst geen algemene Ctrl+scroll-pariteit.
