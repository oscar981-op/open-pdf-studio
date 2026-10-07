# LBVH in de voorbereide DynLex-scene — 8 oktober 2026

De begrensde vectorroute van `PreparedPage` gebruikt nu een CPU-LBVH in Rust
in plaats van het vaste 16×16-bakkenraster. Iedere gevalideerde opdracht krijgt
één blad met zijn bounding box. Het midden wordt naar een 16-bit x/y Morton-code
omgezet; bladeren worden deterministisch gesorteerd. De boom splitst op het
hoogste verschillende bit; gelijke Morton-codes worden gebalanceerd gehalveerd.
Nodes staan in één aaneengesloten vector. Dit is een eigen seriële 2D-uitwerking
van het bekende Morton/LBVH-principe, geen GPU-implementatie of gekopieerde
CUDA-code. Algoritmische achtergrond: [Tero Karras, NVIDIA, 2012](https://developer.nvidia.com/blog/thinking-parallel-part-iii-tree-construction-gpu/).

De boom wordt eenmaal samen met de brongebonden paginascene gebouwd. Een query
vergelijkt geschaalde nodegrenzen met de pixelviewport. Gevonden indices worden
teruggesorteerd naar PDF-tekenvolgorde, voordat alleen die opdrachten naar de
gevalideerde DynLex-buffer gaan. Een viewport die de hele boom bevat gebruikt
direct de oorspronkelijke volgorde. Een GGD van alle gehele coördinaten bewijst
voor de huidige subset in constante tijd of **alle** opdrachtranden op een
gevraagde schaal geheel blijven, inclusief niet-zichtbare opdrachten. Daardoor
is geen volledige lijst geschaalde rechthoeken per tegel meer nodig.

Maximaal 16.384 opdrachten, 32.767 nodes en pagina-afmetingen tot 4096 per zijde
blijven verplicht; pixelgrens blijft 6 miljoen. Een node neemt 28 bytes in:
maximaal 917.476 bytes node-opslag, exclusief scene, opdrachten, tijdelijke
bouwarrays en queryresultaten. De traversatiestack is 64 indices: maximaal
32 verschillende Morton-bits plus 14 niveaus voor gelijke codes begrenzen de
diepte. Ongeldige rechthoeken, dimensies en aantallen worden geweigerd.

Tests vergelijken 8.000 deterministische queries op 6.000 rechthoeken met
uitputtend doorzoeken, inclusief vier schalen. Daarnaast: lege boom, exacte
rakende randen, tekenvolgorde, ongeldige grenzen, 16.384 gelijke codes en limiet
+1. De pixelproeven vergelijken volle pagina's en alle 53×37-tegels op meerdere
schalen met Hayro, inclusief overlappende kleuren en lege tegels. Een extra
test vergelijkt de GGD-uitspraak met controle van ieder coördinaat.

Dit vergroot PDF-objectdekking niet. Alleen bewezen dekkende asgerichte
vectoropdrachten gebruiken deze index. Raster/masker-resampling, fonts,
curven en algemene transparantie zijn hierdoor niet opgelost. De app houdt
voorlopig volledige bitmaps; de viewport-API is nog geen zichtbare, door de UI
aangestuurde tegelcache. Op 100% blijft de bestaande directe scenekernel actief.
LBVH versnelt dus niet automatisch iedere bestaande app-render.

Reproduceren vanuit `rust-studio`:

```sh
cargo test -p opds-dynlex-backend --features kernel lbvh::tests
cargo test -p opds-dynlex-backend --features kernel prepared::
cargo build --release -p opds-dynlex-backend --all-features --bin viewport_probe
python3 scripts/bench_lbvh.py --baseline /pad/naar/viewport_probe-met-grid \
  --candidate target/release/viewport_probe --output docs/dynlex-lbvh-bench-2026-10-08.json
```

De baseline is gebouwd vóór de LBVH-wijziging met dezelfde `viewport_probe`
op de `PreparedPage`-code van commit c830cae3. Binaries en hashes staan in het
rapport. De test genereert twee synthetische éénpagina-PDF's van 1024×768:
6.000 verspreide 4×4-rechthoeken en 6.000 samenvallende 24×24-rechthoeken.
Elke procesrun bereidt eenmaal voor en tekent 15 keer alle 128×128-tegels;
iedere tegel wordt buiten de timer bytegewijs met Hayro vergeleken.
Er zijn drie afwisselende procesparen per geval, dus 45 warme tegelpasses per
methode. OS-cache wordt niet gewist. Het rapport bewaart voorbereidingstijd en
hele-proces-RSS (inclusief Hayro-oracle), niet uitsluitend kernelgeheugen.
De vergelijking meet LBVH **plus** het vermijden van de volledige schaalpass;
zij is geen geïsoleerde microbenchmark van alleen boomtraversatie.

## Gemeten resultaat

| 6.000 opdrachten | Oude grid, mediane tegelpass | LBVH + schaalcontrole | Hele-proces-RSS grid/LBVH |
| --- | ---: | ---: | ---: |
| Verspreide 4×4-vormen | 12,631 ms | 1,527 ms | 19.916 / 20.020 KiB |
| Samenvallende 24×24-vormen | 23,329 ms | 15,418 ms | 19.940 / 20.036 KiB |

In alle drie procesparen is de mediane kandidaatpass sneller. Iedere uitvoertegel
is volledig bytegelijk aan Hayro. Dit is ongeveer 8,3× en 1,5× voor deze warme
tegelpass, niet voor volledige PDF-bestanden of de huidige UI. Voorbereiding
(inclusief PDF-adapter) ligt hier rond 36–43 ms en is apart gemeten. RSS is niet
lager; een geheugenwinst wordt niet geclaimd. Een eerste overdraw-proef met
6.000 paginagrote overlappende vlakken is wegens zeer lange rasterwerktijd
onderbroken en niet als resultaat gebruikt; de beschreven 24×24-proef vervangt
haar niet als bewijs voor paginagrote overdraw.
