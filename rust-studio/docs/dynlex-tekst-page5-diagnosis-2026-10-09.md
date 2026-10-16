# Tekst.pdf pagina 5: hybride inhoud en strikte afwijzing

### Hercontrole van de overige pagina's, 13 oktober 2026

Een native strikte run op de ongewijzigde `Tekst.pdf` pagina 1 bij 100%
weigert op `unsupported operator Tr at 38`, zonder outputbitmap. De
daadwerkelijke paginastream bevat `2 Tr 0.36666 w`: PDF-tekstmodus 2 vult én
contourt de glyphs. De bestaande Type1-tekstworker bewijst alleen
vulling (`0 Tr`), zodat `2 Tr` als inert behandelen de letterranden zou
weglaten. Dit is een echte kernelbeperking, niet een fout bij het openen van
het document. Hayro rendert dezelfde pagina op 100% als 595×841 pixels.
Een uitbreiding moet de contourdekking en verfvolgorde op volledige originele
pagina's en meerdere schalen pixel voor pixel bewijzen voordat DynLex deze
modus accepteert.

De oorspronkelijke lokale `test pdf-bestanden/Originele bestanden/Tekst.pdf`
(SHA-256 `a97db0d26a89643a88976e4a68823d96b3b6f24b29e9dca3cc74f29c48c0fc43`)
heeft op pagina 5 een ingebedde Type1/PFB voor de elf tekstblokken, een
**ongebruikte** symbolische TrueType-fontresource en een 1240×1754
DeviceRGB8/DCTDecode-beeld `/Im4`. De content begint met drie rechthoekige
`W*`-clips, gevolgd door `594.1 0 0 842.75 1.2 1.789 cm /Im4 Do` en
de tekstlagen. Het is dus een echte hybride pagina, geen losse glyphfixture.
De oorspronkelijke paginagrootte is fractioneel.

De tekstworker valideert nu maximaal acht fontresourcenamen maar decodeert
uitsluitend de namen die een `Tf`-operator daadwerkelijk kiest. Een misvormd
ongebruikt font verandert de verfvolgorde niet; een verwijzing ernaar wordt
nog steeds afgewezen. Valide `w`, `J`, `j`, `G` en `RG` worden als inerte
stroke-state erkend **alleen omdat deze frontend iedere stroke-paint en
niet-vullende tekstmodus weigert**. Ongeldige waarden en `S` blijven
afgewezen. De rechthoekige clip wordt vóór PWPG-framing met het harde
pagina-viewport doorsneden; een fractionele rand net buiten de laatste
uitvoerpixel kan geen dekkingsbyte beïnvloeden. Lege doorsneden blijven
unsupported.

Een aparte replay met tekst en vulling die de paginarand raken, een clip die
0,25 PDF-punt buiten beide randen begint/eindigt, vier rotaties, twee
CropBoxes en 25/100/200% is **24/24 volledige RGBA-buffers en 81.098.496
bytes exact** tegen Hayro. `scripts/audit_text_page_clip.py` bewaart de
bron-/binaire-/pixelhashes buiten Git onder
`~/.cache/opds-benchmarks/type1-page-clip-2026-10-09/`. De eerdere
24 integer-clipfixtures en zestien native tekstgevallen blijven exact.
Een worker-unittest controleert ongebruikte fonts, stroke-only state en
afwijzing van actief fontgebruik, slechte operanden en stroke-paint.

Deze wijziging ontsluit de oorspronkelijke pagina **nog niet**. De eerste
strikte afwijzing verschoof van ongebruikt F2-font via `w` en de buiten het
floor-paginavak liggende clip naar **`unsupported operator cm at 14`**.
Daarna volgt bovendien het nog onbewezen `/Im4 Do`-beeld binnen de clip.
Er is geen gedeeltelijke bitmap en geen Hayro-fallback. Alle vijf originele
Tekst.pdf-pagina's op 25/100/200% blijven 0/15 ondersteund en nul
procesfouten. Een grotere limiet of het negeren van `cm`/`Do` zou de pagina
onjuist tekenen.

Volgende concrete stap: een begrensde geordende DynLex-scene waarin het
originele JPEG-beeld met gevalideerde `cm`, clip en laagvolgorde vóór de
Type1-tekst wordt verwerkt. Isoleer eerst beeldplaatsing en sampling uit
pagina 5, vergelijk iedere pixel op meerdere schalen, en activeer het
hybride pad pas na volledige originele-paginapariteit. De bestaande
algemene JPEG-tegelkandidaat was op andere gedeeltelijke placements niet
pixelgelijk en mag niet stilzwijgend worden hergebruikt. Daarna de resterende
symbolische font- en padtoestanden van de andere pagina's onderzoeken.

