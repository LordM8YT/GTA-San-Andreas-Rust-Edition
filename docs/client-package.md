# SARE test client

Extract the whole ZIP into its own folder, outside your original game installation.
Run **sa-launcher.exe** on Windows or **./sa-launcher** on Linux. Cargo is only
needed by developers building from source. Choose your original **PC San Andreas**
installation in Settings, validate it, then Play or Continue free roam. The original
executable is never launched and original files are never modified.

For multiplayer, choose Play together, enter a host IP:port or a relay IP:port
and 12-character join code. A trusted relay's browser lists public rooms on that
relay only. All participants, host and relay must use protocol 6. Required native
mods are verified and cached before gameplay; downloads can be disabled in Settings.
There is no configured public service, Steam relay, account requirement or automatic updater.
Use trusted LAN/VPN peers: this prototype uses unencrypted TCP.

Direct development startup remains available:
`sa-runtime --game-dir "PATH TO ORIGINAL GAME" --play`
Join directly with `--join IP:PORT --play`, or via
`--relay-address IP:PORT --join-code CODE --play`.

Settings, logs, checkpoints and server cache are outside the original installation:
Windows `%LOCALAPPDATA%/SAFreeroam`; Linux `$XDG_CONFIG_HOME/sa-freeroam`
(or `~/.config/sa-freeroam`), with cache under `$XDG_CACHE_HOME/sa-freeroam`
(or `~/.cache/sa-freeroam`). `SARE_CONFIG_DIR` isolates a test profile and cache.
The package's optional `mods/` folder is for local native resources.

Use Resources to preview cache cleanup; active sessions/downloads prevent deletion.
Use Help to preview and save a redacted local diagnostic report. Reports are never
sent automatically. F3 in gameplay shows CPU/frame timings; these are not GPU timings.
Tab/Shift-Tab and Enter navigate the launcher; D-pad/A/B provide corresponding controller actions.

Original Rockstar assets, FiveM scripts, private logs and cached downloads are not
included. Linux CI builds are supported; actual Linux GPU/controller behavior still
needs testing. These are review artifacts, not a published production release.
