> Nieuw: de native route selecteert nu iedere begrensde documentpagina en bewaart maximaal vier scenes. Zie [meerdere pagina’s](dynlex-multipage-2026-10-08.md). De eerste-pagina-beschrijving hieronder is historisch.

> Actuele wijziging: de native DynLex-keuze heeft sinds c830cae3 geen Hayro-fallback.
> Unsupported geeft een melding. De hieronder beschreven eerste 16×16-index
> is inmiddels vervangen door [LBVH](dynlex-lbvh-2026-10-08.md).
> Oudere benchmarkdata hieronder blijven historische scene-cachemetingen.

# Herbruikbare DynLex-paginascene, 8 oktober 2026

`DynlexDocument` bewaart de originele PDF-bytes en bereidt de beperkte eerste
pagina eenmaal voor in een threadveilige `OnceLock`. Ook een niet-ondersteunde
PDF wordt onthouden. De native app houdt deze brongebonden cache vast over
zoomacties en rendererwissels; een nieuw document krijgt zijn eigen cache.
De bestaande bitmapcache en generatiecontrole voor late resultaten blijven
van toepassing. De eerste paginarender bij openen gebruikt nog de bestaande
stateless route; de cache wordt bij de eerste volgende DynLex-render gevuld.
Geïmporteerde annotatie-appearances hebben nog hun afzonderlijke route.

De gevalideerde OPDL-scene blijft herbruikbaar voor raster, vector en hybride
op 100%. Alleen voor de dekkende, pixeluitgelijnde vectorvullingen en
asgerichte stroken is schaalondersteuning toegevoegd: iedere paginagrens en
iedere opdrachtrand moet op de gekozen schaal exact geheel blijven. Een
schaal van 200% kan zo een 1px-strook als een dekkende 2px-rechthoek tekenen.
Fractionele randen, beeldresampling, maskerschaling en algemene antialias
vallen terug op Hayro. Dit is geen algemene zoom- of PDF-dekking.

`PreparedPage::render_viewport` kan voor deze vector-subset een begrensde
rechthoek van de geschaalde pagina tekenen. Een vaste 16×16-index selecteert
opdrachten op hun begrenzing; de originele verfvolgorde blijft behouden.
Er zijn maximaal 16.384 opdrachten en 262.144 indexverwijzingen. Bij overschrijding
van het aantal verwijzingen wordt de index weggelaten en lineair geselecteerd.
Iedere uitvoerscene wordt opnieuw gevalideerd vóór de bestaande DynLex-ABI.
De pagina blijft begrensd tot 4096 px per zijde en 6 miljoen pixels.
Deze viewport-API is een proef: de UI toont voorlopig volledige paginabitmaps,
geen tegelcache of tiled compositor. Er is geen octree en geen verborgen
Hayro/Vello-rasterizer in het DynLex-pad.

De regressietest maakt een echte PDF met overlappende rode/blauwe/groene
rechthoeken. Op 25/50/100/125/200/400% is iedere volledige render bytegelijk
aan Hayro. Dezelfde pagina wordt op elke schaal in aangrenzende 53×37-tegels
verdeeld; iedere tegel is bytegelijk aan de overeenkomstige uitsnede van
Hayro's volledige pagina, inclusief lege tegels en overlappende laagvolgorde.
De bestaande asstreekfixture is op 100/200/300/400% bytegelijk. De bestaande
raster/hybride fixtures blijven op 100% bytegelijk en wijzen andere schalen af.
Ongeldige schalen, lege/buitenliggende/overlopende viewports en unsupported
PDF's worden veilig geweigerd. De tests laten dezelfde voorbereiding over
meerdere calls leven; zij bewijzen geen universele PDF- of tile-pariteit.

Reproduceren vanuit `rust-studio`:

```sh
cargo test -p opds-dynlex-backend --features kernel prepared::tests
cargo build --release -p opds-dynlex-backend --features kernel --bin prepared_probe
target/release/prepared_probe crates/dynlex-backend/tests/fixtures/simple-fill-stroke.pdf 2
```

De probe vergelijkt in één warm proces 30 afwisselende paren van telkens
opnieuw voorbereiden tegenover de behouden scene. Iedere RGBA-buffer wordt
buiten het gemeten interval volledig met Hayro vergeleken. Bestand-I/O,
processtart en Hayro-referentieopbouw zijn niet in de gemeten tijd opgenomen.
Dit is een kleine gecontroleerde fixtureproef, geen algemene snelheidclaim.
DynLex blijft gebouwd met LLVM/C++ en libc; de cache/index/validatie is Rust.

De 30-paarproef staat in `dynlex-prepared-bench-2026-10-08.json`:

| Kleine fixture | Schaal | Opnieuw voorbereiden + render | Behouden scene render | Winstparen |
| --- | ---: | ---: | ---: | ---: |
| Vector fill/stroke | 100% | 0,0694 ms | 0,0321 ms | 30/30 |
| Vector fill/stroke | 200% | 0,1444 ms | 0,1067 ms | 28/30 |
| Raster full-page | 100% | 0,4457 ms | 0,2080 ms | 30/30 |
| Hybride twee beelden | 100% | 0,4834 ms | 0,2474 ms | 30/30 |

Dit zijn medianen van in-process calls, niet de eerder gemeten procestijden
van circa 7–10 ms. Het voordeel komt van vermeden voorbereiding/allocaties;
het rasteralgoritme zelf is hier niet vervangen. De gehele probe had per
fixture 6,6–8,8 MiB piek-RSS, inclusief beide methoden én de Hayro-referentie.
Die meting scheidt het geheugengebruik per methode niet; een afzonderlijke
geheugenwinst wordt niet geclaimd. Alle buffers in alle paren zijn bytegelijk.
