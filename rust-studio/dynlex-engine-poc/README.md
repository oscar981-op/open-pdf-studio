# DynLex PDF-engine POC: drie renderagents

De [inventarisatie van echte PDF-tekeningen](real-pdf-inventory-2026-10-03.md)
laat zien welke schaal- en padfuncties de huidige testsubset nog mist.

Dit is de eerste **kernelproef**, geïsoleerd van de werkende Rust-app. De agenten
zijn uitvoerstrategieën, geen aparte processen of AI-agenten:

| Agent | Eerste kernelwerk |
| --- | --- |
| Raster | bemonstert een gesimuleerde gedecodeerde beeldlaag |
| Vector | evalueert herhaalde verticale, horizontale en diagonale lijnen |
| Hybride | componeert dezelfde vectorlaag over de rasterlaag |

Alle drie lopen viermaal over 1024×768 pixels. Een Python-referentie berekent de
verwachte checksums onafhankelijk; de runner verwerpt afwijkende of instabiele
uitvoer. Dit meet alleen CPU-kernwerk plus processtart. Er worden nog **geen echte
PDF's geparseerd, pixels opgeslagen, tegels gerenderd of afbeeldingen visueel
vergeleken**. De drie getallen zijn daarom geen onderlinge kwaliteits- of
eind-tot-eind-snelheidsranglijst.

## Reproduceren

