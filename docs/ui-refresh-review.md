What this change does: SARE 0.34.0 gives the launcher, title screen, pause menus
and gameplay HUD a shared native interface. It also fixes keyboard activation,
small-window layout and HUD overlap problems found during the visual review.

Reviewed using [Ponytail Review](https://github.com/DietrichGebert/ponytail/blob/main/skills/ponytail-review/SKILL.md)
before pushing, against the 0.33.2 version commit `008dd5ee`. This is a
skill-guided review, not a separate external review service. Expected load: one
native client drawing its interface every frame, including an existing session
of up to 20 players.

## Must fix — resolved

1. **Enter could trigger two menu actions** (`menu/front.rs`, `menu/gta.rs`)
   - **What this is:** Menus support both the selected row and egui's focused button.
   - **Problem:** Enter on a focused Settings button could also start or resume free roam. The regression returned `Some(Play)` when no play action was expected.
   - **Fix:** Let the focused button own keyboard activation. Use the selected-row fallback only when no widget owns keyboard input. Keep Q/E tab switching available unless a text field is focused.
   - **If we skip it:** Keyboard navigation can unexpectedly enter gameplay instead of opening the chosen menu.

2. **Small-window settings hid values behind arrow buttons** (`menu.rs`, `draw_settings`)
   - **What this is:** The settings screen originally kept three columns in a narrow window.
   - **Problem:** At 640×480, values such as the field of view and window mode were covered by their adjustment buttons. Sidebar help also ran into the footer.
   - **Fix:** Give narrow windows a wider settings list and collapsible details inside its scroll area. Keep wide-window details scrollable and omit redundant sidebar hints when the height is too short.
   - **If we skip it:** Players cannot reliably read the setting they are changing.

3. **Title actions and pause labels did not fit small windows** (`menu/front.rs`, `menu/gta.rs`)
   - **What this is:** The old title screen used a long vertical list; pause tabs shared one row.
   - **Problem:** The title list could put Quit below the screen, and long pause-tab labels were squeezed into narrow cells.
   - **Fix:** Group the three play actions separately, keep customization in a second column, wrap narrow pause tabs and size the pause list to the available height.
   - **If we skip it:** Some mouse actions are unreachable or hard to identify on small screens.

4. **The refreshed speedometer covered the area announcement** (`menu.rs`, gameplay HUD)
   - **What this is:** The larger speed readout and the area name both use the bottom-right corner.
   - **Problem:** An early native capture showed the area name crossing the speedometer panel. The old separate top-right clock, weapon and online areas also competed for space.
   - **Fix:** Reserve more vertical clearance for the area name while the speedometer is visible. Group clock, ammo and online information into one layout.
   - **If we skip it:** Important information overlaps during ordinary driving and multiplayer play.

## Connected-code review

Read the changed files, README, shared theme and artwork code, menu actions,
settings controls and persistence paths, launcher readiness/start guards,
controller handling, gameplay HUD callers, streaming location definitions,
packaging and existing UI tests. The shared location definitions are moved without
changing their coordinates, IDs or types. Settings and session launch contracts
are preserved. The new Play layout still uses an actually disabled widget when
the installation is not ready.

The theme adds no per-frame network or file operations, blur passes or animation
timers. The original vector background is drawn only on title/artwork surfaces;
the gameplay HUD remains ordinary egui geometry. The direct egui dependency is
the version already used through eframe; no third-party package versions change.
The preview example uses the real menu and an isolated profile, writes a new PNG
and does not launch a game or connect to a server. Original game files are untouched.

No remaining must-fix findings were found in this change. The review is scoped to
the interface refresh, not every gameplay, server or engine subsystem.

## Verification

- All 315 Rust workspace tests passed; the 19 menu tests were also rerun after layout adjustments.
- All nine client/server packaging tests passed.
- Workspace Clippy with all targets and warnings denied, formatting and `git diff --check` passed.
- The native launcher and the shared-menu preview example build successfully on Linux.
- Regression tests exercise mouse Quit, action bounds and pause-label widths at 640×480, 1280×720 and 2560×1080; setting-value clearance; focused Enter and Q/E; and HUD overlap.
- Native egui captures were inspected at standard, small-window and ultrawide sizes. The final launcher, title, pause and HUD captures are in `docs/images/native-ui/`.
- All ten workspace packages resolve to 0.34.0. Third-party lockfile entries are unchanged.

Verdict: Ship.

Not checked: Windows/macOS builds, release-mode gameplay over original assets,
physical controller operation, large custom UI scales, every long custom label,
server-script overlays or real-game frame rate. Native previews use a software
Vulkan renderer, sample HUD data and no original map tiles. Platform CI and
in-game validation are still required before release.
