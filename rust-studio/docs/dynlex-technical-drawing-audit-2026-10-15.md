# Strikte DynLex-proef op een originele technische tekening

Bron: `test pdf-bestanden/Originele bestanden/Technische tekening.pdf`,
vier pagina's. Deze proef gebruikt de bestaande `backend_probe` op 25% en
beoordeelt uitsluitend strikte DynLex-uitvoer. Een afwijzing levert geen
bitmap; de test bewijst geen ondersteuning voor dit document.

```sh
rust-studio/target/debug/backend_probe \
  'test pdf-bestanden/Originele bestanden/Technische tekening.pdf' \
  /tmp/opds-technical-page3.rgba --dynlex-strict --scale=0.25 --page=3
```

| Pagina | Eerste begrenzing | Status |
| --- | --- | --- |
| 1 | Negen zichtbare annotaties: FreeText, Line en Square. Vier FreeText-annotaties en één Square hebben een appearance stream. | Unsupported, geen bitmap |
| 3 | Geen annotaties. Een veilige Flate `/DecodeParms << /Predictor 1 >>` komt voor; na ondersteuning daarvan bereikt de worker de font/resource-validatie en weigert met `DictKey`. | Unsupported, geen bitmap |

Pagina 3 gebruikt Type0/Identity-H-fonten met CIDFontType2-afstammelingen.
Hun `CIDSystemInfo` bevat `PDFAUTOCAD` / `Indentity0` en de onderzochte
fontwoordenboeken hebben geen expliciete `CIDToGIDMap`. De huidige worker
bewijst alleen de beperkte, gevalideerde fontmapping; een ontbrekend veld
als Identity behandelen zonder volledige glyph- en paginapixelvergelijking
zou een onjuiste bitmap kunnen geven. Dit is een diagnose, geen bewijs dat
de fontmapping de enige resterende blokkade is.
Een kortstondige, niet-behouden onderzoeksvariant die ontbrekende
`CIDToGIDMap` als Identity en de aangepaste CIDSystemInfo accepteerde,
bereikte daarna `unsupported image CTM`. Die variant is direct teruggedraaid:
ook met die aannames zou de originele pagina nog niet volledig renderen en
er is geen volledige pixelproef die de aannames rechtvaardigt.

De no-op-Flate-parameter wordt nu alleen als exact
`/DecodeParms << /Predictor 1 >>` bij een enkelvoudige `FlateDecode`
geaccepteerd. Andere parameters blijven unsupported. Een afzonderlijke
tekstfixture met dezelfde gecomprimeerde content is op 25%, 100% en 200%
over de volledige RGBA-buffer gelijk aan de testoracle; een Predictor 2
wordt afgewezen. De originele technische pagina is daarmee niet ontgrendeld.

Vervolg: maak voor pagina 3 een geïsoleerde font-/contentproef en controleer
glyphmapping, paden en volledige paginapixels op meerdere schalen. Behandel
pagina 1 pas na aparte appearance- en annotatievolgordeproeven. Behoud
`unsupported` zonder bitmap bij iedere onbewezen combinatie.
