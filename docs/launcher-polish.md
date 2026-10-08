# Native launcher polish - 8 October 2026

## Changes

- Servers is a searchable directory with Refresh and All/Favorites filters.
  Rows use the relay's actual names, player counts and protocol compatibility.
  No gameplay ping is displayed because this browser does not measure it.
- Direct connect contains player name, direct IP, relay address and join code.
  Saved connections remain usable even when directory browsing fails. Invalid
  input disables Join/Save favorite; existing argument validation is retained.
- Empty relay configuration stays empty. Localhost is explicitly described as
  local to this PC. Missing setup, loading, empty results and errors have separate
  states. Retry clears old errors; success clears them permanently from the UI.
- Directory networking runs on a separate worker and cannot block offline launch
  readiness. Editing a relay address does not send a request for every keystroke.
  Directory failures stay on Servers, with a short message, Retry and Details.
  Technical errors are appended to the local logs/launcher.log.
- Home retains optional local PNG artwork, its dark gradient, the original green
  fallback, checkpoint/car information and equal-height dashboard cards. Favorites
  opens the matching server filter. No empty-history reconnect action is invented.
- Resources opens directly to an asynchronous local inventory with names, manifest
  status, declared types, measured sizes and per-resource inspection errors.
  Tabs separate local mods, server resources and cached downloads. Unknown sizes
  remain unavailable. Cache usage/preview/cleanup retain the existing lease,
  generation, link and path protections. No local mods or original files are deleted.
- Settings uses Installation, Display, Downloads and Appearance sections. Inputs
  are 40-44 logical pixels tall. The current graphics preset has selected state.
  Validation appears next to the game folder, Save stays near the top, saved status
  is explicit and clears on editing, and game settings are marked for next launch.
- Help uses Controls, Diagnostics and About. Build/protocol details are under About;
  technical errors can be expanded. Report preview, reviewed export and logs-folder
  opening are retained. Reports are never uploaded automatically.
- Shared native egui styling and keyboard/controller event mappings remain in use.
  Supported winit/egui SetTheme(Dark) is requested for the Windows title bar; there
  is no custom window decoration.

## Verification

- Windows workspace: 156 Rust tests passed. Launcher regression tests cover missing
  setup, invalid/unavailable relay, a real empty loopback relay, publication of a
  real host, browsing it and connecting a real guest. Stale responses are rejected;
  browser failures cannot replace the global status message. A real nonresponding
  TCP endpoint also verifies rendering and offline readiness while browsing waits. Controller event/focus
  tests, input geometry, local resource inspection and updater tests also passed.
- cargo fmt --all -- --check, workspace Clippy with -D warnings and release workspace
  build passed. The final launcher-only changes were retested separately.
- tools/test-launcher.ps1 produced 30 native viewport captures with isolated profiles:
  all five pages at 1280x720, 1920x1080, 2560x1080 and 200% scaling, plus compact,
  invalid-installation, direct-connect, missing-relay, empty/live directory, favorites
  and resource-tab cases. Dimensions are asserted; captures were visually inspected.
  The listed test server is a real temporary headless process bound through a real
  loopback relay, not a mock listing. Both owned processes are stopped in finally.
  Long content uses the page scrollbar; sidebar/footer stay separate from it.
- The previous updater commit da848ae passed Windows and Linux CI and published its
  platform ZIPs. An isolated packaged Windows client actually fetched that GitHub
  release, verified it, installed/restarted and retained a local mod sentinel.
  This is separate from screenshot tests, which deliberately disable self-update.

## Limits

The runtime does not export an active resource inventory to this launcher. The
Server resources tab says so and directs the player to /mods in the running game;
it does not pretend a cached inventory is active. Adding an export is outside this
UI-only change. Hosting, protocol, engine, gameplay and original assets are unchanged.

New multiplayer checks are same-PC loopback tests. Physical controller hardware,
Windows file-picker interaction, Linux desktop rendering, cross-PC/NAT operation
and title-bar pixels are not verified by these viewport captures. Screenshot
inspection demonstrates layout, not successful gameplay or network operation.
