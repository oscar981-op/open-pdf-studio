# Experimentele OPGM-v2-laagvolgorde

De aparte PDF→OPGM-v2-exporter accepteert één pagina met geërfde
MediaBox/CropBox/Rotate en 1–64 volledige `rg`/`re`/`f`-trio's. Iedere
gevulde rechthoek moet geheel binnen de zichtbare CropBox liggen en
pixeluitgelijnde gehele coördinaten hebben. Per RGB-kanaal zijn alleen 0
en 1 toegestaan; het resultaat is dekkend. Andere PDF-operatoren,
compound-operanden, negatieve/fractionele afmetingen en onafgemaakte
trio's worden geweigerd. Dit is geen algemene PDF-renderer.

OPGM-v2 begint met `OPGM`, versie 2 en negen little-endian `u32`-woorden:
MediaBox-breedte/hoogte, CropBox `[links, onder, rechts, boven]`,
kwartslag `0/90/180/270` en opdrachtenteller. Daarna volgen precies zoveel
records van 24 bytes: `kind=1`, displayrechthoek `[links, boven, rechts,
onder]` als vier `u32` en RGBA als vier bytes. Maximale invoerlengte is
40+64×24 = 1576 bytes. De Rust-probe valideert lengte, afmetingen,
pixelgrens van 6 miljoen, recordsoort, rechthoeken, binaire RGB en alpha
voordat hij de opdrachten in dezelfde volgorde aan OPDL-v2 doorgeeft.
OPGM-v1 blijft leesbaar; de app en gewone PDF-adapter gebruiken geen OPGM.

De [verifier](rgba/verify_multi_geometry.py) genereert zeven echte PDF's
met overlappende gekleurde rechthoeken of 64 afzonderlijke opdrachten.
Op vier kwartslagen,
90° met verschoven CropBox, een 64-opdrachten-grensgeval en A1-achtig
2384×1684 display zijn alle 17.161.664 RGBA-bytes gelijk tussen
PDF→OPGM-v2→DynLex, Hayro en een
onafhankelijk opgebouwde referentie. Zes niet-ondersteunde PDF's en vijf
ongeldige OPGM-v2-records worden vóór uitvoer geweigerd. Het
[rapport](rgba/multi-geometry-2026-10-05.json) houdt de gevallen vast.

```sh
cd rust-studio
cargo build --release --offline -p opds-opdl-adapter --bin export_geometry_v2
rustc --edition=2024 -O dynlex-engine-poc/rgba/page_geometry_probe.rs \
  -o /tmp/opds-page-geometry-probe
python3 dynlex-engine-poc/rgba/verify_multi_geometry.py \
  --probe /tmp/opds-buffer-probe-large \
  --geometry /tmp/opds-page-geometry-probe \
  --exporter target/release/export_geometry_v2 \
  --hayro target/release/opds-engine-probe
```

De statische DynLex-bufferlibrary en `buffer_probe` worden vooraf volgens
de [v1-proef](opgm-v1-geometry-2026-10-04.md) gebouwd. De v2-proef
vervangt geen tekst, paden, afbeeldingen, clipping of transparantie en
bewijst niets over de volledige echte A1-tekening.
