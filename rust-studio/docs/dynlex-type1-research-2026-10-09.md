# Type1-fontonderzoek voor een externe volledige tekstpagina

`Tekst.pdf` passeert sinds de begrensde indirecte `/Length`-preflight, maar
de strikte DynLex-route weigert alle vijf pagina's op 25/100/200% wegens
het ingebedde Type1/PFB-font `UniviaProRegular`. Dezelfde pagina's hebben
ook een ingebed WinAnsi TrueType-font. Er is nog **geen** Type1-acceptatie
of DynLex-pixelpariteit; er wordt geen Hayro-bitmap als fallback getoond.

Een aparte, uitsluitend onderzoeksgerichte Rust-binary
`crates/dynlex-backend/src/bin/type1_research.rs` valideert de PDF-bron
met de bestaande tekst-preflight, begrenst de FontFile-decode tot 8 MB en
leest daarna de ingebedde PFB met `skrifa::raw::ps::type1::Type1Font`.
Dit is een pure-Rust fontparser; de kandidaat roept geen
Hayro/Vello-renderer aan. De binary inspecteert alleen outlines en
wijzigt native dispatch of annotaties niet. Lopdf draait hierbij in een
vertrouwde onderzoeksaanroep; zet deze binary niet direct op
onbetrouwbare PDF's buiten de begrensde tekstworker.

Reproduceer vanuit `rust-studio`:

```bash
cargo run --release -p opds-dynlex-backend --features kernel --bin type1_research -- '../test pdf-bestanden/Originele bestanden/Tekst.pdf' 1
```

De bron-SHA256 is
`a97db0d26a89643a88976e4a68823d96b3b6f24b29e9dca3cc74f29c48c0fc43`.
Op alle vijf pagina's wordt dezelfde 14.894-byte PFB gevonden: 74 glyphs,
74 PDF-codes met een ingebouwde Type1-encoding, 74 gemapte GIDs,
110 contouren met 110 sluitingen, 515 lijnsegmenten en 756 kubische
segmenten in totaal.
Geen van de 74 outlines overschrijdt de voorlopig gebruikte grens van
acht contouren en 256 segmenten per glyph. Dat zegt niets over
pixelpariteit; de huidige DynLex-glyphvoorbereiding ondersteunt alleen
rechte en kwadratische TrueType-contouren. Een misvormde PFB, NaN-penpunt
en te veel contouren worden in de proef afgewezen.

Volgende stap is een begrensde Type1-kubiek→DynLex-stripproef met
fontmatrix, code→glyph, PDF-advance en niet-nul-winding. Vergelijk eerst
afzonderlijke echte glyphs op meerdere schalen met een geïsoleerde
Hayro-PDF, daarna de volledige oorspronkelijke `Tekst.pdf`-pagina met
gemengde fonts en paintvolgorde. Houd unsupported zolang ook maar één
pixel afwijkt of een font/resource onduidelijk is. De Type1-fontparser
alleen is geen volwaardige PDF-objectdekking of snelheidswinst.

## Eerste zelfstandige kubiekproef: nog niet exact

Dezelfde onderzoeksbinary kan nu uitsluitend een gecontroleerde
256×192-fixture met precies één Type1-tekencode op 80 pt naar een
`OPNW`-contourbuffer afvlakken. Een begrensde eigen recursieve
kubiekonderverdeling (maximaal twaalf niveaus en 256 punten totaal)
voedt de bestaande DynLex nonzero-stripkernel; de Rust-proef schrijft
een RGBA-resultaat. Zij controleert paginageometrie en de **exacte**
geïsoleerde contentstream vóór deze modus. Uitvoer is expliciet
`UNVERIFIED research` en wordt nooit door de normale native dispatcher
gekozen. Het echte PFB wordt op 1000 fontunits getekend; de fase na
PDF→schermtransformatie wordt naar f32 afgerond.

`scripts/audit_type1_cubic_research.py` kloont het originele embedded
font naar zeven kleine PDF's buiten Git, vraagt Hayro uitsluitend als
testoracle en vergelijkt elke RGBA-byte. Met 0,01 px
onderverdelingstolerantie verschillen respectievelijk **62, 37, 12, 12,
12, 13 en 12 pixels** voor fontcodes 1, 5, 27, 50, 57, 58 en 68.
De maximale kanaalafwijking is 70. Een tolerantieproef van 0,005–1 px
maakte code 1 nergens exact. Dit is een echte numerieke AA-/flattening-
blokkade, geen bewijs van Type1-renderdekking. De kandidaat blijft
geïsoleerd en de volledige `Tekst.pdf` blijft strikt unsupported.
Ruwe hashes/resultaten staan buiten Git in
`~/.cache/opds-benchmarks/internet-2026-10-08/dynlex-type1-cubic-research-2026-10-09.json`.

