# Inventarisatie van echte tekeningen voor de DynLex-kern

Bij deze oorspronkelijke inventarisatie accepteerde de PDF→OPDL-v2-adapter
één ongeroteerde nul-oorsprongpagina van maximaal 1024×1024 pixels en
16.384 tekenopdrachten (aanvankelijk 64). De latere experimentele
geordende route ondersteunt een bewezen subset op geroteerde/gecropte pagina's
tot 4096 pixels per zijde en 6 miljoen pixels totaal. Zij rendert nog steeds
geen volledige technische tekening. Deze inventarisatie leest alleen
PDF-inhoud; ze verruimt de render- of parseclaims niet.

Het Rust-commando `inspect_pdf` telt PDF-contentoperators per pagina, volgt
geërfde MediaBox en Rotate in de paginaboom en toont de zestien meest gebruikte
operators. Form-XObject-inhoud wordt nog niet recursief geteld. Reproduceren:

```sh
cargo run --release -p opds-opdl-adapter --bin inspect_pdf -- \
  '../test pdf-bestanden/Originele bestanden/Technische tekening.pdf' --page 2
```

`Technische tekening.pdf` heeft vier A1-pagina's van 1684×2384 pt met
`/Rotate 90`. Hun contentstreams bevatten respectievelijk 14.052, 29.705,
27.359 en 14.310 operators. Pagina 2 alleen bevat 6.343 `l`, 6.242 `m`,
5.763 `S`, 4.898 `w`, 616 `c`, 463 `cm` en 315 `BT`/`Tj`-reeksen.

`Combinatie Raster, vector, tekening images.pdf` heeft één pagina van
1684×1191 pt met 8.721 operators, waaronder 4.034 `c`, 2.697 `l`,
375 `f`, 43 `W`, 12 `W*` en 47 `cm`. De read-only `inspect_pdf --clip-ops`
classificeert 41 clipopdrachten als één `re`-rechthoek en 14 als
`m/l/h`-paden. Twee van die 14 paden zijn vierhoekige asgerichte
rechthoeken (`W`); de overige twaalf gebruiken `W*` met 5 tot 327
lijnsegmenten en vragen nog algemene polygonale clipping. Veel
rechthoekcoördinaten zijn na de globale 0,1-schaal
fractioneel; een gecontroleerde subpixel-rechthoek wordt nu via OPDL-v6
beelddekking bytegelijk aan Hayro gerenderd. Een aparte proef ondersteunt
alleen één kleine convexe polygonale beeldclip met 3–8 zijden en zekere
8-bit-randdekking; deze twaalf complexe `W*`-paden zijn daarmee niet als
ondersteund aangetoond.
Een read-only image-inventaris op 5 oktober
vond 38 image-XObjects: 36 DeviceRGB8/FlateDecode-stroken met PNG-predictor
15 (meestal 3843×68, laatste 3843×25), plus twee Indexed/DeviceRGB-beelden.
De content begint met `0.1 0 0 0.1 0 0 cm`; daarop volgen fractionele
beeldtransformaties. De native DynLex-route ondersteunt Predictor 15 voor
een beperkte RGB8-subset, geneste positieve asgerichte CTM's en inmiddels
fractionele beeldhoeken met een per-pixel binair dekkingsmasker. Een
gecontroleerde 3843×68-strook met op deze PDF gebaseerde geometrie is
pixelgelijk aan Hayro; de oorspronkelijke pagina is daarmee nog niet
ondersteund: duizenden curven en clippaden ontbreken. De twee Indexed-
DeviceRGB8-objecten hebben elk 256 kleuren en een lookupstring van 768 bytes;
deze vorm heeft nu een begrensde pure-Rust decodeproef met Hayro-pixelcontrole.
Reproduceer de metadata met
`inspect_pdf --image-ops` en bekijk de eerste beeldopdracht met
`inspect_pdf --around 4855` op dit bestand. De naam alleen bewijst geen
precieze raster-/vectorverdeling.

Deze aantallen geven prioriteit aan een schaalbare pad-display-list met
`m/l/c/h`, graphics-state/CTM, clipping en Indexed-kleuren. Tekst en fonts
zijn voor de A1-tekening direct relevant.
Een snellere 45°-lijn alleen maakt deze documenten niet renderbaar. Elke nieuwe
deelset moet eerst per pagina en resolutie tegen Hayro/Poppler worden
vergeleken; buiten de bewezen deelset blijft de bestaande Hayro-route actief.
