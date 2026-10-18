SARE free-roam test client, built and tested from the linked commit.

Follow-up fixes after 0.33.1 (pending platform CI):
- Missing texture dictionaries no longer remove otherwise valid world models and collision.
- Transparent surfaces no longer disable depth writes or fallback collision for opaque surfaces sharing the same texture.

See `docs/bug-audit-2026-10-18.md` for the reproduced cases, tests and Ponytail Review.
Original-map and GPU verification remain pending.

Version 0.33.1:
- Fixed on-foot movement getting blocked by walkable slopes and ramp-to-flat seams.
- Prevented overlapping detail and coarser terrain models in nested IPL LOD chains.

Regression tests cover generated slopes, real head-clearance blockers and overlapping
terrain levels. The reported original-map locations still need in-game verification;
this is not a claim that every collision or texture-flicker issue is resolved.

Players: download the Windows, Linux, Apple Silicon macOS or Intel macOS ZIP and extract
into a separate writable folder.
Run `sa-launcher.exe` (Linux/macOS: `./sa-launcher`). Your own original PC San Andreas
installation is required; no original game assets are included.

This launcher checks for tested client updates automatically. Updates wait until
the game closes, verify package/file hashes and keep user settings, saves, cache,
local mods and the original game installation. Automatic updates can be disabled
under Settings → Client updates. Development repo builds continue to use Git/Cargo.

These are experimental community builds, not signed commercial releases.

Server hosts: download `SARE-server-windows.zip` or `SARE-server-linux.zip`, a
FiveM-style dedicated server with binaries in `server/` and your configuration in
`server-data/`. Started with start-server.cmd/.sh it updates itself from new
releases while no players are online. See docs/server-package.md.

SARE Box servers: player licenses are now signed per server (`server.key`).
Copy `server-data/resources/[sarebox]/` from the new server ZIP over yours,
then re-add admins with their new license (`/id`). Old characters move to the
new licenses with `sarebox_migrate`; see docs/sarebox.md. Players need this
client version to keep a saved character.

Map and radar work like GTA V: the radar is a rectangle in the bottom-left
corner that zooms out the faster you drive and keeps you low, so more of the
road ahead shows. The pause map (M) can be dragged and zoomed, has a legend,
and right-click places a waypoint with a GPS route along the roads. Server
scripts can place blips with GPS routes; the SARE Box taxi job uses them.

macOS packages are unsigned executables without an app bundle or notarization.
