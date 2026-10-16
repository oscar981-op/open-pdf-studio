# Begrensde tekstvoorbereiding in een afzonderlijk Rust-proces

De echte Albany-contractpagina 6 kan nu via een begrensde voorbereidingsworker
naar een opnieuw gevalideerde DynLex-scene. De native dispatcher is in deze
stap **niet gewijzigd**. De oplossing begrenst parserfouten en resourcegebruik;
zij maakt van de bestaande preflight geen volledige PDF-validator.

## Procesgrens

`text_worker::prepare` start de applicatie-eigen `truetype_page_probe` met
`--scene-worker`. De PDF komt uitsluitend via stdin binnen. De worker leest
maximaal 16.000.000 bronbytes, voert de bestaande voorcontrole uit en doet
PageSpace-, PDF-, font- en geometrievoorbereiding. Hij geeft uitsluitend een
PWPG-v1-scene op stdout terug. Hij rendert geen bitmap en schrijft geen
PDF/diagnosebestanden in deze modus. De bestaande losse onderzoeksmodi blijven
beschikbaar; rechtstreeks starten daarvan geeft niet automatisch deze
procesgrenzen.

Op Linux stelt de launcher vóór exec harde OS-grenzen in:

- 512 MiB virtuele adresruimte (`RLIMIT_AS`, geen belofte van 512 MiB RSS);
- zes seconden CPU, tien seconden wandtijd vanaf processtart;
- 8 MiB stack, 32 open descriptors, geen core dump of reguliere bestandsuitvoer;
- `no_new_privs`, een eigen procesgroep en een lege geërfde omgeving;
- maximaal 32 MiB scene-uitvoer en 64 KiB stderr; lezen stopt na grens + 1 byte;
- globaal maximaal één actieve voorbereidingsworker binnen het hostproces.

Bij timeout/fout wordt de procesgroep beëindigd en het kind gereaped. De
monitor gebruikt `waitid(WNOWAIT)` zodat opruimen van de groep niet met
PID-hergebruik concurreert. Begrensde pipelezers lopen naast de stdin-writer;
een volle pipe kan de watchdog niet tegenhouden. Achtergebleven groepsleden
worden ook na gewone kind-exit beëindigd voordat de pipelezers gejoined worden.

Dit is **resource-isolatie, geen volledige OS-securitysandbox**: filesystem- en
netwerktoegang zijn niet met seccomp/namespaces afgesloten. De caller kiest een
vertrouwde applicatiebinary; een PDF mag nooit een executablepad bepalen. Er
is geen andere renderer of fontfallback in het workerpad. Parserallocaties die
de lexicale preflight missen, waaronder afwijkende xref-toegang, blijven in het
begrensde kindproces. Dat bewijst op zichzelf nog geen semantische correctheid
voor willekeurige PDF's.

## Scenecontract en cache

PWPG-v1 heeft een 24-byte header: magic, versie, breedte, hoogte, paintaantal en
nul-reserveveld. Iedere paint heeft 16 bytes metadata (origin x/y, drie
RGB-bytes, nul-reservebyte, OPNW-lengte), gevolgd door precies één OPNW-record.
Alle integers zijn little-endian. Er zijn geen pointers, objectreferenties,
fonthandles of door het kind gerasterde pixels in het contract.

De ouder controleert na succesvolle procesexit de volledige framing, versie,
alle reservevelden, paintlimiet, bytegrens, pagina-afmetingen en ieder
OPNW-record opnieuw. Contouren moeten gesloten en eindig zijn; crossing/overlap
is expliciet nonzero-winding, geen versoepeling van OPM2. Origin blijft op het
4×4-stripraster; ieder punt moet ook binnen de uiteindelijke pagina vallen.
Er wordt pas daarna een private aligned ABI-buffer opgebouwd. Een ongeldige
laatste paint levert geen gedeeltelijke scene of RGBA-uitvoer op.

De experimentele `TextDocument` bindt immutable bronbytes aan de vertrouwde
worker. Eén mutex beschermt één cache-entry `(pagina, f32-schaalbits)` met een
voorbereide scene óf expliciete fout. Een nieuwe aanvraag verwijdert de vorige
scene vóór voorbereiding. Gelijktijdige dezelfde aanvragen starten dus niet
meerdere parsers. De scene bewaart maximaal 32 MiB / 8.192 paints; per replay is
er maximaal 24 MB nieuwe RGBA. Ontvangen IPC-bytes, bronbytes, allocatoroverhead
en caller-owned uitvoer komen daar tijdelijk bij. Meerdere documenten kunnen
ieder een scene bezitten; dit is geen totaal-RSS-budget voor de hele app.

