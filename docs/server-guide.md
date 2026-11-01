# Running a SARE server

This guide explains how a SARE server is put together and how to get one running. It covers the current dedicated-server implementation, not planned features.

For the full configuration and command reference, see [Dedicated server](server-hosting.md). For player-hosted sessions, see [Multiplayer](multiplayer.md).

## 1. Choose how to host

SARE currently has two ways to host multiplayer:

- **Player-hosted:** One player hosts from the game. This is convenient for a quick session, but the host must keep the game running.
- **Dedicated server:** A separate, headless `sa-server` process runs without a player being in-game. This is the better choice for a persistent community server.

The dedicated server does not need a GPU, a Steam installation or the original San Andreas game files. The players still need their own supported PC game installation and SARE client.

The current server limit is **20 players total**. This is a configured prototype limit, not a promise that every server setup can sustain 20 fully active players under all conditions.

## 2. Install and start

1. Download the Windows or Linux server package from the [latest SARE release](https://github.com/LordM8YT/GTA-San-Andreas-Rust-Edition/releases/latest).
2. Extract the archive into its own folder.
3. Run `start-server.cmd` on Windows or `./start-server.sh` on Linux.
4. On first launch, choose the **Freeroam** or **SARE Box** template.
5. Stop the server and edit `server-data/server.cfg` before inviting players.

The package separates the server executable from your persistent server data. Keep the whole `server-data/` directory when updating the server; it contains your configuration, resources and saved data.

## 3. Understand the server folder

A typical server data folder looks like this:

```text
server-data/
  server.cfg
  resources/
    [system]/
    [managers]/
    [gamemodes]/
    [examples]/
    [sarebox]/
    [local]/
  kvp/
```

- `server.cfg` configures the endpoint, server name, player limit, permissions and which resources start.
- `resources/` contains the server's scripts and shared native asset resources.
- Bracketed folders such as `[local]` are categories. They help organize resources; they are not themselves resources.
- `kvp/` stores resource key/value data.

Do not replace your entire `server-data/` folder when installing an update. Back it up first.

## 4. Configure the server

A minimal configuration looks like:

```cfg
endpoint_add_tcp "0.0.0.0:7777"
endpoint_add_udp "0.0.0.0:7777"

sv_hostname "My SARE Server"
sv_maxclients 20

ensure mapmanager
ensure chat
ensure spawnmanager
ensure basic-gamemode
ensure sare-map-grove
```

SARE gameplay uses **TCP** on the configured port. The remote console (rcon) uses **UDP** on that port. For direct internet hosting, allow/forward TCP; only expose UDP if you need rcon. Keep rcon disabled unless you have a strong password and actually need it.

The server configuration uses a familiar FiveM-style `server.cfg` format. This does **not** mean that every FiveM feature or resource works in SARE.

## 5. Start and manage resources

A resource is a folder inside `resources/`. Script resources use `fxmanifest.lua` to declare their Lua files and dependencies. For example:

```lua
fx_version 'cerulean'
game 'common'

server_script 'server.lua'
```

Start resources in `server.cfg` with `ensure resource-name`. You can also use server console commands:

- `resources` — list resources
- `ensure name` — start a resource
- `stop name` — stop a resource
- `restart name` — restart a resource
- `status` — show connected players
- `say message` — send a server message
- `quit` — shut down cleanly

SARE supports a documented subset of FiveM-style server Lua APIs. Check [server hosting](server-hosting.md) and [framework compatibility](framework-compatibility.md) before porting a framework or resource. In particular, do not assume GTA V natives, OneSync, NUI, database resources or arbitrary FiveM scripts are available.

## 6. Add native asset resources

Native assets such as supported cars, player models, clothing and map placements can be placed under `resources/[local]/`. These use `resource.json` or `mod.json`, not `fxmanifest.lua`.

A native asset resource can be started/shared with `ensure resource-name`. Native asset inventories are prepared for clients before they join, so **restart the server after changing shared native assets**. Script resources can be restarted live.

Only distribute assets you created or have permission to redistribute. Never include Rockstar game files or converted copies of Rockstar assets in a server package.

See [Creating native resources](creating-resources.md) for the asset workflow.

## 7. Let players connect

- **Direct connection:** Share your public IP/hostname and TCP port. Direct internet hosting normally requires router port forwarding.
- **LAN:** Players on the same local network can connect using the server's local address.
- **Relay:** A separately hosted `sa-relay` can provide browser discovery and join codes without requiring the game host to accept inbound connections. The current relay is an experimental prototype and uses unencrypted TCP; use it only on a trusted network.

The relay is a separate process. It is not the dedicated game server and does not host the game simulation.

## 8. Security and current limitations

- Only run resources you trust. Lua resources execute server-side or in the client's sandbox according to their manifest.
- Use ACL permissions for admin commands. Do not share your rcon password.
- Back up `server-data/`, especially `server.key` and resource data. The server key is used for stable signed player identity on that server.
- The current multiplayer simulation is still limited: movement is client-simulated, and features such as synchronized NPCs, weapons, shared vehicle collision and a full authoritative world simulation are not implemented.
- The prototype network transport is not encrypted and does not provide complete anti-cheat protection. Do not treat an experimental public server as production-secure.

## Further reading

- [Detailed server configuration and Lua API](server-hosting.md)
- [Multiplayer modes and synchronization limits](multiplayer.md)
- [Creating native resources](creating-resources.md)
- [Client Lua scripts](client-scripts.md)
- [Framework compatibility](framework-compatibility.md)
- [SARE Box framework](sarebox.md)