`scripts/isolate_tekst_page5_image.py` maakt buiten Git een afzonderlijke
PDF met **de oorspronkelijke `/Im4`-JPEG en dezelfde rechthoekclip en `cm`**,
maar zonder de tekst. De JPEG is 1240×1754. SHA-256-hashes van bron,
JPEG, geïsoleerde PDF, proefbinary en drie Hayro-RGBA-buffers staan in
`~/.cache/opds-benchmarks/type1-page-clip-2026-10-09/tekst-page5-image-isolation-2026-10-09.json`.
De oracle-afmetingen bij 25/100/200% zijn 148×210, 595×841 en 1190×1683;
zij bevatten samen 10.136.980 RGBA-bytes. De huidige strikte DynLex-route
weigert elk geval bij `cm` zonder outputbestand. Dit is een reproduceerbare
volledige-pagina-beeldoracle voor de afzonderlijke onderzoeksvergelijking
hieronder; de native strikte route verwerkt dit beeld nog niet.

Een tweede read-only isolatie verwijdert uitsluitend de vier operaties
`q cm /Im4 Do Q` uit het oorspronkelijke paginastream; alle overige
oorspronkelijke Type1-tekst, clips en resources blijven staan.
`scripts/isolate_tekst_page5_text.py` rendert deze tekstpagina op
25/87/100/115/156,25/200% via de **native strikte DynLex-route** en Hayro.
Alle zes volledige RGBA-buffers, samen **19.191.004 bytes**, zijn exact;
nul afwijkende kanalen. Het reproduceerbare bron-/PDF-/binary-/pixelrapport
staat buiten Git in
`~/.cache/opds-benchmarks/type1-page-clip-2026-10-09/tekst-page5-text-isolation-2026-10-09.json`.
Daarmee is de echte tekstlaag op deze zes schalen bewezen, maar de
oorspronkelijke hybride pagina blijft unsupported: afbeeldingsplaatsing,
sampling, clip en compositie over de beeldlaag moeten samen bewezen worden.

### Afzonderlijke beeldplaatsingskandidaat

`crates/opdl-adapter/src/bin/partial_image_probe.rs` is uitsluitend een
onderzoeksbinary voor de geïsoleerde beeld-PDF; de native dispatcher roept
hem nooit aan. De kandidaat leest één begrensde DeviceRGB8-JPEG,
rechthoekclip en axis-aligned `cm` uit het vertrouwde onderzoeksbestand,
decodeert met zune-jpeg, resamplet met de bestaande Rust pic-scale-route en
berekent de oorspronkelijke beeldcoördinaten per outputpixel. De f32-
256-kolomherstart en vier verticale lanes volgen de eerder bewezen
beeldsampleconventie. Deze lopdf-probefrontend heeft geen volledige
object-/tijdslimiet voor onbetrouwbare PDF's en hoort niet in native dispatch.

`scripts/audit_partial_image_probe.py` vergelijkt nu de **volledige**
geïsoleerde echte JPEG-pagina op 25/87/100/115/156,25/200%. Alle zes
RGBA-buffers, samen **19.191.004 bytes**, zijn bytegelijk aan Hayro. De
correctie rastert het JPEG-beeld als gealiaste pixelrechthoek, berekent de
fractionele dekking uitsluitend uit de PDF-clip en rondt bron- en witte
achtergrondbijdragen afzonderlijk naar 8 bit af. Clipranden passeren de
f32-schermcoördinaatgrens vóór de dekkingsberekening; anders verschilt een
actieve rechterrand van `N+0,1` één 8-bit-stap. Dit is een algemene
numerieke correctie, geen document- of pixelwhitelist.

`scripts/audit_partial_image_variants.py` gebruikt dezelfde echte ingebedde
JPEG met vier plaatsing-/clipvarianten op 25/100/200%. Ook deze twaalf
volledige buffers zijn bytegelijk: **40.547.920 RGBA-bytes**. De eerdere
onbewezen kandidaat had in één actieve-clipvariant nog 18 afwijkende kanalen
in 15 randpixels. Bron-, PDF-, binary- en pixelhashes staan buiten Git in
`~/.cache/opds-benchmarks/type1-page-clip-2026-10-09/tekst-page5-image-candidate-six-2026-10-09.json`
en `~/.cache/opds-benchmarks/type1-page-clip-2026-10-09/variants/partial-image-variants-2026-10-09.json`.

