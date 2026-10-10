# Dedicated server (FiveM-style)

`sa-server` is laid out and configured like a FiveM server (FXServer): a
`server-data` folder with `server.cfg` and `resources/`, resources described by
`fxmanifest.lua`, server-side Lua scripts with FiveM's API, and the same console
commands. It needs no window, GPU, Steam installation or San Andreas files.
Up to 20 players. Movement is still simulated by clients.

## Start

**Download:** every tested main-branch build publishes `SARE-server-windows.zip`
and `SARE-server-linux.zip` on the [latest release](https://github.com/LordM8YT/GTA-San-Andreas-Rust-Edition/releases/latest).
Extract it and run `start-server.cmd` (Linux: `./start-server.sh`). The first
start asks for a template, **Freeroam** or **[SARE Box](sarebox.md)**, and
creates `server-data/server.cfg` (without a console: `+set sv_template sarebox`). The package keeps binaries in `server/` and your data in
`server-data/`. Started from those scripts, the server installs new releases by
itself while no players are online; see [server package](server-package.md).

From source:
On Windows, run `start-server.cmd`. It builds `sa-server` when Cargo is available
and runs it inside `server-data/`, like `FXServer.exe +exec server.cfg`.
On Linux, `./start-server.sh` builds and runs the server from the project
directory; its first run creates the server template there.

```
server-data/
  server.cfg
  resources/
    [system]/chat/                 chat messages, /say, command suggestions
    [managers]/mapmanager/         game type, map resources and spawn points
    [managers]/spawnmanager/       moves joining players to a spawn point
    [gamemodes]/basic-gamemode/    freeroam game type, join/leave messages, /respawn
    [gamemodes]/[maps]/sare-map-grove/   Grove Street spawn points
    [examples]/                    example framework (sare_lib, sare_core, sare_jobs), not started
    [sarebox]/                     SARE Box framework (sarebox_core, sarebox_admin), started by its template
    [local]/                       your own resources
```

For deployment, copy `sa-server.exe` and your `server-data` folder. Running
`sa-server` in an empty folder asks for a template and creates it. Without
`+exec`, server.cfg runs first. Launch arguments use FiveM syntax:
`sa-server +exec server.cfg +set sv_maxclients 8`.

## server.cfg

| Line | Meaning |
| --- | --- |
| `endpoint_add_tcp "0.0.0.0:7777"` | Game endpoint. rcon answers over UDP on the same port. |
| `sv_hostname "Name"` | Server name (1–24 characters) in the browser. |
| `sv_maxclients 20` | Player slots, 1–20. Can be changed live. |
| `ensure name` / `ensure [category]` | Start a resource, or every resource in a bracket folder. |
| `set` / `sets` / `setr` | Convars; `sets` marks server information (`sv_projectName`, `tags`, `locale`). |
| `exec file.cfg` | Run another cfg file. |
| `rcon_password "secret"` | Enables Quake-style rcon (FiveM's protocol) over UDP. |
| `add_ace` / `add_principal` | Permissions, e.g. `add_ace group.admin command allow`. |
| `set sv_relay "ip:port"` | Publish through a SARE relay: server browser and join code. |
| `sv_master1 ""` | With a relay: hide from the browser, join code only. |

`endpoint_add_udp`, `load_server_icon`, `sv_licenseKey`-style convars are accepted
so FiveM cfg files keep working; SARE gameplay itself is TCP. Allow TCP (and UDP
for rcon) on the port for internet hosting.

## Console

`status`, `resources`, `ensure|start|stop|restart <resource>`, `refresh`,
`clientkick <id> <reason>`, `say <text>`, `add_principal player.<id> group.admin`,
`test_ace`, `list_aces`, `cmdlist`, `quit`. Any `RegisterCommand` command can be
run from the console. Players run commands from chat (`/respawn`); built-in and
restricted commands need `command.<name>` in the ACL.

Player identity is the session ID only. There are no accounts or license
identifiers yet, so grant admin per session (`add_principal player.3 group.admin`).

## Resources and Lua

A resource is a folder with `fxmanifest.lua`:

```lua
fx_version 'cerulean'
game 'common'
server_script 'server.lua'      -- also server_scripts { 'server/*.lua' }
dependency 'spawnmanager'
```

Server scripts run in one Lua 5.4 state per resource, like FiveM. Supported:
`AddEventHandler`, `RegisterNetEvent`, `TriggerEvent`, `TriggerClientEvent`,
`CancelEvent`/`WasEventCanceled`, `RegisterCommand`, `ExecuteCommand`,
`Citizen.CreateThread`/`Wait`/`SetTimeout`, `exports(...)` and
`exports.resource:fn(...)`, `GetPlayers`, `GetPlayerName`, `GetPlayerPed`,
`GetEntityCoords`, `GetEntityHeading`, `SetEntityCoords`, `SetEntityHeading`,
`DropPlayer`, `IsPlayerAceAllowed`, convars, resource metadata,
`LoadResourceFile`/`SaveResourceFile`, `json`, `msgpack`, `vector3` and `promise`.
Events: `playerConnecting` (with deferrals), `playerJoining`, `playerDropped`,
`onResourceStart`/`Stop`, `chatMessage`. Coordinates are San Andreas world
coordinates.

For frameworks: `shared_script '@other/file.lua'` includes, `provide`,
functions passed through exports and local events, server-side state bags
(`GlobalState`, `Player(src).state`, `AddStateBagChangeHandler`), CfxLua syntax
(`` `hash` ``, `+=`, `a?.b`) and its `table`/`string` additions, and the `debug`
library. ox_lib 3.40.0 starts on the server. See
[framework compatibility](framework-compatibility.md) for what Qbox still needs,
the example framework in `resources/[examples]` and `tools/framework-check.py`.

Client scripts run sandboxed on players' machines, and scripts show menus,
dialogs, notifications and progress bars through the game's own UI; see
[client scripts](client-scripts.md).

Players have a `license:` identifier that stays the same on your server
(see [SARE Box](sarebox.md#players-and-saving) for how it works and its
limits), so the ACL can grant rights by license:
`add_principal identifier.license:... group.admin`. Resource KVP
(`SetResourceKvp`, `GetResourceKvpString` ...) saves to `kvp/<resource>.json`
next to server.cfg.

Not supported yet: NUI pages (by design: use the script UI), OneSync entities, routing buckets, replicated state bags,
databases (oxmysql is a Node.js resource), `PerformHttpRequest`, and identifiers
other than `license` and `sare`. `playerConnecting` runs after the player has
joined, so a rejection drops them. FiveM resources that use GTA V natives must
be adapted.

## Native assets

Resources with `resource.json`/`mod.json` (cars, peds, clothing, map placements)
are shared with joining players. `ensure name` shares it; `ensure [category]`
includes native resources whose manifest says `"enabled": true`. Assets are fixed
at server start, because clients download them before joining; restart the
server after changing them. Script resources can be restarted live. Only share
mods you may redistribute.

## Relay hosting

Run a reachable `sa-relay` on a trusted LAN/VPN, as described in
[multiplayer setup](multiplayer.md), then add `set sv_relay "192.168.1.10:7778"`.
Players enable **Use relay / join code** and refresh the server browser. The
server prints its join code; a restart gets a new code. Relay failure currently
stops hosting. Connections use the unencrypted prototype protocol.

## In the game

**T** opens chat, **F8** the console (`connect ip:port`, `disconnect`, `quit`,
and `/commands` sent to the server). Chat messages, suggestions and server
teleports come from the resources above. A player-hosted session (F5 → Host)
has no Lua and relays plain chat itself.

## Verification

`cargo test -p sa-server` runs the shipped resources against a real client
socket: spawn, chat, cancelled messages, player commands, ACL, exports, threads,
live restart and kick, plus the example framework (includes, `provide`, function
references, state bags and deferrals), client script download and the script
UI, and CfxLua translation. `tools/test-multiplayer.ps1 -Dedicated [-Relay]` starts a
server from a generated `server-data` plus two Vulkan game instances.
