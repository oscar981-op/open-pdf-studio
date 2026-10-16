# Ingebedde Type0/Identity-H-tekst in de strikte DynLex-route

De bestaande begrensde Linux-tekstworker verwerkt nu ook een Type0-font met
`/Encoding /Identity-H`, precies één `/CIDFontType2`-descendant,
`/CIDToGIDMap /Identity`, `/CIDSystemInfo` Adobe/Identity en een ingebedde
`/FontFile2`. Twee bronbytes vormen één CID/GID; ontbrekende, oneven of
buiten het ingebedde glyphbereik vallende codes worden geweigerd.
`/DW` en beide PDF-`/W`-vormen worden begrensd en gevalideerd, inclusief
overlappende of omgekeerde bereiken. De bestaande WinAnsi TrueType-route
blijft beschikbaar in dezelfde pagina, met aparte letter- en
woordspatiëringsregels. Het workerproces levert nog steeds uitsluitend
gevalideerde PWPG/OPNW-scenegeometrie aan de zelfstandige DynLex-kernel;
er is geen Hayro/Vello-productierasteraanroep of fallback.

Dit ontsluit **drie volledige niet-lege pagina's** van het originele,
openbare Albany-contract: **7, 31 en 43**. Op 25%, 100% en 200% zijn alle
negen complete native RGBA-buffers exact gelijk aan Hayro. De hele
49-pagina-audit stijgt van 105 naar **114/147 exacte gevallen**, met
33 unsupported, nul verkeerde bitmaps/procesfouten en 372.979.728
vergeleken RGBA-bytes. De drie pagina's gebruiken naast WinAnsi TrueType
een ingebedde SymbolMT Identity-H-CIDFont; acceptatie is op
font-/resource-/operator- en geometriekenmerken gebaseerd, zonder
document- of glyphwhitelist.

Een aparte generator test **48** volledige PDF-gevallen met twee
inhoudsvarianten, vier kwartslagen, twee CropBoxes, 25/100/200%,
gemengde Type0- en WinAnsi-tekst, DeviceRGB-lagen en rechthoekvulling:
alle 15.355.008 RGBA-bytes zijn exact. Zes ongeldige CID-string-/map-/
breedtegevallen geven geen bitmap. Unit tests controleren eveneens
CID-breedte, f32-tekstadvance, oneven codes, te grote GIDs, niet-Identity
mapping en corrupte `/W`. Replays en hashes:

- `scripts/audit_type0_native.py` en `dynlex-native-type0-fixtures-2026-10-08.json`;
- `dynlex-native-type0-albany-2026-10-08.json`;
- volledige openbare audit in
  `~/.cache/opds-benchmarks/internet-2026-10-08/dynlex-full-type0-audit-2026-10-08.json`,
  compacte matrix in `dynlex-native-type0-public-2026-10-08.json`.

De volledige zeven-PDF-audit telt nu **174/1.119 exact**, 945 unsupported,
nul foutieve bitmaps of procesfouten en 598.730.616 vergeleken RGBA-bytes.
Daarvan zijn nog steeds 60 successen twintig lege NPS-pagina's op drie
schalen. De inhoudelijke uitbreiding van deze stap is uitsluitend de negen
Albany-gevallen. Grote vector- en hybride pagina's, Type1, andere
CID-mapping/CMaps, niet-ingebedde fonts, transparantie en annotaties blijven
onbewezen. De voorafgaande audit staat in
`dynlex-full-public-audit-2026-10-08.md`.

Tien afwisselende procesparen op de originele Albany-pagina 7 bij 100%
gaven warme medianen **51,96 ms DynLex / 15,82 ms Hayro** en eerste
renders **183,70 / 21,02 ms**. DynLex won geen paar op 25%, 100% of
200%. Piek-RSS op 100% was circa 18,2 / 14,7 MiB. Alle gemeten beelden
zijn bytegecontroleerd; warme OS-cache, DynLex-scenehergebruik en Hayro's
nieuwe RenderCache per render zijn expliciete verschillen. JSON:
`dynlex-native-type0-bench-2026-10-08.json`. Dit is geen algemene
snelheids- of kernel-only-meting. De nieuwe dekking is correct, maar
nog niet prestatiecompetitief op deze tekstpagina.

De Type0-uitbreiding draait binnen de bestaande workergrenzen (512 MiB
adresruimte, zes CPU-seconden, tien seconden wandtijd en maximaal 32 MiB
gevalideerde scene-IPC). Per font geldt de bestaande 8 MB gedecodeerde
FontFile2-grens; `/W` en fontaantal zijn expliciet begrensd. Fractionele
CID-breedtes worden voorlopig geweigerd: lopdf vernauwt zulke
PDF-decimalen tot f32 vóór de tekstberekening; alleen bewezen gehele
breedtes zijn vrijgegeven. De preflight
blijft aanvullende syntax-/decompressiecontrole, geen volledige
PDF-validator of OS-securitysandbox. De normale Rust-appbinary is in deze
run niet herstart; de native dispatcher is met `backend_probe` en dezelfde
vertrouwde helper getest. Bestaande vensters en onopgeslagen werk zijn
ongewijzigd.

