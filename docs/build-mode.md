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

To see every gameplay behaviour in one place, open the sample world:
`sa-runtime --build content/gameplay-sample`, or *Edit Gameplay Sample* in
the launcher.

## Using it

| Area | What it does |
| --- | --- |
| Toolbar | Select (Q), Move (W), Rotate (E), Scale (R), World/Local axes (L), Duplicate, Undo, Redo, Snap (G) with grid size and angle step, Surface placement, Zones (gameplay overlay), Collision (outlines solid objects), Playtest (F6), Save (Ctrl+S), World settings |
| Asset Browser (left) | Picture tiles with search and category filters. Built-in shapes, models from every installed content pack and this world's own models. Missing or broken models are marked ⚠ |
| World Outliner (left) | The scene tree: show/hide 👁, lock 🔒, double-click to rename, right-click for focus, duplicate, group, ungroup, move to group and delete. Search filters by name |
| Inspector (right) | Selected object: name, position/rotation/scale (drag or type), Visible, Locked, Collision, Gameplay (behaviour and actions), asset and its load status, Advanced (size, triangles, materials, id, extra data). Nothing selected: world name, description, spawn point, time of day and weather |
| Bottom | Console, Errors and warnings (including live gameplay checks; click one to select its object), Asset status, Playtest tabs; the status bar shows selection, grid and save state |

Placing: drag an asset from the Asset Browser into the world and let go.
Or click it, move the mouse over the world (a preview follows) and click to
place; every click places another one until you right-click or press Esc.
`[` and `]` turn the preview.

Your own models: drag `.glb` files from your file explorer onto the SARE
window, or use *File → Import model*. They are copied into the world's
`assets/` folder (dropped files go to `assets/imported/`) and are ready to
place. With *Surface* on, objects land on what is under
the mouse; otherwise on the ground plane. With Snap on, positions snap to
the grid (hold Ctrl while dragging to invert snapping).

Other worlds: *File → New world* (Ctrl+N) makes a new world beside this one
and *File → Open world* (Ctrl+O) lists every installed world. SARE restarts on
the chosen world (each world loads its own base scene at start-up), and asks
first if there are unsaved changes.

Camera: right mouse + WASD/QE flies (Shift faster, wheel changes speed),
middle mouse pans, Alt + left mouse orbits, the wheel zooms, F or a
double-click focuses the selection, Home resets. *Help → Controls* lists
every shortcut.

Playtest (F6 or ▶): unsaved work is autosaved, then the world becomes
playable at its spawn point with every object's collision. F6 (or *Back to
Build Mode*) returns to the editor with the camera, selection and undo
history as they were. Nothing is restarted.

## Gameplay

Any object can get one **behaviour**: select it and use *+ Trigger zone*,
*+ Interaction (E)*, *+ Checkpoint* or *+ Spawn point* in the Inspector's
Gameplay section. The Asset Browser's **Gameplay** category has ready-made
ones; placing one attaches its behaviour:

| Asset | Behaviour |
| --- | --- |
| Spawn point | Players start here, facing the arrow. The first one in the World Outliner replaces the world's spawn |
| Door (opens with E) | An interaction that toggles the door itself: when open it is hidden and has no collision |
| Door to another place | An interaction that teleports the player; pick the destination in the Inspector |
| Teleport pad | A trigger that teleports the player who steps on it |
| Destination point | A place to teleport to; the arrow is the way the player faces |
| Trigger zone | Actions when a player walks in, and when they walk out |
| Interaction point | A prompt, and actions when the player presses E |
| Checkpoint | Numbered stops of a timed course; numbers count up as you place them |

Markers (spawn points, pads, destinations, zones, checkpoints) show only in
Build Mode. Doors are real objects.

**Zones** are boxes. *Zone size* is width, depth and height in metres; the
box stands on the object, turns with it and does not scale with it. The
viewport draws each zone with a label (turn them off with *Zones* in the
toolbar). Dashed lines show where teleports lead, thin lines show which
objects an action shows or hides.

**Actions** run in order: *Show message*, *Teleport player* (to any object,
facing its +Y), *Show object*, *Hide object*, *Toggle object* (an object
hidden this way has no collision), *Set time of day* and *Set weather*.
"This object" means the object the behaviour is on. Problems, such as a
teleport without a destination, a deleted target or two checkpoints with the
same number, show under the behaviour and in *Errors and warnings*.

**Checkpoints:** walking into the lowest number starts the clock, the others
must follow in order, and the highest shows the time. A world can have
several courses; give each its own *Course* name.

**World settings** hold a fixed hour and a weather (clear, cloudy, rain or
fog). Both show while you edit and apply when the world is played, and
actions can change them while playing.

While playing, E uses the interaction you stand in. Zones react to a player
on foot. Every playtest starts the gameplay fresh.

