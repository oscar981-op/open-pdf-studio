# Fractionele tekstclip: afgeronde dekkingsmaskers zijn niet voldoende

De bestaande strikte DynLex-tekstscene accepteert alleen `re W/W* n` wanneer
alle vier randen na paginatransformatie exact op gehele pixels liggen. De
eerste echte Albany-clip op pagina 11 is fractioneel en wordt daarom zonder
bitmap afgewezen. Voor een algemene fractionele route is een afzonderlijke
dekkingsproef uitgevoerd; **er is geen native acceptatie toegevoegd**.

`scripts/diagnose_fractional_text_clip.py` maakt 24 deterministische PDF's
met één zwarte tekstpaint en een fractionele rechthoekclip, verspreid over
0/90/180/270 graden, twee CropBoxes en 25/100/200% zoom. De ongeclipte
DynLex-tekst is in elk geval bytegelijk aan Hayro. Hayro wordt uitsluitend
als oracle gebruikt om drie afzonderlijke beelden te vergelijken: ongeclipte
tekst, geclipte tekst en een zwart volvlak achter dezelfde clip. De onderzochte
kandidaat vermenigvuldigt de **al naar 8 bit afgeronde** glyphdekking met de
**al naar 8 bit afgeronde** zichtbare clipdekking en rondt opnieuw.

Deze eenvoudige kandidaat is slechts **12/24 gevallen exact**. Er zijn in
totaal 46 afwijkende pixels; de grootste dekkingsfout is negen 8-bit-stappen.
Een opvallend voorbeeld heeft een volledig dekkende glyphpixel (255), maar
de afzonderlijk geschilderde clip geeft 11 en de werkelijk geclipte glyph 20.
Dit verschil kan niet worden gerepareerd door alleen een andere afronding van
`glyph_u8 * clip_u8 / 255`. De volledige samples, bron- en binaryhashes staan
in `docs/dynlex-fractional-clip-diagnosis-2026-10-08.json`.

De lokale MIT/Apache-2.0 Vello CPU 0.0.8-bron is als oracle bekeken: het
clip-/paintpad werkt met stripdekking en afzonderlijke laag-/maskercompositie.
De bovenstaande drie gerenderde Hayro-beelden bewijzen niet dat de los
geschilderde zwarte rechthoek exact dezelfde interne clipdekking heeft als een
geclipte glyph. De volgende kandidaat moet daarom de oorspronkelijke
fractionele clipgeometrie en glyphstripwinding tegelijk volgen, met één
uiteindelijke 8-bit-kwantisatie of een expliciet gevalideerde equivalente
compositie. Een product van twee afgewerkte bitmaps is geen veilige shortcut.

Reproduceren met de bestaande release `backend_probe`:

```sh
python3 scripts/diagnose_fractional_text_clip.py \
  --output /tmp/dynlex-fractional-clip-diagnosis.json
```

Deze diagnose wijzigde de productieroute, worker-IPC en appbinary niet.
Een latere, afzonderlijke uitbreiding accepteert alleen fractionele clips
die aantoonbaar geen paint raken; zie
`dynlex-inactive-fractional-clip-2026-10-08.md`. Actieve fractionele
clipranden blijven strikt unsupported zonder Hayro-fallback. Er is geen
algemene snelheids- of PDF-dekkingsclaim.

## Stripmasker-afronding nader onderzocht

De lokale Vello 0.0.8-clipcode gebruikt voor twee 8-bit-stripmaskers
`(a * b + 255) >> 8`. Met **dezelfde afgewerkte zwartvlakbitmap als proxy**
voor het interne clipmasker is deze variant 23/24 gevallen exact, tegenover
12/24 voor de eerdere gewone afronding. Er blijven twee afwijkende pixels in
één 270°/CropBox/200%-geval, met maximaal negen dekkingsstappen fout.
`dynlex-fractional-clip-div255-2026-10-08.json` bevat alle waarden en hashes;
dezelfde scriptreplay berekent nu beide hypothesen.

Dit is nog geen bewijs voor per-paint clipping: de ongeclipte tekstbitmap
bevat reeds de compositie van meerdere glyphs, terwijl de echte clip tijdens
iedere afzonderlijke paint wordt toegepast. Een aparte read-only oracle-proef
bevestigt nu de overlap op beide afwijkende pixels. De glyphs **D** en **G**
hebben daar afzonderlijk geclipte dekkingen 9 en 11, respectievelijk 11 en 7.
Compositie in paintvolgorde geeft 20 en 18, exact de volledige Hayro-uitvoer.
De samengevoegde ongeclipte dekking is daar al 255 en kan die informatie niet
meer herstellen. Bron-/binaryhashes en replay staan in
`dynlex-fractional-clip-overlap-2026-10-08.json` en
`scripts/diagnose_fractional_text_overlap.py`. De afzonderlijk geclipte
glyphs kwamen uitsluitend uit de testoracle, nooit als DynLex-productie-invoer.

De volgende kandidaat moet per **afzonderlijke DynLex-glyphpaint** een eigen
rechthoekig clipstripmasker vormen, `(glyph_u8 * clip_u8 + 255) >> 8`
toepassen en dan in PDF-paintvolgorde compositeren. Daarna moeten alle
24 complete gevallen en echte Albany-pagina's bij meerdere schalen exact
zijn voordat native acceptatie mogelijk is. Native actieve fractionele clips
blijven geweigerd.
