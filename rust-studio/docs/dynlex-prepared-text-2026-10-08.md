# Herbruikbare tekstscene en voorbereide fontcontouren

De zelfstandige DynLex-tekstproef heeft nu een `PreparedWindingPage`: een
geordende lijst gevalideerde OPNW-tegels op één vaste paginaresolutie. De
volledige ongewijzigde Albany-pagina 6 is op acht schalen bytegelijk aan de
onafhankelijke Hayro-referentie, inclusief alle 657 zichtbare glyphs en de
rechthoek. De desktopdispatcher is niet uitgebreid met de onderzoeksparser.

## Implementatie

`PreparedGlyph` bewaart de begrensde contouren van één ingebedde glyph.
Extractie uit het font gebeurt één keer per font/glyph-combinatie. Plaatsing en
quadratic-afvlakking blijven per schermtransformatie gebeuren; een bitmap van
een vorige zoom wordt niet gebruikt. De bestaande `stage` en `stage_line_only`
routes gebruiken dezelfde voorbereiding en behouden hun eigen acceptatiegrens.
Op Albany worden 79 unieke contouren gebruikt voor 657 zichtbare letters.
De glyphcache bewaart 112.480 bytes contourdata, exclusief de BTreeMap/keys en
fontbestanden. De cachegrenzen zijn 1024 entries en 16 MiB contourdata.

De voorbereide pagina valideert ieder OPNW-record voordat het in de private
geometrieopslag komt. Mislukte toevoegingen wijzigen de scene niet. Origin is
4-pixel-uitgelijnd, zodat de globale 4×4-stripfase behouden blijft. Paintvolgorde
verandert niet: tekst en rechthoektegels worden in contentvolgorde samengesteld.

Replay maakt één verse transparante paginabuffer, één dekkingsbuffer en één
OPGC-commandbuffer. De bestaande zelfstandige DynLex strip- en compositiekernels
berekenen de pixels. De eerdere volledige premultiplied-buffercontrole per
letter is verwijderd uit deze nieuwe route: de verse nulbuffer voldoet aan de
invariant en iedere gecontroleerde compositie behoudt die. De bestaande routes
voor aangeleverde RGBA-buffers blijven hun input volledig controleren.
Witte-papiercompositie gebeurt aan het einde in dezelfde buffer.

De scene bevat geen referenties naar tijdelijke PDF-, font- of outputbuffers.
Meerdere threads kunnen dezelfde voorbereide geometrie veilig hertekenen;
elke render heeft eigen uitvoer en werkbuffers. De scene is gekoppeld aan één
resolutie. Een volgende zoom moet opnieuw schermgeometrie voorbereiden met de
herbruikbare fontcontouren; er wordt geen bitmapcache als geometrie gepresenteerd.

## Grenzen

- Maximaal 4096 pixels per zijde en 6.000.000 uitvoerpixels: hoogstens 24 MB RGBA.
- Maximaal 8192 paints en 32 MiB behouden scenegeheugen, inclusief gereserveerde
  paintslots en capaciteit van de uitgelijnde contourbuffers.
- Per OPNW-tegel blijven de bestaande grenzen gelden: 256×192, 1–8 gesloten
  contouren, maximaal 256 punten totaal, eindige coördinaten en exacte framing.
- Glyphvoorbereiding accepteert maximaal 8 MB fontdata per aanroep; de bestaande
  grenzen op contouren, segmenten, afvlakking en tegelgrootte blijven actief.
- Dekking buiten de pagina levert een fout op; een gedeeltelijke uitvoer wordt
  niet teruggegeven. Er is geen stilzwijgende clipping/fallback toegevoegd.

Tijdens replay zijn er bovendien circa 96 KiB vaste werkbuffers en maximaal
8192 kleine geleende paintbeschrijvingen. Bron-PDF, fontdata, parserallocaties,
parallelle caller-owned renders en allocatoroverhead vallen niet onder de
32 MiB scenegrens. Dit is geen maximum-RSS-claim voor het hele proces.

