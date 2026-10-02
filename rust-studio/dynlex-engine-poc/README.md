# DynLex PDF-engine POC: drie renderagents

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
