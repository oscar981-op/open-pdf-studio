# Eerste volledige native DynLex-tekstpagina’s

De optionele Linux DynLex-route in de Rust-app rendert nu volledige
WinAnsi/embedded-TrueType-tekstpagina’s via de begrensde Rust-voorbereidingsworker
en de zelfstandige DynLex strip-/compositiekernels. De originele Albany-pagina
6 is zichtbaar in een echt 1200×832-appvenster met **Renderer: DynLex**. Er is
geen Hayro-fallback of verborgen Vello-rasteraanroep toegevoegd.

Dit is een bewezen beperkte tekstsubset, geen complete PDF-kernel. Onder meer
Type0/Type1, andere encodings, algemene CTM/clips, transparantie en annotaties
blijven buiten deze tekstfrontend. PDFium blijft een afzonderlijke keuze via
Rust-FFI. DynLex gebruikt LLVM/C++ en libc; deze implementatie is niet letterlijk
100% Rust inclusief toolchain.

## Koppeling en distributie

`DynlexDocument` probeert eerst de bestaande voorbereide vector-/rasterroute.
Wanneer deze geen scene kan maken, mag de tekstworker de pagina voorbereiden.
Dit blijft een tweede DynLex-frontend, geen vervangende renderer. Fouten uit een
reeds gekozen vector-/rasterrenderpad worden niet verborgen door tekst alsnog
te proberen.

De vertrouwde applicatiehelper heet `opds-text-worker` en moet **naast de
native executable** staan. Het pad komt uit `current_exe`, nooit uit een PDF.
`DynlexDocument::with_text_worker` biedt dezelfde expliciete registratie voor
embedders/tests; ook daar moet de host een vertrouwde binary kiezen.
Ontbreekt de helper bij documentopening, dan blijft de bestaande subset werken
en is nieuwe tekst unsupported. De helper accepteert uitsluitend het
stdin/PWPG-sceneprotocol; de losse onderzoeks-CLI blijft apart beschikbaar.

```sh
cd rust-studio
./scripts/build_native_dynlex.sh
# PDFium is optioneel en wordt niet gebundeld:
OPDS_RENDER_BACKEND=dynlex target/release/opds-native-app /pad/naar/document.pdf
```

Het script bouwt de helper met de kernel-feature en de native app met alle
rendererfeatures. PDFium vereist nog de bestaande lokale libraryconfiguratie.
Op andere platforms wordt de Linux-tekstworker niet stilzwijgend zonder grenzen
gestart; deze uitbreiding is daar nog niet beschikbaar.

De nieuwe tekstfrontend blijft in een afzonderlijk proces met de eerder
bewezen grenzen: 512 MiB adresruimte, zes CPU-seconden, tien seconden wandtijd,
32 MiB scene-IPC en 64 KiB diagnostiek. De ouder valideert de volledige PWPG/OPNW
scene opnieuw voordat DynLex rendert. Zie de [workerbeschrijving](dynlex-text-worker-2026-10-08.md).
Dit begrenst deze tekstvoorbereiding, **niet alle parsing van de hele app**:
bestaande documentmetadata en oudere vector-/rasterfrontends blijven hun eigen
parsers gebruiken. Resource-isolatie is geen volledige securitysandbox.

Er is één brongebonden tekstcache per document, maximaal 32 MiB / 8.192 paints,
met pagina en exacte f32-schaalbits als sleutel. Wisselen geeft de vorige scene
vrij vóór nieuwe voorbereiding. De bestaande documentbrede RGB-cache blijft
maximaal 120 MB; overige bestaande cachegrenzen zijn niet verhoogd. Ontvangen
IPC/source, fonts in het kindproces, caller-owned RGBA en allocatoroverhead
vallen niet onder alleen de scenegrens. Geen totaal-app-RSS-claim.

Gelijktijdige dezelfde aanvragen gebruiken één voorbereiding. Warme replay
gebruikt de scene opnieuw; andere zoom maakt nog een nieuwe worker en heeft
nog geen fontoutlinecache over zooms. Reeds bestaande appchecks voor oude
asynchrone resultaten en bitmaps blijven van kracht. Na unsupported/failure
komt geen oude/vervangende bitmap terug. Technische workerdiagnostiek staat in
het proceslog; de UI toont een korte afwijzing met handmatige rendererkeuze.

