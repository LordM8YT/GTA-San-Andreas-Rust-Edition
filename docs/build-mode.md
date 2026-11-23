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
| Ribbon (top) | Tabs like other building editors. **Home**: Clipboard (Paste, Copy, Cut, Duplicate), Tools (Select, Move, Scale, Rotate, World/Local axes, Snap), Terrain (Sculpt), Insert (Part, Toolbox), Edit (Undo, Redo, Group, Ungroup, Lock), World (Workspace, Lighting), Test (Play, Save). **Model**: the tools, Snap to Grid (move step, rotate step, Surface placement), Terrain, Insert, Effects (Light) and Edit. **Test**: Play, Set spawn, Workspace and Lighting. **View**: show or hide the Explorer, Properties, Toolbox and Output, the grid, Zones (gameplay overlay) and Collision (outlines solid objects), Focus, Reset camera, fly speed and Controls |
| Toolbox (left) | Picture tiles with search and category filters. Built-in shapes, models from every installed content pack and this world's own models. Missing or broken models are marked ⚠ |
| Explorer (right, top) | The scene tree under **Workspace**, with **Lighting** below it: show/hide 👁, lock 🔒, double-click to rename, right-click for focus, duplicate, group, ungroup, move to group and delete. The filter box searches by name. Click Workspace for the world's name, description and spawn point, or Lighting for the time of day, weather, fog, sky colour and brightness |
| Properties (right, below the Explorer) | Selected object, in category bars: name, position/rotation/scale (drag or type), Visible, Locked, Collision, Gameplay (behaviour and actions), Light, asset and its load status, Advanced (size, triangles, materials, id, extra data). Several selected: batch edits |
| Output (bottom) | Output, Errors and warnings (including live gameplay checks; click one to select its object), Asset status, Playtest tabs; the status bar shows selection, grid and save state |

Placing: drag an asset from the Toolbox into the world and let go.
Or click it, move the mouse over the world (a preview follows) and click to
place; every click places another one until you right-click or press Esc.
`[` and `]` turn the preview.

Your own models: drag `.glb` files from your file explorer onto the SARE
window, or use *Import* (Home or Model tab) or *File → Import model*,
which open your system's file picker. They are copied into the world's
`assets/` folder (dropped files go to `assets/imported/`) and are ready to
place. With *Surface* on, objects land on what is under
the mouse; otherwise on the ground plane. With Snap on, positions snap to
the grid (hold Ctrl while dragging to invert snapping).

Other worlds: *File → New world* (Ctrl+N) makes a new world beside this one
and *File → Open world* (Ctrl+O) lists every installed world. SARE restarts on
the chosen world (each world loads its own base scene at start-up), and asks
first if there are unsaved changes.

Shortcuts familiar from other building editors work too: Ctrl+1, 2, 3 and 4
pick Select, Move, Scale and Rotate, Ctrl+L switches world and local axes,
Ctrl+C, Ctrl+X and Ctrl+V copy, cut and paste objects (groups with their
contents), and F5 plays the world (F6 comes back). Each paste lands one grid
step further on. Cut and paste puts the objects back as themselves, so doors
that lead to them and checkpoint numbers still work; a paste into a group
that is now locked or hidden goes to the top level instead. Every dock has a ✕ to close it; the View
tab brings it back.

Slower computer? *View → Performance → Editor graphics* has three choices.
*Light* draws the editor without sun shadows, wet reflections or bloom, at
two thirds of the window's resolution, and at most 30 frames a second.
*Full* uses your own graphics settings. *Automatic*, the default, picks Light
on integrated graphics and Full on a graphics card. Playtest always uses your
own graphics settings, so you see the world the way players will. Traffic and
pedestrians stay away while you edit. The choice is saved with your other
settings.

Select a group (or several objects) and the gizmo moves, turns and scales
them as one, about the middle of what is selected: a house turns with its
doors and lamps in place. Scaling along one axis spreads the objects out
along it; objects that are turned inside the group stretch along their own
axes.

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
*+ Interaction (E)*, *+ Checkpoint* or *+ Spawn point* in the Properties panel's
Gameplay section. The Toolbox's **Gameplay** category has ready-made
ones; placing one attaches its behaviour:

| Asset | Behaviour |
| --- | --- |
| Spawn point | Players start here, facing the arrow. The first one in the Explorer replaces the world's spawn |
| Door (opens with E) | An interaction that toggles the door itself: when open it is hidden and has no collision |
| Door to another place | An interaction that teleports the player; pick the destination in the Properties panel |
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
View tab). Dashed lines show where teleports lead, thin lines show which
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

**Lighting** (in the Explorer) holds a fixed hour and a weather (clear, cloudy, rain or
fog). Both show while you edit and apply when the world is played, and
actions can change them while playing.

## Lights and environment

Only what SARE's renderer really draws is offered:

