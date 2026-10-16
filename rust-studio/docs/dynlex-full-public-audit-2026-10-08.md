# Volledige openbare DynLex-audit, zeven PDF's

Dit is de nulmeting **vóór** de latere Type0/Identity-H-uitbreiding.
De [nieuwe native Type0-meting](dynlex-native-type0-2026-10-08.md)
komt op 174/1.119 exacte gevallen; de cijfers hieronder blijven als
vergelijkbare historische audit bewaard.

`scripts/audit_strict_dynlex_corpus.py` controleerde alle **373 pagina's**
van het gepinde openbare corpus op 25%, 100% en 200%: **1.119** strikte
pagina-/zoomgevallen. Een aanvaarde DynLex-bitmap is over de hele RGBA-buffer
met Hayro vergeleken; unsupported levert geen vervangende bitmap of
snelheidsscore. De bronhashes en de per-PDF-paginamatrix staan in
`dynlex-full-public-audit-2026-10-08.json`. Het volledige ruwe rapport met
afwijsredenen staat buiten Git in
`~/.cache/opds-benchmarks/internet-2026-10-08/dynlex-full-audit-2026-10-08.json`.

| Originele PDF | Pagina's | Exact / getest | Unsupported | Hoofdblokkade |
| --- | ---: | ---: | ---: | --- |
| attention | 15 | 0 / 45 | 45 | lexicale dictionarylimiet |
| albany-contract | 49 | 105 / 147 | 42 | annotatie/groep, fonts, paden/CTM |
| albany-drawings | 9 | 0 / 27 | 27 | bestaande adapter kan geen scene voorbereiden |
| nps-schematic | 142 | 0 / 426 | 426 | bestaande adapter kan geen scene voorbereiden |
| tfl-tube | 2 | 0 / 6 | 6 | object-/xref-decodegrens |
| nps-drawing-manual | 126 | 60 / 378 | 318 | resources, niet-ingebedde fonts |
| usgs-topo-guide | 30 | 0 / 90 | 90 | resources, annotatie/groep |

Totaal: **165/1.119 exact**, **954 unsupported**, **nul foutieve bitmaps of
procesfouten** en 569.284.848 RGBA-bytes vergeleken. De 60 successen uit
`nps-drawing-manual` zijn **20 lege pagina's op drie schalen**: elk heeft nul
gedecodeerde contentbytes en geen `/Annots`. Zij bewijzen dus geen dekking
voor de tekst of tekeningen van die handleiding. De 105 Albany-successen
omvatten de eerder bewezen JPEG- en tekstsubset; dit is geen bewijs voor
algemene PDF-dekking.

De audit legde een fout in de aanvullende tekst-preflight bloot. `/Size`
werd in **ieder** woordenboek als xref-grootte behandeld. In de NPS-PDF
gebruikt een sampled PDF-functie terecht `/Size [255]`; dat leidde tot een
misleidende xref-afwijzing. De preflight valideert de limiet nu alleen bij
een echte `trailer` of `/Type /XRef`-stream. Gerichte malformed-trailer-,
xref-stream- en geldige functie-`/Size`-tests slagen. Daarna blijven de
318 niet-lege NPS-gevallen strikt unsupported wegens werkelijke
`/ColorSpace`-/`/ExtGState`-resources, niet-ingebedde fonts of groepen.
Dit verandert de native inhoudsdekking niet en heft geen geheugenlimiet op.

Een aparte compositorproef beperkte de clipmaskervermenigvuldiging tot de
conservatieve glyphtegel. Alle 120 gerichte PDF's en 105 Albany-gevallen
bleven exact. Vijftien gepaarde procesdrietallen op Albany pagina 11 gaven
echter alleen bij 25% een consistente verbetering; op 100% en 200% was
geen overtuigend voordeel. Deze kandidaat is teruggedraaid. De werkende
fractionele cliproute blijft ongewijzigd; het meetprotocol en ruwe samples
staan buiten Git in
`~/.cache/opds-benchmarks/internet-2026-10-08/dynlex-mask-scan-discarded-bench-2026-10-08.json`.
De echte tekst-/cliproute blijft langzamer dan Hayro en er is geen
algemene snelheidsclaim.

