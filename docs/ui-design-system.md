# SARE UI design and maintenance guide

This guide is the working contract for the native launcher and in-game interface.
The current refresh is a foundation, not final visual sign-off. Do not label it
AAA-ready until the actual launcher and in-game captures have been reviewed and
the remaining visual issues have been addressed.

## Design goals

- Build a distinct SARE identity inspired by the clarity and restraint of premium
  open-world game interfaces, without copying Rockstar artwork or presenting a
  generic FiveM dashboard.
- Use hierarchy, typography, alignment, contrast and spacing before adding
  decoration. The world should remain the focus during gameplay.
- Keep navigation and interaction consistent across the title screen and pause
  menu. Preserve keyboard, mouse and controller behavior.
- Never fabricate gameplay data (health, fuel, gear, player counts or connection
  state). Show only values supplied by the runtime.
- Keep all player-facing strings in English.

## Source map

| Area | Source of truth |
| --- | --- |
| Shared colors, egui styling and original vector horizon | `native/ui/theme.rs` |
| Main/title screen and its primary navigation | `native/crates/runtime/src/menu/front.rs` |
| Pause-menu tabs, shared pause labels and detail panel | `native/crates/runtime/src/menu/gta.rs` |
| Gameplay status group | `native/crates/runtime/src/menu/hud.rs` |
| Gameplay menus, settings behavior, radar and common menu state | `native/crates/runtime/src/menu.rs` |
| Visual review and native preview instructions | `docs/native-ui-refresh.md` |

When a menu label appears on multiple screens, reuse a shared definition rather
than maintaining parallel lists. Keep colors and reusable styling in the shared
theme; keep screen-specific layout and behavior close to the screen that owns it.

## Contribution rules for Jasper and other developers

- Prefer small, named helpers and focused modules over large blocks of inline UI.
- Make the smallest change that solves the problem; avoid introducing a framework
  or abstraction for a one-off visual element.
- Preserve action routing, persistence, input bindings and accessibility/focus
  behavior unless the task explicitly changes them.
- Explain non-obvious layout and interaction decisions in comments or the PR.
- Do not reformat or rewrite unrelated code just to make a UI change.
- Keep changes reviewable and commit related work in coherent steps.

## Required review gate

Before calling a UI task done:

1. Run `cargo fmt --all -- --check` from `native/`.
2. Run the relevant runtime/launcher tests, then workspace tests where feasible.
3. Run workspace Clippy and build checks when the environment supports them.
4. Capture the actual interface using the documented preview workflow at small,
   standard and ultrawide sizes. Review clipping, spacing, text contrast and
   alignment; do not treat a mockup as proof of implementation.
5. Inspect the actual launcher and in-game UI where possible, including real
   gameplay data and a session, and clearly list anything not tested.
6. Update the preview images and review notes only after the corresponding
   captures/checks have actually been performed.

A successful compile is not visual sign-off. A preview is not proof of real-game
performance. State which checks passed, failed or remain pending.
