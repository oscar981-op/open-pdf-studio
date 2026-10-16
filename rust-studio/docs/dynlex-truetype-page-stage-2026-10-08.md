# Volledige externe tekstpagina: mapping en plaatsing, nog geen renderpariteit

Vervolg: de aparte [OPNW-windingproef](dynlex-text-winding-2026-10-08.md)
rendert nu de hele pagina op drie schalen exact. De hieronder beschreven
standaardproef en native route behouden hun strikte afwijzing.

De zelfstandige `truetype_page_probe` verwerkt de **ongewijzigde pagina 6** van
het openbare Albany-contract. Dit is een aparte read-only onderzoeksbinary;
geen nieuwe route in `PreparedPage` of de native app. De app blijft deze pagina
bij selectie van DynLex expliciet afwijzen, zonder Hayro-fallback.

## Resultaat

De pagina bevat 268 contentoperators, 798 tekencodes, twee ingebedde WinAnsi
TrueType-fonts (Aptos/Aptos-Bold), 657 zichtbare glyphs en één gevulde rechthoek.
Alle operators worden verwerkt of afgewezen; er worden geen onbekende paints
stilzwijgend overgeslagen. Spaties verplaatsen de tekstpositie zonder inkt.

Een afzonderlijke **testoracle** `text_position_oracle` gebruikt de Hayro-
interpreter om glyph-ID's, transformaties en paintvolgorde te registreren. Die
binary wordt nooit door de kandidaat of native DynLex-route aangeroepen.

| Schaal | Pagina | GID-verschillen | Grootste x/y-positieverschil | Grootste verschil lineaire matrixcomponent |
|---|---|---:|---:|---:|
| 25% | 153×198 | 0 / 657 | 0 | 2,80e-14 |
| 100% | 612×792 | 0 / 657 | 0 | 1,12e-13 |
| 200% | 1224×1584 | 0 / 657 | 0 | 2,24e-13 |

De volgorde van alle 658 paints is op alle schalen gelijk. De kleine lineaire
verschillen komen van het bepalen van schermbasisvectoren door twee f64-
punten van elkaar af te trekken. Dit is **geen bewijs van pixelpariteit**:
rectanglevorm/kleur en glyph-AA moeten nog als volledige pagina worden getest.
Het rapport bewaart per glyph de transformatiedelta, bron-/binaryhashes en
uitvoerdiagnose: `dynlex-truetype-text-stage-2026-10-08.json`.

De volledige renderpoging wordt op alle drie schalen bij paint 8 afgewezen:
TT0, GID 992, een typografische apostrof. De bestaande begrensde OPM2-validator
vindt een **zelfkruising in de afgevlakte contour**. Dat betekent niet dat het
bronfont ongeldig is: zowel de afvlakking als de beperking van onze eenvoudige
contourvalidator moeten worden onderzocht. Geen glyph wordt weggelaten, de
validator is niet versoepeld en er wordt geen gedeeltelijke RGBA-pagina
weggeschreven. De huidige standalone renderpoging is dus nog niet bruikbaar
als volledige tekstkernel. Ook geen nieuwe snelheidsmeting of native dekking.

## Kandidaat en grenzen

De kandidaat gebruikt lopdf voor PDF-objecten, `hayro-syntax` alleen als
f64-contentparser, ttf-parser voor Windows Unicode BMP cmap/units-per-em en de
bestaande pure-Rust skrifa-outline-extractie op 1000 units. De bestaande eigen
begrensde flattener en DynLex glyph-/compositiekernel verwerken de contouren.
Er is geen Hayro/Vello-rasteraanroep in deze route. De testoracle staat in de
engine-probe-crate; de kandidaat importeert die binary niet.

Ondersteund in de proef: expliciete ingebedde WinAnsi TrueType-fonts,
ASCII 32–126 en expliciete WinAnsi-punctuatie 145–151, CID-loze code→Unicode→GID,
PDF-breedtes, `Tf/Tm/Tj/TJ/Td/TD/T*`, teken-/woordspatiëring, horizontale schaal,
leading/rise, `Tr 0`, begrensde q/Q, eenvoudige kleuren en rechthoekfills.
Marked-contentmetadata mag geen optional-contentgroep inschakelen. Overige
resourceklassen, CTM, clipping, annotaties, transparantie, CMaps, fontfallback,
Type1/Type3 en algemene paden worden niet geclaimd.

