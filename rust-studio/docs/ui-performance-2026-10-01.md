# UI en snelheid — 1 oktober 2026

Dezelfde schermbreedte van circa 1200 px is gebruikt voor visuele vergelijking van Hakan Seven's [OpenCADStudio-referentie](https://github.com/HakanSeven12/OpenCADStudio/blob/2a15a370f6886d3e8695ab81c7d677e0cd484451/site/workspace.png), de bestaande Open PDF Studio en de huidige native shell. De lokale schermafbeeldingen van draaiende apps zijn niet in Git opgenomen.

| Gebied | OpenCADStudio | Bestaande PDF-app | Huidige Rust-shell |
| --- | --- | --- | --- |
| Ribbon | Dicht, meerdere rijen en gegroepeerde tools | Dicht, zes tabs en veel PDF-tools | Enkele grote iconen, veel lege ruimte; deels Hakan-helpers |
| Documenten | CAD-tekeningen en tabs | Meerdere PDF-tabs, meldingsbalk | Eén PDF-tab, geen meldingsbalk |
| Zijpanelen | Eigenschappen, objectlijst | Uitgebreide linker en rechter panelen | Tekstuele paginalijst, eenvoudige eigenschappen |
| Pagina/viewport | CAD-canvas | PDF met selectie- en annotatielagen | PDF-bitmap met eerste native annotatie-overlay |
| Status | Meerdere controles | Pagina, zoom, meetwaarden | Pagina en zoom |

De 437 gekopieerde upstream-bestanden zijn een bronbasis, geen bewijs van dezelfde UI. De zichtbare shell gebruikt slechts een deel van Hakan's widgets en iconen. Visuele pariteit met zowel Hakan als de bestaande PDF-app is nog ver weg.

## Meting

Dezelfde lokale PDF's zijn drie keer als apart proces met dezelfde Rust/Hayro CLI gerenderd. De wandtijd bevat opstarten, parsing, renderen en PNG-schrijven; `render_ms` komt uit de renderer. Mediane wandtijden:

| PDF | Debug | Release | Renderfase release |
| --- | ---: | ---: | ---: |
| Technische tekening.pdf | 6,58 s | 0,094 s | 0,071 s |
| Zware vector PDF.pdf | 29,1 s | 0,685 s | 0,645 s |

Voor de ribbon-hover is op hetzelfde Rust-tabblad via XTEST een muisbeweging uitgevoerd en de pixelverandering in een kleine vensteruitsnede gepeild (acht pogingen). De debug-softwarebuild gaf 65–371 ms; de releasebuild met wgpu 3–8 ms. Dit is een lokale waarneembaarheidsmeting, geen end-to-end inputlatencybenchmark; de bestaande Tauri-app is hiermee nog niet gelijkwaardig gemeten. De eerste proef met verkeerde venstercoördinaten is verworpen. De release-app had geopende bestanden onder `/dev/dri/renderD128` en draaide op de AMD RADV-renderdevice; er is geen C/C++-PDF-engine aangesloten.

De releasebuild met `wgpu` is daarom de standaard voor UI-controle. De PDF-renderer blijft CPU-gebaseerd en vooral grote vectorpagina's kosten nog circa 0,7 s. De shell opent nu grote pagina's op maximaal circa 550 px breedte en biedt scrollen in beide richtingen. Annotation-overlay en documentmodel zijn Rust; selectie, tekstbewerking, PDF-schrijfpad, printen en visuele pariteit blijven open werk.
