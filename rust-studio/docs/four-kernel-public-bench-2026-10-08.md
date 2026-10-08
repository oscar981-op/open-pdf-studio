# Vier kernels op openbare PDF’s — 8 oktober 2026

Zeven gedownloade documenten (101.134.542 bytes, 373 pagina’s) vormen het nieuwe lokale corpus. Deze benchmark bemonstert **10 pagina’s bij 25%, 100% en 200%: 30 pagina/zoomgevallen**, niet alle 373 pagina’s. Er is geen automatische rendererfallback meegemeten.

## Dekking en meetprotocol

| Kernel | Gerenderde gevallen | Afgewezen | Procesfouten |
|---|---:|---:|---:|
| Hayro | 28/30 | 2 | 0 |
| PDFium | 28/30 | 2 | 0 |
| DynLex strikt | 0/30 | 30 | 0 |
| Eigen Rust strikt | 0/30 | 30 | 0 |

Bij Hayro/PDFium overschrijden beide grote NPS-bladen op 200% de bestaande bitmaplimiet. Dit zijn applicatiegrenzen, geen bewijs dat die libraries de PDF-inhoud niet begrijpen. DynLex ondersteunt geen van deze volledige praktijkpagina’s. Eigen Rust accepteert alleen pagina 1 op 100% en een smalle vectorinhoud; zijn afwijzingen omvatten die routergrens. Afwijzingen krijgen geen snelheidsscore.

Controle buiten de corpusscore: `simple-fill-stroke.pdf`, 256×192 op 100%, rendert via alle vier expliciete routes met identieke 196.608 RGBA-bytes. Zie [controle-JSON](four-kernel-control-2026-10-08.json). De nuldekking van de experimentele kernels komt niet door ontbrekende kernelbuilds.

- Machine: AMD Ryzen 7 3700U, Linux x86_64. Release-probe en PDFium-library zijn met SHA-256 vastgelegd in de ruwe resultaten.
- Eerst een ongetimede controle per backend; daarna zeven opeenvolgende procesruns per geslaagde route, met afwisselende backendvolgorde. 28 × 2 × 7 = 392 getimede renders.
- Tijd omvat processtart, lezen, parsen, renderen en RGBA naar bestand schrijven; piek-RSS komt van `/usr/bin/time`. Pixelvergelijking gebeurt buiten de tijdmeting. Elke herhaling moet dezelfde bitmaphash geven.
- Warme OS-bestandscache; geen claim over koude start, een hergebruikte document-scene, GUI-interactie, Ctrl+scroll-latentie of tegelcaches. De processen hebben een grens van 120 seconden.
- PDFium is rechtstreeks via Rust-FFI aangeroepen. DynLex strikt levert nooit een Hayro-bitmap; Eigen Rust strikt evenmin. De DynLex-route gebruikt nog de Rust PDF-metadata-parser, niet Hayro’s rasterizer.

## Resultaten bij 100%

Procesmediaan in milliseconden; RSS is de mediaan van de piek-RSS per proces, in MiB. Alle tien pagina’s renderen bij deze schaal met Hayro en PDFium.

| Geval | Pagina | Hayro ms | PDFium ms | Hayro RSS MiB | PDFium RSS MiB | PDFium sneller in paren |
|---|---:|---:|---:|---:|---:|---:|
| Wetenschappelijke tekst | 2 | 40.20 | 36.27 | 13.4 | 14.1 | 4/7 |
| Tekst + figuur | 3 | 91.73 | 68.54 | 44.5 | 27.0 | 7/7 |
| Gescande contractpagina | 1 | 370.13 | 279.79 | 201.4 | 26.6 | 7/7 |
| Contracttekst | 6 | 21.94 | 23.64 | 13.9 | 13.4 | 3/7 |
| Zware gescande bouwtekening | 3 | 829.27 | 623.53 | 438.7 | 93.2 | 7/7 |
| Groot vectorontwerpblad | 3 | 146.36 | 127.81 | 69.8 | 68.6 | 7/7 |
| Grote vectorplattegrond | 71 | 118.35 | 142.42 | 70.1 | 70.4 | 0/7 |
| TfL-kaart (afmetingen verschillen) | 1 | 223.41 | 121.31 | 28.3 | 22.4 | 7/7 |
| Handleidingtekst | 48 | 40.00 | 44.96 | 19.8 | 19.8 | 3/7 |
| Tekst + tabel | 7 | 46.79 | 30.82 | 19.0 | 17.8 | 7/7 |

Over alle 28 geslaagde gevallen is de PDFium-mediaan lager in 21 gevallen; PDFium wint 149 van 196 gepaarde procesruns. Dat is geen algemene snelheidsclaim: inhoud, outputpixels en resolutieafronding verschillen. De grote vectorplattegrond op pagina 71 en sommige tekstpagina’s zijn hier sneller via Hayro.

## Pixelverschillen

Van de 28 succesvolle Hayro/PDFium-paren hebben 25 gelijke bitmapafmetingen. **Geen van die 25 is RGBA-bytegelijk.** Dit kan onder meer door font-/AA- en beeldfilterverschillen komen; Hayro als meetreferentie bewijst niet op zichzelf welke renderer correcter is.

De TfL-kaart heeft fractionele paginagrootte: Hayro rondt af met floor, de huidige PDFium-route met ceil. Op 25% zijn de bitmaps 297×226 en 298×227; op 100% 1190×907 en 1191×908; op 200% 2381×1815 en 2382×1816. Deze drie renders zijn niet als gelijke pixelrasters vergeleken.

