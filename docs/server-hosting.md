# Dedicated freeroam server

`sa-server` runs membership, pose relay and native resource distribution without
a window, graphics device, Steam installation or original San Andreas files.
It accepts **20 actual clients**, with no placeholder host avatar. This is a
headless version of the current multiplayer prototype: movement and collisions
are still simulated by clients, and parked/shared cars, passengers and NPCs are
not an authoritative shared world yet.

## Start

On Windows, run `start-server.cmd`. It builds `sa-server` when Cargo is available,
then creates `server.json` in the project directory on first run. Defaults bind
only `127.0.0.1:7777`, with no relay. Type `quit`, edit the configuration and run
again. `status`, `players`, `help` and `quit` are available in the console.

For deployment, build with `cargo build --release -p sa-server` from `native`.
Copy `sa-server.exe`, your `server.json` and permitted native mod folders into
one server directory; the runtime, original archives and GPU libraries are not
needed. Source is platform-neutral Rust; a native Linux build/deployment has
not been tested yet.

## Browser and join code via the project's relay

Run a reachable `sa-relay` on a trusted LAN/VPN, as described in
[multiplayer setup](multiplayer.md). Configure:

```json
{
  "name": "Friends Freeroam",
  "listen": "127.0.0.1:7777",
  "relay": "192.168.1.10:7778",
  "public": true,
  "mods_dir": "mods"
}
```

`relay` selects outbound relay hosting; `listen` is then unused, and the game
server binds an internal loopback port automatically. Players enter the same
relay address in Multiplayer, enable **Use relay / join code**, and refresh the
server browser. The running server prints its join code. With `public: false`,
it is omitted from the browser and the code is required. A restart gets a new
code. The dedicated server stays running when players leave; it does not depend
on any player's game staying open. Relay failure currently stops hosting.

The relay itself must be reachable; no public service is deployed. Connections
use the existing unencrypted TCP prototype, suitable for trusted LAN/VPN tests.
Steam discovery/Valve relay, passwords, administrator identities, bans,
automatic reconnect and host failover are not implemented.

## Direct LAN hosting

Set `relay: null` and `listen: "0.0.0.0:7777"`. Allow TCP 7777 on the server PC.
Guests use direct mode with the server's LAN IP and port. Direct internet hosting
needs an inbound route/port forwarding; this mode does not use the browser.

Relative `mods_dir` paths resolve beside the selected configuration file, not
the executable. `--config <file>` selects another configuration. Server names
accept 1–24 printable characters; unknown configuration keys fail explicitly.

## Native resources

Place enabled native resource folders under `mods_dir`. The server takes an
immutable snapshot at startup, includes only files referenced by each resource
manifest, and shares that inventory with clients. Restart to apply changes.
Original game archives and adjacent unrelated files are not exported. Only share
mods you may redistribute. Guests download missing files, verify hashes and reuse
their cache next time; supported formats and limits are in
[resource preparation](multiplayer.md#native-server-resource-preparation).

The server distributes map assets and catalogs. Selected car, ped and clothing
choices synchronize between clients. The runtime, server and relay must all
use network protocol 2; incompatible versions are rejected.

## Verification

Real socket tests cover zero-player browser listings, 20 actual direct/relay
clients, overflow rejection, departed-slot reuse and removal after shutdown.
`tools/test-multiplayer.ps1 -Dedicated -Relay` starts its own dedicated server,
relay and two Vulkan game instances, then stops only those test processes.
Add `-HostModsDir <server mods>` / `-ClientModsDir <guest local mods>` and
`-CacheDirectory <test cache>` to exercise isolated resource preparation.
Same-PC tests do not establish connectivity or latency on separate networks,
nor rendering performance with 20 real players.

The hosting model follows the two options described by
[ReSkate](https://github.com/Dingo-Shenanigans/ReSkate/blob/main/Server/README.txt):
in-game friend hosting and independent servers in a browser. Our implementation
uses its own protocol/relay; it does not incorporate ReSkate's Steam integration.
