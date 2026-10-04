# Experimentele paginageometrie OPGM-v1

OPGM is een afzonderlijke, **niet-productieve** testcontainer voor één dekkend
rood rechthoekje. Een strikt beperkte Rust-PDF-exporter leest precies één
expliciete rode `rg`, één `re` en één `f`, inclusief geërfde paginaformaten,
CropBox en Rotate. De Rust-probe valideert de OPGM, zet het rechthoekje uit
zichtbare schermcoördinaten om naar PDF-coördinaten en schrijft een bestaande
OPDL-v2-vectorscene voor de DynLex-bufferkernel. De gewone PDF-adapter en app
lezen OPGM niet.

Alle velden zijn little-endian `u32`. Na `OPGM` en versie `1` volgen:
MediaBox-breedte/hoogte, CropBox `[links, onder, rechts, boven]`, rotatie
`0/90/180/270`, displayrechthoek `[links, boven, rechts, onder]`, gevolgd
door vier inktbytes `ff 00 00 ff`. Het record is exact 56 bytes. Grenzen:
MediaBox-zijden maximaal 4096, CropBox binnen MediaBox, display maximaal
6.000.000 pixels, rechthoek binnen display. Ongeldige versie, rotatie,
lengte, crop, kleur of afmeting wordt vóór OPDL-uitvoer geweigerd.

De aparte `--experimental-large`-optie van `buffer_probe.rs` accepteert
OPDL-v2-vectorpagina's tot 4096 per zijde en 6.000.000 pixels, zonder
RGB-beeldlaag. Zonder die optie blijft de bestaande 1024-limiet gelden.
Voor grotere pagina's is uitsluitend vectormodus toegestaan. Deze proef
maakt geen aanspraak op algemene PDF-coördinaten, tekst, curven, andere
annotaties of een echte A1-tekening.

De [verifier](rgba/verify_large_rotation.py) vergelijkt zeven gecontroleerde
PDF's met de DynLex-kernel, Hayro en een onafhankelijke RGBA-referentie:
vier kwartslagen, een verschoven CropBox, een geërfde MediaBox en een A1-achtige
1684×2384-pt pagina bij 90° (zichtbaar 2384×1684). Alle 17.161.664
RGBA-bytes zijn gelijk. Drie ongeldige OPDL-scenes, twee ongeldige grote
renderstanden, vijf ongeldige OPGM-records en negen niet-ondersteunde PDF's
worden geweigerd. Het
[rapport](rgba/large-rotation-2026-10-04.json) bevat alle uitvoerformaten.

```sh
cd rust-studio
cargo build --release --offline -p opds-opdl-adapter --bin export_geometry_v1
rustc --edition=2024 -O dynlex-engine-poc/rgba/page_geometry_probe.rs \
  -o /tmp/opds-page-geometry-probe
rustc --edition=2024 -O dynlex-engine-poc/rgba/buffer_probe.rs \
  -L native=/tmp/opds-dynlex-buffer-build -L dependency=target/release/deps \
  --extern opds_opdl_adapter=target/release/libopds_opdl_adapter.rlib \
  -o /tmp/opds-buffer-probe-large
python3 dynlex-engine-poc/rgba/verify_large_rotation.py \
  --probe /tmp/opds-buffer-probe-large \
  --geometry /tmp/opds-page-geometry-probe \
  --exporter target/release/export_geometry_v1 \
  --hayro target/release/opds-engine-probe
```

De statische DynLex-bufferlibrary wordt vooraf met `rgba/build_buffer.py`
gebouwd volgens de hoofd-README; de `/tmp`-paden zijn voorbeelden, geen
permanente toolchainlocaties. De bovengrens van de C-ABI-wrapper is nu
3.538.984 scenebytes, gelijk aan de Rust-OPDL-v2-validator.
