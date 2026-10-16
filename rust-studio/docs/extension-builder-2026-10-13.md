# Eerste native extensiebouwer

Het Rust-venster opent via **File → Extensions** een editor voor een
extensie-ID, naam, `index.html` en `index.js`. **Build .oppx** schrijft een
ZIP-pakket met `plugin.json` en beide bronbestanden naar het opgegeven pad.
Het pad moet eindigen op `.oppx`; bestaande bestanden worden nooit
overschreven. ID en bronomvang zijn begrensd. **File → Open Extension Package**
opent een afzonderlijk importvenster en kan een eerder met deze editor gemaakt
pakket weer als bewerkbare HTML/JavaScript-bron laden.
Alleen precies deze drie bestanden zijn toegestaan, zodat opnieuw bouwen
geen onbekende resources stilzwijgend weggooit. De import heeft een
3-MiB-pakketgrens, een 16-KiB-manifestgrens en 1 MiB per bronbestand. Na
import is het uitvoerpad leeg: de originele `.oppx` wordt nooit
overschreven. De pakkettest opent de ZIP opnieuw en vergelijkt beide
bronbestanden en manifest-ID.

De bestaande Tauri-app kan zulke `.oppx`-pakketten installeren en voert
`index.js` als pluginmodule uit. De nieuwe Rust-app **bouwt** het pakket,
maar heeft nog geen HTML-weergave of JavaScript-pluginhost; een gebouwde
extensie wordt in Rust niet uitgevoerd of geïnstalleerd. `index.html` is
een bronbestand in het pakket, nog geen automatisch geregistreerd paneel.
De editor schrijft geen PDF en draait geen JavaScript in DynLex. Voor
volledige pariteit zijn een geïsoleerde extensiehost, een expliciet
capaciteitencontract, installatie/deïnstallatie en concrete plugin-API's
nodig. Die mogen pas als werkend worden getoond zodra ze echt zijn getest.

De importinvoer in dezelfde dialoog ging samen met lege `text_editor`-velden
in geïsoleerde eerste-frame-opnamen, hoewel de bron in het model en in de
voorvertoning aanwezig was. De invoer staat nu in een afzonderlijk
importvenster. Een nieuwe 1200×832-opname met een echte PDF toont beide
bewerkbare bronvelden gevuld (`/tmp/opds-rust-extension-editor-after-split.png`).
Het afzonderlijke importvenster is eveneens als 1200×832-opname gecontroleerd
(`/tmp/opds-rust-extension-import-modal.png`).
`Content`-, editactie- en pakkettests bewijzen dat wijzigingen in beide
modelbuffers terechtkomen en na export/import behouden blijven. Een echte
toetsenbordinteractietest ontbreekt nog; de opname bewijst zichtbaarheid,
niet dat een gebruikersvenster alle toets- en selectieacties juist afhandelt.
