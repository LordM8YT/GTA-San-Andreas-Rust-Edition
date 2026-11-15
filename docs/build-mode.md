# Build Mode: SARE's world editor

Build Mode lets you make and edit a SARE world inside SARE itself: place
objects, move, rotate and scale them, organise them, save, and playtest
straight away. It needs **no GTA San Andreas files**: a clean machine with
SARE is enough. Everything you make is a normal [content pack](content-packs.md),
so it plays from the launcher and can be shared by copying its folder.

Build Mode is a general SARE world editor, not a map editor for San Andreas.
It does not read IPL/IDE files, model ids or San Andreas coordinates.

## Start

- **Launcher:** on Home, type a name next to *Build Mode* and press
  **Create world**, or press **Edit <pack>** for an existing content pack.
- **Command line:**

  ```text
  sa-runtime --build new:"My Town"     # a new world in content/my-town
  sa-runtime --build my-town           # a pack id
  sa-runtime --build path/to/pack      # a pack folder
  ```

A new world starts with a 32 m ground plate and a spawn point on it.

## Using it

| Area | What it does |
| --- | --- |
| Toolbar | Select (Q), Move (W), Rotate (E), Scale (R), World/Local axes (L), Duplicate, Undo, Redo, Snap (G) with grid size and angle step, Surface placement, Playtest (F6), Save (Ctrl+S), World settings |
| Asset Browser (left) | Search and category filters. Built-in shapes, models from every installed content pack and this world's own models. Missing or broken models are marked ⚠ |
| World Outliner (left) | The scene tree: show/hide 👁, lock 🔒, double-click to rename, right-click for focus, duplicate, group, ungroup, move to group and delete. Search filters by name |
| Inspector (right) | Selected object: name, position/rotation/scale (drag or type), Visible, Locked, Collision, asset and its load status, Advanced (size, triangles, materials, id, extra data). Nothing selected: world name, description, spawn point and time of day |
| Bottom | Console, Errors and warnings, Asset status, Playtest tabs; the status bar shows selection, grid and save state |

Placing: click an asset, move the mouse over the world (a preview follows),
click to place. Shift-click keeps placing, `[` and `]` turn the preview,
right-click or Esc stops. With *Surface* on, objects land on what is under
the mouse; otherwise on the ground plane. With Snap on, positions snap to
the grid (hold Ctrl while dragging to invert snapping).

Camera: right mouse + WASD/QE flies (Shift faster, wheel changes speed),
middle mouse pans, Alt + left mouse orbits, the wheel zooms, F or a
double-click focuses the selection, Home resets. *Help → Controls* lists
every shortcut.

Playtest (F6 or ▶): unsaved work is autosaved, then the world becomes
playable at its spawn point with every object's collision. F6 (or *Back to
Build Mode*) returns to the editor with the camera, selection and undo
history as they were. Nothing is restarted.

## Files

A Build Mode world is a content pack folder:

```text
content/my-town/
  content.json        # name, id, licence, spawn, "world": { "layout": "layout.json" }
  layout.json         # the placed objects (this format)
  assets/<category>/  # models you imported (File → Import model)
  world.glb           # optional: a base scene made in Blender, shown under the objects
  .autosave.json      # unsaved work; removed by Save
```

`layout.json`:

```json
{
  "format": "sare-world-layout",
  "version": 1,
  "next_id": 4,
  "settings": { "hour": 12 },
  "nodes": [
    { "id": 1, "name": "Ground", "asset": "builtin:ground",
      "transform": { "position": [0, 0, 0], "rotation": [0, 0, 0], "scale": [1, 1, 1] },
      "visible": true, "locked": false, "collision": true },
    { "id": 2, "name": "Props" },
    { "id": 3, "name": "Bench", "parent": 2, "asset": "pack:example-town:assets/props/bench.glb",
      "transform": { "position": [4, -2, 0], "rotation": [0, 0, 90], "scale": [1, 1, 1] } }
  ]
}
```

- Coordinates are Blender's: X east, Y north, Z up, metres, around the pack
  origin. Rotation is Blender "XYZ Euler" in degrees.