Reproduceer met een Python-runtime met `pypdf` vanuit
`rust-studio`:

```sh
python3 scripts/audit_type1_cubic_research.py \
  --source '../test pdf-bestanden/Originele bestanden/Tekst.pdf' \
  --hayro-probe target/release/backend_probe \
  --type1-probe target/release/type1_research \
  --output-dir ~/.cache/opds-benchmarks/internet-2026-10-08
```

De volgende proef moet de lokale f32-kubiekverdeling/stripdekking
op de afwijkende randpixels analyseren, zonder de testoracle als
runtime-renderer in te bouwen. Pas na volledige glyph- én
hele-pagina-bytepariteit kan deze route via de begrensde worker naar
de normale DynLex-dispatch.

## Begrensde scalar-kubiekproef: meerdere groottes, nog geen pagina

Een afzonderlijke `--scalar`-modus volgt de numerieke
kubiek→kwadratiek→lijnverdeling uit de lokaal aanwezige
`vello_common 0.0.8`-bron (`flatten_simd.rs`, MIT/Apache-2.0) met eigen
begrensde Rust-scalarcode in `bin/type1_research/type1_cubic_scalar.rs`. Dit is
**bron-geïnformeerde numeriek**, geen onafhankelijk afgeleide methode;
de kandidaat roept Vello/Hayro niet aan. Vooral de tweestaps f32-FMA
bij het kwadratische controlepunt en de vroege lijnbenadering blijken
voor AA-bytepariteit relevant. De recursieve onderzoeksmodus blijft
beschikbaar als vergelijking.

Een audit van **alle 74 ingebouwde Type1-codes** van het echte ingebedde
font, elk als geïsoleerde originele-font-PDF op 256×192, vergelijkt
iedere uitvoerbyte bij 40, 80 en 120 pt. De kandidaat is bij die
groottes respectievelijk **73/74, 67/74 en 66/74** volledig RGBA-byte-
gelijk aan Hayro: samen 206/222 fixtures en 40.501.248 exacte
RGBA-bytes. De overige 16 fixtures hebben in totaal 18 afwijkende
randpixels; elke afwijkende kleurcomponent is één 8-bit-stap. Code 6
heeft geen contour en wordt na succesvolle code→GID→outline-decode als
inkloze glyph behandeld; de geïsoleerde witte fixture is exact. De
oorspronkelijke zeven onderzochte inktcodes zijn op 80 pt alle exact.

De volledige `Tekst.pdf`-pagina, andere groottes, rotaties, CropBox,
meerdere glyphs, kleur en paintvolgorde zijn hiermee **niet** bewezen.
De native dispatcher blijft Type1 weigeren; de zeven één-byte-randen
worden niet met een tolerantie verborgen. De audit bewaart PDF-/binary-
en uitvoerhashes buiten Git in
`~/.cache/opds-benchmarks/internet-2026-10-08/dynlex-type1-cubic-scalar-all-2026-10-09.json`
en de gelijknamige `-size40`-/`-size120`-rapporten. Die bewaren ook
coördinaten en kanaalwaarden van de resterende afwijkingen. Een aparte
f64-schermpuntproef gaf op 80 pt dezelfde zeven één-byteverschillen;
alleen de transform-precisie verruimen lost het probleem dus niet op.

```sh
python3 scripts/audit_type1_cubic_research.py \
  --source '../test pdf-bestanden/Originele bestanden/Tekst.pdf' \
  --hayro-probe target/release/backend_probe \
  --type1-probe target/release/type1_research \
  --output-dir ~/.cache/opds-benchmarks/internet-2026-10-08 \
  --scalar --all-codes
# Herhaal met --font-size=40 en --font-size=120.
```

Volgende stap is de zestien resterende fixtures met één-byte-randen
tegen de bron-/schermtransformatie en stripafronding vergelijken, dan
rotaties/CropBox en uiteindelijk de volledige gemengde echte pagina
via de begrensde worker. Geen speciale codepuntbehandeling.

## SIMD-lane-precisie in de onderzoeksproef