Dit is nog **geen native ondersteuning**: de proef leest een vertrouwde
geïsoleerde PDF via lopdf, heeft geen volledige object-/tijdslimiet en
schildert slechts één axis-aligned JPEG zonder tekst. De oorspronkelijke
hybride `Tekst.pdf` pagina 5 blijft strikt unsupported en krijgt geen
snelheidsscore. De onderzoeksfrontend weigert directe of geërfde `/Rotate`
en `/CropBox`, `/UserUnit`, `/Group`, `/Annots` en een niet-asgerichte `cm`
nu expliciet. `scripts/audit_partial_image_rejections.py` bevestigt acht
afwijzingen zonder outputbitmap; hashes en foutdetails staan buiten Git in
`~/.cache/opds-benchmarks/type1-page-clip-2026-10-09/rejections/partial-image-rejections-2026-10-09.json`.
Dit is nog geen rotatie-/CropBox-dekking. Volgende stap: die transformaties
zelf pixelgelijk maken en overige malformede invoer toetsen; daarna een
begrensde geordende image+text-scene in de bestaande worker met
volledige pixelcontrole van de **oorspronkelijke** pagina.

`scripts/diagnose_partial_image_rotation.py` bevestigt waarom een bestaande
RGBA-pagina draaien daarvoor niet volstaat. Het vergelijkt uitsluitend
Hayro's oorspronkelijke pagina met Hayro's rechtstreeks gerenderde
`/Rotate 90/180/270`-varianten: op 100% verschillen respectievelijk
25.466, 25.611 en 2.383 pixels van een simpel gedraaide bitmap. Zelfs de
beste gehele verschuiving van hoogstens één pixel laat afwijkingen over.
De negen 25/100/200%-resultaten met bron- en binaryhashes staan buiten Git
in `~/.cache/opds-benchmarks/type1-page-clip-2026-10-09/rotation/partial-image-rotation-diagnosis-2026-10-09.json`.
Dit is een **Hayro-tegen-Hayro-geometriediagnose**, geen DynLex-pariteit.
Rotatie moet in PageSpace, beeldsampling en clipfase opnieuw worden
berekend; een bitmapcache mag deze scherpe PDF-render niet vervangen.

### Geordende beeld- en tekstlagen van de oorspronkelijke pagina

De bestaande voorbereide DynLex-compositor kan nu over een expliciet
gevalideerde, volledig opake RGBA-achtergrond replayen. Afmetingen en
alfakanaal worden vóór de kerneloproep gecontroleerd; de gewone
witpapierroute blijft ongewijzigd. Een unit-test bewaakt laagvolgorde,
achtergrondbehoud en foutafwijzing. De aparte onderzoeksbinary
`hybrid_layer_probe` leest uitsluitend een gevalideerde PWPG-tekstscene en
de eerder bewezen JPEG-beeldbitmap. Hij parseert zelf geen PDF.

`scripts/audit_tekst_page5_hybrid_layers.py` gebruikt de twee hierboven
gedocumenteerde isolaties van **dezelfde oorspronkelijke pagina 5**: eerst
het beeld, daarna alle Type1-tekstpaints in oorspronkelijke volgorde.
De tekstscene komt uit de vertrouwde workerbinary; in deze onderzoeksreplay
wordt de worker rechtstreeks aangeroepen, niet via de begrensde app-launcher.
Het samengestelde resultaat is op 25/87/100/115/156,25/200% over **alle
19.191.004 RGBA-bytes exact** gelijk aan Hayro's render van de **volledige,
ongewijzigde originele pagina 5**. De zes gevalideerde tekstscenes zijn
47.292–63.068 bytes. PDF-/binary-/scene-/pixelhashes en iedere vergelijking
staan buiten Git in
`~/.cache/opds-benchmarks/type1-page-clip-2026-10-09/hybrid-layers/tekst-page5-hybrid-layer-audit-2026-10-09.json`.

Dit bewijst compositie en laagvolgorde voor deze pagina, maar de gewone
DynLex-dispatcher parseert het ongewijzigde hybride PDF nog **niet**. De
beeldfrontend blijft een niet-geïsoleerde vertrouwde onderzoeksproef, en
handmatige PDF-isolatie is geen productiepad. De volgende stap is de
JPEG-resource, `cm`, clip en tekst in één resource-begrensde workerpassage
te stageren, een versieerde image+text-IPC door de ouder volledig te laten
valideren en pas dan de originele pagina native strikt te accepteren.
Overige beeldplaatsing, rotatie, CropBox en meerdere geordende beelden
blijven unsupported tot afzonderlijke pixeltests slagen.

Workspace-allfeaturetests, formattering, de Albany-audit (135/147 exact,
12 unsupported, nul fouten), bestaande native tekst-/clipaudits en de oude
volledige v2/v4-verifier (47 PDF's/4.964.352 pixels plus drie
v4/147.456) slagen. Geen gebruikersvensters, bron-PDF's of onopgeslagen
werk zijn gewijzigd.

