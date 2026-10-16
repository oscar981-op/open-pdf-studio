# PdfCraft naast de bestaande renderers — 13 oktober 2026

PdfCraft 0.7.1 is onderzocht op upstream commit
`3f044b6263aa6605c3aa44ca6d065e1ab37d13b6`. De huidige
`pdfcraft-render` noemt zijn rasterisatie een bootstrap via **Hayro en
Vello CPU**. PdfCraft is daarom een afzonderlijke, expliciet gelabelde
renderpipeline, geen onafhankelijke vijfde rasteralgoritme en nooit een
DynLex-fallback. De Rust-app laadt PdfCraft niet in zijn eigen proces: de
gebruiker installeert de optionele `pdfcraft-cli` naast de app, op `PATH`,
of wijst hem aan met `OPDS_PDFCRAFT_CLI`. De keuze verschijnt als
`PdfCraft (Hayro)`.

Bronnen: upstream [`crates/render/src/lib.rs`](https://github.com/storytold/pdfcraft/blob/3f044b6263aa6605c3aa44ca6d065e1ab37d13b6/crates/render/src/lib.rs)
en [`crates/render/Cargo.toml`](https://github.com/storytold/pdfcraft/blob/3f044b6263aa6605c3aa44ca6d065e1ab37d13b6/crates/render/Cargo.toml).

De geselecteerde hoofdweergave rendert nu strikt via de gekozen route.
DynLex, PDFium, Eigen Rust en PdfCraft lenen bij een afwijzing geen
Hayro-bitmap. PDFium is beschikbaar als `libpdfium.so` op het
systeempad, naast de executable, of via `OPDS_PDFIUM_LIBRARY`. De
optionele PdfCraft-CLI draait met een begrensd bronbestand, een eigen
privé-werkmap, Linux-proceslimieten (adresruimte 1 GiB, CPU 15 s,
uitvoer 64 MiB), een wandtijdlimiet van 20 s en strikte PAM-validatie
voor maximaal acht miljoen pixels. Een CLI-waarschuwing, mislukking of
ongeldige uitvoer geeft geen bitmap.
Afzonderlijke probes met een falende CLI, een afgeknotte PAM en een
opzettelijk hangende CLI leverden alle drie geen outputbestand op;
de hang werd na 20 seconden afgebroken. De definitieve adapter doodt
bij een timeout ook de aparte Linux-procesgroep van de CLI.

| Pagina op 100% | Hayro | PDFium | DynLex | Eigen Rust | PdfCraft |
| --- | --- | --- | --- | --- | --- |
| Kleine `simple-fill-stroke.pdf` | 256×192 | 256×192 | 256×192 | 256×192 | 256×192 |
| Originele `Tekst.pdf` p1 | 595×841 | 596×842 | unsupported | unsupported | 596×842 |
| Originele `Tekst.pdf` p5 | 595×841 | 596×842 | 595×841 | unsupported | 596×842 |

Op de kleine pagina zijn **alle vijf** RGBA-buffers bytegelijk:
196.608 bytes, SHA-256
`32847d8df388d8d70ffc9a0fbc2b0347276a67a9ea0afcaedad5650adfd8257b`.
Op de originele hybride pagina 5 zijn Hayro en DynLex bytegelijk op
595×841. PdfCraft en PDFium gebruiken daar een 596×842-raster; hun
onderlinge beelden verschillen op 30.572 van 501.832 pixelposities.
Dit is een renderer-/afrondingsverschil, geen bewijs dat één van beide
fout is. Vanwege de verschillende afmetingen is een directe
pixelgelijkheid met Hayro hier niet gedefinieerd.

Drie afwisselend gestarte processen op `Tekst.pdf` p5, warme
OS-bestandscache en geen hergebruik van interne rendercaches, gaven de
volgende **alleen op deze pagina geldige** medianen. Tijd omvat
processtart, invoer, rendering en RGBA-schrijfwerk. PdfCraft start daarbij
nog een afzonderlijke CLI; DynLex start zijn tekstworker. RSS is de
1 ms-gesamplede piek van ouder plus levende kinderen: een ondergrens
die gedeelde pagina's dubbel kan tellen. Iedere route leverde in zijn
drie runs steeds dezelfde hash.

| Route | Wandtijdmediaan | Gesamplede ouder+kind-RSS-mediaan |
| --- | ---: | ---: |
| Hayro | 54,47 ms | 19,2 MiB |
| PDFium | 72,40 ms | 22,1 MiB |
| DynLex | 124,29 ms | 26,1 MiB |
| PdfCraft (Hayro) | 83,02 ms | 28,7 MiB |

`Eigen Rust` ondersteunt deze echte vijfde pagina niet en krijgt dus
geen snelheidsscore. Deze ene pagina en drie rondes zijn geen algemene
snelheids- of dekkingsranglijst. Meetdata staan buiten Git in
`/tmp/opds-pdfcraft-paired-tekst5.json`.

In een nieuw, automatisch sluitend Rust-testvenster is de originele
kleine PDF zichtbaar onder Hayro, DynLex, PDFium en PdfCraft. Eigen Rust
weigerde eerst de automatisch berekende 136%-startzoom, hoewel hij de
pagina op 100% aankan. De app probeert voor een afgewezen startzoom nu
dezelfde geselecteerde kernel op 100%; hij schakelt niet naar een
andere kernel. Een tweede geïsoleerde GUI-opname na de release-herbouw
toont de pagina op 100% met `Renderer: Eigen Rust`:
`/tmp/opds-ui-own-rust-fixed.png`. Bestaande gebruikersvensters zijn
niet herstart.

De externe PdfCraft-repository en binaire testbouw staan buiten Git.
De app koppelt zelf geen PdfCraft-, Hayro/Vello- of JavaScript-runtime
aan DynLex. De volledige PdfCraft-CLI bevat eigen afhankelijkheden
voor andere functies; alleen de expliciete optionele renderopdracht
wordt vanuit de app aangeroepen. Een productdistributie moet de
PdfCraft- en PDFium-binaries apart installeren en hun herkomst tonen.
