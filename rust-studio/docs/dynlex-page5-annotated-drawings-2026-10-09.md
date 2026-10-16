# Albany pagina 5 en openbare tekeningen met commentaar

Deze proef is **geen volledige pagina-5-ondersteuning**. De originele Albany-pagina
5 blijft in strikte DynLex zonder bitmap; er is geen Hayro-fallback toegevoegd.
De geopende native app en gebruikersdocumenten zijn niet herstart of gewijzigd.

## Pagina 5: eerste echte blocker en onafhankelijke delen

De originele pagina bevat 956 contentoperators: ingebedde WinAnsi TrueType-tekst,
rechthoekvullingen, negen onzichtbare Link-annotaties en één zichtbaar
225×225 DeviceRGB8/FlateDecode-beeld. Dat beeld staat onder een fractionele
`re W n`-clip, een lokale `cm` en `/GS0 gs`. De named graphics state is
aantoonbaar neutraal voor de ondersteunde opaque fills: normale blending,
volledige alpha, geen soft mask of overprint. De twee frontends accepteren nu
alleen zulke volledig gevalideerde named states; alpha, andere blendmodes,
soft masks, overprint en onbekende entries worden geweigerd.

De beelddecoder verwerkt nu ook `DecodeParms` met de PDF-standaardwaarde
`Predictor=1` wanneer de sleutel ontbreekt. Een synthetische 2×2-RGB-test
controleert exacte Flate-decode en afwijzing van Predictor 2. Dit is een
generieke beeldvoorwaarde, geen documentnaam- of resourcewhitelist.

Twee **onderzoeksisolaties buiten Git** behouden de oorspronkelijke pagina-
resources en contentlexemen, maar bevatten respectievelijk alleen de tekst
of alleen de originele beeld-/clip-/gs-/cm-operators. De tekstisolatie is op
25/50/87/100/115/200% volledig exact gelijk aan Hayro. Bij 100% levert de
strikte DynLex-beeldroute 1.938.816 RGBA-bytes exact gelijk aan Hayro; op
25/50/87/115/200% blijft de beeldroute nog unsupported.
Bij 100% hebben de zichtbare tekst- en beeldinktpixels geen overlap; een
diagnostische witte-papier-unie van beide onderzoeksbuffers is toevallig
ook exact gelijk aan de originele Hayro-pagina. **Deze unie is niet in de
productieroute gezet**: zij bewijst geen correcte paintorder, witte pixels
binnen het beeld, overlappende paints of andere schalen. De volledige originele
pagina strandt nu veilig op de nog niet geïmplementeerde gecombineerde `cm`
en `Do`-paint in de tekstscene.

Volgende stap is een begrensde geordende hybride scene met expliciete
beeldresource-, CTM-, clip- en alphalengtevalidatie over de worker/IPC-grens.
Zij moet ook bij overlap de PDF-paintorder bewaren en per pixel gelijk zijn
op 25/100/200% en vrije zoom voordat pagina 5 zichtbaar mag worden. Een
bitmap uit de onderzoeksisolatie wordt niet als vervangende pagina gebruikt.

## Negen openbare PDF's, 54 tekeningen

