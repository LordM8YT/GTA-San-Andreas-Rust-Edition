# SARE test client

Extract the whole ZIP into its own folder, outside your original game installation.
Run **sa-launcher.exe** on Windows or **./sa-launcher** on Linux. Cargo is only
needed by developers building from source. Choose your original **PC San Andreas**
installation in Settings, validate it, then Play or Continue free roam. The original
executable is never launched and original files are never modified.

For multiplayer, choose Servers, enter a host IP:port or a relay IP:port
and 12-character join code. A trusted relay's browser lists public rooms on that
relay only. All participants, host and relay must use protocol 6. Required native
mods are verified and cached before gameplay; downloads can be disabled in Settings.
There is no configured public multiplayer service, Steam relay or account requirement.
The packaged launcher automatically checks GitHub client releases; see [updates](https://github.com/LordM8YT/GTA-San-Andreas-Rust-Edition/blob/main/docs/client-updates.md).
Use trusted LAN/VPN peers: this prototype uses unencrypted TCP.

Direct development startup remains available:
`sa-runtime --game-dir "PATH TO ORIGINAL GAME" --play`
Join directly with `--join IP:PORT --play`, or via
`--relay-address IP:PORT --join-code CODE --play`.

Settings, logs, checkpoints and server cache are outside the original installation:
Windows `%LOCALAPPDATA%/SAFreeroam`; Linux `$XDG_CONFIG_HOME/sa-freeroam`
(or `~/.config/sa-freeroam`), with cache under `$XDG_CACHE_HOME/sa-freeroam`
(or `~/.cache/sa-freeroam`). `SARE_CONFIG_DIR` isolates a test profile and cache.
The installation ZIP ships **without mods**, including the project-owned demo
resources. To use a local native mod, create a `mods/` folder beside the client
and add a resource you are allowed to use. Source-repository examples are for
development/testing and are disabled by default. Private imports and generated
texture packs remain local; they are not included in releases.

Joining a modded server is a separate, optional download into the server cache;
those resources are not part of the SARE installation. Server operators must
have permission to distribute their resources. Hash validation checks integrity,
not ownership or permission. Updates preserve existing user mods and cache;
they do not add demo packs or remove a user's installed mods.

Use Resources to preview cache cleanup; active sessions/downloads prevent deletion.
Use Help to preview and save a redacted local diagnostic report. Reports are never
sent automatically. F3 in gameplay shows CPU/frame timings; these are not GPU timings.
Tab/Shift-Tab and Enter navigate the launcher; D-pad/A/B provide corresponding controller actions.

Original Rockstar assets, FiveM scripts, private logs and cached downloads are not
included. Linux CI builds are supported; actual Linux GPU/controller behavior still
needs testing. These are experimental community clients, not signed production releases.
