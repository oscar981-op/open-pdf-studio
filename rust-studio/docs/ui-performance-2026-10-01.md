# UI en snelheid — 1 oktober 2026

Dezelfde schermbreedte van circa 1200 px is gebruikt voor visuele vergelijking van Hakan Seven's [OpenCADStudio-referentie](https://github.com/HakanSeven12/OpenCADStudio/blob/2a15a370f6886d3e8695ab81c7d677e0cd484451/site/workspace.png), de bestaande Open PDF Studio en de huidige native shell. De lokale schermafbeeldingen van draaiende apps zijn niet in Git opgenomen.

| Gebied | OpenCADStudio | Bestaande PDF-app | Huidige Rust-shell |
| --- | --- | --- | --- |
| Ribbon | Dicht, meerdere rijen en gegroepeerde tools | Dicht, zes tabs en veel PDF-tools | Home en Annotation: titelbalk 30 px, tabs 28 px, groepen 94 px; groepsscheidingen op circa dezelfde x-posities. Andere tabs blijven voorlopig een gedeeltelijke Hakan-port |
| Documenten | CAD-tekeningen en tabs | Meerdere PDF-tabs, meldingsbalk | Eén PDF-tab, geen meldingsbalk |
| Zijpanelen | Eigenschappen, objectlijst | Uitgebreide linker en rechter panelen | Tekstuele paginalijst, eenvoudige eigenschappen |
| Pagina/viewport | CAD-canvas | PDF met selectie- en annotatielagen | PDF-bitmap met eerste native annotatie-overlay |
| Status | Meerdere controles | Pagina, zoom, meetwaarden | Pagina en zoom |

De 437 gekopieerde upstream-bestanden zijn een bronbasis, geen bewijs van dezelfde UI. De zichtbare shell gebruikt Hakan's ribbonhelpers en iconen, met PDF-specifieke iconen uit de bestaande app. De Home-ribbon is op 1200 px breedte opnieuw tegen een schermafbeelding gemeten: scheidingslijnen liggen bij circa 263/423/599/765/822 px in Rust en 262/423/599/765/822 px in de bestaande app. Het scherm als geheel is nog niet pixelgelijk: meerdere documenttabs, melding, zijpanelen en veel PDF-acties ontbreken.

## UI-interacties (releasebuild, echt venster)

Via XTEST zijn toetsenbord- en muisgebeurtenissen naar het native venster gestuurd; na elke stap is de UI opnieuw vastgelegd.

| Actie | Waarneming |
| --- | --- |
| Ctrl+scroll omhoog op de PDF | Zoom van 92% naar 162%; pagina opnieuw gerenderd |
| Gewoon scrollen op de PDF | Zichtbare pagina schuift, zoom blijft 162% |
| Home: 100%, Zoom In, Zoom Out | 100% → 125% → 100% |
| Home: Next, Last, First | Pagina 1/5 → 2/5 → 5/5 → 1/5 |
| Annotation: rechthoek, markering, selecteren, verslepen | Beide annotaties zichtbaar; selectie en verplaatsing in eigenschappenpaneel |
| Undo en Redo | Verplaatsing eerst teruggezet, daarna opnieuw toegepast |
| Delete, Pages, Properties, Open | Selectie verwijderd, panelen verborgen, PDF-paddialoog zichtbaar |
| Ribbon inklappen/uitklappen | Groepen verdwijnen en verschijnen; tabrij blijft zichtbaar |
| Venster maximaliseren, herstellen, slepen, minimaliseren | Via eigen titelbalk bediend; vensterstaat en positie gecontroleerd |
| Acht formaatgrepen op geselecteerde annotatie | Hoek vergrootte W/H van 140,7/75,8 naar 227,3/140,9 pt; oostgreep vergrootte alleen W naar 281,4 pt. Twee undo- en redo-stappen herstelden beide maten. Over de tegenoverliggende hoek slepen stopte bij circa 3 px en undo herstelde de selectie. |

Na de eerste UI-test is Ctrl+scroll uitgebreid met een muisanker op beide assen. De scrollpositie wordt pas na de geaccepteerde PDF-render aangepast; het anker wordt berekend vanaf de daadwerkelijk getoonde bitmap, ook wanneer nieuwe wielgebeurtenissen eerdere renders inhalen. In de venstertest bleef tekst bij 92% → 122% onder de muis. Vier snelle wielstappen brachten de pagina naar 283% en vier stappen terug herstelden 92% en de oorspronkelijke paginapositie. Op `Technische tekening.pdf` in een venster van 1000 × 700 px werd 23% → 40% → 23% via de UI getest; de tekening keerde terug op dezelfde positie en een rechthoekannotatie bleef plaatsbaar. De actieve CAD-toolknoppen in andere tabs gebruiken nog de oudere layout. Uitgeschakelde knoppen kunnen nog niet functioneel worden getest.

