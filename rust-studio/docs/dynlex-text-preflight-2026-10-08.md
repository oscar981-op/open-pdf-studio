# Tekstfrontend: begrensde objectstream-voorcontrole

Deze stap voegt `text_preflight::inspect` toe **vóór** PageSpace/lopdf in de
zelfstandige `truetype_page_probe`. De gewone native dispatcher is niet
gewijzigd. Dit is een extra resourcecontrole, geen volledige PDF-validator en
geen bewijs dat de frontend al veilig voor native integratie is.

Bij aanvang stond er aanvullend ongecommit voorbereidingswerk in de werkmap:
`PreparedWindingPage`, `PreparedGlyph`, de batch-compositor en
`audit_prepared_text.py`. Dat werk is behouden en opnieuw gecontroleerd. Het
nieuwe preflightwerk schrijft die implementaties niet over en wordt niet met
ander werk samengevoegd in een commit.

## Wat wordt vóór objectparsing begrensd?

De scanner leest fysiek aanwezige PDF-syntaxis en slaat streambytes over met
hun expliciete directe `/Length`. Namen met `#xx` worden gedecodeerd; strings,
comments en hexstrings worden niet als streamsyntax geïnterpreteerd. De
scanner begrenst:

- maximaal 16.000.000 bronbytes, 262.144 value-tokens en nestingdiepte 32;
- maximaal 256 dictionarysleutels, 8.192 arraywaarden, 128 naambytes,
  64 atombytes en 65.536 letterlijke stringbytes;
- maximaal 4.096 streams en directe lengtes die binnen de bron vallen;
- `/ObjStm`: maximaal 4.096 gedeclareerde objecten en maximaal 4 MiB `/First`;
- object-/xref-streams: alleen ongefilterd of één FlateDecode-filter,
  maximaal 4 MiB uitgepakt per stream, **32 MiB gezamenlijk**. De resterende
  gezamenlijke ruimte wordt vóór decompressie als uitvoerlimiet doorgegeven;
- objectstreams worden na uitpakken opnieuw op syntax/nesting/tokenbudget
  gescand. Geneste streams en objectstream-DecodeParms worden geweigerd;
- xref-`Size` en `Index` tot 65.536 entries, drie `W`-velden van maximaal acht
  bytes en kleine expliciete predictorparameters. Een te grote `/Columns`
  kan niet ongemerkt door naar de volgende decoder.

Dubbele dictionarysleutels, indirecte streamlengtes, ongeldige naam-escapes,
afgebroken strings en lengte/endstream-tegenspraak worden conservatief
geweigerd. Dit beperkt de proefsubset; het is geen claim dat zulke indirecte
lengtes volgens PDF ongeldig zijn. Andere beeldstreams worden niet door deze
voorcontrole gedecodeerd; de geselecteerde tekstfrontend accepteert die
resourceklassen later niet.

