# DynLex: geselecteerde pagina’s en geërfde resources

De native DynLex-route kiest nu een 1-gebaseerd paginanummer in het oorspronkelijke
PDF, maximaal 4.096 pagina’s. Zij splitst of herschrijft geen PDF om een losse
pagina aan te bieden. Bestaande contentbytes en precieze decimale lexemen blijven
intact. De oude éénpagina-API’s/fixture-CLI’s houden hun bestaande contract.

Resources worden via de dichtstbijzijnde `/Resources`-dictionary gevonden, met
resolutie van indirecte dictionaries. Een lege lokale dictionary vervangt de
resources van de ouder; resources worden niet samengevoegd. Geërfde MediaBox,
CropBox en Rotate blijven door de bestaande begrensde geometrievalidator lopen.
Geen nieuwe inkt-, font-, curve-, annotatie- of zoomsemantiek is stilzwijgend geactiveerd.

`DynlexDocument` bewaart de eerste paginascene plus drie LRU-paginaslots.
Elke slot is een gedeelde `OnceLock`; gelijktijdige aanroepen van dezelfde
resident gebleven pagina delen de voorbereiding. De cachelock is vrij tijdens
PDF-interpretatie en rasterisatie. Een actieve render houdt zelf zijn slot vast
na cacheverwijdering; vier resident slots is dus geen globale limiet op alle
in-flight allocaties. Onondersteunde pagina’s worden in hun eigen slot onthouden.
Een verwijderde pagina wordt bij terugkeer opnieuw voorbereid. De PDF-bronlimiet
blijft 16 MB; de bestaande scene-/pixelgrenzen blijven intact. Dit is nog geen
herbruikbare volledig geparste documentstructuur en geen UI-tegelcache.

## Bewijs

- Acht echte gegenereerde PDF-pagina’s met verschillende kleuren, indirecte
  geërfde resources, alle kwartslagen en verschoven CropBox zijn op 100% geheel
  bytegelijk aan Hayro. Volgorde 8/1/3/5/7/2/6/4/8/1 test cacheverwijdering.
- Vier gelijktijdige calls voor pagina 8 geven hetzelfde beeld.
- Twee hybride pagina’s delen geërfde image resources; een derde pagina met
  expliciet lege resources wordt afgewezen en vervuilt de andere paginaslots niet.
- Ongeldige inherited Resources/Rotate/UserUnit op pagina 1 worden geweigerd;
  pagina 8 blijft bytegelijk. Paginanummers 0, 9 in een 8-pagina-PDF en 4.097
  leveren geen bitmap.
- De geïsoleerde echte clip van operator 582 is als tweede pagina volledig
  bytegelijk aan Hayro; de contentstream is bij de proef ongewijzigd gebleven.
- In een echt native 1200×832-venster is Last gebruikt: pagina 8/8, miniaturen
  5–8 en status `Renderer: DynLex` zijn zichtbaar. Dit is een gecontroleerde
  achtpagina-fixture, geen complete technische praktijktekening.
- Workspace-all-features-tests, negen prepared-tests, standaardcheck,
  all-features-releasebuild en formattering slagen. De oude volledige verifier
  blijft exact: 47 PDF-pagina’s / 4.964.352 pixels, plus drie v4-pagina’s /
  147.456 pixels; foutafwijzing blijft werken.

Gerichte tests: `cargo test -p opds-dynlex-backend --features kernel prepared::`.
De fixtures voor deze controles worden door de Rust-tests opgebouwd.

## Volledige complexe documenten: nog steeds niet renderbaar

De release-audit van zeven volledige praktijk-PDF’s, 92 pagina’s op 25/100/200%,
geeft nog steeds 0/276 ondersteund, 276 expliciete afwijzingen en nul procesfouten.
Er is geen vervangend Hayro-beeld. Ruwe gegevens met bronhashes staan in
`dynlex-complex-multipage-2026-10-08.json`.

Een aparte aanroep van de drie adapterroutes op pagina 2 (pagina 1 voor het
eenpagina-combinatiebestand) toont de volgende **vroege**, niet uitputtende grenzen:

| Document | Vroege afwijzing |
| --- | --- |
| Barn Relocation | Bronbestand boven 16 MB |
| Zware vector PDF | Bronbestand boven 16 MB |
| Combinatie raster/vector/tekening | Niet-lege resources en compound operands |
| Technische tekening | Niet-lege resources en unsupported page state |
| Tekst, Text pdf gecombineerd, rapport-constructie | Fractionele paginageometrie; niet-lege resources |

Alleen zo’n grens verhogen maakt de achterliggende PDF-operators niet ondersteund.
De volgende noodzakelijke inhoudelijke stap is een algemene begrensde scene voor
fractionele paginageometrie, fonts/tekst en paden, met zelfstandige rasterisatie.
Volledige praktijkpagina’s moeten zichtbaar worden voordat een brede vergelijking
van snelheid zinvol is. Deze wijziging claimt geen volledige dekking of snelheid.
DynLex gebruikt nog steeds de LLVM/C++-toolchain en libc; de appintegratie is Rust.
