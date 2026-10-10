SARE free-roam test client, built and tested from the linked commit.

Players: download `SARE-windows-test.zip` or `SARE-linux-test.zip` and extract
into a separate writable folder.
Run `sa-launcher.exe` (Linux: `./sa-launcher`). Your own original PC San Andreas
installation is required; no original game assets are included.

This launcher checks for tested client updates automatically. Updates wait until
the game closes, verify package/file hashes and keep user settings, saves, cache,
local mods and the original game installation. Automatic updates can be disabled
under Settings → Client updates. Development repo builds continue to use Git/Cargo.

These are experimental community builds, not signed commercial releases.

Server hosts: download `SARE-server-windows.zip` or `SARE-server-linux.zip`, a
FiveM-style dedicated server with binaries in `server/` and your configuration in
`server-data/`. Update by replacing `server/` only. See docs/server-package.md.
