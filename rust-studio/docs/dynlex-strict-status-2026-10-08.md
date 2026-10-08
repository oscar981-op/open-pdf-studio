# DynLex-only: actuele dekking en procesmeting, 8 oktober 2026

Geen van de DynLex-metingen in dit rapport gebruikt Hayro-fallback. Hayro is alleen de apart gemeten referentie. `backend_probe --dynlex-strict` gebruikt dezelfde strikte route als de native viewer. Alle succesvolle buffers zijn volledig bytegelijk aan de referentie.

15 afwisselende paren per ondersteunde PDF, pagina 1 op 100%, nieuw proces per run, warme OS-cache. Wandtijd omvat lezen, metadata-openen, voorbereiden, renderen en RGBA-bestandsuitvoer; dit is geen gecachte zoommeting of werkelijk koude start.

| Bestand | DynLex mediaan | Hayro mediaan | DynLex winstparen | Piek-RSS DynLex/Hayro |
| --- | ---: | ---: | ---: | ---: |
| simple-fill-stroke.pdf | 7.443 ms | 7.801 ms | 10/15 | 5388/6492 KiB |
| full-page-raster.pdf | 8.240 ms | 8.248 ms | 6/15 | 5916/6556 KiB |
| two-images-hybrid.pdf | 9.124 ms | 8.244 ms | 7/15 | 5620/6544 KiB |
| clipped-real-shape-582.pdf | 53.080 ms | 52.129 ms | 7/15 | 20756/22380 KiB |

Dit toont geen overtuigend algemeen snelheidsvoordeel. De eerste drie bestanden zijn kleine gecontroleerde raster/vector/hybride fixtures. `clipped-real-shape-582.pdf` bevat een geïsoleerde clip uit een echt document, niet het volledige document.

Volledige praktijkbestanden die in deze strikte proef **geen DynLex-beeld** leverden:
- Tekst.pdf
- Combinatie Raster, vector, tekening images.pdf
- Technische tekening.pdf
- 20260316 - Barn Relocation - 389 E Hemenway Lane - for Permit.pdf

Zij krijgen geen snelheidsscore; afwijzing is geen snelle render. Hiermee is brede/100% PDF-dekking nadrukkelijk niet aangetoond. Algemene tekst/fonts, curven, complexe graphics state, volledige annotatiedekking, multipage- en zoombeperkingen blijven open. Eerst moeten volledige representatieve praktijkpagina’s zelfstandig renderen; daarna volgt een zinvolle brede prestatievergelijking.

Reproduceren vanuit `rust-studio`:

```sh
cargo build --release -p opds-dynlex-backend --all-features --bin backend_probe
python3 scripts/bench_strict_dynlex.py --probe target/release/backend_probe \
  --rounds 15 --output rapport.json INPUT1.pdf INPUT2.pdf
```

Ruwe samples: `dynlex-strict-bench-2026-10-08.json`. LBVH-timing staat apart in `dynlex-lbvh-2026-10-08.md`; die andere meetgrens mag niet als volledige-procesversnelling worden gepresenteerd.


## Uitgebreid complex corpus

Op verzoek van de gebruiker is daarna ieder paginanummer van zeven volledige
praktijkbestanden gecontroleerd, op 25%, 100% en 200%. Geen fallback is toegestaan.
De uitkomst is **0 ondersteunde pagina/zoom-combinaties van 276**, verdeeld over
92 pagina's. Alle 276 werden expliciet unsupported, zonder RGBA-uitvoer; geen
onverwachte procesfouten. Dit is een objectieve huidige beperkingsmeting, geen
algemene benchmarkrangschikking of snelheidscore.

| Volledig praktijkbestand | Pagina's | Ondersteund / geprobeerd |
| --- | ---: | ---: |
| 20260316 - Barn Relocation - 389 E Hemenway Lane - for Permit.pdf | 7 | 0 / 21 |
| Combinatie Raster, vector, tekening images.pdf | 1 | 0 / 3 |
| Technische tekening.pdf | 4 | 0 / 12 |
| Tekst.pdf | 5 | 0 / 15 |
| Text pdf gecombineerd.pdf | 28 | 0 / 84 |
| Zware vector PDF.pdf | 19 | 0 / 57 |
| rapport-constructie.pdf | 28 | 0 / 84 |

De bestaande adapter heeft bovendien nog een éénpagina-beperking. Afwijzing
van een paginanummer boven 1 is dus geen afzonderlijke analyse van iedere
tekenoperator op die pagina. Voor deze bestanden zijn onder meer multipage,
algemene paden/curven, fonts/tekst, graphics state en clips nog open. De volgende
acceptatiegrens is minimaal één **volledige praktijkpagina**, zelfstandig en
zonder fallback, gevolgd door alle pagina's/zooms van dit corpus. Een geïsoleerde
clip of synthetische fixture telt daar niet als vervanging voor.

Reproduceren: `python3 scripts/audit_strict_dynlex_corpus.py --probe target/release/backend_probe --output rapport.json '../test pdf-bestanden/Testbestanden/'*.pdf`.
De audit gebruikt `pdfinfo` uitsluitend voor het paginatal en vergelijkt
succesvolle pixels met een afzonderlijke Hayro-aanroep. Ruwe documenthashes,
pagina's, schalen en foutredenen: `dynlex-complex-corpus-2026-10-08.json`.


## Herhaling na ondersteuning van meerdere pagina’s

De oorspronkelijke paginanummer-1-grens is verwijderd uit de native route;
legacy fixture-CLI’s behouden hun éénpagina-contract. De audit is opnieuw
uitgevoerd met de releasebuild: **0 ondersteund, 276 unsupported, 0 procesfouten**.
Ruwe resultaten: `dynlex-complex-multipage-2026-10-08.json`.
Dit resultaat wordt dus niet meer uitsluitend veroorzaakt door `page != 1`.
De overige limieten blijven gelden: onder andere maximaal 16 MB bron-PDF,
gehele paginacoördinaten, resources/tekst, algemene paden en annotaties.
Gedetailleerde nieuwe tests en vroege blokkades staan in
[dynlex-multipage-2026-10-08.md](dynlex-multipage-2026-10-08.md).