### Native strikte hybride route (11 oktober)

De bewezen beeldsampling is uit de onderzoeksbinary gehaald naar
`opdl-adapter/src/partial_image.rs`. De begrensde Linux-tekstworker leest nu
uit **één ongewijzigde PDF** maximaal één gevalideerde axis-aligned
DeviceRGB8-DCTDecode-beeldpaint vóór de tekstpaints. Zij verifieert de
XObject-resource, directe JPEG-lengte, paginavoorouders, één rechthoekclip,
`q/cm/Do/Q`-volgorde en alle decode-, bron- en uitvoergrenzen. Andere
beeldvolgorde, CTM, rotatie, CropBox, annotaties, maskers en filterketens
blijven unsupported. De worker draait binnen de bestaande tijd- en
geheugenlimieten; de PDF-parser staat niet in het native hoofdproces.

`PHYG` v1 verpakt een volledig opake beeldbitmap en de eerdere `PWPG`-
tekstscene. De ouder controleert versie, exacte lengtes, reservelden,
afmetingen, alle alpha's, de volledige PWPG/OPNW-geometrie en een gezamenlijke
32 MiB IPC-/scenelimiet vóór compositie. Oude tekstscenes blijven `PWPG`.
De enige toegestane schildervolgorde is beeld eerst en daarna geordende
glyph-/rechthoekpaints. De app kan dus geen gedeeltelijk beeld uit een
afgebroken worker gebruiken.

De **ongewijzigde oorspronkelijke** `Tekst.pdf` pagina 5 rendert nu via
`backend_probe --dynlex-strict`, zonder Hayro-fallback, op
25/87/100/115/156,25/200%. Alle zes volledige RGBA-buffers zijn exact
gelijk aan Hayro: **19.191.004 bytes, nul afwijkende kanalen**. Reproduceer
met `scripts/audit_native_hybrid_page.py`; rapport met PDF-/binary-/pixelhashes
staat buiten Git onder
`~/.cache/opds-benchmarks/type1-page-clip-2026-10-09/native-original/native-hybrid-page-audit.json`.
De vijf pagina's van `Tekst.pdf` bij 25/100/200% geven nu 3 exact,
12 unsupported en nul procesfouten. Albany blijft 135/147 exact,
12 unsupported. Acht bestaande beeldtransformatie-/paginarandafwijzingen
en vier nieuwe resource-/CTM-/dubbelbeeldmutaties produceren geen bitmap.
De volledige workspace-allfeaturetests en hybride IPC-/workerfouttests slagen.