De lopdf-onderzoeksfrontend heeft nog geen volledig afgedwongen object-,
recursie- en allocatiebudget. Daarom is `--prepared-winding` expliciet een
zelfstandige onderzoeksmodus; `PreparedPage`, OPGS/OPM2 en native dispatch zijn
niet verruimd. De audit controleert ook dat de native route pagina 6 nog
expliciet weigert. De volgende stap is een begrensde PDF/frontend-koppeling,
gevolgd door dezelfde pixeltests en een native UI-proef.

## Volledige pixelcontrole

| Geval | Resultaat |
| --- | --- |
| Originele Albany-pagina 6: 25/50/87/100/115/156,25/200/300% | 8/8 volledig bytegelijk |
| Gekleurde overlap, vier rotaties en twee CropBoxes | 8/8 volledig bytegelijk |
| Totaal vergeleken pagina-RGBA | 52.524.160 bytes, geen verschil |
| Bestaande windingaudit | 48 contourgroepen, 11 volledige pagina/zoomgevallen en 5 ongeldige CLI-invoeren blijven slagen |
| Kernel- en adapter-unit-tests | 107 + 34 geslaagd |

Nieuwe tests controleren paintvolgorde, gelijktijdige replay, hergebruik van
contouren over meerdere zooms, ongeldige/truncated invoer, stripuitlijning,
geometrie-/paintbudgetten en behoud van de scene na afwijzing.
Geen productiepad gebruikt een Hayro/Vello-rasteraanroep voor deze uitvoer;
de externe referentie wordt alleen door het auditscript gestart.

Ruwe resultaten: `dynlex-prepared-text-2026-10-08.json` en
`dynlex-prepared-text-legacy-regression-2026-10-08.json`.

## Afzonderlijke replaymeting

Na de correctness-tests is zonder parallelle builds/tests gemeten. Beide
routes krijgen dezelfde voorbereide geometrie en maken verse uitvoerbuffers.
Drie nieuwe processen per schaal, elk drie afwisselende sequential/batch-paren:
negen metingen per route en schaal. Medianen:

| Albany-pagina 6 | Oude compositie per paint | Nieuwe batch | Verhouding |
| --- | ---: | ---: | ---: |
| 100% | 427,44 ms | 47,86 ms | 8,93× |
| 200% | 1565,77 ms | 57,35 ms | 27,30× |

Dit meet uitsluitend replay van voorbereide geometrie: geen PDF-openen,
font-/glyphvoorbereiding, diagnostische bestandsuitvoer of bytevergelijking.
Het is geen algemene PDF-benchmark, koude-startmeting, GUI-latentie of
vergelijking met Hayro. De oude baseline kloont per paint de kleine geometrie
en controleert telkens de hele paginabuffer, zoals de eerdere proefroute.
`prepare_ms` in de correctness-audit bevat diagnostische writes en is geen
zuivere fontvoorbereidingsbenchmark.

Ruwe meetwaarden en binary-/documenthashes:
`dynlex-prepared-text-bench-2026-10-08.json`.

## Reproduceren

```sh
cargo build --release -p opds-dynlex-backend --features kernel \
  --bin truetype_page_probe --bin winding_probe
python3 scripts/audit_prepared_text.py \
  --output docs/dynlex-prepared-text-2026-10-08.json
python3 scripts/audit_text_winding.py --require-exact \
  --output docs/dynlex-prepared-text-legacy-regression-2026-10-08.json
cargo test -p opds-dynlex-backend --features kernel --lib
cargo test -p opds-opdl-adapter --lib
```

Een afzonderlijke `truetype_page_probe INPUT PAGE SCALE OUTPUT --prepared-winding
DIRECTORY` schrijft de volledige pagina, diagnosegeometrie en `prepared.json`.
Die JSON bevat drie afwisselende replayparen en cachegeheugen. De CLI-front-end
blijft bestemd voor vertrouwde onderzoeksdocumenten. Bestaand ongecommit werk
is behouden; de native app en gebruikersvensters zijn niet vervangen.
