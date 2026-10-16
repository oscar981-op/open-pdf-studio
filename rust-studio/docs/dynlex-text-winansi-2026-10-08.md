# Meer volledige tekstpagina’s: WinAnsi en DeviceRGB/Gray

De strikte native DynLex-route rendert nu ook de **volledige originele
Albany-contractpagina’s 17 en 49** op 25%, 100% en 200%, telkens RGBA-bytegelijk
aan Hayro. Pagina 17 strandde op fractionele RGB-kleur, pagina 49 op de
WinAnsi-vermenigvuldigingscode 215. De volledige audit van 49 pagina’s op drie
schalen stijgt van **78 naar 84 exacte gevallen**, met **63 expliciete
unsupported gevallen**, nul verkeerde bitmaps en nul procesfouten. Alle
78 eerdere geaccepteerde gevallen blijven exact.

Dit is inhoudelijke uitbreiding van de bestaande tekstworker, geen fallback,
font-/documentwhitelist of nieuwe runtime-renderer. Er zijn nu 28 geaccepteerde
Albany-pagina’s bij drie schalen: één bestaande JPEG-pagina en 27 tekstpagina’s.
De algemene PDF-kernel, overige fonttypen, clips, transparantie en de gehele
annotatielaag zijn nog niet af. DynLex gebruikt LLVM/C++ en libc; de app en
nieuwe tekstvoorbereiding zijn Rust zonder zelfgeschreven C++-glue.

## Wat is veranderd

### WinAnsi

De eerdere handmatige ASCII/punctuatiemapping is vervangen door de bestaande
expliciete PDF-WinAnsi-tabel van **lopdf 0.34.0**. De dictionary moet nog steeds
`/Type /Font`, `/Subtype /TrueType` en precies `/Encoding /WinAnsiEncoding`
bevatten, met een geldig embedded FontFile2, niet-symbolische flags en de
ondersteunde Windows Unicode BMP cmap. Fontcount, decode-, width- en
geheugengrenzen zijn ongewijzigd.

Per byte wordt een enkele Unicode-scalar uit de tabel aan een bestaande GID
gekoppeld. Geen mapping of GID nul betekent afwijzing wanneer de byte werkelijk
wordt gebruikt; er worden geen tekens stilzwijgend overgeslagen. Dit is geen
Windows-1252-aanname of impliciete fallback. De PDF-specifieke bullet-aliases,
code 160 als gewone ruimte en code 173 als koppelteken volgen dezelfde tabel.

Codes 32 en 160 krijgen dezelfde lege space-glyph; een space-glyph met inkt
wordt afgewezen. Beide hebben hun eigen PDF Widths-advance en character
spacing. **Alleen code 32 krijgt `Tw`-woordspatiëring**. Hierdoor blijft
`A<ruimte160>B<ruimte32>C` correct geplaatst. `/Differences`, Type0, Type1,
MacRoman en ontbrekende cmap/glyphs blijven buiten deze route.

### Kleur en eenvoudige operators

`g` en `rg` accepteren nu eindige componenten in 0..1. De preparatie zet de
originele f64-contentwaarde om naar f32, vermenigvuldigt met 255 en telt 0,5
op vóór omzetting naar u8. Deze numerieke grens is gecontroleerd tegen de
lokale Hayro-interpret 0.7.0 `color.rs::f32_to_u8` en volledige pixels. Er is
geen Hayro-rasteraanroep. Buiten 0..1, verkeerde operandtypes of -aantallen
blijven afgewezen; geen stilzwijgende clamp.

Kleur blijft deel van q/Q-state en van iedere geordende PWPG-paint. De bestaande
DynLex-raster- en compositiekernels ontvangen dezelfde 8-bit-kleuren; geen
nieuwe kleur-/transparantiekernel of aanpassing van antialiasing.

Twee aanvullende algemene grammatica-uitbreidingen zijn afzonderlijk getest:

- `f*` voor **precies één reeds gestagede eenvoudige gesloten rechthoek**.
  Nonzero en even-odd hebben daar dezelfde geometrie. Compound/general paths
  krijgen hierdoor geen even-odd-ondersteuning.
- Gebalanceerde `BMC`/`EMC`-metadata naast bestaande `BDC`, met dezelfde
  nestinglimiet 32. `/OC` blijft afgewezen; geen optionele contentlagen
  stilzwijgend zichtbaar maken. Verkeerde tags en ongebalanceerde markers
  geven een fout.

Deze twee operators leveren in deze audit geen extra volledige pagina op:
Albany 11/12/15 komen nu voorbij `f*` maar stranden terecht op `W`; pagina 26
komt voorbij `BMC` maar strandt op `cm`. Er wordt geen clip of matrix genegeerd.

## Volledige pixelcontroles

| Proef | Resultaat |
| --- | --- |
| Alle 49 oorspronkelijke Albany-pagina’s, 25/100/200% | 84 exact, 63 unsupported, 0 fouten/verkeerde bitmaps |
| RGBA-bytes van geaccepteerde Albany-uitvoer | 274.827.168 exact |
| Nieuwe volledige pagina’s | 17 en 49, elk op drie schalen |
| 48 WinAnsi-/kleur-/q/Q-/f*-/metadata-/rotatie-/CropBox-gevallen | 162.196.992 bytes exact |
| Eerdere zestien native tekst-/zoom-/kleur-/overlapgevallen | 52.524.160 bytes exact |
| Vaste openbare selectie van tien pagina’s × drie schalen | ongewijzigd 6 exact, 24 unsupported, 0 fouten/verkeerde bitmaps |

