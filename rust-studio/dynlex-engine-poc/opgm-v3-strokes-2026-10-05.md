# Experimentele OPGM-v3: fills en asgerichte strokes

De aparte PDF→OPGM-v3-route accepteert één pagina met geërfde
MediaBox/CropBox/Rotate, lege resources en maximaal 64 opdrachten in
PDF-laagvolgorde. Elke opdracht is precies `rg re f` of
`RG 1 w m l S`. RGB-kanalen zijn uitsluitend 0 of 1. Een stroke moet
dekkend, 1 pt breed, horizontaal of verticaal en pixeluitgelijnd zijn;
beide eindpunten vallen op gehele coördinaten langs de lijn en op
halfpixelcoördinaten erdwars. De volledige strook moet binnen de
zichtbare CropBox liggen. Andere graphics state, capstijl, curven,
diagonalen, clipping, tekst en afbeeldingen worden geweigerd. Tot twee
geneste `q`/`Q`-groepen mogen een gehele `1 0 0 1 tx ty cm`-translatie
toepassen op fills en strokes; iedere groep herstelt de vorige translatie.
Schaal, schuifstand, kwartslag via `cm` en niet-gesloten groepen worden
geweigerd. De paginarotatie zelf komt uit `/Rotate`.

OPGM-v3 hergebruikt de begrensde v2-header (40 bytes) en records van
24 bytes. `kind=1` is gevulde rechthoek; `kind=3` is één 1px-strook.
De Rust-probe valideert alle lengtes, afmetingen, kleuren en strookdikte
alvorens deze records in volgorde aan de bestaande OPDL-v2-DynLex-kernel
te geven. De normale PDF-adapter en app gebruiken deze route niet.

`rgba/verify_stroke_geometry.py` genereert negen echte PDF's met
overlappende vormen op 0/90/180/270°, een verschoven CropBox en een
A1-achtige pagina, geneste translatie en herstel na `Q`. De
PDF→OPGM→DynLex-uitvoer is 33.416.896 RGBA-bytes
exact gelijk aan Hayro en een onafhankelijke rechthoek-/strookreferentie.
Acht niet-ondersteunde PDF-states en vier corrupte OPGM-v3-records
worden zonder uitvoer geweigerd. Dit bewijst geen algemene padpariteit
of snelheid op echte technische tekeningen.

```sh
cd rust-studio
cargo build --release --offline -p opds-opdl-adapter --bin export_geometry_v3
rustc --edition=2024 -O dynlex-engine-poc/rgba/page_geometry_probe.rs \
  -o /tmp/opds-page-geometry-probe-v3
python3 dynlex-engine-poc/rgba/verify_stroke_geometry.py
```

De bestaande DynLex-bufferprobe wordt vooraf volgens de OPGM-v1-proef
gebouwd als `/tmp/opds-buffer-probe-large`.
