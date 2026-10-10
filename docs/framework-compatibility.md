# Rammeverk og Qbox: kompatibilitetsoversikt

**Status: SARE er ikke kompatibel med Qbox.** Målet er rammeverkstøtte først
(et FiveM-likt lag med ressurser, exports, events og state bags som rammeverk
kan bygge på), og senere en Qbox-portering. Direkte kompatibilitet blir først
lovet når en bestemt Qbox-versjon og alle dens nødvendige avhengigheter er
testet sammen på `sa-server`, og den testen er en del av CI.

Oversikten gjelder mest serversiden. Klient-Lua kjører i en sandkasse med et
lite sett klientnatives og et UI-API i ox_lib-stil som spillet tegner selv (se
[klientskript](client-scripts.md)). NUI (nettsider) støttes ikke, med vilje, og
GTA V-natives på klienten mangler.

## Testet versjonssett (10. oktober 2026)

| Ressurs | Versjon | Krav fra Qbox | Resultat på sa-server |
| --- | --- | --- | --- |
| `qbx_core` | 1.24.0 (`1825a3c`) | – | Laster med oxmysql-stub, stopper i sin egen oppstartsjekk: krever `ox_inventory` ≥ 2.42.1 |
| `ox_lib` | 3.40.0 (release-zip) | ≥ 3.20.0 | **Starter.** `@ox_lib/init.lua` i en annen ressurs fungerer for testede servermoduler |
| `oxmysql` | `main` `bae715c` | påkrevd | **Kan ikke kjøre:** JavaScript-ressurs for Node 22 (`server_script 'dist/build.js'`) |
| `ox_inventory` | 2.48.0 | ≥ 2.42.1 | Ikke testet: trenger oxmysql, OneSync og NUI |
| `qbx_vehicles` | 1.4.2 | ≥ 1.4.1 | Ikke testet: trenger qbx_core og oxmysql |
| FXServer | – | build ≥ 10731, OneSync | Versjonskrav ignoreres; OneSync finnes ikke |
| Database | – | MySQL/MariaDB + `qbx_core.sql` | Ingen databasestøtte i SARE ennå |

Prøvekjøringen av ox_lib brukte en testressurs med `shared_script
'@ox_lib/init.lua'` som kalte `lib.table.contains`, `lib.math.round`,
`lib.class`, `lib.locale`, `lib.addCommand`, `lib.timer`,
`lib.callback.register` og `lib.string.random`; alle kjørte uten feil. ox_libs
versjonssjekk feiler fordi `PerformHttpRequest` mangler. ox_libs klientdel og
NUI kjører ikke.

## Hva SARE støtter nå

| Funksjon | Brukes av | Status |
| --- | --- | --- |
| `fxmanifest.lua`, `server_script`/`shared_script`, globber | alle | Støttet |
| `shared_script '@ressurs/fil.lua'` | ox_lib, oxmysql, Qbox-ressurser | **Støttet** (nytt) |
| `provide 'qb-core'` for `dependency`, `exports[...]` og `GetResourceState` | qbx_core | **Støttet** (nytt) |
| Funksjoner gjennom exports og lokale events (`__cfx_functionReference`) | oxmysql-callbacks, Qbox-spillerobjekter | **Støttet mellom serverressurser** (nytt); frigjøres når ingen proxy holder dem |
| `exports.res.fn(nil, ...)`: første argument droppes alltid, som i FiveM | ox_lib | **Støttet** (nytt) |
| State bags: `GlobalState`, `Player(src).state`, `Entity(h).state`, `:set`, `AddStateBagChangeHandler`, `Get/SetStateBagValue` | ox_lib, qbx_core | **Delvis** (nytt): bare på serveren, ikke replikert til klienter |
| `msgpack.pack/unpack/pack_args` | ox_lib | **Støttet** (nytt); verdier går via JSON |
| CfxLua-syntaks: `` `hash` ``, `+=` m.fl., `a?.b`, `a?[k]` | ox_lib, qbx_core | **Støttet** (nytt): oversettes til Lua 5.4, også i `load()` |
| CfxLua-bibliotek: `table.clone/type/wipe/create`, `string.strsplit/strjoin/strtrim`, `io.readdir` | ox_lib | **Støttet** (nytt) |
| `debug`-biblioteket | ox_lib (`debug.getinfo`) | **Støttet** (nytt) |
| `playerConnecting` med `deferrals` og `setKickReason` | qbx_core | **Delvis** (nytt): spilleren er allerede koblet til, avvisning kaster spilleren ut |
| `GetPlayerIdentifierByType`, `GetNumPlayerIdentifiers`, `GetPlayerIdentifier` | qbx_core | **Delvis** (nytt): `license:<hex>` (stabil per server, ikke signert) og `sare:<id>`; `discord`, `fivem` m.fl. gir `nil` |
| Resource-KVP (`SetResourceKvp*`, `GetResourceKvp*`, `StartFindKvp`) | ox_lib, qbx_core | **Støttet** (nytt): lagres i `kvp/<ressurs>.json` |
| ACL med `identifier.license:...` | txAdmin-oppsett, Qbox-admin | **Støttet** (nytt) |
| `GetConvarBool` | ox_lib | **Støttet** (nytt) |
| Database (oxmysql-API) | qbx_core, ox_inventory | Mangler |
| `PerformHttpRequest`, `AddConvarChangeListener` | ox_lib | Mangler |
| `glm`-matematikk, `Citizen.InvokeNative` | ox_lib | Mangler |
| Serverentiteter (OneSync): `CreateVehicle`, `GetGamePool`, nettverks-ID-er | ox_lib, qbx_core | Mangler |
| Routing buckets | ox_lib, qbx_core | Mangler (`GetPlayerRoutingBucket` gir 0) |
| Klient-Lua (`client_script`) | alle Qbox-ressurser | **Delvis** (nytt): kjører i sandkasse; spiller- og posisjonsnatives, events, kommandoer, tastebindinger |
| UI: varsler, kontekstmeny, input-dialog, fremdriftslinje, tekst-UI | ox_lib (`lib.notify`, `lib.registerContext`, `lib.inputDialog` ...) | **Delvis** (nytt): eget `UI`-API med samme felter; ox_libs NUI-versjon kjører ikke |
| NUI (`ui_page`, `SendNUIMessage`) | ox_lib, ox_inventory, qbx_core | Støttes ikke (sikkerhet); bruk skript-UI-et |
| GTA V-natives på klienten | alle Qbox-ressurser | Mangler |