Warme replay parseert de PDF/fontdata niet opnieuw. **Een andere zoom start wel
een nieuwe voorbereiding**; outlinehergebruik over zoomniveaus is nog niet
geïmplementeerd. Er is geen annulering van een aanvraag die op de globale
workerlock wacht. Dit moet bij native scheduling expliciet worden meegenomen.
De scene is niet stilzwijgend in `PreparedPage`/`DynlexDocument` geregistreerd.

## Volledige pixelvergelijking

`scripts/audit_text_worker.py` start alleen de kandidaatworker en vervolgens
Hayro als afzonderlijke testoracle. De kandidaat gebruikt pure-Rust
lopdf/hayro-syntax voor objecten/content, ttf-parser/skrifa voor fonts en de
bestaande DynLex strip-/compositiekernels voor pixels. Geen Hayro/Vello
rasteraanroep. De bestaande Vello-geïnformeerde numerieke conventies behouden
hun MIT/Apache-bronvermelding.

- Originele volledige Albany-pagina 6 op 25/50/87/100/115/156,25/200/300%.
- Acht gecontroleerde volledige tekstpagina's met kleur/overlap, vier
  kwartslagen en twee CropBoxes.
- Samen 16/16 exact, **52.524.160 RGBA-bytes** gelijk aan Hayro.
- Iedere kandidaatpagina wordt tweemaal vanuit dezelfde scene opnieuw
  gerenderd en bytegewijs gecontroleerd; slechts één voorbereiding.
- Objectstream-decompressiebom, afgebroken bron, ongeldige xref, NaN-schaal en
  pagina nul worden zonder RGBA-output geweigerd.
- Rust-tests controleren crash, nonzero exit, timeout met geërfde pipes,
  stdout-/stderr-limieten, werkelijke geërfde rlimits, truncatie, NaN,
  buiten-pagina-punten, reserveringen en byte-mutaties zonder panic.
- Gelijktijdige cacheaanvragen bereiden één keer voor; ongeldige schaal wijzigt
  de cache niet, herhaalde dezelfde fout start geen nieuwe worker.

De workspace met alle features slaagt: **270 tests**, inclusief de lokale
PDFium-library. Standaard workspace-check, release-probes, formattering,
Python-compile en diffcheck slagen eveneens. De oude volledige verifier is
opnieuw exact: 47 PDF-pagina’s / 4.964.352 pixels plus drie v4-pagina’s /
147.456 pixels, inclusief acht ongeldige scenes, 24 unsupported states en
tien syntaxisvarianten.

Rapport: `dynlex-text-worker-2026-10-08.json`, met bron-/font-/binaryhashes. De
losse tijden zijn diagnostisch en niet gepaard; geen algemene snelheid-,
GUI-latentie-, koude-OS-cache- of RSS-claim. De native controletest bevestigt dat
Albany-pagina 6 voorlopig nog strikt unsupported is.

```sh
cd rust-studio
cargo build --release -p opds-dynlex-backend --features kernel \
  --bin truetype_page_probe --bin text_worker_probe
cargo test -p opds-dynlex-backend --features kernel --lib text_worker
cargo test -p opds-dynlex-backend --features kernel --lib scene_ipc
python3 scripts/audit_text_worker.py \
  --output docs/dynlex-text-worker-2026-10-08.json
```

## Volgende integratiestap

Lever een expliciet benoemde applicatiehelper naast de native binary mee en
verbind deze voorbereidingsroute aan de strikte DynLex-dispatcher. Maak
beschikbaarheid/failure eerlijk zichtbaar; ontbrekende helper, unsupported
invoer, timeout of crash geeft geen andere renderer. Houd één totale
tekstscenecache en bestaande RGB-cachegrenzen expliciet. Voorkom dat de nieuwe
tekstfrontend alsnog ongecontroleerd in het hoofdproces wordt aangeroepen.
Test stale resultaten, andere pagina's/schalen, herstel na fouten, de volledige
publieke corpusselectie en native UI op pagina 6 voordat activering wordt
geclaimd. Geen fontnaam-/glyph-/document-whitelist als vervanging voor een
onderbouwde algemene subset.

Ander ongecommit werk is behouden; geen commit, PR, appherstart of wijziging
van gebruikers-PDF's. DynLex gebruikt LLVM/C++ en libc; de nieuwe procesgrens
gebruikt libc via Rust-FFI voor Linux OS-limieten, geen zelfgeschreven C++-glue.