De oorspronkelijke bestanden staan buiten Git onder
`~/.cache/opds-benchmarks/drawings-2026-10-09/`. Bron en SHA-256 zijn gepind
in `manifest.json`. De reproduceerbare
`scripts/prepare_public_annotated_drawings.py` bewaart de originelen en maakt
afzonderlijke **synthetisch becommentarieerde** kopieën met `/Text`,
`/FreeText`, `/Highlight` en `/Line` op pagina 1–2. Die opmerkingen zijn
testdata van Open PDF Studio, geen werkelijke NPS-reviews.
`scripts/catalog_public_drawings_text.py` maakt daarnaast een doorzoekbare
tekstcatalogus van alle **54** tekeningbladen en een apart bestand met alle
direct extraheerbare tekst. De Woodson-PDF heeft alleen
een gedrukte tekstindex: zijn 38 tekeningpagina's hebben geen selecteerbare
PDF-tekst, omdat zichtbare letters op onderzochte bladen als vectorcontouren
zijn opgenomen. Een steekproef van pagina's 4, 5, 6, 20 en 41 toont duizenden
tot tienduizenden `m/l`-padoperatoren en geen `Tj/TJ`; pagina 33 bevat daarnaast
ingebedde beelden. De catalogus geeft daarom de originele
bladtitels en paginanummers, geen verzonnen OCR. De Yosemite-figuuronderschriften
zijn wel direct uit de pagina's te extraheren. De zeven Caltrans-brugdetails
zijn echte vector-PDF's met selecteerbare tekst op de tekening; ze bevatten
geen ingebedde paginabeelden. `drawings-text-index.md` geeft een doorzoekbare
bladlijst en `drawings-full-extracted-text.md` de direct extraheerbare tekst.
`scripts/ocr_woodson_drawings.py` rendert de 38 bladen afzonderlijk met
Poppler op 120 dpi en gebruikt een lokaal uitgepakt Tesseract 5.3.4 English
(`--psm 11`) voor machine-OCR. De Ubuntu-pakketten staan uitsluitend in de
externe testcache, niet in de app of Git. `woodson-ocr-all-38.md` bevat die
53.493 onbevestigde tekens; `drawings-all-54-text-with-ocr.md` combineert die met de
direct extraheerbare Yosemite-/Caltrans-tekst. OCR van technische maten en
labels kan fouten bevatten en is **geen** PDF-object- of DynLex-tekstdekking.
Het gecombineerde bestand heeft precies 54 per-tekeningsecties. De OCR kan
met `scripts/ocr_woodson_drawings.py --combine-only` zonder opnieuw renderen
in de gecombineerde catalogus worden verwerkt.