De Annotation-ribbon is op 1200 px naast de bestaande app vastgelegd. De acht groepen Drawing, Text, Stamp, Redaction, Properties, Edit, Measure en Quantities, met 15 PDF-iconen, zijn in de Hakan/Iced-port opgenomen. De gemeten groepsscheidingen in Rust liggen nu bij circa 64/215/391/493/593/693/870 px, tegenover 64/214/391/493/593/693/870 px in de bestaande app. Highlight is via de echte UI getest: slepen maakte een annotatie aan; Ctrl+scroll bracht de pagina met die annotatie van 92% naar 122% met cursorvast anker. Callout, Note, Stamp, Signature, Parametric Symbol, redactie, property-invoer, wissen, meten en Quantities zijn nog uitgeschakeld. Text Box is nu functioneel voor één regel tekst: slepen opent de invoer, Properties kan geselecteerde tekst opnieuw bewerken, en de Rust-kern registreert beide wijzigingen voor undo/redo. In de UI-test versprong de horizontale PDF-scrollpositie aanvankelijk bij het openen van de modal; na herstel bleef de pagina bij sluiten op dezelfde positie. De invoer is met muis en toetsenbord getest, net als undo, redo en Ctrl+scroll van 92% naar 122% met geselecteerd tekstvak. Lokale screenshots: `/tmp/opds-rust-text-scroll-restored.png`, `/tmp/opds-rust-text-undo.png`, `/tmp/opds-rust-text-redo-verified.png` en `/tmp/opds-rust-text-ctrlzoom.png`. Zijpanelen en documentgebied zijn nog duidelijk anders. De schermafbeeldingen staan lokaal in `/tmp/opds-original-annotation-ribbon.png` en `/tmp/opds-rust-annotation-ribbon-final2.png`.

## Meting

Dezelfde lokale PDF's zijn drie keer als apart proces met dezelfde Rust/Hayro CLI gerenderd. De wandtijd bevat opstarten, parsing, renderen en PNG-schrijven; `render_ms` komt uit de renderer. Mediane wandtijden:

| PDF | Debug | Release | Renderfase release |
| --- | ---: | ---: | ---: |
| Technische tekening.pdf | 6,58 s | 0,094 s | 0,071 s |
| Zware vector PDF.pdf | 29,1 s | 0,685 s | 0,645 s |

Voor de ribbon-hover is op hetzelfde Rust-tabblad via XTEST een muisbeweging uitgevoerd en de pixelverandering in een kleine vensteruitsnede gepeild (acht pogingen). De debug-softwarebuild gaf 65–371 ms; de releasebuild met wgpu 3–8 ms. Dit is een lokale waarneembaarheidsmeting, geen end-to-end inputlatencybenchmark; de bestaande Tauri-app is hiermee nog niet gelijkwaardig gemeten. De eerste proef met verkeerde venstercoördinaten is verworpen. De release-app had geopende bestanden onder `/dev/dri/renderD128` en draaide op de AMD RADV-renderdevice; er is geen C/C++-PDF-engine aangesloten.

De releasebuild met `wgpu` is daarom de standaard voor UI-controle. De PDF-renderer blijft CPU-gebaseerd en vooral grote vectorpagina's kosten nog circa 0,7 s. De shell opent nu grote pagina's op maximaal circa 550 px breedte en biedt scrollen in beide richtingen. Annotation-overlay, eenregelige tekstbewerking en documentmodel zijn Rust; meerregelige tekst/opmaak, uitbreiding van het PDF-schrijfpad, printen en volledige visuele pariteit blijven open werk.

## Save As via echte UI

De titelbalkknop Save As opende een PDF-paddialoog. Op een vijfpagina-`Tekst.pdf` zijn via de Annotation-ribbon een Highlight en een tekstvak gemaakt en naar een nieuw bestand geschreven. De uitvoer behield vijf pagina's, metadata en de oorspronkelijke bytes als prefix. Zowel Poppler (`pdfinfo`, `pdftoppm`, `pdftotext`) als de native Hayro-renderer lazen het resultaat; tekst en markering zijn in beide renderers zichtbaar. De modal verandert tijdens opening de horizontale scrollpositie van de onderliggende pagina; bij sluiten wordt de opgeslagen positie hersteld. Lokale screenshots: `/tmp/opds-rust-save-dialog.png`, `/tmp/opds-rust-save-result.png`, `/tmp/opds-native-save-smoke-page.png`, `/tmp/opds-native-save-hayro.png` en `/tmp/opds-rust-reopened-save.png`. De Save As-dialoog is nog geen pixelgelijke port van de bestaande app.

Unicode-tekst is daarna via de UI getest: `Café Ω Ж` is in een Text Box geplakt, via Save As opgeslagen en met Poppler en Hayro zichtbaar gemaakt. `pdftotext` gaf de drie scripts exact terug. De embedded CID TrueType-font en `ToUnicode`-mapping zijn met `pdffonts` gecontroleerd. Ctrl+scroll met geselecteerde Unicode-tekst ging opnieuw van 92% naar 122% met cursorvast anker. De laatste ribbonuitsnede is naast de bestaande app gelegd (`/tmp/opds-unicode-ribbon-comparison.png`); de groepsgrenzen zijn onveranderd. Zie [PDF-opslag](pdf-save-2026-10-01.md).