De Rust-app is afzonderlijk geopend met de ongewijzigde `Tekst.pdf`, pagina
5/5 en `Renderer: DynLex`; de beeld- en tekstlaag en tekstminiatuur zijn
zichtbaar. Het oudere Albany-venster bleef open en onaangeraakt. De volledige
gepinde zeven-PDF-audit (373 pagina's × 25/100/200%) geeft **195 exact,
924 unsupported en nul verkeerde bitmaps of procesfouten**; 60 exacte
gevallen zijn lege NPS-pagina's. Het corpusrapport staat buiten Git onder
`~/.cache/opds-benchmarks/type1-page-clip-2026-10-09/native-original/public-full-audit-2026-10-11.json`.

Dit ontsluit één extra echte hybride pagina; het bewijst geen algemene
beeldplaatsing, PDF-objectdekking, pixelperfecte hele UI of snelheidswinst.
De eerste Ctrl+scroll-latentie is in deze UI-proef niet kwantitatief gemeten.

### Beperkt hergebruik bij terugzoomen (11 oktober)

De tekst/hybride scenecache bewaart nu maximaal vier recente pagina-/zoomscenes
binnen **één gezamenlijke limiet van 32 MiB**. De bronbytes blijven aan het
document gebonden. Gelijktijdige aanvragen blijven onder dezelfde lock
geserialiseerd; de worker-/IPC-validatie en strikte afwijzing veranderen niet.
Bij de echte pagina 5 en de reeks 100→87→115→100%, tweemaal doorlopen in
één proces, daalde het aantal workerpreparaties van zeven naar **drie**;
alle acht volledige RGBA-resultaten bleven exact. De drie bewaarde scenes
beslaan 8.769.820 bytes. De zes afzonderlijke originele-paginaaudits bleven
bytegelijk en de native tekstcache-/fouthersteltests slagen.

Dit verkort alleen een terugkeer naar een recente schaal. **Een nieuwe
zoomstand start nog steeds een worker.** Tien afwisselende nieuwe
procesparen op pagina 5/100% met twee replays en bytecontrole gaven als
mediaan DynLex/Hayro 110,53/46,88 ms voor de eerste render en 6,58/32,40 ms
voor de tweede replay. DynLex-voorbereiding apart was in een diagnostische
run 82,07 ms; die run is niet gepaard. OS-cache was warm en de interne caches
verschillen. GNU time mat alleen de ouderproces-RSS, niet de piek van de
worker, dus hieruit volgt geen totale geheugen- of algemene snelheidsclaim.
Ruwe samples en hashes staan buiten Git in
`~/.cache/opds-benchmarks/type1-page-clip-2026-10-09/native-original/hybrid-firstzoom-bench-2026-10-11.json`.

### Eerste nieuwe zoom: faseonderzoek en kolomhergebruik

Tien directe, vertrouwde lokale workerstarts op dezelfde originele pagina 5
bij 100% lokaliseren de kosten. De mediaan vóór kolomhergebruik was ongeveer
20,6 ms JPEG-decode, 5,7 ms RGB-downsampling en 10,6 ms RGBA-compositie;
lopdf-load was 7,2 ms. Deze fasen zijn diagnostisch, buiten de native
OS-resource-launcher gemeten, en horen niet als gepaarde totale latentie te
worden gelezen. Ruwe samples:
`~/.cache/opds-benchmarks/type1-page-clip-2026-10-09/native-original/hybrid-worker-phases-2026-10-11.json`.

De beeldcompositor berekent de horizontale clipdekking en f32-stripfase nu
één keer per uitvoerkolom, in plaats van op iedere rij. De oorspronkelijke
256-pixel-reset, f32-optelling en 8-bit-compositie zijn behouden. Dezelfde
zes volledige originele-pagina's blijven exact; ook twaalf beeldplaatsing-/
clipvarianten (40.547.920 bytes) zijn exact en acht ongeldige paginastaten
worden zonder bitmap afgewezen. In een nieuwe afzonderlijke tien-run
diagnose was RGBA-compositie 6,2 ms, terwijl JPEG-decode ongeveer 19,9 ms
bleef. De nieuwe gepaarde totale eerste render was nog 110,47 ms voor
DynLex tegenover 44,51 ms voor Hayro: **geen aangetoonde verbetering van
de ervaren eerste zoom**. De fase-runs vóór/na waren niet gepaard; het
praktische vervolg is JPEG-decode en herhaalde workeropstart/-voorbereiding
met behoud van de strikte bron- en IPC-grenzen. Ruwe fase- en paarmetingen
staan in `hybrid-worker-phases-after-columns-2026-10-11.json` en
`hybrid-column-paired-2026-10-11.json` in dezelfde externe map.

Een aanvullende procesboommeting telt de gelijktijdig levende ouder en
tekstworker wél samen. `scripts/measure_process_tree_rss.py` samplet elke
milliseconde `/proc/*/statm` en de directe kinderen. Tien afwisselende
processenparen op de originele pagina 5/100%, telkens met volledige
RGBA-bytecontrole, gaven mediane **gesamplede sompieken** van 26.576 KiB
voor DynLex en 19.438 KiB voor Hayro; de DynLex-ouder alleen piekte rond
11.620 KiB en zijn worker rond 19.012 KiB. Dit is een ondergrens tussen
samples, terwijl gedeelde pagina's dubbel geteld kunnen zijn; het is geen
PSS of exacte gecombineerde piek. De wrapperlooptijd van 121,62/55,94 ms
bevat ook Python-start en bestandsschrijven en is geen rendertijd. Het
procesrapport en alle tien pixelhashparen staan buiten Git in
`~/.cache/opds-benchmarks/type1-page-clip-2026-10-09/native-original/hybrid-tree-rss-2026-10-11.json`.
De eerder gerapporteerde GNU-time ouder-RSS mag dus niet als totaal van de
DynLex-route worden gelezen.

Bij de overige oorspronkelijke `Tekst.pdf`-pagina's is de oude generieke
`DictKey`-afwijzing nader gelokaliseerd: pagina's 1, 3 en 4 selecteren een
TrueType-font **zonder `/Encoding /WinAnsiEncoding`**. Deze oude generieke
afwijzing is hieronder vervangen door een expliciet begrensde symbolische
mapping; de oorspronkelijke pagina's blijven wegens een latere operator
zonder bitmap.

### Begrensde Macintosh-symbolfontmapping

De daadwerkelijke tweede font in die pagina's is een ingebedde symbolische
TrueType-subset met een Macintosh-platform/encoding-0 **format-0 cmap**.
PDF-code 1 wijst daarin naar GID 1. De worker kan nu uitsluitend deze
expliciete mappingvorm zonder `/Encoding` lezen wanneer de fontdescriptor
de symbolische vlag heeft; andere encodings of cmapvormen blijven
unsupported. De bestaande TrueType-outline-, breedte- en glyphgrenzen
gelden ook hiervoor. `scripts/audit_symbolic_truetype.py` maakt 24 volledige
fontfixture-PDF's uit het oorspronkelijke ingebedde font: vier rotaties,
twee CropBoxes en 25/100/200%. Alle 24 native strikte RGBA-buffers zijn
bytegelijk aan Hayro; drie ongeldige vlag-/encoding-/codevarianten leveren geen
bitmap. Bron-, binaire- en pixelhashes staan buiten Git onder
`~/.cache/opds-benchmarks/type1-symbolic-2026-10-11/`.

Dit geeft nog **geen volledige nieuwe oorspronkelijke pagina**: pagina's
1, 3 en 4 stranden vervolgens al op `Tr 2` (gevulde én gestrookte tekst),
waarvoor de DynLex-tekstscene nog geen bewezen stroke-compositor heeft.
De oorspronkelijke `Tekst.pdf` blijft 3/15 exact op drie schalen. De
symbolische fontproeven zijn geen bewijs voor algemene TrueType-encoding
of willekeurige PDF-tekststroke.
De volledige zeven-PDF-audit na de mapping bleef 195/1.119 exact,
924 unsupported en nul verkeerde bitmaps/procesfouten. Dit is dus
nieuwe gevalideerde **fontsubset**, geen extra complete externe pagina.
Het ruwe corpusresultaat staat naast de fontfixtures onder
`~/.cache/opds-benchmarks/type1-symbolic-2026-10-11/public-full.json`.

### Onderzoek naar RGB-hergebruik bij een nieuwe schaal

Een afzonderlijke onderzoeksroute in `partial_image_probe --cache-research`
decodeert het echte ingebedde JPEG uit de geïsoleerde pagina-5-beeldlaag één
keer naar 6.524.880 RGB-bytes en gebruikt die bron voor 25/87/100/115/
156,25/200%. Alle zes volledige buffers (19.191.004 RGBA-bytes samen) zijn
bytegelijk aan de bestaande route die het JPEG voor iedere schaal opnieuw
decodeert. Na deze refactor blijft de native strikte route van de
**ongewijzigde** volledige pagina 5 op dezelfde zes schalen bytegelijk aan
Hayro. De twaalf eerder bewezen plaatsing-/clipvarianten blijven eveneens
exact; acht ongeldige beeldpaginas worden afgewezen.

Dit is nog **geen native cache of bewezen eerste-zoomsnelheidswinst**. De
onderzoeksprobe vertrouwt zijn ene PDF en bindt de hergebruikte RGB-buffer
niet cryptografisch of via gevalideerde IPC aan de oorspronkelijke JPEG.
De applicatie blijft voor iedere nieuwe schaal een resource-begrensde worker
starten en het JPEG daarin opnieuw decoderen. Productie-integratie vergt een
brongebonden, versieerde overdracht of een gelijkwaardig begrensd ontwerp,
inclusief totale cache-/IPC-/RSS-limieten en foutafwijzing. De losse
onderzoekslooptijden sluiten PDF-load, eerste decode en workerstart uit en
zijn daarom geen GUI- of totale renderbenchmark. Reproduceerbare buffers en
een `timings.tsv` staan buiten Git onder
`~/.cache/opds-benchmarks/type1-page-clip-2026-10-09/rgb-reuse-research-2026-10-11/`.

### Procesproef met RGB-overdracht (11 oktober)

Een tijdelijke kandidaat bond de 6.524.880 RGB-bronbytes via SHA-256 aan
het geselecteerde JPEG en verstuurde ze bij iedere nieuwe schaal via
begrensde, opnieuw gevalideerde worker-IPC. De oorspronkelijke volledige
pagina 5 bleef op alle zes schalen bytegelijk aan Hayro. De totale
procesroute werd echter **trager**; de kandidaat is uit productiecode
teruggedraaid. In zes afwisselende oude/nieuwe procesparen voor
100→87→115→100%, met warme OS-cache, gelijke PDF en iedere output
bytegecontroleerd, waren de medianen in milliseconden:

| Stap | Bestaande route | RGB-overdracht |
|---|---:|---:|
| Eerste 100% | 107,36 | 152,97 |
| Nieuwe 87% | 74,92 | 125,78 |
| Nieuwe 115% | 91,32 | 138,09 |
| Terug naar 100% | 8,02 | 3,82 |

De 6,5 MB tussen ouder en iedere nieuwe worker overdragen, hashen en
kopiëren kost hier meer dan opnieuw decoderen. Dit bewijst geen algemene
eigenschap van alle JPEG's of apparaten; de twee varianten hebben bewust
verschillende interne caches. Ruwe samples staan buiten Git onder
`~/.cache/opds-benchmarks/rgb-cache-2026-10-11/paired.json`; de oude
releasebinaries staan in `rgb-cache-before-2026-10-11`. Een volgend
ontwerp moet de bron veilig **binnen** een begrensde worker hergebruiken
of de overdracht vermijden, met eigen tijd-, RSS- en pixelcontrole.

### Afzonderlijke beeld-/tekstthreads binnen dezelfde worker

Een tweede kandidaat liet het bestaande JPEG-paint parallel lopen met de
Type1-tekstvoorbereiding in dezelfde begrensde worker. De oorspronkelijke
pagina 5 bleef op zes schalen volledig bytegelijk aan Hayro. Vijftien
afwisselende procesparen op 100→87→115→100%, ieder resultaat opnieuw
bytegecontroleerd, gaven echter deze medianen in milliseconden:

| Stap | Bestaande worker | Parallelle kandidaat | Kandidaat sneller |
|---|---:|---:|---:|
| Eerste 100% | 106,66 | 111,78 | 7/15 |
| Nieuwe 87% | 75,19 | 79,17 | 5/15 |
| Nieuwe 115% | 87,10 | 93,33 | 3/15 |
| Terug naar 100% | 8,68 | 8,09 | 9/15 |

Parallel uitvoeren binnen één proces geeft op deze machine geen stabiele
winst voor een nieuwe zoom en is daarom teruggedraaid. De release-helper is
na terugdraaien byte-identiek aan de vooraf bewaarde binary. De proeven
gebruiken warme OS-cache en de bestaande scene-LRU; ze bewijzen niets over
GUI-latentie of andere PDF's. Ruwe samples en binaries staan buiten Git
onder `~/.cache/opds-benchmarks/parallel-image-2026-10-11/` en
`parallel-image-before-2026-10-11/`.

Een kleinere workerpoll-proef verkortte uitsluitend de slaap tussen
`waitid`-controles van 5 naar 1 ms. Vijftien afwisselende verse
procesparen op de ongewijzigde pagina 5/100%, met identieke volledige
RGBA-hashes, gaven voor de gemeten renderfase 107,071 versus 104,803 ms
mediaan en 9/15 winsten; de totale probe-medians waren 110,042 versus
109,205 ms en 8/15 winsten. Dit is geen stabiel aantoonbare verbetering
van de eerste zoom. De kandidaat is teruggedraaid; het ruwe paarrapport
en beide proefbinaries staan buiten Git onder
`~/.cache/opds-benchmarks/worker-poll-2026-10-12/`.

Een ouderprocesdiagnose met `OPDS_TEXT_PARENT_PHASES=1` meet bij de
ongewijzigde pagina 5/100% de workerstart, wachten plus scene-overdracht,
en PHYG/PWPG-hervalidatie afzonderlijk. Vijf verse, ongepaarde processen
met volledige RGBA-bytecontrole bij 100→87→115→100% gaven medianen van
24,007 ms voor de eenmalige generieke DynLex-sceneprobe (daarna 0,003 ms
uit de page-slot-cache), 1,700 ms voor workerstart, 66,333 ms voor
wachten/overdracht en 3,436 ms voor ouderhervalidatie. De totale
renderfasen waren 104,942/79,927/89,224 ms voor de drie nieuwe
schalen en 8,485 ms bij terugkeer naar 100%. Dit is een **faselokalisatie**
met warme OS-cache, geen gepaarde snelheidsvergelijking of GUI-meting.
De eerste *nieuwe* schaal na een al geopende pagina zit hoofdzakelijk in
de workerfase; korter pollen is hierboven onvoldoende gebleken. De ruwe
samples, uitvoerhashes en binaryhash staan buiten Git in
`~/.cache/opds-benchmarks/worker-poll-2026-10-12/parent-sequence/generic-profile.json`.

### De volgende originele tekstpagina's vereisen echte tekststroke

De pagina's 1–4 bevatten respectievelijk 7, 6, 8 en 3 actieve `Tr 2`
(vulling plus stroke); pagina 5 bevat er geen. Een afzonderlijke
onderzoeks-PDF verving alleen deze operators door `Tr 0` en werd in beide
varianten met Hayro gerenderd. Op 25/100/200% veranderen op pagina 1
909/7.686/17.781 RGBA-kanalen, op pagina 2
1.332/10.698/25.623, op pagina 3
1.557/12.834/29.916 en op pagina 4 390/3.498/8.517.
Pagina 5 bleef op alle drie schalen bytegelijk, wat de herschrijfproef
controleert. De `Tr 2`-stroke is dus ook bij 25% zichtbaar en mag niet als
inert worden behandeld. Het originele document blijft voor deze vier
pagina's strikt unsupported; volledige ondersteuning vergt een begrensde
stroke-paint met pixelproeven, daarna nog hun overige operators/resources.
De gewijzigde PDF en ruwe RGBA-buffers staan buiten Git onder
`~/.cache/opds-benchmarks/tekst-stroke-2026-10-11/`.

Een read-only operatorcontrole van de oorspronkelijke pagina's 1–4
lokaliseert alle 24 `Tr 2`-gevallen verder: direct na ieder geval staat
`0.36666 w`, gevolgd door `/F1 11 Tf` en één `TJ`-paint vóór `ET`.
`/F1` is telkens het ingebedde Type1-font UniviaProRegular. De volgende
gerichte pixelproef kan daardoor met deze echte glyphoutlines, stroke-
breedte en paintvolgorde beginnen; dit activeert nog geen tekststroke en
verandert de strikte afwijzing van de originele pagina's niet.

### Expliciet neutrale paginageometrie (11 oktober)

De JPEG+Type1-worker accepteert nu ook expliciete `/Rotate 0`, een
`/CropBox` die numeriek exact gelijk is aan de MediaBox, en `/UserUnit 1`.
Deze waarden mogen direct op de pagina of in haar begrensd doorlopen
voorouderketen staan. Andere beeldrotaties, CropBoxes en UserUnits blijven
unsupported; met name wordt geen eerder gerenderde bitmap gedraaid.

Zeven gecontroleerde varianten van de **oorspronkelijke** Tekst.pdf-pagina 5
(de drie afzonderlijke neutrale waarden, hun combinatie, en de drie
geërfde varianten) zijn elk op 25/87/100/115/156,25/200% via de normale
strikte native route volledig bytegelijk aan Hayro: 42 complete buffers,
134.337.028 vergeleken RGBA-bytes. Acht directe/geërfde niet-neutrale of
onveilige varianten (`Rotate 90`, afwijkende CropBox, `UserUnit 2`, Group,
Annots) werden zonder bitmap geweigerd. De onveranderde pagina 5 bleef
op zes schalen exact en Albany bleef 135/147 exact, 12 unsupported,
zonder fout. Dit ontsluit nog geen nieuwe originele documentpagina of
algemene beeldgeometrie.

PDF's, hashes, pixelrapporten en de Albany-matrix staan buiten Git in
`~/.cache/opds-benchmarks/neutral-image-state-2026-10-11/`. Reproduceer
een variant met `scripts/audit_native_hybrid_page.py --pdf <variant.pdf>
--page 5 --backend target/release/backend_probe --output-dir <auditmap>`.

### Gepaarde eerste render met ouder- en worker-RSS (12 oktober)

Vijftien afwisselende verse-procesparen op de **ongewijzigde** Tekst.pdf-
pagina 5/100%, na één opwarmrender per engine met warme OS-cache. Iedere
DynLex- en Hayro-RGBA-uitvoer had dezelfde SHA-256 per paar. Met dezelfde
probe-binary waren de medianen voor de eerste renderfase **112,169 ms
DynLex / 47,114 ms Hayro**; inclusief processtart en bestandsuitvoer
**121,934 / 55,578 ms**. DynLex won 0/15 paren. Deze meting bevat geen
GUI-responstijd en vergelijkt geen gelijkgetrokken interne scenecaches.

Een 1ms-`/proc`-sampler zag de DynLex-worker in alle vijftien paren.
Mediane afzonderlijke pieken waren 11.556 KiB voor de ouder en 18.864 KiB
voor de worker; de mediaan van de **tegelijk** gesamplede ouder+worker-RSS
was 26.428 KiB tegenover 19.284 KiB voor het enkele Hayro-proces. Dit is
een gesamplede ondergrens van de tijdpiek, geen PSS: gedeelde pagina's
kunnen dubbel tellen en kortlevende pieken kunnen gemist zijn. De ruwe
samples, proces-/bronnenhashes, volledige pixelhashes en volgorde staan
buiten Git in
`~/.cache/opds-benchmarks/heartbeat-rss-2026-10-12/paired-time-rss.json`.
Dit specifieke eerste-hybride-zoomgeval levert dus nog geen snelheids- of
geheugenvoordeel op; het zegt niets algemeens over andere PDF's of het
snelle terugzoomen uit de scenecache.
Een vierwegsvergelijking op precies deze pagina is lokaal niet valide:
PDFium-strict kon `libpdfium.so` niet laden en de eigen-Rust-kernel wees
pagina 5 als buiten zijn begrensde subset af. Beide krijgen daarom geen
tijd- of RSS-score voor deze pagina.
