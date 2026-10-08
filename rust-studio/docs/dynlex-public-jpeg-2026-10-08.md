# Eerste volledige openbare praktijkpagina via strikt DynLex

De eerste pagina van het oorspronkelijke 49-pagina Albany-contract rendert nu
op 100% via de native **strikte DynLex-route**: 612×792 pixels,
1.938.816 RGBA-bytes, volledig bytegelijk aan Hayro. Het volledige originele
PDF-bestand wordt geopend; dit is geen uitgeknipte of herschreven fixture.
De bron-URL en SHA-256 staan in `public-pdf-corpus-2026-10-08.json` en het
reproduceerbare resultaat in `dynlex-public-jpeg-parity-2026-10-08.json`.

Dit bewijst één externe rasterpagina, niet alle 49 pagina’s, algemene tekst,
vectoren of zoom. De zichtbare tekst op deze pagina is onderdeel van een scan,
niet door DynLex als font/glyph getekend. Pagina 6 (echte tekst) en de scan op
25%/200% blijven expliciet unsupported, zonder Hayro-fallback of RGBA-uitvoer.

## Implementatie en grenzen

De bestaande geordende beeldscene accepteert nu een enkele `DCTDecode`-filter,
als naam of array met precies één naam, voor DeviceRGB met 8 bits per kanaal.
De optionele image `/Name` wordt als naam gevalideerd. `g` accepteert uitsluitend
exact zwart of wit, inclusief bestaande q/Q-kleurrestauratie; fractionele
grijswaarden blijven buiten deze toevoeging. Een array met één `FlateDecode`
kan dezelfde bestaande begrensde Flate-route gebruiken; filterketens niet.

JPEG wordt gedecodeerd met **zune-jpeg 0.5.15 in Rust**, dezelfde vastgepinde
codecversie die de Hayro-oracle gebruikt. Er wordt geen Hayro/Vello-rasterizer
aangeroepen. Rust interpreteert, decodeert en schaalt met de bestaande
pic-scale Catmull-Rom-route; de geordende DynLex-kernel tekent/combineert de
pixels. Dit is dus geen nieuw zelfgeschreven JPEG-algoritme in DynLex.

- PDF- en gecomprimeerde JPEG-bron maximaal 16 MB; begin-/eindmarkers vereist.
- JPEG-zijden maximaal 8.192; maximaal **40 miljoen bronpixels cumulatief per
  paginavoorbereiding**, ook als hetzelfde image-object herhaald wordt getekend.
  Maximaal 120 MB ruwe RGB-data voor één decode, exclusief codec-/schaalbuffers.
- Decoderheader moet vóór pixelallocatie exact met PDF-breedte/-hoogte
  overeenkomen, drie componenten en RGB/YCbCr aangeven, en precies de
  verwachte RGB-uitvoeromvang hebben. Geen metadata-patching of padding.
- Strict decoder mode en maximaal 16 JPEG-scans; baseline en de gecontroleerde
  progressive fixture werken. Corrupte/onvolledige streams worden afgewezen.
- DecodeParms/ColorTransform, andere kleurruimtes, Indexed+DCT, maskers,
  interpolatie-instellingen en filterketens worden niet stilzwijgend toegelaten.
- Bestaande doelbeeld-/scenelimieten blijven gelden: 4.096 per zijde en maximaal
  zes miljoen uitvoerpixels. JPEG ondersteunt een grotere bron, niet een
  onbeperkte renderbuffer. Bestaande Flate/raw bronlimieten blijven onveranderd.

De source-pixels van het echte image zijn 5100×6600. Alleen die grotere
bronlimiet verhogen was onvoldoende: DCTDecode, de één-filter-array,
image-Name en de expliciete witte g-state waren ook vereist.

## Controle

48 gecontroleerde JPEG-PDF-pagina’s combineren baseline/progressive,
0/90/180/270°, verschoven CropBox, origineel/verkleind/vergroot beeld,
naam/array-filter, blauwe onderlaag en q/Q-herstel van kleur. Alle pagina’s
zijn over de gehele RGBA-buffer exact gelijk aan Hayro. De kleine JPEG-bestanden
zijn lokaal gegenereerde RGB-patronen, geen externe documentextracten.

Tests controleren daarnaast truncatie (ook met kunstmatige EOI), onjuiste
header-/PDF-afmetingen, bronpixelgrenzen, cumulatief decodebudget, verkeerde
kleurruimte/bitdiepte, DecodeParms, maskers, interpolatie en filterketens.
De normale app blijft strikt bij onondersteunde inhoud.

```sh
# Vanuit rust-studio; openbare bron downloaden met de gepinde manifesthash.
cargo build --release -p opds-dynlex-backend --all-features --bin backend_probe
python3 scripts/verify_public_jpeg.py --probe target/release/backend_probe \
  --manifest docs/public-pdf-corpus-2026-10-08.json \
  --output /tmp/public-jpeg-parity.json
cargo test -p opds-dynlex-backend --all-features --test jpeg_page
cargo test -p opds-opdl-adapter jpeg_rgb
```

Het vervolg is schaalafhankelijke voorbereiding van beeldscenes zonder
herhaalde JPEG-decode, met expliciete resampling-/clippariteit. De huidige
voorbereide scène bevat de beeldpixels op 100%; deze mogen niet simpelweg
opnieuw worden vergroot en als correcte PDF-zoom worden gepresenteerd.

## Procesmeting en native venster

Vijftien afwisselende gepaarde procesruns op deze volledige eerste pagina,
warme OS-cache, inclusief lezen/parsen/decode/render/RGBA-schrijfwerk:
DynLex mediaan 325,62 ms en 115,0 MiB piek-RSS; Hayro 373,76 ms en
201,4 MiB. DynLex wint 15/15 paren; elke pair is over de volledige RGBA-buffer
bytegelijk. Dit geldt uitsluitend voor deze scan op 100%, niet algemene
PDF’s of interactieve zoom. Ruwe samples: `dynlex-public-jpeg-bench-2026-10-08.json`.

Dezelfde 30 openbare pagina/zoomgevallen zijn opnieuw strikt geaudit:
**1/30 DynLex ondersteund**, 29 expliciet unsupported, geen procesfouten.
Zie `dynlex-public-jpeg-audit-2026-10-08.json`; het eerdere vier-kernelrapport
blijft als historische nulmeting intact.

De nieuwe release-app is daadwerkelijk geopend met het ongewijzigde openbare
Albany-contract en `OPDS_RENDER_BACKEND=dynlex`. Het 1200×832-X11-venster
toont pagina 1/49 op 100%, met status `Renderer: DynLex` en een echte
miniatuur van pagina 1. Andere onbewezen pagina’s tonen `Unsupported preview`.
Screenshot buiten Git: `~/.cache/opds-benchmarks/internet-2026-10-08/previews/dynlex-native-jpeg.png`.
Er is geen oorspronkelijke Tauri-app gewijzigd of ander venster gesloten.

Validatie: 242 workspace-tests met alle features, standaard native check,
releasebuild van native app/probe/adapter, formattering, Python-compile en
diffcheck slagen. Volledige bestaande verifier: 47 PDF-pagina’s/4.964.352 pixels
exact tussen directe DynLex, CLI, Hayro en referentie; acht ongeldige scenes,
24 unsupported states en tien syntaxisvarianten gecontroleerd. V4: drie
PDF-pagina’s/147.456 pixels exact. De huidige eerste scan blijft zichtbaar in
het nieuwe native releasevenster; opgeslagen bestanden zijn niet gewijzigd.