- **Lights on objects** (Properties → *+ Light*): a glowing corona at the
  lamp and a soft pool of light on the ground under it, from dusk to dawn,
  the same way SARE draws street lamps. Colour, glow size (m), ground light
  radius (m) and where the lamp sits on the object. Lamp posts from the
  Toolbox come with one. Lights do not light up other objects or cast
  shadows: the renderer has one sun and no per-object lights. Set a fixed
  time after 20:00 to see them while editing.
- **Explorer → Lighting**: the hour, the weather, *Fog* (density
  ×0.2 to ×10 over the time and weather's own), *Sky colour* (also the
  colour of the distant haze) and *Brightness* (ambient light ×0.3 to ×1.5).
  The sun follows the hour and casts shadows.

All of it is saved in `layout.json`, shows while you edit, and is used when
the world is played from the launcher or hosted on a server (every player
has the pack, so everyone sees the same).

## Terrain

Place *Terrain 64 m (sculpt)* from the Toolbox (category Ground): flat
ground of 32 × 32 squares that you shape yourself. Pick **Sculpt**
(`T`) in the Home or Model tab, choose a brush and hold the left mouse button on the
terrain:

| Brush | What it does |
| --- | --- |
| Raise / Lower | Lifts or sinks the ground under the brush, most in the middle |
| Smooth | Evens out bumps and sharp edges |
| Flatten | Levels the ground to the height where the stroke began (for building plots and roads) |

The brush circle shows where it works; radius (1-40 m) and strength are in
the ribbon. Each stroke is one undo step. The Properties shows the
terrain's size and height range, its colour, and *Make flat*. Terrain is a
normal object: move, turn or scale it, and it is drawn and solid in
Playtest, from the launcher and on servers. Its heights are saved in
`layout.json`, so it travels with the world.

Moving objects in the **Explorer**: drag a row onto the top or bottom
half of another row to put it before or after it, onto the middle of a group
to put it inside, or below the list for the top level. Dragging a selected
row moves the whole selection. With several objects selected, the Properties panel
shows, hides, locks, unlocks, switches collision, resets or drops them all
at once. Each is one undo step.

While playing, E uses the interaction you stand in. Zones react to a player
on foot. Every playtest starts the gameplay fresh.

A world played from the launcher (outside Build Mode) runs all of it too.
The objects that actions show or hide are kept out of the pack's merged
scene and drawn one by one, and their collision switches with them.

## Multiplayer

