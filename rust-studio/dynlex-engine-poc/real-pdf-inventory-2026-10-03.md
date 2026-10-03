# Inventarisatie van echte tekeningen voor de DynLex-kern

De huidige PDF→OPDL-adapter accepteert bewust één ongeroteerde pagina met
een nul-oorsprong, maximaal 1024×1024 pixels en hoogstens 16.384
tekenopdrachten. De oorspronkelijke limiet bij de inventarisatie was 64.
Dat is voldoende voor pixelcontrole van de beperkte testfixtures, maar niet
voor de technische tekeningen in deze repository. Deze inventarisatie leest
alleen PDF-inhoud; ze verruimt de render- of parseclaims niet.

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
375 `f`, 43 `W` en 47 `cm`. Ook deze pagina past niet in de huidige
dimensielimiet; de naam alleen bewijst geen precieze raster-/vectorverdeling.

Deze aantallen geven prioriteit aan een schaalbare pad-display-list met
`m/l/c/h`, graphics-state/CTM en clipping, plus tegelcoördinaten voor pagina's
groter dan 1024. Tekst en fonts zijn voor de A1-tekening direct relevant.
Een snellere 45°-lijn alleen maakt deze documenten niet renderbaar. Elke nieuwe
deelset moet eerst per pagina en resolutie tegen Hayro/Poppler worden
vergeleken; buiten de bewezen deelset blijft de bestaande Hayro-route actief.
