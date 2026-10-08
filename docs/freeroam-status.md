# Native freeroam – arbeidsstatus 8. oktober 2026

Målet er freeroam på originalkartet med lokale custom ressurser. Missions er
ikke prioritert. Originalinstallasjonen leses; gta_sa.exe kjøres ikke.

Custom vehicle resources can supply bounded native handling values for engine,
brakes, grip, steering and suspension. Server packs include this tuning and
respawning preserves it; see [vehicle dynamics](vehicle-dynamics.md).

## Lokal lagring

Offline-posisjon, kamera, bilvalg, ped og antrekk lagres når du åpner en meny,
ved normal avslutning og før du starter multiplayer. Hovedmenyen viser
«Continue free roam» når et gyldig checkpoint er lastet. Neste oppstart laster
området rundt posisjonen og sjekker gulv, vegger og hodeplass før gjenopptak.
Hvis stedet mangler trygg grunn, starter du ved Grove Street. Fjernede eller
flertydige modellnavn bruker dagens standardvalg; klesvalg gjelder bare samme ped.

Dette lagrer én lokal offline-økt, uten missions eller serverfremdrift. Du
starter til fots, og valgt bil plasseres i ledig rom i nærheten; fart og nøyaktig
parkering lagres ikke. Innendørs, flykamera, hopp, streaming og multiplayer
overskriver ikke det siste trygge checkpointet. Et krasj mellom menybesøk kan
miste nyere fremdrift. Se [lagring og testing](progress.md).

## Multiplayer-prototype

En spiller hoster, og opptil 19 andre kobler til med IP og port. Åpne Multiplayer
fra hoved-/pausemenyen, F5 eller /mp. Spillerposisjoner og bilbevegelse deles;
valgte biler, peds og opptil 16 klesvalg per ped synkroniseres også fra
sesjonens felles ressursliste. Personlige biler vises også etter utstigning,
med separat synlighet for bilen og eieren. Passasjerplasser reserveres av hosten: G ber om plass i en annen spillers
bil, og F går ut igjen. NPC-er, bilbytte og felles bilkollisjoner er ikke synkronisert.
Se [oppsett og begrensninger](multiplayer.md). Direkte internett-hosting krever
videresendt TCP-port (standard 7777); automatisk NAT-traversering er ikke lagt inn.
Relay-modus har serverbrowser, offentlige/private rom og joincode. Begge parter
kobler ut til en tilgjengelig relay, uten portåpning på spillverten. Start relayen
med `start-relay.cmd`; ingen offentlig tjeneste er satt opp. Prototypen bruker
ukryptert TCP og bør testes på betrodd LAN/VPN. Native servermods lastes ned og caches før tilkobling, med versjonssjekk
og gjenoppretting av lokale ressurser etter frakobling. Se [veikartet](roadmap.md) for neste milepæl og cache.

## Start og kontroller

Dobbeltklikk `start-freeroam.cmd` (eller `start-native.cmd`). Skriptet starter
en ny Windows-exe med Rust/Cargo når Cargo er installert, ellers brukes eksisterende exe.
Exe-en ligger i `native/target/release/sa-runtime.exe`.

- WASD: gå. Shift: løp. Space: hopp. Klikk i vinduet for musestyring.
- Xbox-kontroller (standardmapping): venstre stikke går/styrer, høyre stikke ser rundt;
  A hopper/bekrefter, X løper, Y går inn/ut av bil, RT gasser, LT bremser/rygger,
  LB håndbrems. Start åpner pause, View/Back åpner kart, D-pad navigerer og B går tilbake.
  Kontrolleren kan kobles til mens runtime kjører; tastatur/mus virker fortsatt samtidig.
  Ingen vibrasjon eller bindingstilpasning ennå.