Reproduceer vanaf `rust-studio` met de gepinde bronnen in
`docs/public-pdf-corpus-2026-10-08.json`:

```sh
python3 scripts/audit_strict_dynlex_corpus.py \
  --probe target/release/backend_probe --output /tmp/opds-public-full.json \
  ~/.cache/opds-benchmarks/internet-2026-10-08/{attention,albany-contract,albany-drawings,nps-schematic,tfl-tube,nps-drawing-manual,usgs-topo-guide}.pdf
```

Volgende inhoudelijke stap: de echte afwijzingen in externe niet-lege
pagina's aanpakken, met name expliciete resources/kleurruimtes en
font-/padtypen. Alleen bron- of preflightlimieten verhogen levert hier
geen correcte bitmap op. De annotatielaag, algemene vectorpaden en
hybride documenten blijven open.

## Nader onderzoek van technische tekeningen (11 oktober)

De negen pagina's van de gepinde `albany-drawings.pdf` (SHA-256
`07b49d1cbf8117b8043271a0805f3c58cd75f87cf48d7d6c21861cab78acacde`)
zijn in de PDF **geen vectorpaden**: elk paginastream bevat slechts vijf
operators (`q cm g Do Q`) en verwijst naar één 10.200×6.600
DeviceRGB8/DCTDecode-beeld. De 67,32 miljoen bronpixels overschrijden de
huidige afzonderlijke 8.192-zijde- en 40-miljoen-pixellimieten. Dit corpus
mag dus niet als bewijs voor vectorondersteuning of als simpele JPEG-limietfix
worden behandeld.

De echte `nps-schematic.pdf` is wel vectorzwaar. Pagina 1 heeft 2.880.950
gedecodeerde contentbytes en circa 256.561 operators, waaronder 193.622
lijnsegmenten, 13.075 kubieken en 4.173 strokes; de hele bron is
30.720.875 bytes. Een veel kleinere pagina 33 bevat slechts 4.172
contentbytes en 347 operators, maar ook clips, CTM, stroken, marked content
en tekst. De huidige tekstworker weigert die hele bron al vóór de
paginaroute bij zijn 16-MB-brongrens. Een toekomstige geselecteerde-pagina-
overdracht moet de oorspronkelijke objecten en resources gecontroleerd
begrenzen; de limiet zonder zo'n ontwerp verhogen zou geen correcte
vectorbitmap bewijzen. Deze inventarisatie is read-only en verandert geen
native acceptatie- of snelheidscijfers.

Voor een zuivere vervolganalyse is NPS-pagina 33 met haar bestaande
resources buiten Git als zelfstandige PDF gekopieerd (SHA-256
`52dcfb18ea933227ddf9ceef7a3a9782501ae24311f5cd70f5dc4238514c3723`,
511.640 bytes). Hayro's **volledige** RGBA-uitvoer van de kopie is op 25%
en 100% bytegelijk aan die van pagina 33 in het originele document;
bij 200% weigert de huidige Hayro-probe beide wegens `TooLarge`.
De begrensde DynLex-worker kan de kleine kopie fysiek preflighten
(1.847 tokens, tien streams), maar weigert terecht de eerste niet-beeld-
`cm`: zijn bestaande CTM-subset eist onmiddellijk een `Do`. Daarna volgen
ook echte `W`, `S`, Type0-tekst en `/OC`-marked-content. De kopie is
uitsluitend een onderzoeksfixture en telt nadrukkelijk niet als acceptatie
van de **originele** 30,7-MB PDF.

Na de aparte acceptatie van expliciet neutrale `/Rotate 0`, gelijke
`/CropBox` en `/UserUnit 1` is de **volledige** zeven-PDF-audit opnieuw
uitgevoerd: 195/1.119 exact, 924 unsupported, nul verkeerde bitmaps of
procesfouten. Het ruwe nieuwe rapport staat buiten Git als
`~/.cache/opds-benchmarks/neutral-image-state-2026-10-11/public-full-after-neutral.json`.
Er is in dit corpus dus geen nieuwe oorspronkelijke pagina ontsloten.
