# Arbeidslogg: freeroam, natt til 7. oktober 2026

Brukerens prioritet: mest mulig spillbart kart uten missions, og et grunnlag
for mods med klær, peds, biler og eventuelt custom interiører. Arbeidet kan
fortsette til omtrent kl. 10 den 7. oktober, Europe/Oslo.

## Første milepæl

Native kartstreaming, rettet IPL-flagg/interiør-filtrering, original COL,
gåmodus som standard, 1–9 teleport, native data-mods og eget walkable rom.
Se `freeroam-status.md` og `native-mods.md` for bruk og begrensninger.

Verifisert etter siste kildeendringer:

- 13 Rust-tester besto, inkludert mod-rom og IPL-regresjon.
- Clippy besto med `-D warnings -A clippy::chunks_exact_to_as_chunks`.
  Unntaket beholder eksisterende slice-API uten et krav om nyere Rust-API.
- Release-bygg besto. Faktisk skjult GPU-tour rendret og streamet ni områder.
- Private renderbilder ble lagret under `native/target/captures/`.
- `gta3.img` SHA-256 var lik før og etter kontrollene:
  `EFEB84043C37053B53CABB50702AB6D36FCAB6620FEFFFDC19E3156BD590B46C`.

Dette er en milepæl, ikke ferdig FiveM-kompatibilitet eller fullstendig spill.

## Menyer

Brukerens tillegg: god hovedmeny, settings og andre menyer inspirert av spillet.
Implementert native hovedmeny, pause, kart, settings, kontroller, mods og
avslutning. Egen OFL-font og lokal systemfont gir blackletter/gull-uttrykk.
Innstillinger lagres lokalt; pause og fokus-tap stopper spillerbevegelsen.

Etter siste menyendringer besto 15 Rust-tester, Clippy og release-bygg.
GPU-test rendret alle sju menysider og alle ni kartområder, fra prosjektroten
med lokale ressurser aktivert. Hovedmenybildet ble visuelt kontrollert.
Bildene ligger i `native/target/menu-captures-final/`.

Streaming-retting: fem sekunders ventetid etter last-/opplastingsfeil og
eksplisitt håndtering av frakoblet bakgrunnslaster. Regresjonstest simulerer
både mislykket last, retry og bortfall av arbeider. Alle 16 Rust-tester og
Clippy besto etter rettingen.

## Sammenhengende streaming og tak

Hopp kolliderer nå med tak via vertikale prøver ved hodet/kroppens kant.
En test med gulv og lavt tak bekrefter at hodet stopper og spilleren lander.
Null tidssteg endrer ikke hoppet. Etter disse endringene besto 17 Rust-tester,
Clippy og release-bygg.

Ny `--smoke-stream` bruker native rendering og vanlige flybevegelsestaster
gjennom omtrent 1,2 km frem og tilbake, uten teleport. Faktisk kjøring
besto med sju nabolagsbytter og kontrollerte kamerabevegelsen ved hver frame.
Dette verifiserer sammenhengende flystreaming, ikke alle gangbare ruter.

## Gangkollisjon: trinn og åpninger

Sideveis inngang under tak som er for lave blokkeres nå. Trinn opp til
38 cm vurderes ved kroppens fremkant før veggkollisjon; 30 cm testtrinn
kan passeres. Lave tak over trinnet og 60 cm trinn blokkeres. Samlet
takspørring gjenbruker én lokal trekantliste; stillestående spillere hopper
over horisontale kollisjonsspørringer. 19 Rust-tester og Clippy besto,
og release bygget.

Ny `--probe-walk` kjørte fire retninger på original Grove Street-kollisjon,
1200 fysikksteg per retning. Fremdrift var 21,9/31,9/11,1/22,8 meter før
hindringer, med bakkekontakt i 1200 av 1200 steg i hver retning. Ingen fall
ut av kartet. Dette er en kort lokal gangkontroll, ikke full kartverifikasjon.

## DFF-formatdekning

