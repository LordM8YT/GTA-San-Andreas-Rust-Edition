# SARE as a creator platform

Goal: make SARE a free, open multiplayer platform where server owners and
creators can build whatever they want on top of San Andreas (free roam,
roleplay, racing, minigames, custom maps), with the workflow FiveM players
already know and fewer of its limits. This document describes the full
direction, what already exists, and what comes next. It is a plan, not a
promise of dates. The [clean-room rules](../AGENTS.md) apply to every step: no
Rockstar files, no code from the game executable or decompiled sources, and no
paid features.

## Where SARE stands today

Much of the foundation is already in place:

| Layer | Status | Where |
| --- | --- | --- |
| Dedicated server (`sa-server`) with `server.cfg`, `resources/[category]/`, `fxmanifest.lua`, `ensure/start/stop/restart/refresh` and dependency/`provide` ordering | Done | [server hosting](server-hosting.md) |
| Server Lua 5.4: events, commands, exports, threads, function references, state bags, KVP, ACL, rcon | Done | [server hosting](server-hosting.md) |
| Sandboxed client Lua with game-drawn UI (menus, dialogs, notifications) | Partial | [client scripts](client-scripts.md) |
| Native asset resources (DFF/TXD/COL/IFP cars, peds, clothing, map placements), streamed to players with SHA-256 caching | Done | [native mods](native-mods.md) |
| Player identity: signed per-server `license:` | Done | [SARE Box](sarebox.md#player-identity) |
| Server browser and join codes through `sa-relay` | Prototype | [multiplayer](multiplayer.md) |
| Importers for classic SA mods and FiveM/GTA V assets | Partial | [SA import](gta-sa-conversion.md), [FiveM conversion](gta5-conversion.md) |
| Framework compatibility (ox_lib starts; Qbox not yet) | Partial | [framework compatibility](framework-compatibility.md) |
| **Routing buckets (instances)** | **New in this step** | [server hosting](server-hosting.md#resources-and-lua) |

## This step: routing buckets

Instances are a basic building block for almost every game mode: private
garages and apartments, races and deathmatch lobbies, character selection,
tutorials, admin "jail" worlds and separate game modes on one server. Before
this step `GetPlayerRoutingBucket` always returned 0.

Delivered:

- `sa-net` keeps a routing bucket per player on the host. Each pose snapshot is
  built per bucket, so players only see players in their own bucket. The
  server itself still sees everyone.
- Traffic NPCs are only relayed inside the owner's bucket, and not at all in
  buckets where `SetRoutingBucketPopulationEnabled(bucket, false)` is set.
- Passenger seats can only be reserved in the player's own bucket, and an
  active ride ends (`Lost`) when the driver or passenger changes bucket.
- A player's bucket resets to 0 when they leave, so a reused player ID starts
  in the default world.
- Server natives with FiveM names: `SetPlayerRoutingBucket`,
  `GetPlayerRoutingBucket`, `SetEntityRoutingBucket`,
  `GetEntityRoutingBucket` and `SetRoutingBucketPopulationEnabled`.
- `status` in the server console shows each player's bucket.
- Example resource `[examples]/sare_instances` with `/instance`,
  `/instance <number>`, `/instance 0`, a `sare_instances:changed` server event,
  an `instance` player state bag value and the exports `SetInstance`/`GetInstance`.
- Tests: a network test for snapshot, traffic and departure behaviour, a seat
  test, and an end-to-end server test that drives `/instance` from two
  connected clients.

Not in this step: server-owned entities (so there are no entity lockdown
modes), filtering of script events by bucket (FiveM does not do that either),
and client-side bucket awareness beyond what the snapshot already hides. The
change needs no protocol bump: older clients simply receive fewer players.

## The full roadmap

The order follows dependencies: each phase is useful on its own and unlocks
the next one.

### Phase 1 — Server world model

1. **Routing buckets** — done for players (this step).
2. **Server-owned entities.** Vehicles, peds and objects created by the server
   (`CreateVehicle`, `CreateObject`, `DeleteEntity`, `GetGamePool`, network
   IDs), replicated to the players in the entity's bucket and simulated by an
   owning client. This replaces "personal cars only" with scripted cars.
3. **Replicated state bags.** `replicated = true` values on `GlobalState`,
   `Player(src).state` and `Entity(h).state` reach client Lua, with size limits
   and server-only write access by default.
4. **Bucket-aware streaming.** Resources can mark map placements as belonging
   to a bucket, so interiors and custom maps only load for the players in it.

### Phase 2 — Scripting platform

1. **Server APIs creators expect:** `PerformHttpRequest` (with an allow-list
   convar), `AddConvarChangeListener`, and an oxmysql-compatible database
   resource in Rust (`query`, `single`, `scalar`, `insert`, `update`,
   `transaction`, `prepare`) with SQLite built in and MySQL/MariaDB optional.
2. **Live development:** `sv_devMode` watches started script resources and
   restarts a resource when its files change, printing script errors with
   file and line.
3. **Client Lua growth:** more SA equivalents of common natives, an ox_lib
   bridge that maps `lib.notify`/`lib.registerContext` to the script UI, key
   mappings, and per-resource client permissions.
4. **Resource permissions.** ACL objects for dangerous natives
   (`resource.<name>` principals, as FiveM uses), so a downloaded resource can
   be restricted without reading its code.

### Phase 3 — Networking and hosting

1. **Server authority:** validation of teleports, speed and vehicle ownership
   on the server, with an event hook so resources can reject movement.
2. **Transport:** encrypted connections and a UDP gameplay channel with
   interest management by distance and bucket, so servers can grow beyond 20
   players.
3. **Public discovery:** a documented, self-hostable directory service, so the
   server browser works without a shared trusted LAN relay; NAT traversal with
   relay fallback.
4. **Deferrals before admission:** `playerConnecting` runs before the player
   enters the world, with queue and whitelist support.

### Phase 4 — Creator tooling and SDK

1. **SDK documentation site:** a generated native reference (server and
   client), typed Lua stubs for editors (`sare.d.lua`) and versioned examples.
2. **`sare` CLI:** `sare new resource`, `sare check` (manifest, globs, natives
   that do not exist), `sare pack` and `sare serve` for a local test server.
   `tools/new-resource.py` and `tools/framework-check.py` are the starting point.
3. **Resource hub, free and self-hostable:** publish and install resources by
   name and version with SHA-256 verification, never paid.
4. **Map editor:** place SA and native-resource models in game and export
   them as a map resource.

### Phase 5 — Compatibility targets

1. Qbox core with ox_lib, the oxmysql replacement and ox_inventory, pinned and
   tested in CI.
2. Wider FiveM asset conversion (more YDR/YFT/YTD variants, MLO interiors).
3. A stable, versioned SARE API so resources written today keep working.

## How to help

Pick an item above, open an issue describing the design first for anything in
phases 1 and 3 (they touch the network protocol), and follow the [server
guide](server-guide.md) and [creating resources](creating-resources.md) for
resource work. Every change needs tests in `cargo test --workspace` and must
follow the clean-room rules.