Een nieuwe, nog steeds geïsoleerde scalarproef bewaart de f32-afronding van
vier SIMD-lanes bij kubieksampling en lijnuitvoer. Iedere lane begint met
zijn eigen `0..3 * stap` en wordt daarna per blok met `4 * stap` verhoogd;
iedere sample opnieuw uit zijn absolute index berekenen gaf andere
randdekking. Dit volgt de lokaal aanwezige `vello_common 0.0.8`-bron
(`flatten_simd.rs`, MIT/Apache-2.0) als numerieke referentie. Er is geen
Vello- of Hayro-runtime-aanroep in de kandidaat.

Dezelfde audit van alle 74 codes op 40, 80 en 120 pt is nu respectievelijk
**74/74, 72/74 en 73/74** volledig RGBA-bytegelijk: samen 219/222
fixtures en 43.057.152 exacte bytes. De drie resterende fixtures hebben
ieder precies één randpixel met een verschil van één 8-bit-stap: code 23
en 30 op 80 pt, code 28 op 120 pt. De eerdere versie had 206/222 exacte
fixtures. De ruwe rapporten, inclusief bron-, binary- en uitvoerhashes,
staan buiten Git onder
`~/.cache/opds-benchmarks/internet-2026-10-08/dynlex-type1-cubic-scalar-all{-size40,-size120}-2026-10-09.json`;
de eerdere rapporten zijn als `*-before-lane-2026-10-09.json` bewaard.

Dit is geen productieroute of bewijs voor volledige Type1-pagina's.
`Tekst.pdf` blijft in de native DynLex-dispatcher unsupported zonder
vervangende bitmap. De volgende stap is de drie overgebleven randpixels
algemeen verklaren en daarna rotatie, CropBox, kleur, gemengde tekst en
de volledige originele pagina via de begrensde worker toetsen.

## Alle geïsoleerde Type1-codes exact op drie groottes

Een afzonderlijke oracle voor afgevlakte krommen vergeleek de scalarpunten
van de drie resterende glyphfixtures met de lokale `vello_common 0.0.8`-
afvlakking, zonder die in DynLex aan te roepen. De puntenaantallen waren
gelijk, maar 2, 7 en 15 punten verschilden één of enkele f32-ULP's. De
oorzaak was algemeen: de bron berekent de integraalconstante als
`1.0_f32 - 0.67_f32`. De rechtstreeks afgeronde literal `0.33_f32` in de
kandidaat ligt één ULP hoger. De onderzoeksflattener behoudt nu die
f32-aftrekking. Alle 24 oorspronkelijke krommen uit deze drie glyphs
leveren daarna exact dezelfde lijnpunten als de afzonderlijke oracle.

De volledige pixelreplay van **74 codes × 40/80/120 pt** geeft nu
**222/222 exacte PDF-fixtures, 43.646.976 RGBA-bytes** tegen Hayro. De
vorige 219/222-meting blijft hierboven als diagnose bewaard. Nieuwe
PDF-/binary-/pixelhashes staan buiten Git in
`~/.cache/opds-benchmarks/type1-exact-2026-10-09/dynlex-type1-cubic-scalar-all{-size40,-size120}-2026-10-09.json`.
De aparte oraclebinary `type1_curve_oracle` accepteert maximaal 256
begrensde krommen en vergelijkt alleen lijnpunten; hij wordt nooit door
de native app of DynLex-kernel aangeroepen.
Bijvoorbeeld vanuit `rust-studio`, nadat de 80pt-audit de PDF-fixture
buiten Git heeft gemaakt:

```sh
target/release/type1_research \
  ~/.cache/opds-benchmarks/type1-exact-2026-10-09/type1-glyph-code23.pdf \
  1 --code=23 --tolerance=0.01 /tmp/type1-23.rgba \
  --scalar --font-size=80 --dump-curves=/tmp/type1-23.curves
target/release/type1_curve_oracle /tmp/type1-23.curves
```

Dit bewijst uitsluitend de geïsoleerde embedded Type1-fontcodes en deze
drie schermgroottes. Rotatie, CropBox, kleur, meerdere glyphs, gemengde
fonttypen en de hele oorspronkelijke `Tekst.pdf`-pagina blijven onbewezen.
De normale DynLex-dispatch blijft dus strikt afwijzen; de volgende stap is
die hele pagina via de begrensde worker voor te bereiden en pixelgewijs
te toetsen vóór activering.

## Paginarotatie, CropBox en kleur in de afzonderlijke proef