Undersøkte originalmodellene som ble avvist. Flere har blokk 43 (UV-ordbok)
før clump-blokk 16. Loaderen leser nå forbi denne ordlisten til geometrien;
UV-animasjon er fortsatt ikke implementert. Noen modeller har NaN i UV-data,
men gyldige posisjoner. Kun disse UV-verdiene får 0 som reserve; ugyldige
posisjoner avvises fortsatt. Nye tester kontrollerer både innledende ordbok,
grenser og forskjellen mellom UV-reserve og ugyldig romlig data.

21 Rust-tester, Clippy og release besto. Read-only `audit_dff`-eksempelet
dekodet 12 955/12 955 modeller og 4 576 878 trekanter i lokal `gta3.img`.
GPU-tour med ni områder besto; Las Venturas økte fra 1675/381734 til
1677/381982 plasseringer/trekanter. Private bilder ligger under
`native/target/captures-coverage/`. Ingen spill-assets ble skrevet til source.

Formatreferanse: DragonFF-dokumentasjonen for blokktyper og ordbok/clump-lesing.

## Teksturarv og statiske anim-objekter

Implementert IDE `txdp`-oppslag med barn først, foreldrekjede, syklusvern
og dybdegrense. Lokal TXD-bytecache per last er begrenset til 64 MiB.
Test bekrefter parse, prioritetsrekkefølge og sirkulær referanse.
Gjenværende reservefarger i de ni utsnittene er uendret; de aktuelle
ordbøkene har ingen deklarert forelder som løser teksturene.

IDE `anim` tas nå med som statisk kartgeometri. Test bekrefter registrering
av modell/TXD uten å tolke animasjonsnavnet som tekstur. GPU-tour besto
alle ni områder. Stranden økte til 1664 plasseringer/323515 trekanter,
San Fierro til 1454/400599, ørkenen til 279/74650. 23 Rust-tester,
Clippy med alle targets og release besto.

