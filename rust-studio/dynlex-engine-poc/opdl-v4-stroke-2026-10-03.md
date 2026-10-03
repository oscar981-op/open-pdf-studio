# Experimentele OPDL-v4-strokelijst

`crates/opdl-adapter/src/paint_v4.rs` bewaart meerdere strokepaden in
bestandsvolgorde. Een aparte `export_strokes_v4`-proef leest nu geïsoleerde
PDF-strokes met expliciete `RG`-kleur en `w`-lijnbreedte. Dit is een
**niet-renderend opslagcontract**: er is nog geen DynLex-v4-renderer of
app-koppeling. De gewone PDF-adapter blijft OPDL-v2 schrijven en weigert curven.

De header blijft 20 bytes: `OPDL`, versie `u32=4`, paginabreedte `u32`,
paginahoogte `u32` en recordaantal `u32`, little-endian. Elk record begint met
totale lengte `u32`, opcode `u8` en drie nulbytes. De geometriecodes blijven
apart van v3, zodat oudere versies ongewijzigd blijven:

| Opcode | Inhoud na recordheader | Totale lengte |
| --- | --- | ---: |
| 1 `m` | x,y als twee `f64` | 24 |
| 2 `l` | x,y als twee `f64` | 24 |
| 3 `c` | drie x,y-paren als zes `f64` | 56 |
| 4 `h` | geen | 8 |
| 5 StrokeStyle | RGB8, alpha=255, vier nulbytes, breedte als `f64` | 24 |
| 6 Stroke | geen; sluit en schildert het opgebouwde pad | 8 |

De validator staat alleen dekkende RGB-stijlen en eindige breedtes groter dan
nul tot 1000 toe. Een stijl moet vóór padgeometrie staan; een pad vereist
`m` en minstens één `l` of `c` vóór `Stroke`. Een stijlwijziging midden in een
onafgemaakt pad, ontbrekende stroke, onbekende opcode, verkeerde lengte,
niet-nulflags, niet-eindige coördinaten, overtollige bytes en truncatie worden
geweigerd. De harde grenzen zijn 16.384×16.384 pixels, 16.384 records en
917.524 inputbytes. Een test muteert elke byte van een geldige tweestrokelijst
en bevestigt dat decoderen niet crasht; dit bewijst geen volledige
fuzzdekking.

V4 bewaart nog geen fill, transparantie, cap/join, clip, CTM, lettertype of
anti-aliasregel. De `Stroke`-records begrenzen de paden en leggen laagvolgorde
vast, maar een renderer mag deze lijst pas tekenen na pixelvergelijking met
Hayro op echte PDF's. De bestaande v3-geometrie-export en
[`curve-reference-2026-10-03.json`](rgba/curve-reference-2026-10-03.json)
blijven daarvoor afzonderlijke referenties.

De PDF-proef gebruikt de begrensde `PathStage` en accepteert één ongecropte,
ongeroteerde pagina met nul-oorsprong en uitsluitend `RG`, `w`, `m`, `l`, `c`,
`h`, `S`. Kleur en breedte moeten vóór het eerste pad expliciet staan. De
actuele stijl wordt vóór elk afgesloten pad in de lijst geschreven; zo blijven
PDF-strokegrenzen en verfvolgorde expliciet. Clip, transformatie, alpha,
andere PDF-operators en een onafgemaakt pad worden afgewezen.

```sh
cd rust-studio
cargo build --release -p opds-opdl-adapter
python3 dynlex-engine-poc/verify_strokes_v4.py \
  --exporter target/release/export_strokes_v4 \
  --v2-adapter target/release/opds-opdl-adapter
```

De echte PDF-fixture heeft op 1684×2384 punten een rode 1pt-Bézierstroke en
daarboven een blauwe 2pt-lijn. De proef controleert alle acht v4-recordtags,
RGB-bytes, breedtes, curvepunten en de afwijzing door de normale v2-adapter.
Zeven ongeldige kleur-/breedte-/padtoestanden geven geen uitvoerbestand.

`bridge_v4_axis` vertaalt **uitsluitend** 1px, pixeluitgelijnde horizontale
en verticale strokes zonder curves, clip of beeldlaag naar reeds geverifieerde
OPDL-v2-`kind=3`-opdrachten. De DynLex-v2-kernel tekent het resultaat; er is
nog geen zelfstandige v4-kernel. De brug weigert 2pt, diagonalen, `c`, `h`,
meerdere segmenten per stroke en pagina's boven 1024 pixels. Een echte
256×192-PDF met rode horizontale en blauwe verticale stroke wordt via
PDF→v4→v2→DynLex pixelgewijs met Hayro en een onafhankelijke RGBA-referentie
vergeleken in `rgba/buffer_verify.py`. Dit beperkt de eerste integratiestap
tot bestaande bewezen pixels.