## Pixelbewijs via native dispatcher

Alle rendervergelijkingen hieronder gebruiken `--dynlex-strict`, dezelfde
`DynlexDocument`-dispatcher als het native venster. Hayro is uitsluitend de
onafhankelijke testoracle.

| Controle | Resultaat |
| --- | --- |
| Originele Albany-pagina 6, acht schalen 25–300%, plus acht kleur/overlap/rotatie/CropBox-fixtures | 16/16 exact, 52.524.160 RGBA-bytes |
| Zelfde controle na laatste compacte foutmelding | opnieuw 16/16 exact |
| Echte Ctrl+scroll-schaal 1,3225 op Albany-pagina 6 | 809×1047, 3.388.092 RGBA-bytes exact |
| Alle 49 originele Albany-pagina’s op 25/100/200% | 78/147 exact, 69 unsupported, nul procesfouten of verkeerde bitmaps |
| Geaccepteerde uitvoer in die 49-pagina-audit | 255.196.656 vergeleken RGBA-bytes |
| Vaste openbare selectie: tien pagina’s uit zeven PDF’s op drie schalen | 6/30 exact, 24 unsupported, nul verkeerde bitmaps/procesfouten |

De geselecteerde openbare audit steeg van 3 naar 6 geaccepteerde gevallen door
Albany-pagina 6 op drie schalen. De volledige Albany-audit heeft 26 aanvaarde
pagina’s op alle drie schalen: 1, 4, 6, 14, 18, 19, 21, 22, 23, 24, 25, 27, 28,
29, 32, 34, 36, 37, 40, 41, 42, 44, 45, 46, 47, 48. Dit is **geen
acceptatiewhitelist in de code**; de operators/resources/geometrie bepalen
acceptatie. Pagina 1 is de eerder bewezen JPEG-scan; de overige 25 zijn nieuwe
volledige tekstpagina’s in deze audit. Een extra replay per geaccepteerde
pagina bevestigt dit met de werkelijke tekstvoorbereidingsteller; zie
`dynlex-native-text-routes-2026-10-08.json`. De 69 afwijzingen worden niet als
snelheidsresultaat meegeteld.

Rapporten:

- `dynlex-native-text-2026-10-08.json` en `dynlex-native-text-final-2026-10-08.json`;
- `dynlex-native-text-ctrlzoom-2026-10-08.json`;
- `dynlex-native-text-albany-2026-10-08.json`;
- `dynlex-native-text-public-2026-10-08.json`.

`audit_native_text.py` simuleert een ontbrekende helper, crash en beschadigde
IPC in een aparte tijdelijke applicatiemap. De live helper/app worden niet
verplaatst. Alle drie worden zonder output/fallback afgewezen. Twee nieuwe
native integratietests controleren herhaalde/concurrente aanvragen,
zoomwissel, andere pagina, unsupported state, foutcache, herstel en NaN.
Historische standalone audits staan zowel expliciete native afwijzing als
inmiddels bewezen native uitvoer toe; een geaccepteerde native bitmap moet
daar ook volledig met Hayro overeenkomen.

De workspace-allfeaturetests slagen: **275 tests**, inclusief lokale PDFium en
de native workerintegratietests. Standaardcheck, releasebuild, formattering,
Python-compile en diffcheck slagen. Na de korte UI-foutmeldingwijziging zijn de
native integratietests en de 16-case audit herhaald. De oude volledige verifier
is opnieuw exact: 47 PDF-pagina’s / 4.964.352 pixels plus drie v4-pagina’s /
147.456 pixels, inclusief foutafwijzingen.

## Echte UI