Volgende stap: onderzoek volledige niet-lege pagina's buiten het
Albany-contract, met name veilige expliciete graphics resources,
Type1/CFF en algemene paden. Houd onbekende operatoren/states strikt
unsupported; alleen bron- of preflightlimieten verhogen levert geen
correcte rendering op.

Aanvullende audit: een expliciet lege `/Annots []` (ook als indirecte array)
wordt nu als inerte paginastaat aanvaard. Een gevulde of ongeldige `/Annots`
en iedere `/Group` blijven afgewezen; gerichte tests controleren die grens.
Dit ontsluit de lokale combinatie-PDF nog niet: na die grens volgt terecht
`unsupported resource type`. Vier lokale documenten, 24 pagina-/zoomgevallen
in totaal, leverden nul nieuwe aanvaarde resultaten en nul procesfouten.
`Tekst.pdf` strandt eerder op indirecte streamlengte in de preflight;
de twee andere documenten bevatten echte annotaties. Deze diagnose is
geen extra PDF- of annotatiedekking en verandert de gepubliceerde
zeven-PDF-corpuscijfers hierboven niet. Het ruwe rapport staat buiten Git in
`~/.cache/opds-benchmarks/internet-2026-10-08/dynlex-local-text-audit-2026-10-08.json`.

Een volgende strikte tekstworkeruitbreiding behandelt onbenutte
`ExtGState`-, `XObject`-, `Shading`-, `Pattern`- en `Properties`-resources
als inert: hun schilderoperatoren blijven buiten de contentgrammatica en
worden afgewezen. Een `ColorSpace`-resource mag alleen named entries zonder
`DefaultGray`, `DefaultRGB` of `DefaultCMYK` bevatten, omdat zulke defaults
ook zonder `cs`-operator de betekenis van `g`/`rg` kunnen wijzigen.
Een Link-annotatie wordt alleen als onzichtbare paginametadata behandeld
wanneer zij geen `/AP` heeft, expliciet `/Border [0 0 0]` bevat en een
aanwezige `/BS` eveneens `W 0` heeft. Andere annotaties blijven unsupported.
Een echte gecontroleerde tekst-PDF met zo'n Link is bij 25/100/200% over
de volledige RGBA-pagina bytegelijk aan Hayro; een zichtbare rand wordt
geweigerd. De openbare NPS-pagina 7 bevat werkelijk zo'n onzichtbare Link,
maar blijft na deze grens unsupported door haar niet-ingebedde Arial-font.
Ook de vier lokale documenten blijven unsupported. De uitbreiding laat
onbekende graphics-operators en default-kleurruimtes dus niet stilzwijgend
door en is nog geen algemene annotatie- of resourcedekking.

In het oorspronkelijke openbare Albany-contract ontgrendelt juist die
onzichtbare Link-subset **zeven extra volledige tekstpagina's**: 2, 3, 8,
30, 35, 38 en 39. Elk van hun 25/100/200%-renderbuffers is bytegelijk
aan Hayro. Albany stijgt daarmee van 114 naar **135/147 exact**, met twaalf
unsupported gevallen. De volledige zeven-PDF-audit telt nu **195/1.119
exact**, 924 unsupported, nul foutieve bitmaps/procesfouten en 667.437.408
vergeleken RGBA-bytes. Daarvan blijven 60 successen lege NPS-pagina's;
alle inhoudelijke niet-lege successen komen dus nog uit Albany.
Het volledige ruwe rapport staat buiten Git in
`~/.cache/opds-benchmarks/internet-2026-10-08/dynlex-full-inert-resources-links-2026-10-08.json`.
Dit is bewijs voor onzichtbare links bij de ondersteunde tekstsubset,
niet voor zichtbare annotatie-appearances of PDF's buiten Albany.

### 9 oktober: indirecte streamlengte in de tekst-preflight

De geïsoleerde tekstworker accepteert nu ook `/Length n g R` wanneer een
begrensde klassieke xref-tabel de exacte object-/generatiepositie aanwijst
en daar één direct geheel getal met `endobj` staat. De xref-lezer heeft een
maximum van 65.536 entries; de bestaande bron-, token-, stream- en
decompressielimieten blijven gelden. Ontbrekende, afwijkende of ongeldige
xref-verwijzingen worden vóór `lopdf` afgewezen. Xref-streams en complexe
incrementele ketens zijn met deze nieuwe route niet ondersteund. Dit is
aanvullende fysieke preflight binnen de resource-begrensde worker, geen
volledige PDF-validator en geen securitysandbox.

`Tekst.pdf` (vijf pagina's) passeert daardoor de eerdere
`indirect stream Length`-blokkade. De eerste inhoudelijke afwijzing is nu
het ingebedde Type1-font `UniviaProRegular`, waarvoor de DynLex-tekstkernel
nog geen bewezen outline-/rasterroute heeft. De vijf pagina's op
25/100/200% blijven alle vijftien **unsupported**, zonder vervangende
bitmap of procesfout; de combinatie-PDF blijft ook unsupported. De
reproduceerbare strikte audit staat buiten Git in
`~/.cache/opds-benchmarks/internet-2026-10-08/dynlex-indirect-length-local-2026-10-09.json`.
Dit voegt dus nog geen volledige externe pagina toe. Een onafhankelijke,
begrensde Type1/CFF-route met pixelpariteit is de volgende inhoudelijke
stap; fontsubstitutie zou de vergelijking ongeldig maken.
