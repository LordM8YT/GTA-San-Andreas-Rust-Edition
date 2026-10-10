# SARE dedicated server

This package is set up like a FiveM server install: the server binaries live in
`server/` (like FXServer artifacts) and your configuration and resources live in
`server-data/` (like cfx-server-data). No San Andreas installation, GPU or
Cargo is needed.

```
server/            sa-server, sa-relay, license notices
server-data/       resources/, and server.cfg after the first start — this folder is yours
start-server.cmd   Windows: runs sa-server +exec server.cfg inside server-data
start-server.sh    Linux:   the same
start-relay.*      optional relay for a server browser and join codes
```

1. Extract the ZIP into its own folder, for example `C:\SARE-Server`.
2. Run `start-server.cmd` (Linux: `./start-server.sh`). The first start asks
   which server you want and creates `server-data/server.cfg`:
   - **Freeroam**: free roam with chat and spawn points.
   - **SARE Box**: a roleplay framework with saved characters, money, jobs,
     paychecks and an admin menu.
   Without a console (hosting panels, systemd) pass
   `+set sv_template sarebox` or `+set sv_template freeroam`; with neither,
   Freeroam is created.
3. Edit `server-data/server.cfg` (name, slots, rcon password, admins) and
   restart. To pick another template later, rename `server.cfg` and start again.
4. For internet hosting, allow TCP and UDP 7777 in the firewall/router,
   or publish through a relay with `set sv_relay "ip:port"`.

Put your own resources in `server-data/resources/[local]/` and add
`ensure name` to `server.cfg`. Extra launch arguments use FiveM syntax:
`start-server.cmd +set sv_maxclients 8`.

## Automatic updates

Download the server once. When started with `start-server.cmd` / `start-server.sh`,
the server checks this repository's latest GitHub release at startup and every
30 minutes. A newer server ZIP is downloaded and verified (size and SHA-256 from
GitHub, then every file against `server/sare-build.json`) in the background.
As soon as no players are online, it replaces the files in `server/`, stops
cleanly and the start script starts the new version. `server-data/`, the start
scripts and your resources are never changed. A failed check or download leaves
the running server untouched; a failed install restores the previous files.

Disable it with `set sv_autoUpdate false` in `server.cfg`. Running `sa-server`
directly (without the start scripts) only reports that an update is ready.
For systemd or hosting panels, set `SARE_SERVER_SUPERVISED=1` and restart the
process when it exits with code 42.

To update manually, download the newest server ZIP and replace only the
`server/` folder, just like updating FXServer artifacts.

Full reference: <https://github.com/LordM8YT/GTA-San-Andreas-Rust-Edition/blob/main/docs/server-hosting.md>
Help and people to play with: <https://discord.gg/F8KwXJqw6D>

These are experimental community builds; the prototype uses unencrypted TCP.