Een nieuwe eigen testinstantie toont pagina 6/49 op 100% met tekstminiatuur en
Renderer: DynLex. Ctrl+scroll produceerde 132% (1,3225); die uitvoer is apart
bytegewijs gecontroleerd. Naar unsupported pagina 7 navigeren wist de bitmap;
terug naar pagina 6 en Home 100% herstelt echte DynLex-uitvoer. De laatste
release-instantie staat op pagina 6/49, PID **184241**, log
`/tmp/opds-native-text-ui-final.log`. Controleer actuele processen voor een
volgende herstart. Alleen onze eigen nieuwe testinstantie zonder annotaties
werd vervangen; alle eerder aanwezige gebruikersvensters en PDF’s bleven intact.

Screenshots buiten Git:
`~/.cache/opds-benchmarks/internet-2026-10-08/previews/dynlex-native-text-final-page6.png`,
`dynlex-native-text-final-page7-rejected.png` en
`dynlex-native-text-page6-132.png`.
Dit is geen gemeten GUI-latentie of bewijs van perfecte zoomfocus/UI-pariteit.

## Eerlijke prestatiemeting

Vijftien afwisselende procesparen per schaal op de oorspronkelijke Albany-pagina
6. Elk proces doet één initiële render en twee warme replays op dezelfde
pagina/schaal; iedere uitvoer is volledig bytegecontroleerd buiten de timer.
De voorbereiding/worker-IPC zit in de initiële render. GNU time registreert de
procespiek inclusief voorbereiding en verificatie. Warm OS-cache; geen koude
OS-cache. DynLex bewaart geometrie, Hayro maakt in de huidige app-route per
render een nieuwe RenderCache. Geen gelijkgetrokken interne caches of
kernel-only-/GUI-meting.

| Schaal | Eerste DynLex / Hayro | Warm DynLex / Hayro | Piek-RSS DynLex / Hayro |
| --- | ---: | ---: | ---: |
| 25% | 130,77 / 4,26 ms | 35,87 / 2,87 ms | 14,83 / 9,97 MiB |
| 100% | 134,05 / 9,99 ms | 38,74 / 5,93 ms | 16,63 / 13,81 MiB |
| 200% | 145,54 / 21,09 ms | 52,97 / 15,86 ms | 29,42 / 25,71 MiB |

DynLex wint **0/15** warme paren op iedere schaal. Deze tekstpagina is dus nog
langzamer dan Hayro, ondanks scenehergebruik. De vroegere verbetering tegen
de eigen per-paintcompositor is geen voordeel tegenover Hayro. Ruwe samples en
binaryhashes: `dynlex-native-text-bench-2026-10-08.json`. De daaropvolgende
UI-foutmeldingwijziging veranderde alleen het afwijzingsbericht, niet het
succesvolle renderpad; de eindbinary kreeg opnieuw de 16-case pixelcontrole.

```sh
python3 scripts/audit_native_text.py --output docs/dynlex-native-text-final-2026-10-08.json
python3 scripts/audit_native_text_public.py --output docs/dynlex-native-text-public-2026-10-08.json
python3 scripts/audit_strict_dynlex_corpus.py --probe target/release/backend_probe \
  --output docs/dynlex-native-text-albany-2026-10-08.json /pad/naar/origineel/albany-contract.pdf
cargo build --release -p opds-dynlex-backend --all-features --bin jpeg_zoom_probe
python3 scripts/bench_native_text.py --pairs 15 --output docs/dynlex-native-text-bench-2026-10-08.json
```

## Volgende stap

Profileer de glyph-striprasterisatie/compositie en workerstart/voorbereiding
apart op deze complete pagina, bij kleine en grote zoom. Onderzoek minder
werk per glyph, gedeelde scratch/gebatchte ABI-aanroepen en exact herbruikbare
geometrie/dekking; neem bestaande tile-bounds-optimalisaties mee voordat iets
opnieuw wordt gebouwd. Eis alle 78 geaccepteerde Albany-cases pixelgelijk plus
gecontroleerde contour-/rotatie-/CropBox-regressies. Geen onnauwkeurige
subpixelafronding of bitmapopschaling om een snelheidclaim te halen. Breid
tegelijk inhoudelijke dekking uit met de echte afwijzingsredenen, onder meer
andere fonts, zonder document-/glyphspecialcases.