De onderzoeksbinary verwerkt voor dezelfde geïsoleerde embedded-fontglyph nu
ook de gevalideerde `PageSpace`-transformatie in plaats van uitsluitend de
onveranderde 256×192-oriëntatie. Haar uitvoer blijft begrensd tot 256×192.
De onderzoekspagina moet een exacte, verwachte één-glyph-contentstream
hebben; de pagina- en bronlimieten zijn ongewijzigd. Een optionele RGB-vulling
loopt door de bestaande DynLex-premultiplied compositor en wordt daarna op
wit papier geplaatst. Een eerste handmatige vermenigvuldiging van reeds
afgeronde kleur en dekking gaf op de gekleurde proef één-byte-randverschillen;
die kandidaat is vervangen door de bestaande gevalideerde compositor.

`scripts/audit_type1_geometry_research.py` gebruikt vijf echte glyphcodes
(1, 23, 30, 50, 68) uit de oorspronkelijke `UniviaProRegular` en genereert
vier kwartslagen × twee CropBox-posities × zwart of DeviceRGB
`0.2 0.4 0.6`: **80/80 volledige 256×192-fixtures en 15.728.640 RGBA-bytes
exact** tegen Hayro. De verschoven CropBox heeft dezelfde zichtbare omvang,
maar een andere paginacoördinatenoorsprong. Bron-/binary-/PDF-/outputhashes en
per-fixture-afwijkingen staan buiten Git in
`~/.cache/opds-benchmarks/type1-geometry-2026-10-09/dynlex-type1-geometry-2026-10-09.json`.
De proef gebruikt het echte embedded font en de zelfstandige DynLex-strip- en
compositiekernels; Hayro is uitsluitend testoracle.
Na deze compositorwijziging zijn ook alle 74 oorspronkelijke codes op elk van
40/80/120 pt herhaald: opnieuw **222/222 exact**, zonder afwijzing. De nieuwe
regressiehashes staan in `~/.cache/opds-benchmarks/type1-geometry-regression-2026-10-09/`.

Dit bewijst nog geen volledige `Tekst.pdf`-pagina, gemengde fonts, paintvolgorde,
clipping of veilige Type1-acceptatie in de native app. De onderzoeksexecutable
gebruikt nog lopdf op vertrouwde invoer; hij mag niet als onbegrensde
productiefrontend worden aangesloten. Volgende stap blijft Type1-resource-,
encoding- en outlinevoorbereiding binnen de bestaande resource-begrensde
tekstworker, gevolgd door volledige pagina- en foutafwijzingstests.

## Eerste Type1-resourcevoorbereiding in de begrensde tekstworker

De Linux tekstfrontend herkent nu een **ingebedde** Type1/PFB met de ingebouwde
encoding, `/FontFile`, directe `/Widths` en zonder expliciete `/FontMatrix`. Zij
decodeert de fontstream onder de bestaande 8-MB-grens en valideert code→GID,
fontomvang en gehele, begrensde PDF-breedtes. Een `/Encoding`-override,
iedere expliciete FontMatrix, ontbrekende PFB en fractionele breedte blijven
afgewezen. Dit draait in de bestaande OS-resource-begrensde worker wanneer
de native dispatcher het vraagt; de losse `--stage-only`-diagnose gebruikt
dezelfde code uitsluitend op vertrouwde fixtures.

Een geïsoleerde vijf-glyphtekst met het **originele** `UniviaProRegular`
leverde dezelfde vijf GIDs en teksttransformaties als de afzonderlijke
Hayro-interpreteroracle; de grootste matrixdelta was alleen f64-rekenruis.
Drie gemuteerde resourcegevallen werden vóór de staging afgewezen. De
strikte native route was in dit eerste stadium ook bij het geldige geval
**bewust unsupported zonder bitmap of fallback**: Type1-kubiekoutlines waren
nog niet in de worker-scenerasterisatie aangesloten. Dit voorkwam dat alleen
correcte posities
voor volledige PDF-pixelondersteuning worden aangezien.

Reproduceerbaar via `scripts/audit_type1_worker_stage.py`; de vier PDF-fixtures,
bron-/binaryhashes, oracletrace en JSON staan buiten Git in
`~/.cache/opds-benchmarks/type1-worker-stage-2026-10-09/`. De toen volgende
inhoudelijke stap was de eerder pixelbewezen kubiekstaging op deze gevalideerde
fontrecords aansluiten, vervolgens meerdere glyphs, gemengde fonttypen en
de **hele oorspronkelijke** `Tekst.pdf`-pagina inclusief haar overige paden
en graphicsstate vergelijken. Er is nog geen nieuwe complete pagina buiten
Albany ondersteund.