| Geval op 100% | Afwijkende pixels | Afwijkende RGBA-kanalen | Gem. absolute kanaalfout (0–255) |
|---|---:|---:|---:|
| Wetenschappelijke tekst | 95257 | 285771 | 4.9826 |
| Tekst + figuur | 69327 | 207054 | 3.2793 |
| Gescande contractpagina | 80373 | 214790 | 0.3549 |
| Contracttekst | 25307 | 75921 | 1.4950 |
| Zware gescande bouwtekening | 348472 | 1015152 | 1.5197 |
| Groot vectorontwerpblad | 403079 | 1209237 | 2.4212 |
| Grote vectorplattegrond | 117896 | 353688 | 0.2281 |
| Handleidingtekst | 67541 | 202623 | 3.5565 |
| Tekst + tabel | 18100 | 53030 | 0.5145 |

## Corpus en reproduceerbaarheid

Bestanden staan uitsluitend in `~/.cache/opds-benchmarks/internet-2026-10-08/`; geen externe PDF’s in Git. Het manifest bewaart bron-URL, gedownloade revisiehash, grootte, pagina-aantal en `pdfinfo`. Publieke beschikbaarheid is geen toestemming tot herdistributie. De downloader weigert een gewijzigde bronhash in plaats van stil een andere revisie te benchmarken.

| ID / bron | Pagina’s | MB (decimaal) | Aard |
|---|---:|---:|---|
| [attention](https://arxiv.org/pdf/1706.03762) | 15 | 2.22 | Wetenschappelijke tekst, formules en figuren |
| [albany-contract](https://news.albanyoregon.gov/images/stories/finance/bids/749-SS-25-02_2025_Pipe_Bursting_Project/02_Contract_Documents_SS-25-02.pdf) | 49 | 2.47 | contract text |
| [albany-drawings](https://news.albanyoregon.gov/images/stories/finance/bids/749-SS-25-02_2025_Pipe_Bursting_Project/08_Construction_Drawings_SS-25-02.pdf) | 9 | 49.57 | Gescande bouwtekeningen (raster), geen vector |
| [nps-schematic](https://parkplanning.nps.gov/showFile.cfm?projectID=116110&sfid=768676) | 142 | 30.72 | Grote ontwerp-/technische vectorbladen |
| [tfl-tube](https://content.tfl.gov.uk/standard-tube-map.pdf) | 2 | 0.54 | vector map |
| [nps-drawing-manual](https://www.nps.gov/dscw/upload/ReferenceManual10A_April2001.pdf) | 126 | 8.79 | drawing manual |
| [usgs-topo-guide](https://pubs.usgs.gov/tm/tm11b2/downloads/tm11b2_v2.pdf) | 30 | 6.82 | text and map images |

PDF’s zijn met `pdfinfo`, `pdfimages` en geselecteerde Poppler-paginapreviews gecontroleerd. Een lokale contact sheet staat in `previews/contact-sheet.png` onder de corpuscache. NPS-pagina 3 bevat 10.546 contentoperators, waaronder 1.255 lijnen, 727 Tj-opdrachten en 260 curven; de 49,6-MB Albany-tekeningenset bestaat juist uit grote JPEG-scans. De geselecteerde handleiding-/USGS-pagina’s bevatten tekst/tabellen, niet de kaartbeelden van andere pagina’s.

```bash
# Vanuit rust-studio; Python met Pillow en Poppler pdfinfo vereist.
python3 scripts/download_public_corpus.py --manifest docs/public-pdf-corpus-2026-10-08.json \
  --directory "$HOME/.cache/opds-benchmarks/internet-2026-10-08"
cargo build --release -p opds-dynlex-backend --all-features --bin backend_probe
python3 scripts/bench_public_corpus.py \
  --manifest "$HOME/.cache/opds-benchmarks/internet-2026-10-08/manifest.json" \
  --probe target/release/backend_probe --pdfium-library /absoluut/pad/naar/libpdfium.so \
  --rounds 7 --output /tmp/four-kernel-public-bench.json
```

Bronrevisie vóór deze toevoeging: `154a3290` met de nieuwe strikt-directe probevlaggen uit deze wijziging. De exacte gemeten binaryhash staat in [ruwe samples](four-kernel-public-bench-2026-10-08.json). Build-/scriptformattering na de meting verandert de oorspronkelijke metingen niet.

## Gevolg voor het vervolg

DynLex is nog niet bruikbaar voor deze volledige praktijkpagina’s. Eerste prioriteit is één complete externe pagina zelfstandig ondersteunen met correcte geometrie, fonts/paden of beelddecoding. LBVH/tiling alleen lost de ontbrekende PDF-interpretatie niet op. Houd unsupported expliciet; herhaal dezelfde corpusbench pas na inhoudelijke uitbreiding. Voor eerlijke directe pixelvergelijking moet bovendien de PDFium-dimensieafronding worden gelijkgetrokken en afzonderlijk geverifieerd.

Validatie na toevoeging: 238 workspace-tests met alle features geslaagd; standaard-, losse PDFium- en losse Eigen Rust-probechecks geslaagd; release-probebuild, `cargo fmt`, Python-compile en `git diff --check` geslaagd. App-rendercode en caches zijn voor deze benchmark niet gewijzigd.

## Latere uitbreiding: eerste volledige externe scan

De nulmeting hierboven blijft historisch intact. Met de daarna toegevoegde begrensde DCTDecode-route rendert Albany-contract pagina 1 op 100% zelfstandig via DynLex en volledig bytegelijk aan Hayro: 1/30 gevallen van het openbare corpus. Zie [JPEG-implementatie, grenzen, native UI en gepaarde meting](dynlex-public-jpeg-2026-10-08.md). Geen algemene tekst/vector/zoomdekking.