A world played from the launcher (outside Build Mode) runs its spawn points,
triggers, interactions, teleports, checkpoints, time and weather. Showing
and hiding objects works only in Playtest for now, because a played pack
draws its layout as one merged scene. The logic runs on each player's own
computer and is not yet synchronised between players on a server.

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
  "next_id": 5,
  "settings": { "hour": 12, "weather": "clear" },
  "nodes": [
    { "id": 1, "name": "Ground", "asset": "builtin:ground",
      "transform": { "position": [0, 0, 0], "rotation": [0, 0, 0], "scale": [1, 1, 1] },
      "visible": true, "locked": false, "collision": true },
    { "id": 2, "name": "Props" },
    { "id": 3, "name": "Bench", "parent": 2, "asset": "pack:example-town:assets/props/bench.glb",
      "transform": { "position": [4, -2, 0], "rotation": [0, 0, 90], "scale": [1, 1, 1] } },
    { "id": 4, "name": "Gate", "asset": "builtin:door",
      "transform": { "position": [0, 5, 0] },
      "gameplay": { "kind": "interact", "size": [2.4, 3, 2.6], "prompt": "Open the gate",
                    "actions": [ { "do": "toggle" } ] } }
  ]
}
```

- Coordinates are Blender's: X east, Y north, Z up, metres, around the pack
  origin. Rotation is Blender "XYZ Euler" in degrees.
- A node without `asset` is a group (a folder in the outliner). Groups pass
  Visible, Locked and Collision down to their contents; they have no
  transform of their own yet.
- Ids are stable and never reused, even after undo.
- `gameplay` is optional. `kind` is `spawn`, `trigger` (`size`, `once`,
  `on_enter`, `on_leave`), `interact` (`size`, `prompt`, `actions`) or
  `checkpoint` (`size`, `race`, `order`). Every action has a `do`: `message`
  (`text`, `seconds`), `teleport` (`target`), `show`, `hide` or `toggle`
  (`node`; leave it out for the object itself), `set_time` (`hour`) or
  `set_weather` (`weather`). Targets are object ids. A kind or action this
  SARE does not know is kept unchanged and does nothing; a known one with
  wrong fields is refused with a message that names it.
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
| Built-in assets | `sa-scene::builtin_assets` | Original shapes generated in code (glTF space); `editor_only` markers are neither drawn nor solid in play |
| Gameplay | `sa-scene::world_logic` | Components and actions (serde; unknown kinds kept), zone maths, `check` for problems, `preset` for gameplay assets, and `Logic`: a deterministic engine fed the player's feet and `dt` that returns effects. No GPU or UI, so a server can run it later |
| Packs with layouts | `sa-scene::content_pack` | `world.layout`, `world.scene` now optional; `PackWorld::open` places layout objects into the world, `open_base` leaves them out for the editor; `AssetResolver` finds `builtin:`/`self:`/`pack:` models; `place_layout` reports missing ones |
| Editor core | `sa-editor` crate | No GPU or UI: `project` (create, open, save, save as, autosave, recovery, import), `history` (document + command-based undo/redo), `ops` (place, delete, duplicate, rename, group, ungroup, reparent), `catalog` (asset providers), `viewport` (rays, picking, gizmo maths, snapping, orbit/fly camera) |
| Runtime glue | `sa-runtime/src/build_mode/` | `gpu.rs` uploads each asset once and gives each object its own vertex buffers, rewritten in place when it moves (like cars and peds); `viewport.rs` camera, picking, placement, gizmos, grid, gameplay and collision overlays; `panels.rs` the egui interface; `gameplay_ui.rs` the Inspector's Gameplay section; `mod.rs` session, autosave, playtest, `--smoke-build` |
| Playing gameplay | `sa-runtime/src/play_logic.rs` | Runs `Logic` in playtest and in played packs: messages, the E prompt, teleports, rebuilding playtest collision when objects are hidden, time and weather (`daycycle::apply_weather`) |
| Hooks in `main.rs` | `--build`, `State.build` | Editor camera drives the view, editor objects join `drawn_batches`, editor UI replaces the HUD while editing, F6 toggles playtest, keyboard/mouse go to egui while editing |
| Launcher | Home page | *Create world* and *Edit <pack>* start `sa-runtime --build` |

Frame pacing (`build_mode/pacing.rs`): while editing, Build Mode draws only
when something changes. Input, egui animations, flying the camera, dragging
and loading draw at up to 60 FPS (lower if the frame rate limit is lower);
with nothing going on it draws 2 frames a second, and 1 in the background or
minimised. Playtest is paced like the game.

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
- **New behaviours or actions:** add a variant to `world_logic::Component` or
  `Action` (and its name to `Tagged::KINDS`), handle it in `Logic`, `check`
  and `gameplay_ui`, and in `play_logic` if it needs a new `Effect`. Older
  SARE versions keep the new kind as unknown data.

Testing on a GPU: `sa-runtime --build content/gameplay-sample --smoke-build
--capture-dir shots` edits the sample world, playtests it (opens the gate
with E, walks into the night zone, goes through the front door), returns to
the editor, checks that nothing changed and saves screenshots.
`content/gameplay-sample` is generated by `tools/make-gameplay-sample.py`.

## Not done yet

Milestone 1 and part of milestone 2 are in. Still to come, roughly in order:

- A native file picker for import.
- Drag and drop in the outliner; group transforms (groups are folders now).
- Showing and hiding objects when a pack is played from the launcher (it
  works in Playtest), and gameplay synchronised between players on a server.
- More actions (sounds, items, script events) and Lua hooks so server
  resources can react to zones.
- A wireframe view of the exact collision triangles (the Collision view
  outlines object bounds).
- Lights (point, spot, sun), sky and fog colour settings. The renderer has no
  per-object lights or per-object shadow switches yet, so the Inspector does
  not offer them.
- Terrain, roads and paths, visual scripting, publishing worlds to servers.