## Trinnplan

1. **Ressurslaget (ferdig):** `@`-includes, `provide`, funksjonsreferanser,
   state bags på serveren, msgpack, identifikatorer, deferrals.
2. **CfxLua (ferdig):** syntaks og biblioteksutvidelser, `debug`. ox_lib 3.40.0
   starter på serveren.
3. **Database og server-API:** en oxmysql-kompatibel ressurs skrevet i Rust
   (samme exports: `query`, `single`, `scalar`, `insert`, `update`,
   `transaction`, `prepare`) mot MySQL/MariaDB, slik at `@oxmysql/lib/MySQL.lua`
   fungerer uendret. I tillegg `PerformHttpRequest`, resource-KVP og
   `AddConvarChangeListener`.
4. **Identitet (delvis ferdig):** `license:` fra en nøkkel hos spilleren,
   saltet per server. Gjenstår: signatur, slik at en server ikke kan låne en
   annen servers license, og deferrals før spilleren slippes inn.
5. **Serverentiteter:** biler, peds og objekter som serveren eier, routing
   buckets og replikerte state bags.
6. **Klient-Lua og UI (delvis ferdig):** sandkasse, nedlasting med SHA-256,
   UI tegnet av spillet. Gjenstår: flere SA-varianter av GTA V-natives og en
   ox_lib-bro som oversetter `lib.notify`/`lib.registerContext` til `UI`.
7. **Qbox-test:** fest versjonene over, kjør qbx_core, ox_lib, oxmysql-erstatningen
   og ox_inventory sammen i CI. Først da kan en bestemt Qbox-versjon kalles
   kompatibel.

## SARE Box

SARE Box er SAREs eget rammeverk i Qbox-form (spillerobjekter, penger, jobber,
lønn, adminmeny, lagring per license). Det er ikke Qbox, men viser at et
rammeverk med samme oppbygning kjører på SARE i dag. Se [SARE Box](sarebox.md).

## Bygge et rammeverk på SARE nå

`server-data/resources/[examples]/` viser mønsteret som Qbox og ox_lib bruker,
med SARE-API-et som finnes i dag:

- `sare_lib`: et delt bibliotek som andre ressurser laster med
  `shared_script '@sare_lib/init.lua'`, med moduler som lastes ved behov og
  callbacks lagret som funksjonsreferanser.
- `sare_core`: en kjerne med spillerobjekter (penger, jobb) som eksporteres med
  funksjoner, `provide 'sare-core'`, state bags og `playerConnecting`-deferrals.
- `sare_jobs`: en ressurs som bare bruker kjernen gjennom navnet `sare-core`.

Start dem med `ensure sare_jobs` i `server.cfg` (avhengighetene starter selv).
I spillet: `/job taxi`, `/work` og `/money`. `cargo test -p sa-server` kjører
dem mot en ekte klientforbindelse.

## Sjekke en ressurs

`python tools/framework-check.py <mappe> [--all]` leser `fxmanifest.lua` for hver
ressurs i mappen, finner Lua-filene serveren ville kjørt, og lister
FiveM-globaler som SARE mangler, sammen med funksjoner som state bags,
CfxLua-syntaks, deferrals og OneSync. `--all` tar også med moduler som lastes
med `LoadResourceFile` + `load`. Resultatet er et statisk estimat: delte
skript inneholder ofte klientkode bak `IsDuplicityVersion()`, og en ressurs uten
manglende navn kan fortsatt feile når den kjører.

Med versjonene over gir verktøyet 34 manglende globaler for ox_lib (`--all`) og
84 for qbx_core, nesten alle entitets-, nettverks- og klientnatives.
