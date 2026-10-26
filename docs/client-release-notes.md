SARE free-roam test client, built and tested from the linked commit.

Version 0.36.1:
- Lighter, quicker car handling: the wheels turn faster, cars keep more steering at speed, grip is higher and bodies turn in sooner, so driving is closer to the original game's easy feel. Nose-heavy cars such as the Taxi now run wide at full lock instead of spinning.

Version 0.36.0:
- New clean-room car handling read from your own handling.cfg: each car's top speed, pull, engine lag, grip, drive layout, brake balance and weight now shape how it drives. Rear-drive muscle cars step out under power, trucks and the Rhino are slow to answer, supercars stay planted, handbrake turns swing harder, and steering into a slide catches it. Steering also turns a car a little in mid-air.

The new handling passed automated driving tests. It still needs a feel check on a real install.

Version 0.35.1:
- Cars drive with SARE's own handling model again. The vehicle code added on 25 October was withdrawn because it was built from decompiled game code, which SARE does not use. Per-car handling is still read from your own handling.cfg. A new clean-room handling model follows in the next update.

Version 0.35.0:
- Optional temporal anti-aliasing (Graphics menu, off by default): smoother edges and less shimmer, and the groundwork for FSR 3 and DLSS.
- Trailer camera: `--trailer` flies ten scripted shots across the map; `--trailer-record <folder>` saves them as 60 FPS MP4 clips through ffmpeg. See `docs/trailer.md`.

Temporal anti-aliasing passed a GPU shader test on a software renderer; in-game image quality and the trailer shots still need checking on a real install.

Version 0.34.1:
- Joining or hosting a session while the first map area is still loading no longer drops that session's collision and water.
- Faster area streaming: a texture shared by solid and transparent surfaces is decoded once instead of twice.
- World streaming from 19 October: the first area loads in the background, identical textures upload once, and curbs, short stairs and small drops are easier to walk over.

See `docs/review-2026-10-19.md`. GPU and original-map gameplay checks for these fixes are still pending.

Version 0.34.0:
- A consistent native interface across the launcher, main menu, pause menus and gameplay HUD.
- Original sunset artwork, clearer navigation and compact clock, ammo and session status.
- Responsive pause tabs and title actions, with zone notices kept clear of the speedometer.
- Built-in interface text is in English. Custom local launcher artwork remains supported.

See `docs/native-ui-refresh.md` for actual native previews and validation limits.

Version 0.33.2:
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
