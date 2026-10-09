# Native Rust-runtime: første kjørbare milepæl

Prosjektet har et separat Cargo-workspace i `native/`. `sa-runtime.exe` åpner
et eget Windows-vindu med winit og wgpu. Når den er bygget, trenger den ikke
Python, nettleser, WebView eller original `gta_sa.exe`.

## Kjøring

Fra prosjektmappen: bygg med `cargo build --release --workspace --manifest-path native/Cargo.toml`,
og dobbeltklikk `start-sare.cmd`. Velg installasjonen i launcheren og trykk Play.
For direkte oppstart og utviklerverktøy:

```powershell
cd native
cargo run --release -p sa-runtime -- --game-dir "E:\GTA San Andreas\Grand Theft Auto San Andreas"
cargo run --release -p sa-runtime -- --first-model
cargo run --release -p sa-runtime -- --cuttest
cargo run --release -p sa-runtime -- --inspect-cutscene cuttest
cargo run --release -p sa-runtime -- --prologue
```

Standardstien er den oppgitte `E:`-installasjonen. `--first-model` viser
original `cj_wastebin` og `miamibin`-teksturen. Standardvalget laster radius
120 rundt Grove Street. Klikk i vinduet for musestyring; Esc frigjør musen.
WASD flyr, Q/E endrer høyde, Shift øker farten, og R nullstiller kameraet.
Trykk P i Grove Street for gåmodus med underlag, veggkollisjon og tyngdekraft.
WASD går, Shift løper og Space hopper. Trykk P igjen for flykamera.
Vinduet kan endre størrelse og bruker faktisk vindusformat, også ultrawide.

## Arkitektur og grenser

- `sa-assets`: validert VER2-indeks og direkte lesing av IMG-entries, rigid DFF,
  materialer/rammer, samt DXT1/3/5, BGRA/XRGB, 8-bit palett, 16-bit (565/1555/4444) og
  luminans fra originale TXD-filer, inkludert eldre D3D8-ordbøker og
  RenderWare 3.3–3.5-filer.
- `sa-scene`: IDE/IPL fra `data/default.dat` fulgt av `data/gta.dat` i registrert
  rekkefølge, inkludert binære IPL-streams. Uregistrerte filer ignoreres.
  Senere registrert IDE-definisjon med samme ID erstatter tidligere definisjon.
  Plasseringer konverteres fra GTA XYZ/quaternion til rendererens X,Z,-Y.
- `sa-runtime`: original geometri og teksturer rendres i eget wgpu-vindu,
  med dybdebuffer, enkel alpha-pass, resize, flykamera og enkel gåmodus.
- Ingen diskcache eller spillfiler skrives av runtime. `native/target` er
  ignorert for Git; ingen originalfiler er lagt inn i prosjektet.

Gåmodus bruker en kapsel mot de synlige, opake trekantene i det lastede
kartutsnittet. Dette er en foreløpig tilnærming, ikke originale COL-filer;
usynlige kollisjonsflater, enkelte trapper og kartkant kan derfor avvike.
Det finnes ingen synlig spillerfigur, trafikk, missions eller moderne lys.
Alpha sorteres bare per materialbatch. LOD, generell TXD-arv,
spesielle RenderWare-plugins og animert verdensgeometri er ikke generelt støttet.

## SCM og cutscenes

`sa-script` er første isolerte kompatibilitetslag. Den begrensede SCM-kjernen
tolker `0001` (wait), `0002` (jump med absolutt offset i den avgrensede
kodebufferen) og `004E` (avslutt script) med umiddelbare 8-/32-bit heltall,
instruksjonsbudsjett og bounds-kontroll. Ukjente opcodes stoppes med feil.
`main.scm` blir **ikke** kjørt ennå: header/segmenter, globale/lokale
variabler, tråder, alle nødvendige opcodes og spilltilstand gjenstår. Ingen
missions er gjenskapt manuelt.

`--inspect-cutscene cuttest` leser den originale `.cut`-, `.dat`- og
`.ifp`-trioen i `anim/cuts.img` og sjekker modell i
`models/cutscene.img` samt at lydarkivet finnes. `--cuttest` viser en native
forhåndsvisning av den originale `csgoldrec`-modellen med original TXD og
IFP-rotnøkler. FOV leses fra original `.dat`; kameravinkelen justeres for å
ramme inn objektet. Dette er **ikke komplett cutscene-avspilling**: ingen
undertekst, lydstrøm, figur-skjelett, scenehendelser eller SCM-styrt start.
Andre cutscenes enn `prolog1` blir foreløpig bare inventert.

`--prologue` åpner en avgrenset forhåndsvisning av original `prolog1`, den
første oppstartssekvensen funnet i `main.scm` på denne installasjonen.
Kameraets posisjon, mål og FOV kommer fra `prolog1.dat`. Taxiens rotanimasjon
kommer fra `prolog1.ifp`, modellen fra `models/cutscene.img`, og teksturene
fra spillets `taxi.txd` og `models/generic/vehicle.txd`. Original
`cstaxi92.txd` er tom og bruker disse overordnede teksturene. Denne visningen
laster nå også originalkartet rundt `.cut`-filens X/Y-koordinater. Originalen
plasserer scenen ca. én kilometer over kartet; forhåndsvisningen flytter hele
scenen ned til lokalt gatenivå, med relativ kamerabevegelse bevart. Ett
kartmateriale uten matchende TXD-tekstur får en ensfarget reservefarge.
Dette er en teknisk kartplassering, ikke dokumentert original sceneoppsett.
Denne visningen
har foreløpig bare taxien: figurene, kofferter, animasjon av enkeltdeler,
undertekster, lyd og SCM-styrte scenehendelser er ikke implementert. Den er
derfor ikke den komplette åpningscutscenen.

Neste kompatibilitetssteg er å validere SCM-segmenttabellen og starttrådene
mot brukerens `main.scm`, bygge typed variable/operand-decoding og prioritere
opcode-settet brukt av den første originale mission-starten. Cutscene-systemet
trenger generell IFP-boneanimasjon, modell/texture-oppløsning (inkludert
spillerfigur), kamera-tracktolkning, tekst fra GXT og synkronisert lyd.

## Verifikasjon

På denne installasjonen ga Rust-proben 169 plasseringer, 29 833 trekanter,
185 teksturbatcher og 26 869 foreløpige kollisjonstrekanter. Prologvisningen
lastet 146 kartplasseringer pluss taxien, til sammen 40 944 trekanter i 132
teksturbatcher. Enkeltmodellen og kartet ble
vist og visuelt kontrollert i det faktiske native Windows-vinduet.
`cuttest`-metadata ble lest fra originalinstallasjonen, og den animerte
forhåndsvisningen ble åpnet i native vindu. `cargo test --workspace` brukes
som kontroll. Disse observasjonene er ikke en garanti for andre GTA SA-utgaver
eller maskinvare.

Kilder for API- og scriptformatvalg: [wgpu 30-eksempel](https://github.com/gfx-rs/wgpu/blob/trunk/examples/standalone/02_hello_window/src/main.rs),
[winit 0.30](https://docs.rs/winit/0.30.13/winit/),
[Sanny Builder opcode-dokumentasjon](https://docs.sannybuilder.com/edit-modes/opcodes-list-scm.ini).
Implementasjonen er skrevet for dette prosjektet uten kopiert tredjepartskode.
