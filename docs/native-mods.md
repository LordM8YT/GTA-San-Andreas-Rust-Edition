# Native mod-ressurser

Native freeroam leser `mods/<ressurs>/mod.json`, alfabetisk etter mappenavn.
Start runtime på nytt etter endringer. `--no-mods` slår dem av og
`--mods-dir PATH` velger en annen rotmappe. En senere ressurs med samme
modell-ID erstatter definisjonen.

```json
{
  "schema_version": 2,
  "enabled": true,
  "name": "Mitt bygg",
  "models": [
    { "id": 30000, "dff": "building.dff", "txd": "building.txd", "col": "building.col" }
  ],
  "placements": [
    { "model_id": 30000, "position": [2500, -1670, 12.35], "rotation": [0, 0, 0, 1] }
  ],
  "exclude_model_ids": [],
  "texture_overrides": { "cj_bins:miamibin": "textures/my.png" }
}
```

Posisjoner er originale GTA XYZ. Quaternion bruker IPL-konvensjonen.
Velg en ubrukt ID, for eksempel fra 30000. En eksisterende ID erstatter
originalmodellen på dens eksisterende kartplasseringer. Modellfilene må være
lokale til ressursmappen. DFF uten teksturer trenger ikke `txd`.

Rigid PC RenderWare-DFF støttes for kartmodeller. Skinnet PC DFF støttes
for `player` som beskrevet nedenfor. Native-/multimorph-geometri støttes
ikke. TXD bruker D3D9-dekoderne. PNG kan være opptil
4096 × 4096. COL må ha nøyaktig én modell. Uten COL brukes synlige opake
trekanter som kollisjonsreserve. Med COL brukes modellens egen fysikk.
Dette lar deg lage custom bygg/rom du kan gå i, men er ikke GTA V MLO-import.

`placements` kan bruke en original model-ID uten en `models`-oppføring.
`texture_overrides` og `exclude_model_ids` bruker samme nøkkelstil som den
eldre WebGL-testen. Feltet `settings` i de eldre demonstrasjonene påvirker
foreløpig bare WebGL-runtime.

`mods/native-room-demo` inneholder et eget generert DFF-rom med dør, gulv,
tak, vegger og benk. Sett `enabled` til `true` for å plassere det på Grove
Street. Det er deaktivert som standard så du kan velge plasseringen selv.
`tools/generate_native_room.py` gjenskaper filen fra egen geometri.

Dette er data-ressurser; DLL-er eller scripts kjøres ikke. ID-er for peds og
biler gir ikke automatisk AI, skjelettanimasjon, klær eller kjørefysikk.
FiveM-filer og FiveM-script-API er ikke kompatible med dette formatet.

Kjørbare custom biler registreres separat:

```json
{
  "schema_version": 2,
  "enabled": true,
  "name": "Min bil",
  "vehicles": [{ "dff": "car.dff", "txd": "car.txd" }]
}
```

Foreløpig støttes én aktiv custom bil totalt. Den erstatter taxien på F9
og bruker de samme kjørekontrollene. `txd` kan utelates for en uteksturert
DFF. Alle teksturer må finnes i ressursens TXD; automatisk søk i originale
vehicle.txd er foreløpig bare tilgjengelig for standardtaxien. Bruk en
vanlig personbil med original SA-størrelse og modellakse. Fysikken bruker
en fast personbilform, uten egen handling, hjulanimasjon eller skader.

`mods/native-car-demo` er en egen enkel blå bil uten originale assets.
Aktiver `enabled` i dens `mod.json` og start på nytt for å prøve den.
`tools/generate_native_car.py` gjenskaper geometrien. Demoen er deaktivert
som standard slik at normal oppstart fortsatt bruker taxien.

Custom spillerfigur registreres med `player`:

```json
{
  "schema_version": 2,
  "enabled": true,
  "name": "Min spillerfigur",
  "player": { "dff": "ped.dff", "txd": "ped.txd", "ifp": "ped.ifp" }
}
```

Én aktiv spillerressurs støttes. Modellen må ha PC Skin/HAnim-data med
bein-ID-er og vekter. `txd` kan utelates for uteksturert geometri. `ifp`
kan utelates når skjelettet passer til original `anim/ped.ifp`; ellers
må egne ANP3-klipp hete `idle_stance`, `walk_player` og `run_player`.
Alle bein med vertexvekter må ha et spor i hvert av disse klippene.
Spor bindes foreløpig etter bein-ID, ikke navn. ID -1 med bare navnebinding
støttes derfor ikke for spillerens vektede bein. Modellen må være oppreist
og mellom 1 og 2,5 meter høy i tomgang. Fysikken bruker fortsatt den faste
spillerkroppen; skjelettstørrelsen endrer ikke kollisjonsformen.

`mods/native-ped-demo` inneholder en egen skinnet figur og egne klipp,
generert av `tools/generate_native_ped.py`. Aktiver `enabled` og start
på nytt for å prøve den. Demoen er deaktivert som standard.
Dette erstatter hele spillerfiguren. Figuren skjules foreløpig under bilkjøring.

Separate skinnede plagg kan legges til spillerressursen:

```json
"player": {
  "dff": "ped.dff",
  "txd": "ped.txd",
  "clothes": [{ "name": "Jakke", "dff": "jacket.dff", "txd": "jacket.txd", "enabled": true }]
}
```

Inntil 16 plagg støttes. Hvert plagg må ha Skin/HAnim-data og samme bein-ID-er,
modellkoordinater og bind-pose som spilleren. Bein rekkefølgen i paletten
kan variere; lasteren kobler dem etter ID. Bind-matrisene må passe innen
0,01. Plagg følger spillerens tomgang, gange og løp og kan bruke egen TXD.
Uteksturerte plagg trenger ikke TXD. Kroppsgeometrien beholdes under klærne,
så plagget må tilpasses for å unngå gjennomstikk; kroppsdelmasker gjenstår.
F6 åpner garderoben. Plagg kan slås av og på med mus eller piltaster og
Enter mens spillet kjører. `name` er valgfritt visningsnavn, og `enabled`
velger starttilstanden (standard: true). Antrekket lagres foreløpig ikke
mellom spilløkter. CJ sin
opprinnelige komponentmontering og FiveM-klær lastes ikke direkte.

`mods/native-clothing-demo` viser en separat rød jakke og gul hatt på vår
egen demofigur. Den er deaktivert som standard. Aktiver bare én av
spiller-demoene om gangen. `tools/generate_native_clothes.py` gjenskaper den.

Filer begrenses til 16 MiB hver, totalt 512 modeller, 512 teksturoverstyringer
og 256 MiB mod-ressurser. Ressurser kan ikke referere til filer utenfor egen
mappe. Del egne assets; originaldata leses fra brukerens installasjon.
Dekodede spillerteksturer har en egen grense på 128 MiB.