Op het **volledige originele Albany-document** (49 pagina's, 2.474.873 bytes)
meet de voorcontrole 28.200 tokens, 122 streams, 39 objectstreams en 166.404
uitgepakte object-/xref-bytes. De originele bron blijft ongewijzigd.

## Cruciale resterende grens

De scanner volgt de fysieke bronvolgorde. Hij bewijst **niet** dat iedere
xref-offset naar een gecontroleerde objectgrens wijst. Een verwijzing naar
objectachtige bytes binnen een overgeslagen opaque stream, een afwijkende
xref-keten of een reparatiepad van de downstreamparser is hiermee niet als
veilig gecertificeerd. Hij valideert evenmin het volledige PDF-objectmodel.

Daarom blijft deze voorcontrole uitsluitend in de onderzoeksbinary. Alleen
"de preflight is geslaagd" is **geen** grond om native tekst te activeren.
Voor integratie is een expliciet begrensde reader met gevalideerde
xref/objectstream-toegang nodig, of een geïsoleerde pure-Rust
voorbereidingsworker met harde geheugen-/tijdgrenzen en een opnieuw
gevalideerde geordende scene-uitvoer. Vermijd dat deze lexicale helper zich
zonder helder contract uitbreidt tot een tweede volledige PDF-parser.

Lokale lopdf 0.34.0-bron bevestigt dat objectstreams tijdens `Document::load_mem`
worden uitgepakt, vóór onze eerdere font/contentlimieten. Zijn reader gebruikt
wel een visited-set voor `/Prev`, maar dat vervangt geen gezamenlijk
allocatiebudget of verificatie van alle objectoffsets. Hayro-syntax wordt hier
alleen voor metadata/content gebruikt, niet als rasterizer.

## Pixel- en regressiecontrole

De aangetroffen voorbereide route is met de voorcontrole opnieuw uitgevoerd:
**16/16 volledige pagina/zoomgevallen, 52.524.160 RGBA-bytes exact tegen Hayro**.
Dat zijn Albany-pagina 6 op 25/50/87/100/115/156,25/200/300% en acht
kleur/overlap/rotatie/CropBox-tekstfixtures. Alle paints blijven in PDF-volgorde.
De audit bevestigt ook dat de gewone native DynLex-route deze pagina nog
expliciet weigert, zonder output of fallback.

Rapport: `dynlex-text-preflight-parity-2026-10-08.json`. De daarin opgeslagen
replaytijden zijn een vergelijking van twee compositors op **dezelfde reeds
voorbereide geometrie**, geen vier-kernelbenchmark, geen GUI-zoomlatentie en
geen koude-cache- of RSS-meting. Zo waren op 100% de drie snelle replays circa
43–45 ms tegenover circa 408–429 ms voor de per-paint baseline. Deze kleine
herhaling is geen algemene prestatieclaim.

De bestaande `PreparedWindingPage` is aan één pagina en schaal gebonden, niet
reeds een native documentcache voor vrije zoom. Hij bewaart maximaal 8.192
paints en 32 MiB gevalideerde geometrie; de frontend beperkt de tijdelijke
unieke glyphcache tot 1.024 outlines / 16 MiB. De oorspronkelijke Albany-pagina
op 100% gebruikt 79 unieke outlines, 112.480 outlinebytes en 708.608
scenebytes. De batch-compositor hergebruikt coverage/command-scratch en scant
het paginabuffer niet opnieuw voor iedere glyph. Deze onderdelen stonden al
in de werkmap en zijn in deze run behouden.

De workspacecontrole met alle features slaagt: **266 tests**, inclusief de
PDFium-route met de lokale dynamische library. De standaard workspace-check,
release-probebuild, formattering en diffcheck slagen eveneens. De oude volledige
v2/v4-verifier is opnieuw exact: 47 PDF-pagina’s / 4.964.352 pixels plus drie
v4-pagina’s / 147.456 pixels; ook 8 ongeldige scenes, 24 unsupported states en
10 syntaxisvarianten zijn gecontroleerd.

Drie gerichte tests bewijzen de individuele en gezamenlijke decompressiegrens,
nesting, ongeldige xref/predictorvelden, framing, duplicate keys, indirecte
lengtes en byte-mutaties zonder panic. Ze zijn geen volledige parserfuzzing.

```sh
cd rust-studio
cargo test -p opds-dynlex-backend --features kernel text_preflight --lib
cargo build --release -p opds-dynlex-backend --features kernel --bin truetype_page_probe
python3 scripts/audit_prepared_text.py \
  --output docs/dynlex-text-preflight-parity-2026-10-08.json
```

Geen appbinary/herstart, geen PDF-bestandswijziging, geen PR of merge. DynLex
gebruikt LLVM/C++ en libc; fontgegevens komen uit pure-Rust ttf-parser/skrifa.
De bestaande Vello-geïnformeerde strip-/afvlakconventies behouden hun
bronvermelding; geen verborgen runtime Hayro/Vello-renderer.
