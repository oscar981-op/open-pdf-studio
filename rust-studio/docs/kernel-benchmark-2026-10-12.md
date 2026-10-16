# Nieuwe vier-kernelmeting op echte PDF's (12 oktober)

De strikte CLI-probe gebruikte telkens hetzelfde originele PDF-bestand, dezelfde
pagina en schaal. Elke sample startte een nieuw proces na een aparte preflight;
de volgorde wisselde per ronde. De OS-cache was warm, interne rendercaches zijn
niet gelijkgetrokken. De tijden omvatten processtart, lezen, parsen, renderen
en RGBA schrijven. Dit is geen GUI- of eerste-zoomlatentie. Een geweigerde
pagina heeft geen snelheidsscore.

| Bron/pagina/schaal | Hayro mediaan | PDFium mediaan | DynLex mediaan | Eigen Rust |
|---|---:|---:|---:|---|
| Albany-contract p6, 100%, 10 rondes | 23,76 ms | 24,99 ms | 127,67 ms | unsupported |
| Originele Tekst.pdf p5, 100%, 10 rondes | 56,83 ms | 63,49 ms | 125,58 ms | unsupported |
| Albany-drawings p3, 25%, 3 rondes | 1111,56 ms | 1001,01 ms | unsupported | unsupported |
| NPS-schematic p3, 25%, 3 rondes | 96,51 ms | 93,52 ms | unsupported | unsupported |

DynLex en Hayro leverden op de twee ondersteunde pagina's identieke volledige
RGBA-buffers. PDFium week op Albany p6, Albany-drawings p3 en NPS p3 op
respectievelijk 25.307, 36.003 en 48.675 pixels van Hayro af. Bij Tekst p5
was de PDFium-buffer 596×842 tegenover 595×841, dus niet pixelvergelijkbaar.
Dit is geen oordeel dat PDFium inhoudelijk onjuist is; de rendercontracten en
antialiasing kunnen verschillen. De grote Albany-tekening is een hoge-resolutie
scan, geen vectorpagina. NPS is wel een technisch vector-schema.

De `/usr/bin/time`-RSS van de CLI-probe omvat niet noodzakelijk de apart
gestarte DynLex-tekstworker. Een extra 1 ms-procesboomsampling op Tekst p5
vond een piek van 11.696 KiB ouder, 19.072 KiB kind en 26.728 KiB som. Die
som is een gesamplede ondergrens, telt gedeelde pagina's dubbel en is geen
PSS. De andere RSS-waarden zijn daarom niet eerlijk vergelijkbaar met deze
som. Geen algemene snelheidswinst of geheugenzuinigheid is aangetoond.

Ruwe per-run tijden, hashes, afmetingen, preflightstatus en probe-/PDFium-binaryhashes
staan buiten Git in
`~/.cache/opds-benchmarks/current-four-kernels-2026-10-12.json` en
`~/.cache/opds-benchmarks/current-four-kernels-supported-10rounds-2026-10-12.json`.
De procesboomsampling staat in
`~/.cache/opds-benchmarks/current-dynlex-tree-rss-2026-10-12.json`.