- I dypt vann: WASD beveger i overflaten, Space gir et løft.
- P: gåmodus/flykamera. Q/E: ned/opp i flykamera. Esc: pausemeny.
- M: kartmeny. Mus, piltaster/Enter eller Xbox D-pad/A brukes i menyene.
- /cars eller F7: velg og spawn bil. /peds eller F8: bytt spillerfigur.
  /mods viser ressurslisten, med serverens liste mens du er tilkoblet.
  Trykk /, skriv kommandoen og Enter. Menyene finnes ogsaa i pausemenyen.
  Velg Spawn nearby eller S i peds-menyen for en figur i verden (maks aatte),
  med idle-animasjon uten kamp-/gang-AI.
- F9: hent valgt bil foran deg og sett deg inn. F: gå ut / inn nær bilen.
- I bil: W/S gass og brems/rygging, A/D styring, Space håndbrems.
- R: last Grove Street og gå tilbake dit.
- 1: Grove Street; 2: Los Santos sentrum; 3: Santa Maria-stranden.
- 4: LS flyplass; 5: landsbygda; 6: San Fierro.
- 7: Las Venturas; 8: ørkenen; 9: Mount Chiliad.

Området lastes ferdig før teleport. Et bakgrunnsarbeid laster neste nabolag
etter omtrent 140 meter. Gjeldende område beholdes mens dette pågår.
Bevegelse stoppes ved sikkerhetsgrensen dersom lasting ikke har rukket å bli
ferdig. Vindustittelen viser FPS, GTA-koordinater, modus og lasting.
Mislykket kartlasting beholder området og venter fem sekunder før nytt
forsøk. Hvis bakgrunnslasteren stopper, frigjøres lasteindikatoren og
feilmelding vises i menyen.

## Implementert

- Original 2DFX-skilttekst lastes fra DFF-filene og tegnes med `roadsignfont`
  fra installasjonens `models/particle.txd`. Linjer, farger, piler og symboler
  beholdes. Alle synlige tegn deler én tekstur og tegnegruppe og inngår ikke i
  kollisjonskartet. Originalinstallasjonen ga 489 tekstfelt i 207 modeller;
  Vulkan-testen bekrefter lesbar tekst ved broen nær Grove Street. Dette dekker
  dynamisk skilttekst; andre manglende bygningsteksturer må undersøkes separat.
  `--smoke-signs --capture-dir <mappe>` tar et testbilde av et nærliggende skilt.