Referanse for foreldresøk: [TxdStoreFindCB](https://github.com/gta-reversed/gta-reversed/blob/master/source/game_sa/TxdStore.cpp).

## Vann og overflatebevegelse

Leser original `water.dat` med 301 firkanter og seks trekanter. Synlighetsflagg
respekteres ved rendering; høyder kan spørres separat. Fast verdensopprinnelse
brukes også her. Lokal `particle.txd:waterclear256` leses kun ved runtime.
Vann holdes utenfor vanlig kollisjonsgulv. Enkel oppdrift holder kroppen i
overflaten; WASD beveger saktere i vann, Space løfter, og pause fryser fysikk.

25 Rust-tester, Clippy for alle targets og release besto. `--probe-water`
besto 600 fysikksteg i original kystgeometri ved vannhøyde 0, med øyehøyde
0,4 og omtrent 25 meter bevegelse. GPU-tour besto ni områder, med bilder
i `native/target/captures-water/`. Strandbildet ble visuelt kontrollert.
Bølger, dykking, strøm, båter og svømmeanimasjon gjenstår.

Formatreferanse: [WaterLevel blokklesing, flagg og teksturer](https://github.com/gta-reversed/gta-reversed/blob/master/source/game_sa/WaterLevel.cpp).

## Fjern-LOD

LOD-plasseringer inkluderes nå opptil 2,5 km etter transformerte modellgrenser.
De holdes ute dersom grensene berører detaljområdet, og inngår aldri i
kollisjon. Fjernplanet økt til 3,5 km. Grove-lasten inkluderer 1899 ekstra
fjernplasseringer (4692 totalt / 707673 trekanter). Noen LOD-materialer
mangler teksturer og bruker reservefarge; store grupper kan fortsatt gi
overgangshull eller synlig bytte. Full foreldrehierarki/fading gjenstår.

26 Rust-tester, Clippy for alle targets og release besto. GPU-tour besto
ni områder. Renderbilder ligger i `native/target/captures-lod/`; landsbygda
og Mount Chiliad ble inspisert visuelt.
Sammenhengende `--smoke-stream` besto også omtrent 1,2 km frem/tilbake
med sju nabolagsbytter, uten teleport eller kamerahopp.

## Kjørbar taxi

Lagt inn separat dynamisk kjøretøygeometri og enkel controller med gass,
rygging, styring, brems, tyngdekraft og to kroppsprøver mot original kollisjon.
F9 henter taxi og setter spilleren inn; F går ut/inn. Følgekamerа følger bilen.
Teleporter og gjenoppretting går tilbake til gangmodus. Menyer stopper tiden.

Vanlig `taxi.dff` brukes med `_dam`, `_vlo` og extras filtrert bort; hjulmesh
instansieres på wheel-dummy-rammer. Materialer skilles for å holde glass og
karosseri fra hverandre. Original paint-markør erstattes med taxigul.
Opake karosseriteksturer får solid alpha. Renderpass bytter nå også tilbake
til opakt pass når dynamiske modeller tegnes etter kartets alpha-flater.

En egen modellvariant-test og kjøretøytest er lagt inn. Videre gjenstår
fjæring/tilt, hjulrotasjon, riktig kapsel-/karosserisweep, kamerahindringer,
skade, motorlyd, trafikk og kjørbare custom bilressurser.

Etter endringene besto 28 Rust-tester, Clippy for alle targets og release.
Ny `--smoke-car` besto med faktisk bevegelse over én meter, bremsing,
utstigning og ny innstigning. Renderbildet ble visuelt kontrollert og ligger
i `native/target/captures-car-verified/car.png`. Niområders GPU-tour besto
også med dynamisk bilmodell inkludert.

Gjenstående: kontinuerlig bevegelses-/streamingtest, feiltilstand/retry,
ytelse (bl.a. COL-kuler), vann, fjern-LOD, TXD-arv, synlig spiller og
animasjon, kjørbare biler, utvidede custom ressurser og interiører.

## Custom drivable resources

Added a bounded `vehicles` manifest entry for one active custom DFF/TXD car.
F9 uses that resource instead of the original taxi. The custom car shares
the existing arcade controller; handling, suspension, wheels and damage
remain unfinished. Added an original-free procedural blue car demo, disabled
by default. All 28 Rust tests, Clippy and release build passed. A GPU car
smoke with a temporary enabled demo passed movement, braking, exit/re-entry;
`native/target/captures-custom-car/car.png` was visually inspected.

## Vehicle camera and exit clearance

Chase camera now clips a five-ray segment against the collision spatial grid
with clearance before walls, roofs and terrain. A coincident target has a
finite view fallback. Exit searches side/opposite/front/rear placements,
requires grounded body/head clearance, and rejects paths through walls.
If no exit is clear, the player stays inside and the car stops.
Two regressions cover both-sided long camera rays, ground/roof intersections,
blocked-side exit, missing floor and low roof. 30 workspace tests passed;
Clippy and release build passed after a range-style correction. Original
taxi and custom demo GPU car smokes both passed. Captures are under
`native/target/captures-car-camera` and `captures-custom-car-camera`.

## Vehicle placement and turn collision

F9 now searches five nearby placements, samples nine ground points under
the footprint, rejects unsupported/steep footprints, checks roof clearance
and body walls. It uses the player/car ground level, rather than treating
a low roof as a spawn floor. Failure preserves the previous vehicle.
Turning tests the proposed body pose and rolls back blocked rotation;
motion uses bounded substeps and stops before a blocked pose. These remain
approximate three-circle body checks rather than a complete convex sweep.
Two regressions cover blocked preferred placement, missing support, low
roof, and steering parallel to a nearby wall. The roof test first exposed
an incorrect maximum-floor height, which was fixed. 32 workspace tests,
Clippy and release build passed. Original and custom car GPU smokes passed;
captures are `native/target/captures-car-placement` and
`native/target/captures-custom-car-placement`.

## Ped skinning foundation

Added `sa_assets::skin` with bounded PC DFF Skin/HAnim parsing, palette-to-
frame mapping, frame hierarchy evaluation, inverse bind matrices and four-
weight CPU skinning. Poses transform positions and normalize normals.
Legacy matrix markers are read; native and split skin palettes remain
unsupported. This API is not yet wired into the player renderer or mods.

Read-only `audit_skin` checked all 265 peds listed in local peds.ide that
exist in gta3.img: bind poses match rigid atomic-frame geometry to <0.01m
(the printed maximum is 0.000000 for those assets). Eleven null/special
names were absent; player.dff was excluded because CJ is assembled from
separate components. The initial comparison against raw mesh coordinates
was incorrect because the atomic frame transforms those coordinates;
the corrected audit compares against decoded atomic-frame geometry.

Original-free regressions cover DFF/HAnim/skin round trip, weighted
hierarchical pose changes, finite values, truncated buffers, bone indices,
weight sums, unsupported split palettes and cyclic/out-of-range frames.
Clippy for all targets passed. Audit log: native/target/ped-skin-audit.log.
Format reference: DragonFF documentation for DFF format details.

## Native ANP3 animation foundation

Added bounded ANP3 decoding for compressed and float rotation/translation
frames (types 1-4), clip durations, quaternion normalization, shortest-arc
interpolation, looped sampling and bone-ID binding to the skin model.
In-place sampling removes root XY motion while retaining vertical bob.
Tracks without matching HAnim IDs are currently skipped; name-based
binding for bone ID -1 remains to implement. This is not yet in the live
player renderer and rendered animation orientation remains unverified.

The complete local ped.ifp decoded: 294 clips, 112150 keys. Five poses per
clip were skinned on fam1 and checked finite/bounded. All 32 tracks in
idle_stance, walk_player, run_player and sprint_civi bind to the skeleton.
Durations: idle 1.5s, walk 1.2s, run 0.733s, sprint 0.533s. Original-free
fixtures verify compressed/float equivalence, translation /1024, times
/60, truncated inputs, invalid data, shortest quaternion arc and in-place
root looping. 37 workspace tests passed. Clippy passed after cleaning a
fixture loop; release runtime built. Read-only command:
`cargo run --release -p sa-assets --example audit_ifp -- <ped.ifp> <gta3.img> fam1.dff`.
Primary format references:
https://raw.githubusercontent.com/gta-reversed/gta-reversed/master/source/game_sa/Animation/AnimSequenceFrames.h
https://raw.githubusercontent.com/gta-reversed/gta-reversed/master/source/game_sa/Animation/AnimManager.cpp
The former confirms compressed translation scale 1024 (not 4096).

## Live player figure

Added animated fam1 DFF/TXD player mesh, using native skin/IFP sampling
and GPU vertex updates. Third person is default; V toggles first person.
View camera collision leaves physical player coordinates unchanged.
Actual displacement selects idle/walk/run and facing; animation time
freezes in menus. Figure is hidden when flying, driving or first person.
Feet alignment uses the upright idle pose. Jump/swim/vehicle-seat clips,
blending, shadows, custom player resources and CJ clothing are unfinished.

Nine-region GPU tour passed with the figure active. The upright textured
Grove figure was inspected in captures-ped-first/region-1.png. A dedicated
smoke uses a controlled 1/60s step for >20m walk/run, idle stop and first/
third-person switching; all phases passed and run capture was visually
inspected in captures-ped-verified/ped-run_player.png. Initial smoke mode
accidentally entered the tour branch; this harness conflict was fixed.
Two hung owned test processes were stopped after a failed rebuild caused
by the live EXE lock; the fresh build and repeat smoke passed.
37 workspace tests, Clippy and release build passed. Car smoke also passed
with the new player system installed. Images remain private under target.

## Custom animated player resources

Native mod manifest now accepts one `player` DFF, optional TXD and optional
ANP3 IFP. Without IFP it uses local original ped.ifp. Required idle/walk/run
clips must contain every weighted bone ID. Upright idle height is validated;
texture decoding has a 128 MiB budget. This replaces the whole figure, not
separate clothing. Name-only bone binding and CJ components remain pending.

Added an original-free eight-bone procedural humanoid and three custom clips
in mods/native-ped-demo (disabled by default). Regression coverage checks
custom loading, pose movement, weighted-bone rejection and manifest integration.
39 workspace tests passed. Clippy and release passed after moving the
test module and simplifying an Option match. Dedicated custom-ped GPU
smoke passed >20m walk/run, idle and camera toggle. Run capture visually
inspected: native/target/captures-custom-ped/ped-run_player.png.

## Separate skinned clothing

Added player.clothes (max 16) with bounded DFF/TXD reads. Clothing bone
palettes remap by HAnim ID to the player, and inverse bind matrices must
match within 0.01. Clothing uses the player skeleton/clip poses; named
textures get a clothing namespace so separate TXDs can share names.
Geometry parts use separate batch prefixes. Body geometry remains visible
under clothing; body masking and wardrobe selection are still pending.

The original-free native-clothing-demo includes a red jacket and yellow hat
on our own eight-bone figure. It is disabled by default. Regression tests cover
a reordered bone palette, moving jacket sleeves, incompatible bind rejection,
and full manifest-to-player clothing integration. 41 workspace
tests and Clippy passed; release built. Dedicated GPU ped smoke passed
walk/run/idle and camera switching with the clothing resource enabled.
Run image inspected: native/target/captures-custom-clothes/ped-run_player.png.


### Live wardrobe

Added F6 wardrobe access and main/pause menu entries. Imported clothing
resources expose names and initial enabled states. Mouse and keyboard
choices update the skinned player immediately. Hidden clothing keeps the
GPU layout stable and only clears its own vertex alpha, preserving the
body and other garments. The disabled clothing demo now has separate
jacket and hat assets. Outfit persistence and body masks remain pending.

Verification: all 43 workspace tests passed; strict Clippy passed; release
build passed. The controlled wardrobe smoke walked/ran/stopped, toggled
clothing off/on and checked camera changes. All eight menu pages were
captured. wardrobe-off.png, wardrobe-on.png and menu-7.png were visually
inspected under native/target: jacket visibility changes correctly while
the hat and body remain visible.


### Original interior loading

WorldLoader now indexes gta_int.img alongside gta3.img, including streamed
IPL placements, DFF/TXD assets and COL models. Duplicate names preserve
gta3.img precedence. Exterior loads filter dimension zero; load_interior
filters a nonzero dimension and omits exterior water. Runtime access from
menus and doors remains the next task. See native-interiors.md.

Verified --probe-interiors against CJ house, Sweet house and Madd Dogg
mansion: safe entrances, 600 walking/jump steps per room, no floor fall,
grounded finish. Indexed 44804 exterior/5965 interior placements and 10155
collision models. --probe-world loaded all nine exterior destinations.
All 44 workspace tests and strict Clippy passed. Updated constant-size
chunk iteration to the toolchain-preferred as_chunks API without changing
decoder semantics; release rebuilt and interior probe rerun successfully.


### Cline handoff - October 7

Inspected the latest Cline session and current Git working copy before
continuing. Kept the English menus, gamepad input, radar UI, speedometer,
vehicle settings and Kira audio services. The existing baseline passed
55 tests and strict Clippy. No unrelated removals or modifications were
reset. Cline had stopped during audio-prototype verification.

Implemented bounded original PCM SFX bank decoding and stereo frontend
back/highlight/select cues. Verified 710 indexed banks and 690 sounds /
12416408 samples in banks 0..143. Loop offsets are sample indices, confirmed
against the primary loader; bank 138 includes a valid 2021 Hz sample. A
large unrequested speech bank no longer blocks directory indexing.

Fixed radar loading: original radar00.txd through radar143.txd are entries
in gta3.img, not a standalone models/radar.txd. Both the minimap and full
map now render original tiles; removed the fabricated overview polygons.
Actual GPU map capture visually inspected. Nine menu pages passed GPU smoke,
with audio output initialized and Xbox Controller detected.

Completed live interior travel checks. The first route exposed an open-door
floor fall in Sweet house. Added room movement that rejects unsupported
portal edges, with a floor-edge/jump regression test. Three interiors and
return to Grove Street now pass the GPU route. Hide player geometry when
the collision camera is too close, avoiding a screen full of shirt inside
small rooms; the corrected Sweet capture was visually inspected. Show
speedometer only while driving. Latest 59 workspace tests, strict Clippy
and release build passed. Original radio, automatic footsteps/engine sound
and volume controls remain follow-up work, documented in native-audio.md.
