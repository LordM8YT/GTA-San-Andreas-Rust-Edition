# SARE Studio audit (2026-11-26)

SARE Studio is Build Mode (`native/crates/editor` = `sa-editor`,
`native/crates/runtime/src/build_mode`) plus the Create page of SARE Client
(`native/crates/launcher`). This audit was made from the code at 0.61.0
(commit a06b201), not only from [build-mode.md](build-mode.md).

## Works completely (verified in code and tests)

| Area | Where |
| --- | --- |
| Ribbon (Home, Model, Test, View), Toolbox, Explorer, Properties, Output, closable docks | `build_mode/panels.rs` |
| Select/Move/Rotate/Scale, world/local axes, grid + angle snapping, surface placement, arrow-key nudge, Arrange | `panels.rs`, `viewport.rs`, `arrange.rs`, `sa-editor::viewport` |
| Undo/redo for every edit (commands), live edits merged into one step | `sa-editor::history` (`Command`, `VecDeque` history) |
| Autosave + recovery after a crash | `sa-editor::project` (`.autosave.json`) |
| New / Open / Save / Save As inside the editor; templates (Blank, Example Town, Gameplay Sample, SARE RP Town) in Client › Create | `panels.rs` File menu, `launcher/experience.rs::TEMPLATES` |
| Groups as models with pivots, reparenting by drag, multi-select batch edits, copy/cut/paste/duplicate | `sa-editor::ops`, `panels.rs` |
| Parts (block, ball, cylinder, wedges) with size, colour, material, transparency, Anchored physics | `sa-scene::part` |
| GLB import (picker, drag-drop, dedup by content), thumbnails, missing/broken asset marks, triangle/material counts | `sa-editor::project::import_model`, `catalog.rs`, `thumbnail.rs` |
| My Models per world with linked copies | `build_mode/saved_models.rs` |
| Terrain: sculpt, smooth, flatten, paint, holes | `sa-scene` terrain, `viewport.rs::sculpt` |
| Roads along points, closed loops | `sa-scene` road |
| Lighting: hour, weather, fog, sky colour, brightness, looks; lamps on objects | Lighting page in `panels.rs` |
| Gameplay behaviours: zones, interactions, checkpoints, spawn points, actions; live checks in Output | `sa-scene::world_logic` (`check`) |
| Lua world scripts with editor, highlighting, syntax check, examples, API help; command bar | `build_mode/script_ui.rs`, `sa-editor::command_bar` |
| Playtest in place (F5/F6), local server + 1-4 player windows, publish to a server folder | `mod.rs`, `local_test.rs`, `sa-editor::publish` |
| Low-PC pacing (60/2/1 FPS) and Light editor graphics | `build_mode/pacing.rs` |

## Partial, or hard to use

- **World info:** only name and description are editable. Author, version and
  licence live in `content.json` but have no field in Studio, and every new
  world is "All rights reserved" with no way to change it except by hand.
- **Sharing:** Discover installs worlds from `community/worlds.json`, but
  Studio cannot make the ZIP, its SHA-256 or the entry. Creators must zip by
  hand and run `sha256sum`.
- **Cover image:** the client shows `cover.png`, but Studio cannot make one.
- **Naming:** the client says "SARE Studio", the editor says "Build Mode".
- **Player character:** a world without its own `characters.glb` gets the
  blocky stand-in. A CC0 rigged character set exists (SARE RP Town) but is
  not used as the default.
- **World checks:** gameplay checks exist; there is no world-level check
  before sharing (licence, spawn, missing assets, size limits, script errors).
- **Open world:** lists installed worlds only; no recent list, no folder
  picker for worlds elsewhere.

## Documented but not implemented

From build-mode.md and world-scripts.md "Not done yet": shared My Models
between worlds, dragging a pivot in the viewport, road junctions and
terrain-following roads, terrain bigger than one object and textured terrain
materials, spot lights and lamp shadows, visual scripting, moving zones with
their objects, tilting/scaling objects and sounds from scripts.

## Blocked by the engine, not the editor

- Spot lights and lamp shadows (renderer has point lamps without shadows).
- Skinned characters (characters are rigid parts on bones, by design).
- Large streamed worlds (no streaming cells for packs yet).
- Standalone audio, NPCs and weapons for packs.

## Priority list

1. **Share your world** (A): author/version/licence in Workspace, cover from
   the viewport, *Export world* to a Discover-ready ZIP with SHA-256 and the
   `worlds.json` entry, and a world check that runs first.
2. **Default SARE character** (D): the CC0 character set built into SARE for
   every world without its own; worlds can still ship theirs.
3. **SARE Studio naming** (A): editor title, menus, launcher buttons, docs.
4. **Server resources in Studio** (B): new resource from a template (RP job),
   edit its Lua in the script editor, attach it to the world for Test › Server.
5. **Open from anywhere + recent worlds** (A).
6. **Visual scripting, first slice** (B): blocks that compile to the existing
   behaviours and actions, not a second engine.
7. Shared local My Models library (C), road junctions (B), moving zones (B).

Each step is its own pull request with tests.
