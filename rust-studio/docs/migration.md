# Migratie naar Rust

Doel: uiteindelijk de hele PDF-app, niet alleen een viewer. De volgende fasen beschrijven ontbrekend werk; het bestaan van deze lijst betekent niet dat de functies zijn geïmplementeerd.

| Fase | Werk | Acceptatie |
| --- | --- | --- |
| 0 — basis | Eigen branch/workspace, Hakan UI-referentie, dependencybeleid, viewport-kern | Kern compileert/test; oude app niet geraakt |
| 1 — engineproef | Hayro/eigen renderer, font- en codec-audit, corpus runner | Tekeningen, scans, rotatie/CropBox, transparantie, embedded fonts; verschillen verklaard; snelheid/geheugen gemeten |
| 2 — native viewer | UI-shell, tabs, openen, pagina's, zoek/tekstselectie, pan/zoom | Geen WebView/JS; UI blijft reageren tijdens zware renders; annuleren en stale resultaten correct |
| 3 — annoteren | Selectie, grips, vormen, tekst, lagen, maten, undo/redo | Groot annotatiecorpus; coördinaten en weergave op alle zoomniveaus; save/reopen zonder verlies |
| 4 — PDF bewerken | Pagina's, tekst, beelden, vectorfragmenten, formulieren, OCR | Geen native enginefallback; duidelijke matrix voor nog niet ondersteunde onderdelen |
| 5 — uitvoer | Atomair opslaan, print, raster/DPI, ODT/XLSX, CAD, handtekeningen | Corpus-roundtrips; print/geometrie; bescherming tegen documentwissel en save-fouten |
| 6 — product | MCP, plugins, account/keyring, i18n, toegankelijkheid, installers | Linux/Windows/macOS; toetsenbord/schermlezer; featurepariteit en verpakte dependency-audit |

OCR is een afzonderlijk onderzoeksrisico: Tesseract kan niet blijven bij de gekozen eis. Digitale handtekeningen, kleurbeheer, fontsubstitutie, complexe scripts en printintegratie moeten expliciet in de compatibiliteitsmatrix komen. Een niet-ondersteunde functie mag niet stilzwijgend weggelaten worden bij opslaan.

Performance meten met dezelfde bestanden en handelingen als de huidige app: cold/warm openen, eerste pagina, pan/zoom, veel annotaties, snippets, save, print en piekgeheugen. Rapporteer hardware en buildprofiel; bepaal acceptatiegrenzen na een gemeten baseline. Geen volledige migratieclaim voordat alle geaccepteerde productfuncties zijn gecontroleerd.