De nieuwe gecontroleerde PDF’s bevatten **alle WinAnsi-bytes 32..255** in het
embedded LiberationSans-font, twee deterministische kleurenreeksen rond
halve 8-bit-afrondingsgrenzen (±1e-8), DeviceGray/RGB, gekleurde onderlaag,
overlap, q/Q-herstel, verschillende spaties, vier kwartslagen en twee
CropBoxes op drie schalen. Dit is breed gericht bewijs binnen de begrensde
subset, geen universele font-/PDF-pariteit.

Negen ongeldige native CLI-fixtures worden zonder bitmap/fallback geweigerd:
onbekende controlecode, negatieve/te grote/verkeerd gevormde kleur,
optionele content, verkeerde/ongebalanceerde marker en samengesteld
rechthoekpad. Unit-tests controleren de echte GID-/advance-relatie bij
WinAnsi-aliases, kleurherstel, NaN/Infinity, malformed markers en nesting.

**282 workspace-allfeaturetests** slagen, inclusief de lokale PDFium-library.
Standaard workspace-check, release-helper/probe/app, fmt, Python-compile en
diffcheck slagen. De volledige oude v2/v4-verifier blijft exact:
47 PDF’s / 4.964.352 pixels plus drie v4 / 147.456 pixels, inclusief de eerdere
foutafwijzingen. Die kernelroute is in deze run niet gewijzigd. Na de laatste
`f*`/`BMC`-wijziging zijn workspace-tests, 48-fixture-audit, zestien-case-audit,
complete Albany-audit en openbare selectie opnieuw uitgevoerd.

Rapporten (met bron-/helperhashes in de gerichte audits):

- `dynlex-text-winansi-colors-2026-10-08.json`;
- `dynlex-text-winansi-native-2026-10-08.json`;
- `dynlex-text-winansi-albany-2026-10-08.json`;
- `dynlex-text-winansi-public-2026-10-08.json`.

De `*-initial-2026-10-08.json`-bestanden bewaren de tussenstap vóór `f*`/`BMC`.
De eerste nieuwe native corpusstand was eveneens 84 exact; eindmetingen
bevestigen dat die kleine grammatica-uitbreiding geen stille acceptatie van
clipping/CTM oplevert.

De renderuitvoer van pagina 17 en 49 is ook visueel geïnspecteerd als PNG,
buiten Git in `~/.cache/opds-benchmarks/internet-2026-10-08/previews/`:
`dynlex-winansi-page17.png` en `dynlex-winansi-page49.png`. Dit zijn uitvoerbeelden
van de native dispatcher, **geen nieuwe screenshot van een appvenster**.
Bestaande vensters en PDF-bestanden zijn niet gesloten, herstart of gewijzigd.
De vertrouwde helper is bijgewerkt naast de applicatiebinary; PWPG blijft v1.
Een al gecachete afwijzing in een open document verdwijnt niet automatisch
na alleen het vervangen van de helper (nieuwe documentinstantie is nodig).

## Prestaties en vervolg

Deze run vergroot dekking en doet **geen nieuwe snelheidsclaim**. De eerdere
gepaarde optimalisatiemeting van Albany 6 (100%: oude DynLex 39,05 ms, nieuwe
DynLex 11,80 ms, Hayro 6,06 ms) blijft historisch bewijs onder de beschreven
cachevoorwaarden; zij meet niet de nieuwe pagina’s 17/49. Kernel/replaycode
is hier ongewijzigd, maar de worker bouwt een uitgebreidere tekenmapping.

Nieuwe prioriteit: begrensde rechthoekige tekstclips voor de echte resterende
`re W/W* n`-gevallen, met expliciete clipstate in q/Q en geordende scene-/IPC-
validatie. Start met pixeluitgelijnde clipgrenzen en bewijs volledige pagina’s,
rotatie/CropBox, overlap en glyphs die de cliprand kruisen. Geen clips negeren
of fractionele glyphranden afronden. Algemene CTM, andere fonts en annotaties
blijven volgende doelen. Profileer ook eerste-renderkosten per voorbereidings-
fase en houd de workerisolatie en cachegrenzen intact.

```sh
cargo build --release -p opds-dynlex-backend --all-features \
  --bin opds-text-worker --bin truetype_page_probe
python3 scripts/audit_text_winansi.py --output /tmp/winansi.json
python3 scripts/audit_native_text.py --output /tmp/native-text.json
python3 scripts/audit_strict_dynlex_corpus.py --probe target/release/backend_probe \
  --output /tmp/albany.json /pad/naar/originele/albany-contract.pdf
python3 scripts/audit_native_text_public.py --output /tmp/public.json
```

De werkmap blijft ongecommit. Al het eerdere JPEG/UI/adapter-/tekstwerk is
behouden; geen PR of merge gemaakt en geen fysieke printer aangeroepen.