- A node without `asset` is a group (a folder in the outliner). Groups pass
  Visible, Locked and Collision down to their contents; they have no
  transform of their own yet.
- Ids are stable and never reused, even after undo.
- Asset references are portable, never absolute paths:
  `builtin:<name>`, `self:<path in this pack>`, `pack:<pack id>:<path>`.
  A world that uses `pack:` assets needs that pack installed; import the
  model (it is copied into `assets/`) to make the world self-contained.
- Missing models are never dropped: the object stays in the layout, shows
  as a magenta cube in the editor, is listed under *Asset status*, and is
  skipped (with a log line) when the world is played.
- Unknown fields, at any level, are kept and written back unchanged.
- Versioning: `version` is checked on load. Older versions go through
  `world_layout::migrate`; a file from a newer SARE is refused with a clear
  message instead of being half read.
- Saves are atomic (write to a temporary file, then rename). Autosave runs
  15 seconds after a change and when SARE closes; after a crash Build Mode
  offers to recover the autosave.

## Architecture

Build Mode reuses the existing engine; there is no second renderer, asset
pipeline or project format.

| Part | Where | Notes |
| --- | --- | --- |
| Layout format | `sa-scene::world_layout` | Serde types, validation, migration hook, atomic writes, Blender-to-glTF transform maths |
| Built-in assets | `sa-scene::builtin_assets` | Original shapes generated in code (glTF space) |
| Packs with layouts | `sa-scene::content_pack` | `world.layout`, `world.scene` now optional; `PackWorld::open` places layout objects into the world, `open_base` leaves them out for the editor; `AssetResolver` finds `builtin:`/`self:`/`pack:` models; `place_layout` reports missing ones |
| Editor core | `sa-editor` crate | No GPU or UI: `project` (create, open, save, save as, autosave, recovery, import), `history` (document + command-based undo/redo), `ops` (place, delete, duplicate, rename, group, ungroup, reparent), `catalog` (asset providers), `viewport` (rays, picking, gizmo maths, snapping, orbit/fly camera) |
| Runtime glue | `sa-runtime/src/build_mode/` | `gpu.rs` uploads each asset once and gives each object its own vertex buffers, rewritten in place when it moves (like cars and peds); `viewport.rs` camera, picking, placement, gizmos, grid; `panels.rs` the egui interface; `mod.rs` session, autosave, playtest |
| Hooks in `main.rs` | `--build`, `State.build` | Editor camera drives the view, editor objects join `drawn_batches`, editor UI replaces the HUD while editing, F6 toggles playtest, keyboard/mouse go to egui while editing |
| Launcher | Home page | *Create world* and *Edit <pack>* start `sa-runtime --build` |

Undo/redo: every change is an `sa_editor::history::Command` (insert, remove,
modify nodes, world info, settings, or a batch). A gizmo drag or an
Inspector drag changes the document live and records one command when it
ends, so a whole drag is one undo step.

Extending it:

- **New asset sources** (an importer, a GTA adapter, an online library):
  implement `sa_editor::catalog::AssetProvider` and add it in
  `Catalog::providers_for`. Categories are plain strings from the provider.
- **New object data** (scripts, triggers, lights): add fields to
  `world_layout::Node` with `#[serde(default)]`; older SARE versions keep them
  as unknown data. Format changes that are not additive need a `VERSION` bump
  and a `migrate_step` arm.
- **New edits:** add a function in `sa_editor::ops` that returns a `Command`;
  undo/redo works without more code.
- **New inspector sections:** `panels::object` and `panels::world_settings`.

## Not done yet

Milestone 1 and part of milestone 2 are in. Still to come, roughly in order:

- Asset thumbnails (the placement preview is the preview for now) and a
  native file picker for import.
- Drag and drop in the outliner; group transforms (groups are folders now).
- Collision and wireframe views; marking objects without collision.
- Lights (point, spot, sun), sky and fog settings. The renderer has no
  per-object lights or per-object shadow switches yet, so the Inspector does
  not offer them.
- New world / open another world from inside the editor (use the launcher).
- Terrain, roads and paths, trigger volumes, interactive objects, visual
  scripting, publishing worlds to servers.
