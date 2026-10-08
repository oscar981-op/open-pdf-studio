# Fractionele JPEG-zoom: zelfstandige samplingproef

Een aparte Rust→DynLex-proef rendert pagina 1 van het **volledige oorspronkelijke
Albany-contract** op twintig zoomstanden van 13% tot 301%, inclusief 115%, 87%
en 156,25%. Alle **91.783.472 RGBA-bytes** zijn gelijk aan Hayro. De scan wordt
één keer met zune-jpeg gedecodeerd; dezelfde 100.980.000 RGB-bronbytes bedienen
alle schalen. Dit ondersteunt nog geen nieuwe tekst-, vector- of annotatieobjecten.

**Niet ingeschakeld in de native renderer.** `FractionalProbe` en
`fractional_jpeg_probe` staan buiten `DynlexDocument`/`PreparedPage`. De bestaande
app blijft op deze vrije schalen strikt unsupported. Er is geen nieuwe appbuild
gestart en er zijn geen gebruikersvensters gesloten. De eerdere 25/50/100/125/200%
route blijft intact. De openbare 30-case inhoudsaudit blijft 3/30; deze proef
voegt geen nieuwe inhoudelijke pagina toe.

## Twee kandidaten, dezelfde oorspronkelijke JPEG

Beide kandidaten gebruiken de f32-pagina-afmetingen × schaal, gevolgd door floor
voor de bitmap. Het oorspronkelijke beeld krijgt een apart f32/ceil-tussenrooster,
met pic-scale-resampling en inverse schermtransformatie voor pixelcentra.
De resampling gebeurt in Rust; de gevalideerde geordende OPDL-v3-buffer wordt
door de bestaande DynLex-kernel gecomposeerd. Geen Hayro/Vello-renderaanroep
binnen de kandidaten; Hayro is uitsluitend de onafhankelijke PDF-oracle.

De geometrische f64-projectie was op vijftien schalen exact. Op vijf schalen
bleven verschillen in enkele pixelkolommen bestaan:

| Zoom | Afwijkende pixels | Afwijkende kanalen | Grootste kanaalfout |
|---|---:|---:|---:|
| 87% | 180 | 478 | 10 |
| 156,25% | 178 | 501 | 126 |
| 101% | 221 | 640 | 186 |
| 137% | 211 | 550 | 19 |
| 301% | 628 | 1.734 | 177 |

Dit zijn samplingfaseverschillen, geen bewijs van verkeerde JPEG-decode. Bij
87% betroffen ze kolommen 475–477; bij 156,25% kolom 636. De tweede kandidaat
reproduceert de relevante numerieke conventie: herstart op 256 kolommen,
f32-positieaccumulatie langs horizontale beelden en vier verticale lanes met
f32 multiply-add. De kwartslagen gebruiken de overeenkomstige inverse assen.
Deze kandidaat is op alle twintig externe gevallen exact. De eerste kandidaat
blijft reproduceerbaar beschikbaar; het JSON bevat beide hashes, alle aantallen
en de eerste 24 afwijkende kanaalposities per geval.

De scalar-rekenwijze is geïnformeerd door lokaal vastgepinde
`vello_cpu 0.0.8/src/fine/common/image.rs`, `fine/mod.rs` en
`vello_common 0.0.8/src/{coarse,encode}.rs` (MIT/Apache-2.0). Het is geen
volledig onafhankelijk ontworpen samplingconventie. De MIT-bronvermelding staat
in `../licenses/vello-mit.txt`; er is geen nieuwe Vello-runtimeafhankelijkheid.

## Grenzen en regressies

- Eén opaque DeviceRGB8-JPEG vult de volledige MediaBox, met bestaande strikte
  q/Q/cm/g/Do- en resourcevalidatie. Geen overlays, transparantie of nieuwe clips.
- Alleen downsampling in beide assen. Bron- en decodegrenzen blijven 16 MiB,
  8192 per bronzijde, 40 miljoen bronpixels / 120 MB RGB.
- Doel én tussenrooster maximaal 4096 per zijde en zes miljoen pixels. Geen
  afgeronde 100%-bitmap als bron; iedere schaal resamplet oorspronkelijke RGB.
- PageSpace controleert originele f64-paginageometrie en bitmapdimensies.
- Twintig gegenereerde PDF-gevallen: vier kwartslagen × vijf fractionele schalen,
  een verschoven CropBox en de bestaande 81×81-kleurenfixture. Volledige RGBA-
  vergelijking met Hayro. Een eerste f64-kandidaat had bij 270°/33% nog 117
  afwijkende kanalen; ook dat geval is met de f32-lanes exact.
- NaN, oneindig, nul, negatieve/grote schaal en upsampling worden afgewezen.
  De gewone adapter wordt door deze diagnostische route niet verruimd.

Er is geen prestatieclaim. De CLI geeft diagnostische tijden, maar dit is geen
gepaarde benchmark, geen GUI-latentiemeting en geen vergelijking van gelijke
interne caches. De probe houdt één decodebuffer, maar haar eigenaarschap vervangt
niet de bestaande documentbrede mutex/LRU-cache; dat moet bij integratie behouden
blijven. DynLex gebruikt nog steeds LLVM/C++/libc in zijn toolchain.

## Reproduceren

Vanuit `rust-studio`:

```sh
cargo build --release -p opds-dynlex-backend --features kernel --bin fractional_jpeg_probe
cargo test -p opds-dynlex-backend --features kernel --test jpeg_zoom
python3 scripts/diagnose_fractional_jpeg.py \
  --candidate target/release/fractional_jpeg_probe \
  --oracle target/release/backend_probe \
  --manifest docs/public-pdf-corpus-2026-10-08.json \
  --output /tmp/fractional-jpeg-diagnosis.json
```

Opgeslagen resultaat: `dynlex-fractional-jpeg-diagnosis-2026-10-08.json`.
Volgende stap: bredere differentiële JPEG-/rotatie-/CropBox-proeven rond
samplinggrenzen en vervolgens gecontroleerde integratie met de bestaande
documentbrede decodecache, strikte foutafwijzing en native Ctrl+scroll-tests.
Twintig schalen van één scan bewijzen geen algemene image-placementpariteit.

Validatie: 249 workspace-tests met alle features, standaard native check,
releasebuild van de proef, formattering en Python-compile slagen. De volledige
bestaande verifier blijft exact: 47 PDF-pagina’s / 4.964.352 pixels plus drie
v4-PDF’s / 147.456 pixels. Geen native appbinary vervangen.
