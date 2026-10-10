# SARE dedicated server

This package is set up like a FiveM server install: the server binaries live in
`server/` (like FXServer artifacts) and your configuration and resources live in
`server-data/` (like cfx-server-data). No San Andreas installation, GPU or
Cargo is needed.

```
server/            sa-server, sa-relay, license notices
server-data/       server.cfg and resources/ — this folder is yours
start-server.cmd   Windows: runs sa-server +exec server.cfg inside server-data
start-server.sh    Linux:   the same
start-relay.*      optional relay for a server browser and join codes
```

1. Extract the ZIP into its own folder, for example `C:\SARE-Server`.
2. Edit `server-data/server.cfg` (name, slots, rcon password, resources).
3. Run `start-server.cmd` (Linux: `./start-server.sh`).
4. For internet hosting, allow TCP and UDP 7777 in the firewall/router,
   or publish through a relay with `set sv_relay "ip:port"`.

Put your own resources in `server-data/resources/[local]/` and add
`ensure name` to `server.cfg`. Extra launch arguments use FiveM syntax:
`start-server.cmd +set sv_maxclients 8`.

**Updating:** download the newest server ZIP and replace only the `server/`
folder. Keep `server-data/`, just like updating FXServer artifacts.

Full reference: <https://github.com/LordM8YT/GTA-San-Andreas-Rust-Edition/blob/main/docs/server-hosting.md>
Help and people to play with: <https://discord.gg/F8KwXJqw6D>

These are experimental community builds; the prototype uses unencrypted TCP.
