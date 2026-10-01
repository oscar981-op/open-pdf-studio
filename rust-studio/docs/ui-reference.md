# OpenCADStudio als UI-codebasis

Op uitdrukkelijk verzoek van de gebruiker wordt Hakans code daadwerkelijk hergebruikt.

- Bron: https://github.com/HakanSeven12/OpenCADStudio
- Auteur/project: Hakan Seven en OpenCADStudio contributors.
- Commit: `2a15a370f6886d3e8695ab81c7d677e0cd484451`.
- Ongewijzigde bronkopie: `../vendor/opencadstudio/src/ui/`.
- Ongewijzigde SVG-iconen: `../vendor/opencadstudio/assets/icons/`.
- Originele GPL-3.0-licentie: `../vendor/opencadstudio/LICENSE`.
- Actieve port: `../crates/native-app/src/hakan_ui.rs`.

De actieve port gebruikt `tool_btn_style`, de grote toolknop-layout uit
`render_large`, `quick_access_btn` en de groepsopbouw uit `render_group`.
De Iced Git-API is aangepast naar Iced 0.14 (`extended_palette`); CAD-events
zijn vervangen door PDF-commands. De SVG's worden rechtstreeks uit de
gekopieerde assets geladen. De native-app crate vermeldt GPL-3.0-only;
de overige Rust-crates behouden hun bestaande licentievermelding.

Voor de Home-ribbon zijn de PDF-specifieke SVG's uit de bestaande
`open-pdf-studio/js/solid/data/ribbonIcons.js` en `HomeTab.jsx`
zonder vormwijziging onder `../assets/ribbon/home/` opgenomen. Het 16 px
titelicoon onder `../assets/ribbon/title-icon.rgba` is afgeleid van
`open-pdf-studio/public/icon.png`. Deze assets komen uit de bestaande Open
PDF Studio-codebasis. Voor de Annotation-ribbon zijn 15 SVG's uit `ribbonIcons.js` en
`CommentTab.jsx` onder `../assets/ribbon/annotation/` opgenomen. De acht
groepen zijn in Hakan's port ingepast; niet-gemigreerde acties zijn
uitgeschakeld. Hakan's helpers en groepsopbouw blijven de technische basis.

Openen, paginawissel, zoom en paneelzichtbaarheid zijn aangesloten. Niet
gemigreerde editoracties zijn uitgeschakeld. De oorspronkelijke dock-,
properties-, menu- en dialoogcode staat klaar in de bronkopie, maar is nog
niet volledig aangesloten. De bestaande Open PDF Studio blijft de doelreferentie
voor PDF-functionaliteit en uiteindelijke schermpariteit.

De ingebedde tekstfont voor PDF Save As is de ongewijzigde `LiberationSans-Regular.ttf` uit de bestaande PDF.js-assets. De oorspronkelijke [SIL Open Font License](../assets/fonts/LICENSE_LIBERATION) en de font staan onder `../assets/fonts/`; deze font is geen onderdeel van Hakan's OpenCADStudio-code.