Gebruik de Linux-release 0.0.1 van
[OpenAEC DynLex](https://github.com/OpenAEC-Foundation/dynlex/releases/tag/0.0.1)
(SHA-256 van `dynlex.tar.gz`:
`922d6b0ba714a8d152a7ee30f22f4edc92d176cb3f3d1aeb00b3d6e2efe5a5f8`).
De tarball is op de werkmachine buiten de repository uitgepakt en is niet
meegenomen in deze POC.

```sh
python3 bench.py --compiler /pad/naar/dynlex/bin/dynlex --runs 15
```

De runner gebruikt de bij die compiler horende `share/dynlex/lib` en maakt
een tijdelijke bouwmap. `--stdlib` kan een afwijkende installatie aanwijzen.
De uitvoer bevat versie, checksums, compileertijd en mediane/minimale/maximale
proceslooptijd. Andere DynLex-versies moeten opnieuw gecompileerd en gemeten
worden; de huidige bron op `master` heeft al andere taal-/stdlib-patronen.

## Eerste meting (2 oktober 2026)

Release 0.0.1, `-O3`, 15 runs, inclusief processtart maar exclusief compileertijd:

| Agent | Checksum | Mediaan | Bereik |
| --- | ---: | ---: | ---: |
| Raster | 401080320 | 4,295 ms | 2,785–5,688 ms |
| Vector | 45907200 | 13,025 ms | 10,332–20,476 ms |
| Hybride | 422315840 | 14,502 ms | 10,688–20,997 ms |

Deze run is een basislijn voor de DynLex-kern, geen vergelijking met Hayro,
PDFium of de bestaande app: die verwerken echte PDF-objecten, fonts en beelden.
De bestaande Rust/Poppler-corpusmeting staat in
[`../docs/engine-probe-results.json`](../docs/engine-probe-results.json).

## RGBA-proef met OPDL v1 (2 oktober 2026)

[`rgba/verify.py`](rgba/verify.py) maakt een vaste 256×192-display-list. Het
`OPDL`-formaat heeft een magic, versie 1, paginabreedte/-hoogte, een clipvak en
de lengte van een RGB-beeldlaag in een 36-byte little-endian header. Daarna
volgen de beeldbytes, een opdrachtenteller en 24 bytes per tekenopdracht.
Opdracht 1 is een gevuld rechthoekpad; opdracht 2 is voorlopig een 45°-lijn
van drie pixels breed. Elke opdracht heeft een RGBA-kleur. De hybride agent
tekent ze in bestandsvolgorde over het beeld, met integer-alpha-compositing;
de andere agenten tekenen alleen hun eigen laag. Buiten het clipvak is de
pagina wit. Dit is een expliciet **tussenformaat**, nog geen PDF-interpreter.

```sh
python3 rgba/verify.py --compiler /pad/naar/dynlex/bin/dynlex \
  --runs 5 --artifacts /tmp/opds-dynlex-rgba-qa
```

De drie DynLex-programma's schrijven een rauwe RGBA-bitmap. De Python-runner
berekent alle 49.152 pixels per agent opnieuw en vergelijkt **iedere byte**.
Hij weigert ook een verkeerde formaatversie, een afgekapt bestand en een
onbekende opdracht. In de eerste vijf-runsproef waren de mediane
proceslooptijden inclusief lezen en schrijven 3,1 ms (raster), 7,5 ms
(vector) en 7,6 ms (hybride); de compileertijd was circa 7,7 seconden
**per agent**. Deze getallen zijn uitsluitend voor deze kleine synthetische
fixture en kunnen niet tegen de oudere checksumproef of een echte PDF-render
worden afgezet. De optionele PNG's zijn voor visuele controle; de bytevergelijking
is de exacte acceptatietest. De v1-lezer accepteert alleen vertrouwde
testfixtures en heeft nog geen productierijpe beveiliging voor willekeurige PDF-
of display-list-input.

## Eerste route vanaf echte PDF's en OPDL v2

De tijdelijke pure-Rust CLI `opds-opdl-adapter` leest via `lopdf` een **strikt
beperkte** PDF-subset: één pagina van maximaal 1024×1024 pt met nuloorsprong,
een optionele ongecomprimeerde volledige RGB8-afbeelding, gevulde,
pixeluitgelijnde rechthoeken en horizontale/verticale, precies één pixel brede
`m`/`l`/`S`-lijnen op halve-pixelcoördinaten. Vul- en lijnkleur zijn afzonderlijk.
De adapter verwerpt diagonale, bredere of niet-uitgelijnde lijnen en onbekende PDF-operators,
rotatie, CropBox, annotaties, filters, niet-volledige afbeeldingsmatrices en
onbekende XObjects. Dit is bewust een testadapter; de DynLex-kern blijft de
pixelrenderer en de bestaande Rust/Hayro-engine blijft de app-renderer.

[`pdf_fixture.py`](pdf_fixture.py) maakt drie **echte PDF's** met image-XObject,
`re`/`f`-rechthoeken en twee gekleurde `m`/`l`/`S`-lijnen. OPDL v2 bewaart
een lijn als opdrachtsoort 3 met zijn exacte pixelstrook; v1 verwerpt die soort.
[`pdf_verify.py`](pdf_verify.py) zet elke PDF om
naar OPDL, rendert met de passende DynLex-agent en vergelijkt alle 49.152
RGBA-pixels op 256×192 en 196.608 op 512×384 met zowel een onafhankelijke
sceneberekening als Hayro. Alle drie kwamen op beide groottes exact overeen.
Fixtures met een onbekende padoperator, diagonale lijn of brede lijn worden afgewezen. De oudere
synthetische OPDL-v1-fixture blijft eveneens bytegelijk renderen.

OPDL **v2** laat de RGB-laag weg als zij ontbreekt. De vector-PDF met twee lijnen
is nu 590–596 bytes en de overeenkomstige display-list **160 bytes**.
De DynLex-kern initialiseert dan wit en tekent de vectoropdrachten rechtstreeks
over hun begrensde rechthoek, in laagvolgorde. Er worden geen opdrachten meer
voor elke paginapixel gedecodeerd. Raster en hybride behouden hun echte RGB-laag.

```sh
cd rust-studio
cargo build --release -p opds-opdl-adapter -p opds-engine-probe --locked
python3 dynlex-engine-poc/pdf_verify.py \
  --compiler /pad/naar/dynlex/bin/dynlex \
  --adapter target/release/opds-opdl-adapter \
  --hayro target/release/opds-engine-probe --runs 15
```

De Hayro-probe kan voor deze controle `.rgba` direct schrijven, zodat beide
routes dezelfde ruwe uitvoer produceren. Lokale 15-runsmeting inclusief
processtart, PDF-lezen en RGBA-schrijven (ms, medianen):

| Grootte | PDF | Adapter + DynLex | Hayro PDF→RGBA | Pixelverschil |
| --- | --- | ---: | ---: | ---: |
| 256×192 | Raster | 6,8 | 5,5 | 0 |
| 256×192 | Vector | 5,2 | 6,0 | 0 |
| 256×192 | Hybride | 6,0 | 7,4 | 0 |
| 512×384 | Raster | 12,7 | 12,6 | 0 |
| 512×384 | Vector | 6,3 | 8,0 | 0 |
| 512×384 | Hybride | 11,2 | 13,0 | 0 |

De tweeprocessenroute, kleine fixtures, warme OS-cache en meetruis beperken
deze cijfers. In deze run zijn vector en hybride op 512×384 sneller dan Hayro,
terwijl raster ongeveer gelijk uitkomt; eerdere runs wisselden van rangorde.
Dit bewijst geen algemeen snelheidsvoordeel. De CLI-route bestaat uit twee processen en schrijft een tussenbestand,
terwijl Hayro één proces gebruikt. DynLex-compilatie kostte 6,9–7,0 seconden
per agent en is niet in de looptijd inbegrepen. De runner registreert ook
piek-RSS per proces: op 512×384 respectievelijk circa 4,4 MiB adapter, 2,7 MiB
DynLex en 8,0 MiB Hayro voor hybride. Die afzonderlijke waarden mogen niet
als één geïntegreerde app-geheugenmeting worden vergeleken. Er zijn nog geen
grote tekeningen, cachingstrategieën of brede PDF-functies gemeten.

Een aparte **dichtere 1024×768-fixture** gebruikt 48 gekleurde gevulde
rechthoeken en twee lijnen in de vector- en hybride pagina. `pdf_verify.py`
kan deze met `--sizes 1024x768 --dense --runs 15` reproduceren. Alle
786.432 pixels per agent zijn opnieuw exact gelijk aan Hayro en de
onafhankelijke scenereferentie. Warme-cache medianen inclusief processtart,
PDF-lezen en RGBA-schrijven:

| PDF | Adapter | DynLex | Adapter + DynLex | Hayro | OPDL |
| --- | ---: | ---: | ---: | ---: | ---: |
| Raster | 17,2 ms | 16,7 ms | 32,6 ms | 31,6 ms | 2,36 MB |
| Vector | 2,5 ms | 14,8 ms | 18,9 ms | 17,2 ms | 1.240 B |
| Hybride | 18,9 ms | 20,8 ms | 37,4 ms | 31,6 ms | 2,36 MB |

De afzonderlijke medianen tellen niet exact op tot de gemeten pijplijnmediaan.
Op deze fixture is de vectorrenderer de grootste stap van het vectorpad;
voor raster en hybride kost ook de volledige RGB-laag inlezen en kopiëren veel.
De tweeprocessenroute is hier op alle drie de pagina's niet sneller dan Hayro.
Dit blijft een kleine, kunstmatige PDF-subset zonder tekst, complexe paden,
transparantie of echte technische tekeningen. De volgende snelheidsstap moet
gericht de volledige beeldkopie en rendererloops profileren voordat verdere
optimalisatieclaims zinvol zijn.

De eerste gerichte rendererwijziging schrijft volledig dekkende vectoropdrachten
rechtstreeks naar RGBA; alleen gedeeltelijk transparante opdrachten lezen de
achtergrond en rekenen drie mengkanalen uit. De OPDL-v1-fixture inclusief
alpha-laag en alle drie de dichte PDF-fixtures bleven pixelgelijk. Een
afwisselende A/B-proef van de oude en nieuwe `-O3`-binaries, elk 30 gemeten
warme procesruns op dezelfde OPDL-bestanden, gaf voor alleen de DynLex-renderer
op 1024×768: vector 15,7→14,6 ms en hybride 18,2→16,8 ms. De volledige
PDF→RGBA-route varieert meer: in een afzonderlijke 15-run proef waren de
medianen na de wijziging raster/vector/hybride 35,1/17,3/32,5 ms tegenover
Hayro 29,5/17,7/32,9 ms. De directe rendererwinst is dus bescheiden en
proces-, I/O- en cachekosten blijven belangrijk; geen claim dat de hele engine
nu sneller is.

De A/B-proef is nu reproduceerbaar met
[`rgba/ab_bench.py`](rgba/ab_bench.py). Die compileert twee Git-versies van
de renderer met dezelfde DynLex-compiler, geeft beide identieke OPDL-invoer,
controleert de volledige RGBA-uitvoer tegen de onafhankelijke referentie en
wisselt de uitvoervolgorde per run af:

```sh
python3 dynlex-engine-poc/rgba/ab_bench.py \
  --compiler /pad/naar/dynlex/bin/dynlex \
  --adapter target/release/opds-opdl-adapter \
  --baseline-ref '70722565^' --candidate-ref 70722565 \
  --size 1024x768 --dense --runs 30
```

Een herhaling gaf als warme rendererprocesmedianen raster 17,4→17,5 ms,
vector 16,2→14,9 ms en hybride 18,1→16,5 ms. De runner meldt de eerste
procesrun afzonderlijk, maar wist de OS-bestandscache niet: dit is **geen**
echte cold-cache-meting. Compiler- en adaptertijd tellen niet mee in deze
directe renderervergelijking.

Een volgende proef vervangt uitsluitend het vullen van een volledig witte
achtergrond door `memset`; de RGB-beeldkopie bleef in het bestaande pad. De
eerste poging om ook die kopie in een lineaire pixel-loop onder te brengen gaf
in de dichte A/B-proef geen consistente verbetering en is teruggedraaid.
Met `rgba/ab_bench.py --baseline-ref e1c37358 --size 1024x768 --dense
--runs 30` waren de warme procesmedianen voor wit-vullen raster 17,2→16,6,
vector 14,2→12,9 en hybride 17,4→18,3 ms. Alle RGBA-bytes, inclusief de
oudere v1-fixture met clipping en alpha, bleven gelijk. Vooral de vectorwinst
is zichtbaar; hybride heeft in deze run een regressie. De eerste procesrun
varieerde sterk en is geen cold-cache-meting. Een volgende optimalisatie moet
de hybride regressie onderzoeken en de volledige PDF-route opnieuw meten.

De regressieproef liet zien dat dezelfde witte-vulvertakking in de hybride
binary niet helpt. Daarom geldt `memset` nu alleen voor de vectoragent; raster
en hybride gebruiken hun eerdere beeldinitialisatie. Een afwisselende A/B-run
tegen `727abdb8` op de dichte fixture gaf hybride 17,4→16,4 ms, maar vector
12,1→13,7 ms. Tegen de versie vóór witte-vuloptimalisatie (`e1c37358`)
bleef vector 14,9→13,0 ms sneller en hybride 17,9→17,6 ms ongeveer gelijk;
raster varieerde van 16,4→17,2 ms. Alle bitmaps waren bytegelijk, en de
OPDL-v1-fixture met alpha en clipping slaagde opnieuw. De waarden zijn
procesmedianen met warme cache; het kleine verschil voor hybride/raster kan
meetruis zijn. De winst voor de volledige PDF-route blijft onbewezen.

Voor de **volledige route** wisselt `pdf_verify.py` de uitvoervolgorde van
adapter+DynLex en Hayro nu per ronde af. Het JSON-rapport bewaart beide
reeksen, het aantal rondes waarin DynLex sneller was en de mediaan van de
gepaarde tijdsverschillen. Dit verkleint systematische volgordebias maar is
nog steeds een warme proces-/bestands-cacheproef, zonder app-integratie. Een
15-runsmeting op de huidige renderer gaf:

| Fixture | Raster | Vector | Hybride |
| --- | ---: | ---: | ---: |
| 256×192, DynLex-winst/rondes | 4/15 | 6/15 | 7/15 |
| 512×384, DynLex-winst/rondes | 5/15 | 12/15 | 6/15 |
| 1024×768 dicht, DynLex-winst/rondes | 5/15 | 13/15 | 5/15 |
| 1024×768 dicht, gepaard mediaanverschil | +1,4 ms | −2,9 ms | +3,9 ms |

Positief verschil betekent dat adapter+DynLex langer duurde. Vector wordt
op deze beperkte, grotere fixtures vaak sneller; raster en hybride blijven
meestal achter. De kleine pagina's worden sterk door processtart gedomineerd.
Eerdere tabellen hierboven waren niet afwisselend gepaard gemeten en zijn
daarom geen directe trendvergelijking met deze tabel. De volledige beeldkopie
en de tweeprocessenroute blijven belangrijke onderzoekspunten.

De tijdelijke Rust-adapter kopieerde voor raster/hybride eerst de volledige
RGB-XObject naar een `Vec` en daarna nogmaals naar OPDL. Hij leent die bytes nu
uit het al geparseerde `lopdf`-document totdat de OPDL-buffer is opgebouwd.
Een directe afwisselende A/B-proef met 30 warme adapterruns op de dichte
1024×768-fixture gaf raster 17,3→14,9 ms en hybride 16,6→15,6 ms; de OPDL-
uitvoer was bytegelijk. Piek-RSS van het adapterproces daalde in de
verificatie van circa 11,4 naar 9,0 MiB voor raster en 11,6 naar 9,3 MiB
voor hybride. De volledige 15-runs PDF→RGBA-proef na deze wijziging bleef
op dezelfde fixture voor raster/hybride ongeveer 1–3 ms trager dan Hayro,
terwijl vector 14 van 15 gepaarde rondes won. Adapter en DynLex blijven twee
processen; deze winst in de testadapter bewijst geen voordeel in de app.

Een in-process proef kan niet met de huidige CLI-renderer worden geclaimd:
DynLex 0.0.1 biedt `--emit-llvm`, maar de huidige bron exporteert in die IR
alleen `main` en leest/schrijft zelf bestanden. Een echte koppeling vergt een
nieuwe kernel-functie met buffer-invoer/-uitvoer en een expliciete ABI.
`pdf_verify.py` kan nu met `--scratch-dir /dev/shm` een tijdelijke tmpfs-map
gebruiken om bestandskosten te onderzoeken. Op dezelfde dichte 1024×768-
fixture gaf een afzonderlijke 15-runs tmpfs-proef raster/vector/hybride
27,1/11,6/26,1 ms voor adapter+DynLex en 25,9/14,9/29,2 ms voor Hayro;
de gepaarde winsttellers waren 9/14/9 van 15. Ook Hayro werd in die run
sneller dan in de ext4-run. De runs zijn niet onderling afwisselend uitgevoerd,
dus dit is **geen** geïsoleerde meting van de OPDL-tussenbestandskosten of
een bewijs van een in-process snelheidswinst. Beide routes bleven pixelgelijk.

De volgende grens is de echte PDF-subset uitbreiden met diagonale/algemene paden en
transformaties, clipping paths, tekst/fonts en transparantie. Daarna moeten
dezelfde raster-, vector- en gemengde productiebestanden op identieke resolutie,
met koude/warme cache, piekgeheugen en beeldverschillen worden gemeten. Een
snelle synthetische kernel is daarvoor geen bewijs.

DynLex compileert volgens het [upstream project](https://github.com/OpenAEC-Foundation/dynlex)
via LLVM; de huidige compiler is in C++ gebouwd. De DynLex-testprogramma's
gebruiken bovendien libc voor bestandstoegang en allocatie. De keuze om de enginekernel
in DynLex te schrijven wijzigt dus de eerdere eis dat **alle** applicatiecode
en afhankelijkheden Rust zijn. De bestaande Rust-engine blijft voorlopig
functioneel en ongewijzigd totdat een DynLex-kernel op echte PDF's is bewezen.

## Eerste in-process DynLex-bufferkernel

[`rgba/buffer_kernel.dl`](rgba/buffer_kernel.dl) rendert vertrouwde OPDL-v2-
buffers naar een door Rust beheerde RGBA-buffer. De afzonderlijke CLI-renderer
blijft de referentie. [`rgba/build_buffer.py`](rgba/build_buffer.py) laat DynLex
0.0.1 LLVM-IR uitsturen, voegt een stabiele `opds_render_buffer`-C-ABI-wrapper
toe en bouwt met LLVM 20 een statische library. DynLex 0.0.1 kan zelf geen
gedeelde library uitsturen. De Rust-probe valideert vóór de FFI-aanroep de
volledige header, lengte, clip en alle opdrachten; de DynLex-functie zelf
vertrouwt die validatie. Intern staan twaalf controlebytes vóór de pixels in
de uitvoerbuffer. Dit is een POC-contract, **geen veilige algemene PDF- of
OPDL-API** en nog geen koppeling met de applicatie.

Op deze Linux-machine is `llc-20`/`opt-20` tijdelijk uit het Ubuntu-pakket
`llvm-20` onder `/tmp/opds-llvm20` uitgepakt; geen systeeminstallatie is
aangepast. Met een eigen, passende LLVM-20-toolchain zijn de controles zo te
herhalen:

Op 4 oktober zijn de tools opnieuw buiten de repository onder
`~/.cache/opds-toolchains/` opgehaald. De officiële DynLex-0.0.1-tarball
`https://github.com/OpenAEC-Foundation/dynlex/releases/download/0.0.1/dynlex.tar.gz`
heeft bovenstaande SHA-256. Het LLVM-20-pakket komt uit de officiële
`apt.llvm.org` noble-20-index: bestandsnaam
`llvm-20_20.1.8~++20250804090239+87f0227cb601-1~exp1~20250804210352.139_amd64.deb`,
SHA-256 `48911311c972fa02d2d20978d0f7b8360f1681fbd0308560bdc06632583e60e5`.
Uitpakken kan zonder systeeminstallatie met `dpkg-deb -x`. De herstelde paden
zijn `~/.cache/opds-toolchains/dynlex-0.0.1/dynlex/bin/dynlex` en
`~/.cache/opds-toolchains/llvm-20/root/usr/bin/{opt-20,llc-20}`; controleer
beide hashes vóór gebruik. Dit zijn lokale toolchainbestanden, geen broncode
in de repository.

```sh
cd rust-studio
python3 dynlex-engine-poc/rgba/buffer_verify.py \
  --compiler /pad/naar/dynlex/bin/dynlex \
  --opt /pad/naar/opt-20 --llc /pad/naar/llc-20 \
  --rustc /pad/naar/rustc \
  --adapter target/release/opds-opdl-adapter \
  --hayro target/release/opds-engine-probe --bench-calls 100
```

Negen echte PDF-fixtures op 256×192, 512×384 en dicht 1024×768 kwamen
pixel voor pixel overeen met CLI-DynLex, Hayro en de onafhankelijke referentie:
in totaal 3.096.576 gecontroleerde pixels. Vier ongeldige OPDL-bestanden
werden door de Rust-validator afgewezen. In één Rust-proces, met 100 calls op
de reeds ingelezen dichte display-list, was de kernelcall gemiddeld circa
2,43 ms raster, 0,66 ms vector en 2,72 ms hybride. Dit sluit PDF-parsing,
OPDL-opbouw, processtart en bestand-I/O uit en is **niet** vergelijkbaar met
de volledige Hayro-tijden hierboven. De compiler- en LLVM-versies, FFI-ABI en onbetrouwbare invoer vereisen nog
hardening.

## Directe PDF-naar-RGBA-probe

De beperkte adapter is nu ook een Rust-library. `buffer_probe.rs` accepteert
naast OPDL-v2 een `.pdf`: dezelfde Rust-procesaanroep leest de PDF, bouwt OPDL
in geheugen op, valideert die en roept de DynLex-bufferkernel aan. De oude
adapter-CLI en CLI-renderer blijven beschikbaar. Bouw eerst de release-adapter
(`cargo build --release -p opds-opdl-adapter --locked` vanuit `rust-studio`);
`buffer_verify.py` linkt de library automatisch en kan met `--bench-runs 15`
de volledige probeprocesroute afwisselend naast Hayro meten.

Negen PDF-pagina's op drie resoluties waren bytegelijk voor directe probe,
OPDL-probe, DynLex-CLI, Hayro en onafhankelijke referentie (3.096.576 pixels).
De vier ongeldige OPDL-inputs werden nog steeds geweigerd. Op de dichte
1024×768-fixture, 15 afwisselende warme procesruns per agent, waren de
medianen directe route/Hayro respectievelijk raster 24,48/31,70 ms, vector
15,55/19,98 ms en hybride 23,50/30,74 ms. Direct won 15/15, 13/15 en
14/15 gepaarde rondes. Dit omvat processtart, PDF-lezen/parsen, OPDL-opbouw,
renderen en RGBA-schrijfkosten, maar alleen voor de kleine, strikt ondersteunde
PDF-subset. De OS-cache was warm en piekgeheugen en koude cache zijn nog niet
gepaard gemeten. Dit bewijst geen snelheidswinst op echte tekeningen of in de
app. De bestaande Hayro-route blijft ongewijzigd.

## Gepaard procesgeheugen

`buffer_verify.py --bench-runs 15 --bench-memory --bench-json /pad/naar/metingen.json`
meet nu per proces de piek-RSS via Linux `/usr/bin/time -f %M` en bewaart
iedere gepaarde ruwe sample. Beide routes lopen beurtelings eerst. De gemeten
wandtijd bevat bij deze optie ook de identieke `time`-wrapper; vergelijk die
niet rechtstreeks met eerdere runs zonder wrapper. De eerste procesaanroep
wordt apart gerapporteerd, maar de PDF, binaries en bibliotheken kunnen dan al
in de OS-cache staan. De harness wist de OS-cache niet en doet dus **geen**
echte koude-cacheclaim.

Op de dichte 1024×768-fixture, 15 warme procesparen met deze optie, waren de
medianen als volgt (directe DynLex-route / Hayro):

| Agent | Wandtijd | Piek-RSS | Winstparen |
| --- | ---: | ---: | ---: |
| Raster | 25,37 / 38,08 ms | 9.112 / 14.928 KiB | 15/15 |
| Vector | 20,30 / 25,93 ms | 5.372 / 12.548 KiB | 13/15 |
| Hybride | 28,51 / 37,02 ms | 9.288 / 15.020 KiB | 13/15 |

De meting omvat PDF-parsing, OPDL-opbouw, kernel, processtart en uitvoer naar
bestand. De lagere piek-RSS en wandtijd zijn op deze kleine fixture
reproduceerbaar, maar representeren geen algemene PDF-ondersteuning. Een
volgende stap is grotere en complexere PDF-pagina's ondersteunen, pixelpariteit
behouden en pas dan app-geheugen en werkelijk koude start meten.

## Beperkte PDF-translatie

De Rust-adapter begrijpt nu voor gevulde rechthoeken een lokale PDF-
`q 1 0 0 1 tx ty cm ... re f Q`-groep met gehele verschuiving. De vertaling
wordt vóór schermcoördinaten toegepast en mag de rechthoek niet buiten de
pagina brengen. De bestaande full-page image-matrix blijft apart; schalen,
rotatie, schuine matrices en fractionele verschuivingen blijven expliciet
afgewezen. Er is nog geen algemene graphics-state-
stack of vectorpadparser.

Twee aanvullende echte PDF-fixtures (vector en hybride) met een verschoven
rechthoek brengen de verificatie op elf pagina's en 3.194.880 pixels.
Directe DynLex-probe, DynLex-CLI, Hayro en onafhankelijke pixelreferentie zijn
bytegelijk. Schaal-, fractionele en buiten-de-pagina-transformaties worden
geweigerd. Dit is een kleine uitbreiding van de veilige testsubset, geen
ondersteuning voor willekeurige PDF-transformaties.

## Verschoven asgerichte lijnen

Dezelfde gehele `cm`-translatie werkt nu ook voor horizontale en verticale
1-pixelstreken met butt caps (`m l S`). Beide eindpunten worden naar de
paginacoördinaten verschoven voordat de bestaande begrensde lijnconversie
plaatsvindt. De vector- en hybride PDF-fixtures bevatten naast de rechthoek
ook een horizontale en een verticale verschoven lijn. Directe DynLex-probe,
CLI, Hayro en onafhankelijke pixelreferentie blijven op elf PDF-pagina's
(3.194.880 pixels) bytegelijk. Diagonale lijnen, onafgemaakte lijnpaden,
schalen, fractionele verschuivingen en vormen buiten de pagina worden
afgewezen. Dit is nog geen algemene PDF-padrenderer.

## Rechthoekige PDF-clipvakken

De beperkte PDF-adapter accepteert `re W n` op de pagina en in één
`q`/`Q`-groep. Vanaf dat punt snijdt hij elke volgende gevulde rechthoek en asgerichte
1-pixelstreek op het clipvak bij, terwijl eerder getekende raster- en
vectorlagen intact blijven. Opeenvolgende clipvakken nemen hun doorsnede.
Beeld-XObjects na een clipvak, `W*`, niet-rechthoekige paden,
graphics-state-groepen dieper dan twee niveaus en transparantie blijven afgewezen. Er is dus
nog geen algemene PDF-clipping.

Twee nieuwe echte PDF-fixtures testen vector en hybride met een clipvak dat
een gevulde rechthoek en twee lijnen deels doorsnijdt. Dertien pagina's,
3.293.184 pixels, zijn bytegelijk voor directe DynLex-probe, DynLex-CLI,
Hayro en onafhankelijke referentie. Afgekapt `W` zonder `n`, `W*`,
een clip zonder pad en een beeldlaag ná clipping worden geweigerd.

## Doorsnede van meerdere clipvakken

De adapter neemt nu bij herhaalde `re W n`-opdrachten de doorsnede met
het bestaande clipgebied. Een lege doorsnede wordt als leeg clipvak
bewaard; latere vectoropdrachten leveren dan geen pixels of ongeldige
OPDL-opdracht op. Vier extra echte PDF-fixtures (vector/hybride met
overlap en met lege doorsnede) brengen de controle op 17 pagina's en
3.489.792 pixels. Directe DynLex-probe, CLI, Hayro en onafhankelijke
referentie blijven bytegelijk.

## Cliptoestand bewaren en herstellen

Eén `q`/`Q`-groep bewaart nu het bestaande clipvak. Een `re W n`
binnen de groep verkleint dat vak tijdelijk; na `Q` wordt de eerdere
clip hersteld. Dezelfde groep blijft bruikbaar voor de bestaande
gehele translatie of full-page beeldlaag. Groepen dieper dan twee niveaus, kleur- of lijnstijlwijziging binnen
de groep, niet-rechthoekige clip en beelden
ná een actief clipvak blijven buiten de veilige subset.

Twee extra echte PDF-fixtures tekenen eerst binnen een tijdelijk
kleiner clipvak en daarna buiten dat kleinere, maar binnen het herstelde
oudervak. Negentien pagina's/3.588.096 pixels blijven bytegelijk tussen
de directe DynLex-probe, DynLex-CLI, Hayro en de onafhankelijke referentie.
Een derde geneste `q` en een onafgesloten clippad worden geweigerd.

## Twee niveaus cliptoestand

De adapter bewaart nu maximaal twee `q`/`Q`-niveaus voor een begrensde
rechthoekige clip. De binnenste clip wordt bij haar `Q` hersteld naar de
buitenste; de tweede `Q` herstelt het oorspronkelijke paginaclipvak.
De twee extra vector-/hybride fixtures kleuren elk van deze drie gebieden
afzonderlijk. Daarmee komen 21 PDF-pagina's/3.686.400 pixels exact overeen
tussen directe DynLex-probe, CLI, Hayro en onafhankelijke referentie.
Een derde genest niveau blijft afgewezen. Dit is nog geen algemene
PDF-graphics-state-stack: CTM-, kleur- en lijnstijlinstellingen binnen
een geneste groep worden nog niet volledig opgeslagen of hersteld.

## Eerste geantialiaste PDF-diagonaal

OPDL-v2 heeft afzonderlijke `kind=4` en `kind=5` voor beide 45°-
1-pixeloriëntaties met halfpixel-eindpunten en butt caps. De DynLex-CLI en
bufferkernel tekenen drie dekkingstroken (64/233/64) en halve
eindpunten (117). De oude synthetische `kind=2` blijft ongewijzigd.
De Rust-adapter accepteert dit nieuwe primitief met dekkende RGB-inkt,
zonder actief clipvak. Andere hoeken,
brede streken en algemene PDF-paden worden geweigerd. De OPDL-validator
weigert niet-dekkende `kind=4`- en `kind=5`-commando's.

De eerste vector-PDF was pixelgelijk aan Hayro. De overeenkomstige hybride
PDF week op 110 randpixels licht af: DynLex gebruikte gewone afronding,
terwijl Hayro's lokale `vello_cpu`-backend een 8-bit-benadering van delen
door 255 gebruikt: `(waarde + 255) >> 8`. De kernel gebruikt voor deze
gecontroleerde diagonaal dezelfde kleurmenging. Zwarte en rode 45°-streken
zijn in beide oriëntaties op vector- en hybride PDF-pagina's bytegelijk aan
Hayro en de onafhankelijke referentie. De huidige `rgba/buffer_verify.py`
controleert 47 fixtures/4.964.352 pixels, waaronder de grotere opdrachtfixture;
de oude OPDL-v1-fixture blijft ongewijzigd.
Andere hoeken en transparante diagonalen vereisen een eigen gecontroleerd pad.

## Meer tekenopdrachten per pagina

OPDL-v2 accepteert nu maximaal 16.384 tekenopdrachten in plaats van 64.
De Rust-adapter en beide validators hanteren dezelfde grens; de CLI weigert
bestanden groter dan 3.538.984 bytes vóór de geheugenallocatie. Een PDF-fixture
met honderd rode rechthoeken, elk zestig keer herhaald (6.000 opdrachten),
overschrijdt de oude grens en blijft in
vector- en hybride uitvoer pixelgelijk aan Hayro en de onafhankelijke referentie.
Dit verandert niets aan de huidige 1024×1024-pixellimiet of de beperkte
PDF-operators. De echte A1-tekening heeft ook curven, tekst, transformaties
en rotatie nodig voordat zij door deze route kan lopen.

De [gepaarde 6.005-opdrachtenmeting](rgba/bench-many-2026-10-03.md) lokaliseerde
de PDF-contentdecoder als bottleneck. Een begrensde snelle route voor canonieke
fixture-syntaxis won daarna in een A/B-proef van de algemene lopdf-decoder;
dit zegt nog niets over grotere echte tekeningen.

De adapter kan nu een PDF-stroke met meerdere **monotoon collineaire**
`m/l/l/.../S`-segmenten als één asgerichte lijn samenvoegen. Horizontale en
verticale fixtures komen in vector en hybride pixelgelijk met Hayro overeen.
Een streng afgebakende uitzondering is een 1px-pad met één of twee haakse
bochten. Het horizontale buitenste eind ligt op een gehele x, het verticale op
een gehele y en iedere bocht op een halve x en y. De segmenten zijn minstens
twee pixels lang en wisselen horizontaal/verticaal af; er is geen clip. Een
begrensde regel maakt per segment een dekkende OPDL-strook, zodat de gedeelde
hoekpixel volledig gevuld is zoals bij Hayro. De acht L-richtingen en drie
twee-bochtenvormen zijn op vector- en hybride PDF-fixtures pixelgelijk aan
Hayro en de onafhankelijke referentie. Halfpixel-buiteneindpunten, niet-haakse
segmenten, drie of meer bochten, curven en clipping blijven geweigerd. Dit is
geen algemene padondersteuning.

De PDF-adapter bewaart `m/l/.../S` nu eerst als een begrensde reeks punten
(maximaal 16.384 segmenten) en classificeert pas bij `S` of het pad een
bewezen rechte lijn of pixeluitgelijnd pad met hoogstens twee bochten is. De
oude losse toestanden voor beginpunt, lopende lijn en hoek zijn vervangen. Een
fixture met 128 monotoon collineaire segmenten levert precies één OPDL-opdracht
en is in vector en hybride uitvoer
pixelgelijk aan Hayro. Reversals, drie of meer bochten en onbewezen joins blijven
afgewezen. `c`-Béziersegmenten worden nu met hun zes getallen en positie tussen
`l`-segmenten begrensd in `PathStage` opgeslagen. Bij `S` weigert de adapter
ieder pad met een curve expliciet: er is nog geen geverifieerde curve-rasterisatie
of OPDL-curveopdracht. De buffer bereidt bredere padgeometrie voor, maar
ondersteunt nog geen `h`-sluitingen of algemene PDF-paden.
Een pad kan maximaal 16.384 lijn- en curvesegmenten samen stagen; de logische
payload van 16.384 curve-items blijft onder 1 MiB, exclusief Vec-capaciteit.
De 128-segment rechte fixture wordt nog altijd één OPDL-opdracht. Een
algemene curve zou een nieuwe, geverifieerde OPDL-opdracht en pixeltest vereisen.
Een [afzonderlijke OPDL-v3-padcontainer](opdl-v3-path-2026-10-03.md) legt nu
`m/l/c/h` met expliciete recordlengtes en strikte grenzen vast. De bestaande
PDF-adapter en DynLex-renderers gebruiken dit formaat nog niet. Alleen de
aparte `export_path_v3`-proef schrijft de geometrie van één compleet,
geïsoleerd PDF-pad; de normale v2-route weigert curven nog steeds.
De [experimentele OPDL-v4-strokelijst](opdl-v4-stroke-2026-10-03.md) voegt
expliciete dekkende RGB-lijnstijl en `Stroke`-grenzen voor meerdere paden toe.
Een aparte `export_strokes_v4`-proef schrijft inmiddels meerdere echte
PDF-strokes met kleur en breedte in volgorde. De normale PDF-adapter/app blijven
de v2-route gebruiken.
Voor uitsluitend pixeluitgelijnde 1px-horizontale, verticale en 45°-strokes
zet `bridge_v4_axis` de v4-strokelijst om naar bestaande OPDL-v2-opdrachten.
Drie echte PDF-pagina's zijn via deze brug voor alle 147.456 pixels gelijk
aan Hayro en een onafhankelijke RGBA-referentie. Curven, andere hoeken en
2pt-streken worden door de brug afgewezen.
Een afzonderlijke [`v4_stroke_kernel.dl`](rgba/v4_stroke_kernel.dl) tekent
dezelfde bewezen subset in DynLex, met Rust-validatie en een gedeelde
`verified_strokes`-classificatie die losstaat van v2-byte-indeling. De
rasterkernel zelf is onafhankelijk van de v2-kernel en ondersteunt geen
ruwe v4-records, curven of brede lijnen. `rgba/buffer_verify.py` bouwt en
vergelijkt beide kernelroutes met Hayro op de drie PDF-pagina's.

[`rgba/curve_probe.py`](rgba/curve_probe.py) legt daarom twee kleine echte
PDF-Bézierstroken vast tegen de bestaande Hayro-renderer. Herhaal met
`python3 dynlex-engine-poc/rgba/curve_probe.py --hayro
target/release/opds-engine-probe --adapter target/release/opds-opdl-adapter`.
De [referentie-JSON](rgba/curve-reference-2026-10-03.json) bevat per vector-
en hybride pagina het veranderde pixelvak en een SHA-256 van de RGBA-waarden.
De probe leidt nu bovendien voor de zwarte boog en S-curve één 8-bit-dekking
per pixel af die zowel de vector- als hybride uitvoer exact verklaart met
Vello's benaderende `/256`-mengregel. De gecomprimeerde
[`arch`](rgba/curve-coverage-arch.zlib)- en
[`s-bend`](rgba/curve-coverage-s-bend.zlib)-kaarten bevatten elk 256×192
dekkingsbytes na decompressie; de SHA-256 staat in de JSON. Herhaal de
regressie met `--expected-report rgba/curve-reference-2026-10-03.json` bij
`curve_probe.py`. Dit is meetdata voor een toekomstige curvekernel, geen
curve-rendering of bewezen rasterisatie-algoritme; de adapter weigert `c` nog.
[`rgba/curve_candidate.py`](rgba/curve_candidate.py) probeert los van de app
een 512-segmentenpolylijn met butt-eindpunten en 4×4, 8×8 of 16×16
subpixelbemonstering. De [ruwe vergelijking](rgba/curve-candidate-2026-10-04.json)
laat bij 16×16 nog 258 afwijkende boogpixels en 247 afwijkende S-curvepixels
zien, met maximale dekkingsfout 64 en 103. Meer samples alleen levert dus nog
geen Vello-pariteit op. Dit is een diagnostische **mislukte kandidaat**; zij
wordt nergens in de v2/v4-renderroute gebruikt. Herhaal met
`python3 dynlex-engine-poc/rgba/curve_candidate.py --samples 4 8 16
--expected-report dynlex-engine-poc/rgba/curve-candidate-2026-10-04.json`.
De lokale bron van Hayro 0.7.1 gebruikt Kurbo `Stroke` met PDF-butt-caps;
Vello CPU 0.0.8 roept `kurbo::stroke_with` aan met tolerantie 0,25, vlakt de
resulterende **gevulde omtrek** af en berekent daarna analytische pixeloppervlakte
via coverage strips. De afzonderlijke
[`curve_vello_probe.rs`](../crates/engine-probe/src/bin/curve_vello_probe.rs)
tekent dezelfde twee paden rechtstreeks door Vello, zonder PDF-parser. Zijn
alpha-buffer is voor beide 49.152-byte-dekkingskaarten exact gelijk aan de
Hayro-afleiding. Reproduceren:

```sh
cargo build --release -p opds-engine-probe --bin curve_vello_probe
python3 dynlex-engine-poc/rgba/verify_curve_vello.py \
  --probe target/release/curve_vello_probe
```

Dit is een bron- en geometrieverificatie van de referentierenderer; het is
**geen DynLex-curvekernel** en bewijst geen onafhankelijke curvepariteit.
[`curve_outline_probe.rs`](../crates/engine-probe/src/bin/curve_outline_probe.rs)
legt met dezelfde Kurbo 0.13.1-stroke-instellingen de omtrek afzonderlijk vast
als [boog](rgba/curve-outline-arch.txt) en [S-curve](rgba/curve-outline-s-bend.txt).
Beide bestaan uit een gesloten pad met twee cubic-randen en een rechte
eindkap; dit verklaart waarom de eerdere afstand-tot-middellijnproef een
andere geometrie gebruikte. Reproduceer na `cargo build --release -p
opds-engine-probe --bin curve_outline_probe` met
`target/release/curve_outline_probe arch /tmp/curve-outline-arch.txt`.

De onafhankelijke [`curve_area_candidate.py`](rgba/curve_area_candidate.py)
vlakt die omtrek met een **uniform** aantal stappen af en knipt de resulterende
polygoon per pixel analytisch. De
[metingen](rgba/curve-area-candidate-2026-10-04.json) tonen bij 16 stappen
nog 244 afwijkende boogpixels en 231 S-curvepixels; bij 32 stappen neemt de
fout weer toe. Vello gebruikt een adaptieve flattening met tolerantie 0,25 en
coverage strips. De uniform-afgevlakte oppervlaktemethode is dus nog geen
pariteit en wordt niet in de kernel gebruikt. Herhaal met
`python3 dynlex-engine-poc/rgba/curve_area_candidate.py --steps 2 4 8 12 16 24 32
--expected-report dynlex-engine-poc/rgba/curve-area-candidate-2026-10-04.json`.
De afzonderlijke [`curve_flatten_probe.rs`](../crates/engine-probe/src/bin/curve_flatten_probe.rs)
legt vervolgens de **daadwerkelijke** Vello-common-0.0.8-lijnsegmenten van de
stroke-omtrek vast: 28 voor de boog en 30 voor de S-curve. De bestanden staan
in [`curve-flat-arch.txt`](rgba/curve-flat-arch.txt) en
[`curve-flat-s-bend.txt`](rgba/curve-flat-s-bend.txt). De eigen Python-rasterproef
gebruikt deze segmenten uitsluitend als geometrie-invoer en knipt de gesloten
polygoon zelf per pixel. Met `--flat-lines` levert zij voor **beide** kaarten
49.152/49.152 exacte dekkingsbytes; zie
[`curve-flat-area-2026-10-04.json`](rgba/curve-flat-area-2026-10-04.json).

```sh
cargo build --release -p opds-engine-probe --bin curve_flatten_probe
target/release/curve_flatten_probe arch /tmp/curve-flat-arch.txt
target/release/curve_flatten_probe s-bend /tmp/curve-flat-s-bend.txt
python3 dynlex-engine-poc/rgba/curve_area_candidate.py --flat-lines \
  --expected-report dynlex-engine-poc/rgba/curve-flat-area-2026-10-04.json
```

Dit is alleen pariteit van een **eigen oppervlaktemeting gegeven Vello's
segmenten** op twee kleine fixtures. De flattening is nog niet onafhankelijk
nagebouwd, de proef is Python en er bestaat nog geen DynLex-curveopdracht.
Curven blijven in de gewone adapter en app geweigerd.
Dezelfde per-pixel polygon-clip en oppervlakteberekening draait inmiddels ook
als zelfstandige [Rust-proef](rgba/curve_area_rust.rs), zonder Vello-aanroep bij
het rasteren. Haar streng begrensde tekstinvoer bevat uitsluitend verbonden,
gesloten, eindige lijnstukken uit de vastgelegde fixtures; ongeldige invoer
wordt vóór uitvoer afgewezen. Met Vello's vooraf vastgelegde segmenten levert
zij wederom beide 49.152-byte-kaarten exact. Reproduceren:

```sh
rustc --edition=2024 -O dynlex-engine-poc/rgba/curve_area_rust.rs -o /tmp/opds-curve-area-rust
python3 dynlex-engine-poc/rgba/verify_curve_area_rust.py \
  --rasterizer /tmp/opds-curve-area-rust
```

Dit vervangt nog geen DynLex-code of PDF-parser; de onafhankelijke stap is
uitsluitend de area-rasterisatie, niet de cubic-flattening.
De volgende interface is een afzonderlijke, [begrensde OPLN-v1-segmentbuffer](rgba/curve_scene.rs):
`OPLN`, versie 1, 256×192, puntaantal en vervolgens little-endian `f64`-paren.
Maximaal 4.097 punten (65.572 bytes totaal), uitsluitend eindige coördinaten
binnen ±4096 en een gesloten omtrek. De Rust-validator weigert truncatie,
verkeerde versie/afmetingen/aantal, extra bytes, NaN en open paden vóór
rasterisatie. De area-proef leest zowel de oude tekstfixture als deze buffer;
beide leveren de twee kaarten bytegelijk. Reproduceren:

```sh
rustc --edition=2024 -O dynlex-engine-poc/rgba/curve_scene.rs -o /tmp/opds-curve-scene
rustc --edition=2024 -O dynlex-engine-poc/rgba/curve_area_rust.rs -o /tmp/opds-curve-area-rust
python3 dynlex-engine-poc/rgba/verify_curve_area_rust.py \
  --rasterizer /tmp/opds-curve-area-rust --scene-tool /tmp/opds-curve-scene
```

OPLN is alleen een experimenteel kernelinvoercontract. DynLex leest het nog
niet als volledige renderopdracht; er is geen onafhankelijke adaptive
flattening of algemene curveclaim. Een kleine
[`curve_decode_kernel.dl`](rgba/curve_decode_kernel.dl)-proef bevestigt nu wel
de eerste ABI-grens: na volledige Rust-validatie worden OPLN-punten op een
8-byte-uitgelijnde offset genormaliseerd. DynLex 0.0.1 leest alle `f64`-x-waarden,
geeft aantal, eerste/laatste x en som terug en de Rust-probe vergelijkt die met
de validator. De C-wrapper weigert null-pointers en te korte buffers voordat
DynLex wordt aangeroepen; beschadigde OPLN-inputs bereiken de kernel niet.

```sh
python3 dynlex-engine-poc/rgba/build_curve_decode.py \
  --compiler /pad/naar/dynlex --opt /pad/naar/opt-20 --llc /pad/naar/llc-20 \
  --output-dir /tmp/opds-curve-decode-build
rustc --edition=2024 -O dynlex-engine-poc/rgba/curve_decode_probe.rs \
  -L native=/tmp/opds-curve-decode-build -o /tmp/opds-curve-decode-probe
python3 dynlex-engine-poc/rgba/verify_curve_decode.py \
  --scene-tool /tmp/opds-curve-scene --probe /tmp/opds-curve-decode-probe
```

Een tweede, afzonderlijke [DynLex-pixelproef](rgba/curve_pixel_kernel.dl)
neemt dezelfde door Rust gevalideerde en uitgelijnde OPLN-buffer. Vier
Sutherland-Hodgman-clips en de shoelace-oppervlakte worden nu in DynLex
uitgevoerd. De [Rust-probe](rgba/curve_pixel_probe.rs) vergelijkt voor een
rechthoek en voor **alle 49.152 pixels** van beide vooraf afgevlakte
curve-omtrekken met de onafhankelijke Rust-berekening; alle waarden zijn binnen
1e-9 gelijk. Null-/te korte ABI-buffers worden vóór de kernel afgewezen.

```sh
python3 dynlex-engine-poc/rgba/build_curve_pixel.py \
  --compiler /pad/naar/dynlex --opt /pad/naar/opt-20 --llc /pad/naar/llc-20 \
  --output-dir /tmp/opds-curve-pixel-build
rustc --edition=2024 -O dynlex-engine-poc/rgba/curve_pixel_probe.rs \
  -L native=/tmp/opds-curve-pixel-build -o /tmp/opds-curve-pixel-probe
/tmp/opds-curve-pixel-probe dynlex-engine-poc/rgba/curve-flat-arch.txt
/tmp/opds-curve-pixel-probe dynlex-engine-poc/rgba/curve-flat-s-bend.txt
```

DynLex 0.0.1 crashte tijdens compilatie van een dynamische `float`-allocatie;
de proef alloceert daarom bytes en gebruikt een uitgelijnde `f64`-weergave.
De proef schrijft nu ook zelf de volledige 256×192-dekkingsbuffer als 8-bit
grijswaarden. [De verificatie](rgba/verify_curve_raster.py) vergelijkt voor
boog en S-curve ieder van de 49.152 bytes exact met de opgeslagen
Hayro/Vello-kaart; vijf ongeldige invoeren leveren geen bestand op. De Rust-
validator accepteert tekst of OPLN en normaliseert naar dezelfde uitgelijnde
buffer. Reproduceren, na compilatie van de bovenstaande library:

```sh
rustc --edition=2024 -O dynlex-engine-poc/rgba/curve_raster_probe.rs \
  -L native=/tmp/opds-curve-pixel-build -o /tmp/opds-curve-raster-probe
rustc --edition=2024 -O dynlex-engine-poc/rgba/curve_scene.rs \
  -o /tmp/opds-curve-scene
python3 dynlex-engine-poc/rgba/verify_curve_raster.py \
  --probe /tmp/opds-curve-raster-probe --scene-tool /tmp/opds-curve-scene
```

De [gepaarde meetproef](rgba/bench_curve_raster.py) vergelijkt vijftien warme
procesruns per fixture met de zelfstandige Rust-area-rasterizer en controleert
in elke ronde alle bytes. De ruwe samples staan in
[`curve-raster-bench-2026-10-04.json`](rgba/curve-raster-bench-2026-10-04.json).
Medianen van de eerste versie DynLex/Rust: boog **19,12/6,00 ms**, S-curve
**19,96/5,82 ms**; piek-RSS ongeveer **2,1/2,1 MiB**. Processtart en
bestand-I/O zijn inbegrepen; de OS-cache is niet gewist. De eerste DynLex-
versie alloceerde twee puntbuffers per pixel. De rasterfunctie hergebruikt nu
twee begrensde buffers voor de hele pagina; de afzonderlijke eenpixel-ABI
behoudt eigen buffers. Dezelfde 49.152 bytes, inclusief binaire OPLN-invoer,
blijven exact, evenals de afwijzing van corrupte invoer.

Een tweede [A/B-meting met ruwe samples](rgba/curve-raster-reuse-bench-2026-10-04.json)
draait dertig warme procesrondes afwisselend met oude DynLex, nieuwe DynLex en
Rust; iedere uitvoer wordt opnieuw bytegewijs gecontroleerd. Mediaan
nieuw/oud/Rust: boog **13,97/18,03/6,05 ms**, S-curve
**13,57/17,88/5,75 ms**. Piek-RSS ligt bij alle drie rond 2,1 MiB; kleine
RSS-verschillen zijn op deze schaal niet betekenisvol. De nieuwe DynLex-route
won 26/30 boogparen en 30/30 S-curveparen van de oude
route, maar geen paar van de Rust-proef. Herhaal met
[`bench_curve_raster.py`](rgba/bench_curve_raster.py) en `--baseline` voor een
apart gebouwde oude binary. Dit is een procesmeting op twee kleine, vooraf
afgevlakte fixtures, geen PDF→RGBA-benchmark of algemene snelheidsclaim.

De volgende proef bepaalt eenmaal de begrenzing van de gevalideerde punten en
schrijft voor pixels die de omtrek zeker niet kunnen raken direct nul. Dit
laat de vier clips alleen lopen waar geometrie aanwezig kan zijn. De twee
Hayro-dekkingskaarten blijven bytegelijk; vijf rechthoekgevallen testen ook
linker-/rechterpaginagrens, geheel buiten de pagina, fractionele dekking en
een volledig gevuld blad.
Vijf ongeldige inputs blijven geweigerd. De
[30-ronden-A/B-samples](rgba/curve-raster-bbox-bench-2026-10-04.json) vergelijken
deze versie met de vorige bufferhergebruikversie en Rust, met bytecontrole na
iedere run. Mediaan nieuw/vorig/Rust: boog **5,57/12,54/6,09 ms**, S-curve
**5,66/14,43/6,02 ms**. Nieuw wint 30/30 paren van vorig. Tegen Rust wint
nieuw 20/30 en 16/30; dat kleine procesverschil is geen bewezen
kernelvoordeel. Piek-RSS blijft rond 2,1 MiB. OS-cache is niet gewist,
processtart en bestand-I/O zijn inbegrepen, en de input is nog steeds een
vooraf afgevlakte, kleine testomtrek.

Dit is nog geen volledige DynLex-curve-renderer: de cubic-flattening blijft
Vello-afhankelijk, compositing is niet geïntegreerd en de gewone PDF-adapter
blijft curven weigeren. Onafhankelijke flattening en de resterende
rasterprestaties moeten worden onderzocht voordat deze proef richting
productierendering gaat.

Een eerste [zelfstandige Rust-flattener](rgba/curve_flatten_rust.rs) verdeelt
de twee vastgelegde Kurbo-stroke-omtrekken recursief met de Casteljau, een
begrensde afstand-tot-koorde-test, maximaal 16 niveaus en 4.097 punten.
Hij gebruikt geen Vello/Kurbo-aanroep tijdens de proef, schrijft gevalideerde
OPLN en laat die door de DynLex-dekkingskernel rasteren. De
[diagnose](rgba/curve-independent-flatten-2026-10-04.json) vergelijkt
segmentaantallen en alle pixels met de Vello/Hayro-kaarten. Bij tolerantie
0,25 zijn het 34 versus 28 boogsegmenten en 54 versus 30 S-curve-segmenten;
244 respectievelijk 243 pixels wijken af. Tolerantie 0,5 geeft de S-curve
wel 30 segmenten, maar nog 229 afwijkende pixels. Een gelijk segmentaantal
bewijst dus geen gelijke segmentposities of rasterdekking. Vier ongeldige
outline-/tolerantiegevallen worden zonder uitvoer afgewezen. Reproduceren:

```sh
rustc --edition=2024 -O dynlex-engine-poc/rgba/curve_flatten_rust.rs \
  -o /tmp/opds-curve-flatten-rust
python3 dynlex-engine-poc/rgba/verify_curve_flatten_rust.py \
  --flattener /tmp/opds-curve-flatten-rust \
  --rasterizer /tmp/opds-curve-raster-probe \
  --scene-tool /tmp/opds-curve-scene \
  --expected-report dynlex-engine-poc/rgba/curve-independent-flatten-2026-10-04.json
```

Deze kandidaat is bewust niet geïntegreerd: de exacte adaptieve segmentverdeling
en daarna de onafhankelijke stroke-outline ontbreken nog. Vello's lokale
`flatten_simd.rs` is alleen gebruikt om het verschil te onderzoeken.

Een tweede, [begrensde scalar-f32-proef](rgba/curve_flatten_scalar.rs) vertaalt
de kwadratische onderverdeling en krommingsparameterisatie uit het lokaal
beschikbare, Apache-2.0/MIT-gelicentieerde `vello_common 0.0.8` naar eigen
Rust-code zonder die bibliotheek tijdens uitvoering aan te roepen. De
[verifier](rgba/verify_curve_flatten_scalar.py) gebruikt Kurbo's vastgelegde
stroke-omtrek als fixture, valideert de resulterende OPLN-buffer en laat de
DynLex-kernel alle pixels tekenen. Boog en S-curve hebben nu precies 28 en 30
segmenten en ieder **49.152/49.152 bytegelijke dekkingspixels** tegen de
vastgelegde Hayro/Vello-kaarten. Een derde, ondiepe curve met 18 segmenten
is eveneens 49.152 bytes exact tegen een afzonderlijke Vello CPU-oracle. De
segmentcoördinaten zijn nog niet exact: maximale afwijking respectievelijk
0,000061, 0,000862 en 0,000023 px. Zes ongeldige/onbegrensde inputs worden
zonder uitvoer geweigerd. Het rapport staat in
[`curve-scalar-flatten-2026-10-04.json`](rgba/curve-scalar-flatten-2026-10-04.json).

```sh
rustc --edition=2024 -O dynlex-engine-poc/rgba/curve_flatten_scalar.rs \
  -o /tmp/opds-curve-flatten-scalar
python3 dynlex-engine-poc/rgba/verify_curve_flatten_scalar.py \
  --flattener /tmp/opds-curve-flatten-scalar \
  --rasterizer /tmp/opds-curve-raster-probe \
  --scene-tool /tmp/opds-curve-scene \
  --expected-report dynlex-engine-poc/rgba/curve-scalar-flatten-2026-10-04.json
```

Dit is een scalar herwerking van bestaande Vello-wiskunde, geen nieuw
afgeleid algoritme of zelfstandige PDF-engine. De stroke-outline komt nog
uit Kurbo, de extra ondiepe curve heeft alleen een directe Vello-oracle, en
drie kleine fixtures bewijzen geen algemene pariteit. Curven blijven in de
gewone adapter en app uitgeschakeld.

De ondiepe referentie gebruikt besturingspunten `(20,5;111,5)`,
`(43,5;93,5)`, `(67,5;93,5)`, `(90,5;111,5)` en is gemaakt met de
afzonderlijke `curve_outline_probe`, `curve_flatten_probe` en
`curve_vello_probe` in `crates/engine-probe/src/bin`. Zij schrijven de
vastgelegde `curve-outline-shallow.txt`, `curve-flat-shallow.txt` en de
gecomprimeerde 256×192-kaart; de scalar/Rust-DynLex-verifier roept deze
referentiecode niet aan.
De boog verandert 250 pixels in beide modi; de S-vorm 237 vectorpixels en
235 hybride pixels. Dat verschil is een verschil met de ondergrond, geen
bewijs voor andere geometrische dekking. De proef controleert ook dat de
adapter beide curves nog weigert. Voor activering zijn per-pixeldekking,
compositing en joins afzonderlijk te verifiëren; deze JSON is geen DynLex-
curvebenchmark.

Een volgende [zelfstandige Rust-proef](rgba/curve_outline_candidate.rs) maakt
de 1px butt-cap-omtrek van één cubic zonder Kurbo/Vello runtime-aanroep. Zij
berekent normaal-offsetpunten bij `t=0,1/3,2/3,1` en past per zijde één cubic
door de twee binnenpunten. Dit is bewust een beperkte geometriekandidaat:
Kurbo 0.13.1 gebruikt in zijn Apache-2.0/MIT-gelicentieerde `stroke.rs` en
`offset.rs` een adaptieve offsetsolver met foutcontrole; vier samples zijn
geen equivalent. De [verifier](rgba/verify_curve_outline_candidate.py) voert
de omtrek door de eigen scalar flattener en DynLex-area-kernel en vergelijkt
alle dekkingsbytes met drie bestaande en een vierde asymmetrische
Vello/Kurbo-oracle. Het [rapport](rgba/curve-outline-candidate-2026-10-04.json)
toont respectievelijk 233, 233, 114 en 305 afwijkende pixels, met
maximale dekkingsfout 20, 106, 2 en 15. Vijf ongeldige of onbegrensde
invoerpatronen worden zonder output geweigerd. De nieuwe asymmetrische
fixtures `curve-outline-asymmetric.txt`, `curve-flat-asymmetric.txt` en
`curve-coverage-asymmetric.zlib` zijn read-only referenties. De kandidaat
wordt nergens in de PDF-adapter of app gebruikt.

Een [adaptieve vervolgproef](rgba/curve-outline-adaptive-2026-10-04.json)
past per zijde maximaal 16 cubics met behoud van eindpunttangenten,
31 kleinste-kwadraten-meetpunten per interval en 63 afzonderlijke
foutcontrolepunten. Intervallen worden gesplitst tot de bemonsterde
geometrische fout hoogstens 0,02 px is; bij onhaalbare fout of degeneratie
weigert de proef de uitvoer. Zeven ongeldige gevallen worden getest. Op
boog/S-curve/ondiep/asymmetrisch blijven 244/229/126/315 pixels afwijken;
voor drie van vier vormen is dit slechter dan de een-cubic-baseline op commit
`d1c47a1f`. Meer segmenten alleen reconstrueren Kurbo's offsetsolver dus
niet. Het JSON-rapport bewaart ook segmentaantallen en eerste afwijkende
pixel. De `curve_outline_candidate.rs` in de huidige boom is deze adaptieve
diagnose; de eerste een-cubic-versie en zijn rapport zijn op die commit
reproduceerbaar.

```sh
rustc --edition=2024 -O dynlex-engine-poc/rgba/curve_outline_candidate.rs \
  -o /tmp/opds-curve-outline-candidate
python3 dynlex-engine-poc/rgba/verify_curve_outline_candidate.py \
  --candidate /tmp/opds-curve-outline-candidate \
  --flattener /tmp/opds-curve-flatten-scalar \
  --scene-tool /tmp/opds-curve-scene \
  --rasterizer /tmp/opds-curve-raster-probe \
  --json /tmp/opds-curve-outline-candidate.json
```

Voor een onafhankelijke stap richting A1 bestaat nu een
[versieerbare OPGM-v1-paginageometrieproef](opgm-v1-geometry-2026-10-04.md).
Een strikt beperkte Rust-PDF-exporter en aparte probe vertalen overgeërfde
MediaBox/CropBox en vier kwartslagen van één
pixeluitgelijnde rechthoek naar OPDL-v2; een experimentele vector-only
bufferoptie vergroot de paginalimiet tot maximaal 6 miljoen pixels. Zeven
echte PDF-fixtures, inclusief 90°/CropBox en A1-achtig 2384×1684 zichtbaar,
zijn bytegelijk aan Hayro en een onafhankelijke referentie. De normale
adapter en app blijven ongewijzigd.

De aparte [OPGM-v2-laagproef](opgm-v2-layers-2026-10-05.md) voegt maximaal
64 expliciet gekleurde `rg/re/f`-rechthoeken in PDF-laagvolgorde toe. Zeven
PDF-fixtures met overlappende RGB-vormen zijn op kleine kwartslag-/CropBox-
pagina's en A1-achtig formaat 17.161.664 RGBA-bytes exact gelijk aan Hayro
en onafhankelijke referentie. OPGM-v1, gewone OPDL-v2-app en de bestaande
DynLex-kernel blijven werken; de exporter/probe zijn experimenteel.