| Bron | Pagina's | Originele `/Annots` | Tekensets |
| --- | ---: | ---: | --- |
| [NPS Carter G. Woodson-tekeningen](https://parkplanning.nps.gov/showFile.cfm?projectID=43543&sfid=139526) | 41 | 0 | architectuur en constructie |
| [NPS Yosemite-schematische tekeningen](https://parkplanning.nps.gov/showFile.cfm?projectID=27563&sfid=112832) | 12 | 0 | schematische tekeningen |
| [Caltrans XS16-034 brugdetail](https://dot.ca.gov/-/media/dot-media/programs/engineering/documents/bridgestandarddetails/chap-16/2024-xs16-034-a11y.pdf) | 1 | 0 | vector en selecteerbare tekst |
| [Caltrans XS16-010 brugdetail](https://dot.ca.gov/-/media/dot-media/programs/engineering/documents/bridgestandarddetails/chap-16/202504-xs16-010-a11y.pdf) | 1 | 0 | vector en selecteerbare tekst |
| [Caltrans XS16-020](https://dot.ca.gov/-/media/dot-media/programs/engineering/documents/bridgestandarddetails/chap-16/202504-xs16-020-a11y.pdf) | 1 | 0 | vector en selecteerbare tekst |
| [Caltrans XS16-025](https://dot.ca.gov/-/media/dot-media/programs/engineering/documents/bridgestandarddetails/chap-16/202607_xs16-025-a11y.pdf) | 1 | 0 | vector en selecteerbare tekst |
| [Caltrans XS16-026](https://dot.ca.gov/-/media/dot-media/programs/engineering/documents/bridgestandarddetails/chap-16/202504-xs16-026-a11y.pdf) | 1 | 0 | vector en selecteerbare tekst |
| [Caltrans XS16-027](https://dot.ca.gov/-/media/dot-media/programs/engineering/documents/bridgestandarddetails/chap-16/202504-xs16-027-a11y.pdf) | 1 | 0 | vector en selecteerbare tekst |
| [Caltrans XS16-030](https://dot.ca.gov/-/media/dot-media/programs/engineering/documents/bridgestandarddetails/chap-16/202504-xs16-030-a11y.pdf) | 1 | 0 | vector en selecteerbare tekst |

De afgeleide PDF's zijn met pypdf opnieuw ingelezen. Er zijn aparte
**drawing-comments**-kopieën met alle vier de opmerkingen op een echt blad:
Woodson-pagina 4, Yosemite-pagina 3 en alle Caltrans-PDF's op pagina 1. De eerdere
cover-pageproeven blijven als afzonderlijke kopieën bestaan. Die hebben bij NPS
één annotatie op pagina 1 en drie op pagina 2; de eenpagina-Caltrans-kopieën
hebben alle vier op pagina 1. Poppler opent en rendert pagina 2 van beide
NPS-kopieën. Hayro rendert voor elk NPS-origineel en elke kopie pagina 1–2
op 25% zonder procesfout. Op 100% verschillen de NPS-kopieën op pagina 2
8.544 RGBA-kanalen van hun origineel; de Text-popup zonder `/AP` op pagina 1
geeft in deze Hayro-route geen paginapixelverschil. De Caltrans-originelen
en -kopieën renderen op 100% elk 3.877.632 RGBA-bytes; de commentaarkopieën
verschillen respectievelijk 18.824 en 18.891 kanalen van hun origineel.
Annotatieobjectdekking en zichtbare appearance moeten daarom afzonderlijk
worden gemeten. Ruwe manifest-, audit- en appearanceverschillen staan bij de
PDF's buiten Git.

Een strikte DynLex-steekproef van pagina 1, 2 en de laatste pagina van
ieder origineel en iedere kopie bij 25/100% gaf **0/24 ondersteund**, 24
veilige afwijzingen en nul fouten of vervangende bitmaps. De Woodson-set
strandt bij huidige resourcevalidatie (`DictKey`); Yosemite heeft fonts buiten
de bewezen embedded TrueType/Identity-H-subset. Dit zijn nieuwe, eerlijk
zichtbare corpusdoelen; er is geen pixel- of snelheidsclaim voor DynLex op
deze tekeningen.

Een afzonderlijke strikte steekproef van beide Caltrans-originelen en beide
cover-pagecommentaarkopieën op 25/100% geeft **0/8 ondersteund**, acht
veilige afwijzingen. De originelen vereisen fontsoorten buiten de bewezen
DynLex-tekstsubset en bevatten meer dan 100.000 contentoperators per blad;
de kopieën worden al op hun zichtbare annotaties afgewezen. Het zwaarste
vectorblad XS16-010 is in een aparte native Rust-dev-instantie geopend met
Hayro als zichtbare renderer; de bestaande Albany-instantie blijft staan.
DynLex kan linksonder worden geselecteerd, maar toont voor dit blad terecht
nog geen vervangende bitmap. De audit staat in `caltrans-strict-audit.json`
naast de corpus-PDF's.

## Vier-kernelmeting op tekenbladen

`scripts/bench_annotated_drawings.py` neemt negen originelen plus negen
drawing-comments-kopieën. Per PDF wordt een werkelijk tekenblad bij 25% en
100% getest: 36 gevallen. De `backend_probe` is in release met `kernel,pdfium,own-rust`
gebouwd. Alle vier routes draaien strikt, zonder vervangende renderer:
Hayro **36/36**, PDFium **36/36**, DynLex **0/36** en Eigen Rust **0/36**.
Geen procesfouten. PDFium en Hayro hebben in alle gelijkgedimensioneerde
gevallen pixelverschillen, dus PDFium is hier geen exact Hayro-equivalent.
PDFium heeft in 6/36 gevallen een lagere mediane procestijd; de overige 30
vallen in het voordeel van Hayro uit. Dit is geen algemene kernelsnelheidsranglijst.

Drie nieuwe processen per geslaagd geval, afwisselende backendvolgorde en
warme OS-cache; tijden omvatten processtart, parsen, renderen en RGBA-schrijven.
Dit meet **geen interactieve eerste zoom** en unsupported krijgt geen tijdscore.
Voorbeelden van de originele bladen op 100% (mediaan wandtijd / proces-piek-RSS):

| Tekening | Hayro | PDFium | Verschillende RGBA-pixels |
| --- | ---: | ---: | ---: |
| Woodson AS0.01, vectorcontouren | 114,24 ms / 44,6 MiB | 117,59 ms / 41,1 MiB | 210.847 |
| Yosemite B-1 | 432,07 ms / 262,5 MiB | 142,50 ms / 41,1 MiB | 244.586 |
| Caltrans XS16-034, vector/tekst | 72,40 ms / 17,7 MiB | 104,61 ms / 19,1 MiB | 80.673 |
| Caltrans XS16-010, zware vector/tekst | 97,61 ms / 16,8 MiB | 121,79 ms / 20,5 MiB | 86.509 |

Volledige per-gevalmetingen, hashes, afwijsredenen en de commentaarkopieën
staan buiten Git in `four-kernel-benchmark.json` naast de PDF's. De Caltrans-
originelen gebruiken allemaal **niet-ingebedde `/Type1 /Times-Roman`**.
Hun actuele eerste workerafwijzing ligt echter al bij de contentgrenzen,
vóór de fontoperatoren (zie hieronder). Zichtbare testannotaties worden
eveneens afgewezen. Dit maakt content-, font-, pad- en annotatiedekking concrete
volgende doelen. Een aparte UI-aanpassing verkort uitsluitend de wachttijd voordat een
enkele zoomstap scherp begint te renderen van 160 naar 60 ms (later 24 ms); herhaalde stappen
blijven 160 ms samengevoegd. Die verandering is unit-getest, maar er is nog
geen gemeten GUI-latentiewinst of snellere DynLex-rasterkernel uit af te leiden.

Voor die fontstap is de huidige Hayro-testoracle concreet: de lokale
`hayro-interpret`-versie gebruikt voor Times-Roman een FoxitSerif Type1/PFB
fontbestand. Dat verklaart waarom willekeurige systeemfontsubstitutie geen
verdedigbare pixelpariteitsoplossing is. Een zelfstandige DynLex-stager zou
het standaardfont en zijn licentie, encoding, metrics en kubische outlines
expliciet moeten beheren, daarna de volledige vectorpagina bewijzen. Er is
nog geen standaardfont-rasterroute geactiveerd.

### Actuele eerste afwijzing van de zware vectorbladen (11 oktober)

Een herhaalde directe workerdiagnose bij 25% en een onafhankelijke
read-only `pypdf`-inventaris van de zeven gepinde originelen geven:

| Caltrans-blad | Gedecodeerde content | Operators | Eerste workerafwijzing |
|---|---:|---:|---|
| XS16-010 | 1.382.388 B | 129.195 | geaggregeerde contentlimiet |
| XS16-020 | 1.581.506 B | 149.014 | geaggregeerde contentlimiet |
| XS16-025 | 1.349.225 B | 126.196 | Flate-uitvoer boven streamlimiet |
| XS16-026 | 1.449.400 B | 136.508 | geaggregeerde contentlimiet |
| XS16-027 | 1.387.399 B | 130.158 | geaggregeerde contentlimiet |
| XS16-030 | 1.736.925 B | 164.639 | geaggregeerde contentlimiet |
| XS16-034 | 1.204.082 B | 111.178 | Flate-uitvoer boven streamlimiet |

De worker staat nu maximaal 1.000.000 gedecodeerde contentbytes en 16.384
operators toe. Zelfs na een veilige, vooraf begrote verruiming blijven dus
de operatorgrens, niet-ingebedde standaardfont en algemene paden afzonderlijke
blokkades. De limieten zijn niet verhoogd: dat zou geen correcte volledige
bitmap opleveren. De bestaande 0/36 DynLex-acceptatie op deze tekenbladen
blijft daarom ongewijzigd.

## Herhalen

```sh
python3 scripts/prepare_public_annotated_drawings.py
target/release/backend_probe \
  ~/.cache/opds-benchmarks/internet-2026-10-08/albany-contract.pdf \
  /tmp/albany-p5.rgba --dynlex-strict --scale=1 --page=5
```

De beeld-/tekstisolaties en de per-schaal-JSON staan in dezelfde externe
cachemap als de tekeningen. Zij zijn onderzoeksdata, geen nieuwe productroute.