- Originale RenderWare-teksturanimasjoner brukes på rullende skilt, neonskilt
  og fossefall. 21 spor indekseres én gang ved oppstart; grunngeometri og
  kollisjoner beholdes mens de animerte materialenes UV-er oppdateres.
  Animasjonsdata følger også kartet gjennom bakgrunnslasting og GPU-opplasting.
  Første UV-kanal støttes; full MatFX/dual-texture-belysning og tidsstyrte
  dag-/nattobjekter er fortsatt ikke implementert.
  `--smoke-neon --capture-dir <mappe>` tester det rullende Las Venturas-skiltet.
  Formatgrunnlag: [original skiltkode](https://github.com/gta-reversed/gta-reversed/blob/master/source/game_sa/CustomRoadsignMgr.cpp)
  og [RenderWare UV-animasjoner](https://github.com/aap/librw/blob/master/src/uvanim.cpp).

- Hovedmeny inspirert av San Andreas, tilpasset freeroam, med verden som bakgrunn.
- Pause, kart med ni reisemål, innstillinger, kontroller og lokal mod-oversikt
  med synlig status for aktive og deaktiverte ressurser.
- Innstillinger for synsfelt, mus, fullskjerm, VSync, HUD, radar/minikart,
  speedometer, kjøreegenskaper og flyfart lagres i
  `%LOCALAPPDATA%/SAFreeroam/settings.json`. Menyer stopper spillerbevegelsen.
- Radar/minikartet bruker de originale radarflisene fra installasjonens
  `models/radar.txd`, plassert etter San Andreas-verdenskoordinater. Det roterer
  med spillerens retning, har justerbar zoom og viser reisemålsmarkører. Hvis
  originalfilen mangler, vises ikke et oppdiktet kart. Speedometeret
  vises når spilleren kjører bil. Begge kan slås av hver for seg i innstillingene.
- Tap av vindusfokus åpner pausemenyen. Kartmenyen støtter også tallene 1–9.
- `--smoke-menus` rendrer menysidene og kan lagre renderbilder.

- Indeksering av 44 771 utendørsplasseringer på denne installasjonen.
- Rettet IPL-feil: bare de nederste åtte bitene er interiør-ID. Flaggene over
  disse bitene skal ikke føre til at utendørsterreng forkastes.
- Faktiske transformerte modellgrenser brukes i nabolagsutvalget.
- Objekter fra IDE `anim` tas med som statisk geometri. Bevegelsen deres
  er fortsatt ikke implementert.
- IDE `txdp` kobler teksturordbøker til foreldre. Barnets teksturer har
  prioritet; sirkler og for lange kjeder avbrytes. Ordlistene mellomlagres
  under én områdelast med 64 MiB grense.
- Original `water.dat` leses som trekanter/firkanter, med synlighetsflagg
  og høydeinterpolering. Vann rendres med original `waterclear256` fra
  lokal `particle.txd`; geometrien inngår ikke i vanlig gulvkollisjon.
- Enkel flyting og overflatebevegelse hindrer synking til havbunnen.
  Dette er foreløpig uten bølger, dykking eller svømmeanimasjon.
- `--probe-water` kontrollerer flyting med original kystgeometri.
- Foreldre angitt via IPL LOD-lenker filtreres bort fra detaljvisningen.
- Fjern-LOD vises opptil 2,5 km. LOD-modeller hvis grenser berører
  detaljområdet holdes ute; fjernmodellene inngår aldri i gangkollisjon.
  Kameraets fjernplan er 3,5 km. Overgangen kan fortsatt ha synlige hull
  eller sprang for store grupper; dette er ikke full original LOD-styring.
- Originale COL1/2/3/4-kollisjonscontainere dekodes. 8 255 originalmodeller
  ble indeksert. Trekanter brukes direkte; bokser og kuler trianguleres.
- Visuell kollisjon brukes som reserve for modeller uten COL.
- Førstepersons gåmodus starter automatisk. Gjenoppretting ved fall langt
  under kartet. P beholder flymodus for utforskning og feilsøking.
- Native mods: DFF, TXD, PNG, valgfri COL, plasseringer og modell-erstatning.
- DFF med innledende UV-animasjonsordbok leses med støttet UV-animasjon.
  Ugyldige UV-koordinater får reserveverdi; romlige data avvises ved feil.
  Metadata-audit dekodet 12 955 av 12 955 DFF-er fra lokal `gta3.img`.
  Denne metadata-auditen beviser geometrilesing, ikke alle materialeffekter.
- GPU-opplasting beholder ikke CPU-kopier av statisk, ikke-animert geometri.
- Nabolagsstreaming gjenbruker GPU-teksturer fra det aktive området når samme
  uendrede ressursloader fortsatt brukes. Server-/modbytte får egne opplastinger,
  slik at like teksturnavn med ulike bilder ikke blandes. CPU-bilder frigjøres
  gradvis innenfor opplastingens tidsbudsjett.
  Før/etter-test på RTX 3070/Vulkan langs samme 1,2 km-rute med sju områdeskifter
  reduserte samlet opplasting fra 2871,6 til 871,5 MiB (omtrent 70 prosent).
  Antall opplastingsframes gikk fra 1100 til 417. Dette måler redusert arbeid,
  ikke en garantert FPS eller fravær av alle driver-/lastingshakk.
- Én aktiv kjørbar bil, valgt fra bilmenyens originale og custom modeller,
  med følgekamera, terrengkontakt, veggkollisjon og inn/ut.
  `--smoke-car` kontrollerer kjøring, bremsing, utstigning og ny innstigning.
  Bilens dynamiske geometri beholdes når nabolag byttes.
  Parkerings- og pauseframes gjenbruker nå bilens GPU-geometri til den flytter
  eller roterer; modell- og sesjonsbytte tvinger ny oppdatering. Skyline-testen
  gikk fra 54,01 MiB til 0 MiB med gjentatte bilmesh-opplastinger over 30
  parkerte frames. Dette måler fjernet arbeid, ikke en bestemt FPS-økning.
- Bakkekontakt måles fra hvert hjuls forventede høyde på den skrå bilen.
  Fjæringen følger også veiens høydeendring under kjøring, slik at den lave
  akslingen ikke alene trekker bilen ned i veien. Oppover-/nedoverbakker,
  sidehelling, bakketopper og fall testes med egne kollisjonsflater.
  Den lesebaserte sjekken `sa-scene --example vehicle-slopes` kontrollerte
  106 retninger på skrå originalflater rundt Grove Street, med maksimalt
  1,9 cm avvik fra forventet bilhøyde. Dette er ikke en audit av hele kartet.
- Egne fragmentpass for alpha cutout og blending.
- `--smoke-tour`: skjult native GPU-test gjennom ni områder.
- `--smoke-stream`: sammenhengende frem-/tilbakerute på omtrent 1,2 km
  med vanlige flybevegelsestaster. Kontrollerer flere nabolagsbytter,
  endelige koordinater og at sceneinstallering ikke hopper kameraet.
- Hopp stoppes av tak over hodet, inkludert treff ved kroppens ytterkant.
  Pause med null tidssteg endrer heller ikke hopp-/gravitasjonstilstanden.
- Små trinn opp til 38 cm undersøkes ved kroppens fremkant før veggløsning.
  Sideveis inngang blokkeres dersom hele kroppshøyden ikke får plass.
- `--probe-walk`: fire retninger fra Grove Street med original kollisjon,
  1200 fysikksteg per retning. Logger faktisk fremdrift og kontakt med bakken.
- `--capture-dir PATH` sammen med smoke-testen lagrer private renderbilder.

## Verifikasjon

Arbeidsløkken bruker `cargo test --workspace`, `--probe-world` og
`--smoke-tour`. GPU-testen venter på vellykket rendering, laster ni områder
og sjekker endelige spillerposisjoner. Bilder ligger privat under
`native/target/captures/`. Grove Street-bildet er visuelt inspisert.

Utvalg før/etter retting av IPL-flagg, målt på 81 grunnprøver per område:
landsbygda 27 til 81, Las Venturas 62 til 81, ørkenen 11 til 74.
Dette beviser underlag ved prøvene, ikke at alle veier, broer eller trapper
er uten feil. Noen prøver kan ligge på vann eller bratte flater. Første
niområders-test brukte omtrent 0,2–1,1 sekund per CPU-last.

IPL-regresjonstesten feilet som forventet mot den gamle filtreringen. Et eget
generert rom testes gjennom native mod-loader og kollisjon: spilleren kan
gå gjennom døren og stå på gulvet. Testdata inneholder ingen spill-assets.

## Gjenstående arbeid

Bilfysikken er fortsatt enkel: grunnleggende fjæring og tilt finnes, men
videre tuning, hjulanimasjon, skade og trafikk gjenstår.
Én aktiv custom DFF/TXD-bil kan overta F9-bilen via `vehicles` i mod-formatet.
Følgekameraet testes mot vegger, tak og terreng.
F9 prøver flere bilplasseringer med underlag under fotavtrykket,
karosseriklaring og takhøyde; blokkert rotasjon i svinger avvises.
Utstigning prøver flere
sider og krever gulv, kroppsklaring og en fri vei ut. Dette bruker samme
COL/visuelle reservegeometri som øvrig kollisjon, med et begrenset antall prøver.
Spilleren har nå en synlig `fam1`-figur med tomgang, gange og løp i
tredjeperson. V bytter mellom første- og tredjeperson. Kameraet har
kollisjon mot kartet. AI for andre peds, riktig hoppe-/svømme-/bilsete-
animasjon og overgangsblending gjenstår. Multiplayer finnes som prototype;
personlige parkerte biler, passasjerreservasjoner og custom utseende deles.
Bilbytte og felles kollisjonssimulering gjenstår.
Asset-laget har nå en separat Skin/HAnim-laster og CPU-skinning med
beinvekter og inverse bind-matriser. Bind-posen er kontrollert mot 265
originale peds i det lokale arkivet. Dette brukes nå av spiller-renderingen,
og én custom skinnet spillerfigur kan overta den via `player` i mod-formatet.
Separate skinnede klær støttes via `player.clothes` med kompatible HAnim-
bein og bind-matriser. Garderoben (F6) lar deg slå importerte plagg av og
på under spilling. Kroppsdelmasker og montering av originale CJ-komponenter
gjenstår. Antrekk lagres foreløpig ikke mellom spilløkter.
ANP3-klipp kan nå samples og kobles til skjelettet via bein-ID. Alle 294
klipp i lokal `ped.ifp` er lest og kontrollert med fem gyldige prøveposer
per klipp. Gange, tomgang, løp og sprint matcher alle 32 spor til skjelettet.
Live tomgang og løp er visuelt kontrollert i GPU-bilder. `--smoke-ped`
tester 20 meter gange/løp, stopp og kamerabytte med kontrollert tidssteg.
FiveM/GTA V-ressurser lastes ikke direkte. En eksperimentell
[konverterer](gta5-conversion.md) kan klargjøre støttede Legacy-modeller som
native ressurser. Custom DFF-rom kan plasseres med
kollisjon, men dette er ikke en ferdig GTA V MLO-/portal-/interiørmotor.
Originalinteriørarkivet leses nå også. Separat romlasting og 600 steg med
gange/hopp er kontrollert i CJ sitt hus, Sweet sitt hus og Madd Dogg sin
villa. Interiørmenyen (I) og retur via R/kart er GPU-kontrollert. Dørinnganger gjenstår; se
`native-interiors.md` for kontrollen og begrensningene.

Kartet har detaljstreaming med foreløpig fjern-LOD. Bølger, dykking, tidsstyrte objekter,
enkelte RenderWare-effekter, transparentsortering og mer
korrekt fysikk gjenstår. Modeller kan hoppes over med forklaring i konsollen,
og noen materialer får reservefarge. COL-kuler bruker trekanttilnærming og
kroppskollisjon bruker et begrenset antall prøver fremfor en full kapselsweep.

Neste milepæl er å kjøre sammen med venner i egne biler. Se
[prioriteringer og kriterier](roadmap.md). Streaming, ytelse og videre tuning
av bilfysikken følger arbeidet som løpende kvalitetskrav.

Formatkilder: [COL](https://gtamods.com/wiki/Collision_File),
[IPL-flagg](https://gtaundergroundmod.com/pages/ug-mp/documentation/dl/map-dl/ipl/inst).

Kjørefysikken bruker nå treghet, gradvis styring og sideveis dekkgrep.
Se [vehicle dynamics](vehicle-dynamics.md) for detaljer og begrensninger.

Grafikkmenyen har egne kategorier, kvalitetsprofiler og fungerende FSR 1.
Se [grafikkstatus](graphics.md) og [Linux/Vulkan](linux.md). DLSS og temporal
FSR er fortsatt ikke integrert.

Original motorlyd og lokale fottrinn er koblet inn. Motorer høres også fra
nærliggende multiplayer-biler, inkludert bilen man sitter i som passasjer.
Voluminnstillingene gjelder disse lydene. Kollisjonslyd, materialavhengige
fottrinn og originalradio gjenstår; se [lydstatus](native-audio.md).

Bilfysikken bruker akselbaserte dekkrefter, delt grep for bremsing/svinging,
fire bakkekontakter og dempet fjæring/karosseribevegelse. Styreretningen med
tastatur og kontroller er rettet. Skade, bevegelige hjul og full
rigid-body-kollisjon er fortsatt ikke simulert.

En lokal prøvepakke med 32 oppskalerte teksturer ved 2x er laget og prøvd i
Vulkan. Dette er ikke en oppskalering av hele kartet eller en FPS-måling.
[Verktøyet for teksturoppskalering](texture-upscaling.md) lager PNG-overrides
som en separat native mod; originalfilene endres ikke. De genererte
spillteksturene er lokale og følger ikke Git-repoet.
