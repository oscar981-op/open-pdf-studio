# UI en snelheid — 1 oktober 2026

Dezelfde schermbreedte van circa 1200 px is gebruikt voor visuele vergelijking van Hakan Seven's [OpenCADStudio-referentie](https://github.com/HakanSeven12/OpenCADStudio/blob/2a15a370f6886d3e8695ab81c7d677e0cd484451/site/workspace.png), de bestaande Open PDF Studio en de huidige native shell. De lokale schermafbeeldingen van draaiende apps zijn niet in Git opgenomen.

| Gebied | OpenCADStudio | Bestaande PDF-app | Huidige Rust-shell |
| --- | --- | --- | --- |
| Ribbon | Dicht, meerdere rijen en gegroepeerde tools | Dicht, zes tabs en veel PDF-tools | Home: titelbalk 30 px, tabs 28 px, groepen 94 px; groepsscheidingen op circa dezelfde x-posities. Andere tabs blijven voorlopig een gedeeltelijke Hakan-port |
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

De Annotation-ribbon is daarna op 1200 px naast de bestaande app vastgelegd. De oorspronkelijke groepen zijn Drawing, Text, Stamp, Redaction, Properties, Edit, Measure en Quantities. De Rust-tab toont nog Document, Annotation, Edit, View, Navigate en Panels met Hakan-iconen; hier is de afwijking veel groter dan in Home. De volgende visuele port moet deze groepindeling en PDF-iconen overnemen zonder de werkende overlayacties te verliezen.

## Meting

Dezelfde lokale PDF's zijn drie keer als apart proces met dezelfde Rust/Hayro CLI gerenderd. De wandtijd bevat opstarten, parsing, renderen en PNG-schrijven; `render_ms` komt uit de renderer. Mediane wandtijden:

| PDF | Debug | Release | Renderfase release |
| --- | ---: | ---: | ---: |
| Technische tekening.pdf | 6,58 s | 0,094 s | 0,071 s |
| Zware vector PDF.pdf | 29,1 s | 0,685 s | 0,645 s |

Voor de ribbon-hover is op hetzelfde Rust-tabblad via XTEST een muisbeweging uitgevoerd en de pixelverandering in een kleine vensteruitsnede gepeild (acht pogingen). De debug-softwarebuild gaf 65–371 ms; de releasebuild met wgpu 3–8 ms. Dit is een lokale waarneembaarheidsmeting, geen end-to-end inputlatencybenchmark; de bestaande Tauri-app is hiermee nog niet gelijkwaardig gemeten. De eerste proef met verkeerde venstercoördinaten is verworpen. De release-app had geopende bestanden onder `/dev/dri/renderD128` en draaide op de AMD RADV-renderdevice; er is geen C/C++-PDF-engine aangesloten.

De releasebuild met `wgpu` is daarom de standaard voor UI-controle. De PDF-renderer blijft CPU-gebaseerd en vooral grote vectorpagina's kosten nog circa 0,7 s. De shell opent nu grote pagina's op maximaal circa 550 px breedte en biedt scrollen in beide richtingen. Annotation-overlay en documentmodel zijn Rust; tekstbewerking, PDF-schrijfpad, printen en volledige visuele pariteit blijven open werk.