## Begrensde native Type1-kubiekstaging voor de bewezen subset

De hierboven beschreven expliciete renderafwijzing is nu vervangen door een
begrensde `PreparedType1Glyph` **binnen dezelfde Linux tekstworker**. De worker
leest een gevalideerde ingebedde PFB, tekent de ruwe outline op 1000 fontunits
met pure-Rust skrifa, bewaart hoogstens 1.024 unieke glyphs en 16 MiB
outlinegeometrie, en past de reeds onderzochte f32-scalar-kubiekverdeling toe
na de schermtransformatie. Elke glyph heeft maximaal acht gesloten contouren,
256 oorspronkelijke segmenten en 256 afgevlakte punten in een 256×192-tegel.
De ouder accepteert uitsluitend de bestaande gevalideerde PWPG/OPNW-scene;
geen fontbytes of pointer worden via IPC naar de renderer doorgegeven. Het
Type1-pad gebruikt de bestaande nonzero DynLex-strip- en compositiekernels.
De numerieke afvlakking blijft expliciet geïnformeerd door `vello_common`
0.0.8 (MIT/Apache-2.0); Vello/Hayro wordt niet tijdens productie aangeroepen.

De **native strikte route** is bytegelijk aan Hayro op 222/222 geïsoleerde
glyph-PDF's (74 oorspronkelijke fontcodes op 40/80/120 pt; 43.646.976
RGBA-bytes) en 80/80 rotatie/CropBox/kleurgevallen (15.728.640 bytes).
Een vijf-glyphfixture met dezelfde originele PFB levert de vijf juiste
GIDs/posities en alle 196.608 RGBA-bytes exact. Een lege maar geldig gemapte
glyph verplaatst de tekstcursor zonder inkt te tekenen. Een
encoding-override, fractionele `/Widths` en ontbrekende PFB geven alle een
strikte afwijzing zonder bitmap of fallback. De PDF's, binaire hashes en
per-pixelresultaten staan buiten Git onder
`~/.cache/opds-benchmarks/type1-native-all-{40,80,120}-2026-10-09/`,
`~/.cache/opds-benchmarks/type1-native-geometry-2026-10-09/` en
`~/.cache/opds-benchmarks/type1-worker-stage-2026-10-09/`.
Reproduceer via `scripts/audit_type1_cubic_research.py --native-probe
target/release/backend_probe --scalar --all-codes --require-exact`,
`scripts/audit_type1_geometry_research.py --native-probe
target/release/backend_probe` en `scripts/audit_type1_worker_stage.py`.

Een aanvullende **gemengde** volledige fixturepagina met de originele
UniviaProRegular-PFB en een embedded WinAnsi TrueType-font tekent twee
geordende, gekleurde en overlappende tekstruns. Vier kwartslagen, twee
CropBoxes en 25/100/200% zijn 24/24 native strikte pagina's en 82.944.000
RGBA-bytes exact tegen Hayro. De reproduceerbare generator en pixelcontrole
staan in `scripts/audit_type1_mixed_native.py`; PDF's en ruwe JSON staan buiten
Git onder `~/.cache/opds-benchmarks/type1-mixed-native-2026-10-09/`.

Alle vijf **oorspronkelijke** `Tekst.pdf`-pagina's blijven bij 25/100/200%
zonder bitmap geweigerd: 0/15 ondersteund, 15 unsupported, nul procesfouten
in de nieuwe volledige-documentaudit buiten Git onder
`~/.cache/opds-benchmarks/type1-native-geometry-2026-10-09/`.
De eerste pagina heeft een tweede symbolische TrueType-resource; ook CTM,
clips, lijnstrokes en een `Do`-object vereisen nog een geordende hybride scene.
Haar paginageometrie is fractioneel en moet in die combinatie worden getoetst.
Deze fixtures bewijzen dus **geen oorspronkelijke volledige
pagina buiten Albany**, geen algemene Type1-ondersteuning en geen
snelheidsvoordeel.
De vorige Albany-audit blijft 135/147 exact, 12 unsupported en nul foutieve
bitmaps; zestien bestaande native tekstgevallen, workspace-tests en de oude
v2/v4-verifier blijven exact. De volgende stap is een gemengde
Type1/TrueType-tekstpagina en daarna de echte volledige pagina met
gevalideerde graphicsstate, paden en resources. Onbewezen inhoud blijft
strikt unsupported.