De eerste f64-spacingkandidaat week maximaal circa 1,23e-5 px af. Inspectie van
lokale `hayro-interpret-0.7.0/src/interpret/state.rs` en de tekstoperators toont
dat Hayro tekststaat/advances in f32 berekent, maar tekstmatrices/Td/Tm in f64
bewaart. De kandidaat volgt die numerieke grens. Het is een door de oraclebron
geïnformeerde compatibiliteitskeuze, geen claim van onafhankelijk bedachte
numerieke conventies. De reeds bestaande Vello-geïnformeerde flattener behoudt
zijn bron-/licentievermelding. DynLex zelf gebruikt LLVM/C++ en libc.

Harde proefgrenzen: 16.000.000 PDF-bronbytes, maximaal 8 fonts, 8 MB per
fontstream/16 MB samen, 16 contentstreams/1 MB totaal, 8192 tekencodes,
16384 operators en TJ-arrayelementen, q/Q-diepte 16, marked-contentdiepte 32.
PageSpace valideert schaal/rotatie/boxen/pixelgrens. Glyphs gebruiken begrensde
256×192-tiles; de bestaande topologie-/buffercontrole blijft actief.
Flate-decode krijgt vooraf een uitvoerlimiet; DecodeParms wordt afgewezen.
Beide contentparsers moeten een toegevoegd operandvrij eindmerk bereiken;
afgebroken strings/arrays en losse slotoperanden worden niet als geldig prefix
geaccepteerd. Het tekstbestand `TPR1` is alleen een diagnostische trace, geen
nieuw geaccepteerd display-list-ABI.

**Vertrouwde onderzoeksbestanden:** lopdf-objectparsing heeft hier nog geen
volledig afgedwongen object-/recursie-/allocatiebudget. Ondanks bovengrenzen is
deze proef geen algemene veilige parser voor onbetrouwbare PDF's en mag hij
niet rechtstreeks de native dispatcher in. De volgende integratiestap moet
resource-/statevalidatie en totale allocatiegrenzen expliciet aantonen.

## Reproductie

```sh
cd rust-studio
cargo build --release -p opds-dynlex-backend --features kernel --bin truetype_page_probe \
  -p opds-engine-probe --bin text_position_oracle
python3 scripts/audit_truetype_text_stage.py \
  --candidate target/release/truetype_page_probe \
  --oracle target/release/text_position_oracle \
  --manifest docs/public-pdf-corpus-2026-10-08.json \
  --output docs/dynlex-truetype-text-stage-2026-10-08.json
```

De audit vereist identieke glyph-ID's, paintvolgorde en transformaties binnen
1e-9, én controleert dat een afgewezen render geen output achterlaat. Wanneer
de render later slaagt, stopt de audit bewust: voeg dan een volledige
Hayro-RGBA-vergelijking toe voordat de nieuwe uitvoer wordt geaccepteerd.
Diagnostische OPM2-bestanden voor het blokkadeonderzoek zijn reproduceerbaar:

```sh
target/release/truetype_page_probe \
  "$HOME/.cache/opds-benchmarks/internet-2026-10-08/albany-contract.pdf" \
  6 1 /tmp/albany-text-candidate.rgba --diagnostics /tmp/albany-text-diagnostics
```

Drie nieuwe gerichte tests controleren TJ/spacing/TD/T*/q/Q-herstel,
WinAnsi-punctuatie, foutieve fontencoding, begrensde Flate-decode en veertien
ongeldige content-/statevarianten. Werkmap bevat eerder werk; deze proef is
niet samen met die wijzigingen gecommit. Native appbinary en gebruikersvensters
zijn deze run niet vervangen.

## Volgende inhoudelijke stap

Vergelijk de originele quadratic-outline van GID 992 met de afgevlakte
kruising, op dezelfde schalen. Bouw algemene begrensde nonzero-windingdekking
voor zulke glyphs of herstel aantoonbaar foutieve flattening; maak geen
uitzondering voor deze apostrof en sla hem niet over. Test daarna de **hele
originele pagina 6**, alle pixels en paintvolgorde, inclusief de rechthoek.
Laat de normale DynLex-route afwijzen totdat volledige pariteit en veilige
integratie bewezen zijn. De openbare native audit blijft voorlopig 3/30
ondersteund (dezelfde scanpagina), 27 unsupported.

Validatie van deze stap: 257 workspace-tests met alle features geslaagd,
standaard workspace-check, release-probes, formattering, Python-compile en
diffcheck. Na de laatste space-glyph-afwijzingsguard zijn de drie gerichte
tests en de complete tekststage-audit opnieuw geslaagd. De oude volledige
verifier blijft exact: 47 PDF-pagina’s / 4.964.352 pixels plus drie v4-pagina’s /
147.456 pixels, inclusief ongeldige scenes en unsupported PDF-states.