Host the world and everyone plays the same gameplay. Put the pack in
`content/<id>` next to the server's `server.cfg` and add
`sets content_pack <id>` (see [server hosting](server-hosting.md#build-mode-worlds)).
Players who join get the pack's world and the server runs its gameplay:

- **The server decides.** It loads the pack's `layout.json` and checks each
  player's zones against the positions it receives. Clients never send
  effects or state; they draw what the server says.
- **Shared:** objects shown or hidden (doors), the hour and the weather. A
  door one player opens is open for everyone, also for players who join
  later. Each routing bucket (instance) has its own copy, starting from the
  world as it was saved.
- **Per player:** messages, teleports, checkpoint progress and timers, and
  "only the first time" triggers.
- **E** sends a request to the server, which accepts it only if the player
  stands in that interaction (with 2 m to spare for network delay) and not
  more often than every 250 ms. Refusals are logged on the server.
- Players who leave are forgotten; the world stays as it is.
- Server resources react to `sare:zoneEnter`, `sare:objectUsed`,
  `sare:raceFinished` and more, and change the world with
  `SetWorldObjectVisible`, `SetWorldHour` and `SetWorldWeather`.

Without the pack on the server, each player's game runs the gameplay on its
own, unsynchronised, as before.

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
  "settings": { "hour": 12, "weather": "clear", "fog": 1.5, "sky": [0.5, 0.65, 0.9], "brightness": 1.0 },
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
- `terrain` is optional: `{ "size": 64, "cells": 32, "heights": [...],
  "color": [0.36, 0.5, 0.27] }` with (cells + 1)² heights in metres, row by
  row from south to north (`sa_scene::terrain`). Up to 128 cells and
  ±200 m.
- `light` is optional: `{ "color": [1, 0.85, 0.6], "size": 1, "radius": 6,
  "offset": [0, 1, 4.25] }` (linear RGB 0-1, metres, offset in the object's
  axes). `settings.fog` (0.2-10), `sky` (RGB 0-1) and `brightness`
  (0.3-1.5) are optional too; values out of range are refused on load.
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
| Terrain | `sa-scene::terrain` | `Terrain` heights on a node, `model` (drawn + solid), `height_at`, brushes `raise`/`smooth`/`flatten`; `content_pack::node_model` gives every path (pack, Playtest, editor) the terrain instead of the asset; the Sculpt tool is in `build_mode/viewport.rs` |
| Built-in assets | `sa-scene::builtin_assets` | Original shapes generated in code (glTF space); `editor_only` markers are neither drawn nor solid in play |
| Gameplay | `sa-scene::world_logic` | Components and actions (serde; unknown kinds kept), zone maths, `check` for problems, `preset` for gameplay assets, and the engine: `World` (compiled layout, no state), `Shared` (per world instance: shown/hidden objects, hour, weather, a revision and `Snapshot`) and `PlayerState` (zones, once-triggers, courses, `Happened` events for scripts). `Logic` bundles one of each for single player. `wire` holds the network events. No GPU, UI or GTA types: the server and the game run the same code |
| Server authority | `sa-server/src/world.rs` | Loads the pack named by `content_pack`, steps every player each frame against the poses the server has, keeps `Shared` per routing bucket, sends snapshots (on change, arrival, bucket change) and personal effects, validates `__sare:worldUse` (position with 2 m slack, 250 ms rate limit), forwards events to Lua; natives in `natives.rs`. Tests: `world_tests.rs` with several connected players |
| Packs with layouts | `sa-scene::content_pack` | `world.layout`, `world.scene` now optional; `PackWorld::open` places layout objects into the world, `open_base` leaves them out for the editor; `AssetResolver` finds `builtin:`/`self:`/`pack:` models; `place_layout` reports missing ones |
| Editor core | `sa-editor` crate | No GPU or UI: `project` (create, open, save, save as, autosave, recovery, import), `history` (document + command-based undo/redo), `ops` (place, delete, duplicate, rename, group, ungroup, reparent), `catalog` (asset providers), `viewport` (rays, picking, gizmo maths, snapping, orbit/fly camera) |
| Runtime glue | `sa-runtime/src/build_mode/` | `gpu.rs` uploads each asset once and gives each object its own vertex buffers, rewritten in place when it moves (like cars and peds); `viewport.rs` camera, picking, placement, gizmos, grid, gameplay and collision overlays; `panels.rs` the egui interface; `gameplay_ui.rs` the Properties panel's Gameplay section; `mod.rs` session, autosave, playtest, `--smoke-build` |
| Playing gameplay | `sa-runtime/src/play_logic.rs` | Runs `Logic` in playtest and in played packs: messages, the E prompt, teleports, rebuilding playtest collision when objects are hidden, time and weather (`daycycle::apply_weather`) |
| Hooks in `main.rs` | `--build`, `State.build` | Editor camera drives the view, editor objects join `drawn_batches`, editor UI replaces the HUD while editing, F6 toggles playtest, keyboard/mouse go to egui while editing |
| Launcher | Home page | *Create world* and *Edit <pack>* start `sa-runtime --build` |

Frame pacing (`build_mode/pacing.rs`): while editing, Build Mode draws only
when something changes. Input, egui animations, flying the camera, dragging
and loading draw at up to 60 FPS (lower if the frame rate limit is lower);
with nothing going on it draws 2 frames a second, and 1 in the background or
minimised. Frames are scheduled from when they were due, so timers that
wake late (Windows rounds to about 16 ms) do not lower the rate. The light
editor graphics (`Settings::light_editor`, picked by
`EditorPerformance`) apply only while editing: `State::draw_settings` is what
the frame is drawn with, and the post-processing targets follow its render
size. Playtest is paced like the game.

Undo/redo: every change is an `sa_editor::history::Command` (insert, remove,
modify nodes, world info, settings, or a batch). A gizmo drag or an
Properties drag changes the document live and records one command when it
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
the editor, checks that nothing changed and saves screenshots. The same
flag on a played pack, `sa-runtime --content content/gameplay-sample
--smoke-build`, opens and closes the gate (checking that it is drawn and
that a walker gets through only while it is open) and uses the front door.
Joined to a server that runs the world (`--content content/gameplay-sample
--join 127.0.0.1:<port> --smoke-build`), it waits for the server's world
state, presses E at the gate and passes only when the server opens it.
`content/gameplay-sample` is generated by `tools/make-gameplay-sample.py`.

Switchable objects: `world_logic::switchable` lists the objects actions
show or hide. `content_pack::place_layout` leaves them out of the merged
scene; `CollisionWorld::add_switchable` and `set_hidden` switch their
collision without rebuilding the world (also used by Playtest).

## Not done yet

Milestone 1 and part of milestone 2 are in. Still to come, roughly in order:

- A pivot of its own for a group: today a group turns and scales about the
  middle of its contents, and has no position or rotation you can type in.
- Multiplayer: interactions while driving, server checks of teleport
  destinations against the player's later position, and a server console
  command to reset a world instance.
- More actions (sounds, items, script events) and Lua hooks so server
  resources can react to zones.
- A wireframe view of the exact collision triangles (the Collision view
  outlines object bounds).
- Real per-pixel point and spot lights (needs renderer work; today lights
  are glow and ground light), per-object shadow switches.
- Terrain: painting several materials, holes, and terrain bigger than one
  object (place several side by side today).
- Roads and paths drawn along spline points with their own collision; until
  then, use the Road pieces and sculpted terrain.
- Visual scripting, publishing worlds to servers.
