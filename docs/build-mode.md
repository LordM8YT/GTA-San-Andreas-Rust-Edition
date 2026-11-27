# SARE Studio (Build Mode): SARE's world editor

SARE Studio (called Build Mode in older versions and in the code) lets you make and edit a SARE world inside SARE itself: place
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
and *File → Open world* (Ctrl+O) lists the worlds you opened recently first,
then every installed world. *Browse for a world folder…* opens a world
anywhere on your PC (pick the folder that holds its `content.json`); a
folder that is not a world says so instead of failing later. The recent
list (up to 10) is kept in `studio-recent.json` beside SARE's settings. SARE restarts on
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

**Models.** *Group* (Ctrl+G) makes the selection a model: one thing, like
a car, a house or a market stall, with a **pivot** of its own (📦 in the
Explorer). The pivot starts in the middle of the contents, at their base.
Moving or turning a model moves and turns its pivot, and everything inside
comes along; inside, objects keep their place relative to the pivot, so
moving the model never disturbs them. A click on any part of a model in the
world selects the whole model; **Alt+click** selects the object inside
(hovering shows both names). Its Properties show the **Pivot** (position
and rotation to type in), *Centre pivot* to put the pivot back in the middle
of the contents without moving them, and *Model*: turn it off to make the
group a plain folder, whose objects a click picks one by one. Groups from
older worlds are folders without a pivot; the first time one is moved or
turned it gets one in the middle of its contents.

Objects keep their place in the world when they go into or out of a model
(dragged in the Explorer, cut and pasted, ungrouped), and a copy of a model
is a model with its own pivot. Scripts, players and servers see every
object where it is in the world.

Several objects selected (or a model with the Scale tool) move, turn and
scale as one, about the middle of the selection. Scaling along one axis
spreads the objects out along it; objects turned inside a group stretch
along their own axes.

Camera: right mouse + WASD/QE flies (Shift faster, wheel changes speed),
middle mouse pans, Alt + left mouse orbits, the wheel zooms, F or a
double-click focuses the selection, Home resets. *Help → Controls* lists
every shortcut.

Playtest (F6 or ▶): unsaved work is autosaved, then the world becomes
playable at its spawn point with every object's collision. F6 (or *Back to
Build Mode*) returns to the editor with the camera, selection and undo
history as they were. Nothing is restarted.

Spawn: *Set spawn* (Test ribbon, or *Click to set spawn* under Workspace in
Properties) waits for a click in the viewport and puts the spawn on the
ground or object under the mouse, facing the way the camera looks.
Right-click or Esc cancels. You can also grab the green Spawn flag in the
viewport and drag it somewhere else (one undo step). If the world has a
Spawn point object, that object moves instead (it wins over the world's own
spawn); select it and use the Move tool like any object. While a world is played,
R and falling below the world put the player back at the spawn.

## My Models

A model you will want again (a lamp post with its light, a market stall, a
house) can be saved: select it and press **Save to My Models** in its
Properties (or right-click it in the Explorer). It is written to the world's
`models/` folder and shows in the Toolbox under **My Models**, with a
picture, to place as often as you like. The model you saved is its first
copy.

Copies stay linked to the saved model. Change one (move a wall, paint it,
add a door) and its Properties say *Changed here*: **Save to all copies**
writes it to My Models and gives every other copy the same contents, each
keeping its own place, turn and name. **Revert** throws the changes to one
copy away; **Unlink** makes a copy an ordinary model that later saves no
longer touch. Undo puts the copies back as they were; the saved file keeps
the change until it is saved again.

Inside the world a copy is an ordinary model: players, servers, scripts and
older SARE versions see its objects like any others. Behaviour inside a
saved model that points at objects inside it (a teleport, a door to open)
points at the copy's own objects; pointing at things outside the model is
not saved. Objects inside a copy get new ids when the copy is updated, so
a script or teleport elsewhere in the world that points at one of them
points at nothing afterwards (scripts that find objects by name keep
working).

## Testing with a server and players

*Test › Server* starts the world as it is now (saved or not) on a SARE
server on this computer and opens one to four player windows joined to it,
as players will meet it: the server runs the gameplay, scripts and
physics, and each window is a player. What the server and each player
print shows in Output as `[Server]`, `[Player 1]` and so on. *Stop* closes
them all; *Restart* starts again with the world as it is then.

The server gets a copy of the world in a test folder (`sare-studio-test-<id>`
in the temp folder), so your files are not touched and the server is not
listed publicly.

## Publishing

*Test › Publish...* puts the world on a SARE server: type the server's
folder (the one with `server.cfg`) and press *Publish*. The world is saved,
copied to the server's `content/<id>/` (an older copy there is replaced;
a failed copy leaves it as it was) and `server.cfg` gets
`sets content_pack <id>`, so players who join play this world. Players who
do not have it download it when they join. Restart the server for players to
get the new version.

## Sharing your world

Click **Workspace** in the Explorer. Under *Sharing*:

- **Author**, **Version** and **Licence** go into the world's
  `content.json` and show on its page in SARE Client and in Discover. A new
  world is *All rights reserved*: players may play it, but nobody may share
  copies. Pick CC0, CC-BY, CC-BY-SA, CC-BY-NC or MIT to let others share it
  (hover a licence to see what it allows). Each change is one undo step and
  is saved with the world.
- **Use this view as cover** saves what the viewport shows (without the
  panels, gizmos or grid) as `cover.png`, the middle 16:9 of the window at
  up to 1280 × 720. SARE Client shows it on the world's card and page.
- **Export world…** (also *File → Export world (ZIP)…*) saves the world,
  then packs its folder into `exports/<id>-<version>.zip` beside the
  content folder. Hidden files (the autosave) stay out; Discover's limits
  (1024 files, 256 MiB) are checked first. The window shows the ZIP's size
  and SHA-256, **Copy Discover entry** copies the ready-made entry for
  `community/worlds.json` (only the download link is left to fill in), and
  **Open folder** shows the ZIP. The same steps are written to a
  `.share.txt` file next to it.

Before it packs, Export checks the world and lists what to fix first: no
licence to share under, no author or description, no cover, gameplay
problems, objects whose model is missing, and models borrowed from other
content packs (players need those packs too).

Send the ZIP to friends (they unpack it into their `content/` folder), or
upload it and add the entry to `community/worlds.json` with a pull request
so everyone can install it from Discover (see
[community/README.md](../community/README.md)).

## Vehicles

Workspace › Vehicles holds the world's own drivable cars, trucks and vans.
*Import vehicle…* takes a `.glb` from Blender, copies it into the world's
`vehicles/` folder and checks it: you get its size and wheel count, and a
warning for what to fix (no wheels named `wheel_lf`, `wheel_rf`, `wheel_lb`,
`wheel_rb`; a scale that is far off; a nose that does not point along
Blender +Y; no `paint` material; too many triangles for weak PCs).

Open a vehicle to change its name, colour, top speed, acceleration, brakes,
grip, steering and driven wheels (front, rear or all), or *Reset handling*.
*Make it F9's car* moves it to the top: F9 and the car menu start with it.
*Remove* takes it out of the world (Undo brings it back). The model name
(shown under the name) is what scripts spawn: `CreateVehicle("towtruck", ...)`.

Save the world, then play it (or host it) to drive them; players who join
download the world with its vehicles. For a server that adds cars to any
world, see [addon vehicles](native-mods.md#addon-vehicles-gltf-cars-for-sare-servers).
Model rules are in [content packs › Vehicles](content-packs.md#vehicles).

## Physics

A part's Properties have **Anchored** (on for every part placed). Untick
it and the part falls when the world is played: straight down, faster and
faster, until it lands on what is below it: the ground, objects or other
parts, so loose blocks stack. It falls again when a script moves it and
lets go, or when what it stands on is hidden. Nothing falls while you
edit.

Falls run where the world's gameplay runs (on the server when there is
one), and players see them through the usual snapshots, so everyone sees
the same pile. Not yet: tumbling, sliding, or players and cars pushing
parts; only parts can be unanchored.

## Command bar

Under Output is a line for Lua (the `>` field). It runs once against the
world you are editing, with the same API as [world scripts](world-scripts.md)
plus `selection()`, the selected objects:

```lua
for _, lamp in ipairs(world.tagged('Lamp')) do lamp.visible = false end
for _, o in ipairs(selection()) do o.color = rgb(1, 0.5, 0) o:set('Team', 'Orange') end
world.find('Gate').position = vector(10, 4, 0)
```

What it changes (shown, moved, painted parts, attributes, tags, copies made
with `:clone`) is one undo step. `print` writes to Output, and errors show
there with their line. Up and Down go through earlier commands. While
editing, only parts keep a colour; `wait`, `every` and events do nothing
here, as the command runs once.

## Arranging

- **Nudge:** the arrow keys move the selection one grid step (10 cm with
  snapping off) along the ground: up goes the way the camera looks, along
  the nearest world axis. Page Up and Page Down move it up and down. Each
  press is one undo step.
- **Align** (Model tab › Arrange): lines the selection up with the last
  object selected: west, east or north sides, middles, bottoms or tops.
- **Space evenly**: three or more objects get the same distance between
  their middles along an axis; the two at the ends stay where they are.

Both work in the world, by what is drawn (a model counts as everything in
it), and move models by their pivots.

## Attributes and tags

Every object and group has **Attributes** and **Tags** in its Properties.

- An attribute is a value of your own with a name: a number (a price, a
  speed), a text (a team, an owner), true/false (for sale) or a vector
  (a spot, a size, a colour). Type a name, pick the kind and press *Add*;
  the trash button removes one. Scripts read them with
  `object:get('Price')` and may change them while the world runs.
- A tag is a label that sorts objects into sets: `Lamp`, `Coin`,
  `Shop`. Scripts find every object with one with `world.tagged('Coin')`.
  Type `#coin` in the Explorer's filter to see them all.

A group's attributes and tags apply to everything inside it (an object's
own attribute of the same name comes first), so tagging a model tags its
parts. Copies of a saved model carry them too; on the copy itself they
count as a change to the saved model.

## Parts

Parts are the shapes you build with: walls, floors, steps, pillars,
roofs, signs. *Part* on the Home ribbon places a block (*Shapes...* under it,
or the Toolbox's **Parts** category, has the others):

| Shape | |
| --- | --- |
| Block | A box |
| Ball | A ball; an uneven size stretches it |
| Cylinder | Upright, round across its X and Y size |
| Wedge | A slope rising to the north (+Y) |
| Corner wedge | A slope rising to its north-east corner |

Select one and its **Properties** show:

- **Size** in metres (in Transform, instead of Scale). The Scale tool's
  handles resize a part too: drag a side handle and that side moves out
  while the other side stays put; the top handle grows it upwards. With
  Snap on, sizes snap to the move step.
- **Part**: the shape, a **colour** (the colour button picks any; the
  swatches are one click), a **material** and **transparency**.
  Materials are patterns the colour is painted over: Plastic, Smooth,
  Concrete, Wood, Brick, Tiles, Metal and Glass (always see-through).
  Patterns keep their size in metres, so a longer wall shows more bricks
  instead of stretched ones. Transparency 1 makes a part invisible but
  still solid, for invisible walls; turn Collision off for a part players
  walk through.

Select several parts to give them all the same shape, colour or material
in one step. Parts are solid when played, also on servers, and scripts can
colour, move, show, hide and copy them like any object.

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

### Scripts

For more than the actions above, give an object a **script**: Lua that runs
while the world is played, here, from the launcher and on servers. *Script*
on the Home ribbon adds one to the selected object (or to the whole world)
and opens the script editor; *+ Add script* in an object's Properties starts
from an example (a door that opens on E, a lift, a sliding door, race
results and more). Scripts can react to zones, E, checkpoints and players
joining, show, hide and move objects (a moving object carries players
standing on it), send messages, teleport players and set the time and
weather. See [World scripts](world-scripts.md).

## Lights and environment

Only what SARE's renderer really draws is offered:

- **Lights on objects** (Properties → *+ Light*): a glowing corona at the
  lamp that lights the walls, ground, cars and people around it from dusk
  to dawn, softly fading out a little past its *ground light radius*.
  Colour, glow size (m), ground light radius (m) and where the lamp sits on
  the object. Lamp posts from the Toolbox come with one. The 32 lamps
  nearest the camera light the scene (others show their painted pool), and
  lamps do not cast shadows. Played from the launcher, lamps on objects that
  nothing shows, hides or moves are part of the world's merged scene and
  keep the painted pool. Players who
  turn off *Lamp lighting* in their graphics settings (and the light editor
  graphics) see a painted pool of light on the ground instead. Set a fixed
  time after 20:00 to see them while editing.
- **Explorer → Lighting**: the hour, the weather, *Fog* (density
  ×0.2 to ×10 over the time and weather's own), *Sky colour* (also the
  colour of the distant haze) and *Brightness* (ambient light ×0.3 to ×1.5).
  The sun follows the hour and casts shadows.
- **Look** (also on the Lighting page): how the world's colours look on
  every player's screen. Pick Classic (as drawn), Natural, Vivid,
  Cinematic, Sunset, Cold night, Dreamy or Noir, and fine-tune it under
  *Adjust the look*: exposure, contrast, colour, warmth (cold to warm), glow
  and dark corners. A look works on top of each player's own picture
  settings: exposure, colour and glow multiply theirs, so a player who
  turned glow off keeps it off, and any look runs on any PC.

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
| Paint | Paints the squares under the brush with a material: Ground (the terrain's own colour), Grass, Dirt, Sand, Rock or Snow. Neighbouring materials blend softly. Painting over a hole fills it |
| Hole | Cuts the squares under the brush out: nothing is drawn or solid there, for cave and tunnel entrances |

The brush circle shows where it works; radius (1-40 m) and strength are in
the ribbon. Each stroke is one undo step. The Properties shows the
terrain's size and height range, how much is painted or cut out, its
colour, *Make flat* and *Clear paint*. Terrain is a
normal object: move, turn or scale it, and it is drawn and solid in
Playtest, from the launcher and on servers. Its heights are saved in
`layout.json`, so it travels with the world.

## Roads

Place *Road along points* from the Toolbox (category Roads): an 8 m road
through three points with a gentle bend. Select it and its points show as
white dots in the world:

- Drag a dot to move that point (it stays at its height; with Snap on it
  lands on the grid).
- **Draw points** in the Properties: every click in the world adds a point
  at the end of the road, on whatever surface is under the mouse.
  Right-click or Esc stops. Clicking the first point (with three points
  or more) joins the end to the start and stops drawing.
- **Closed loop** in the Properties does the same: the road runs on from
  its last point back to its first, for a ring road or a race track.
- **+ Add point** continues the road 16 m in its direction; × removes a
  point (a road keeps at least two).
- Type a point's X, Y and Z (the road's own metres) or the width (1-40 m).

The road runs on a smooth curve through every point. Heights ease from one
point to the next, so a road can climb a sculpted hill: raise a point's Z.
It has edge lines, a dashed centre line and a short kerb so it never floats
over uneven ground, and it is solid when played, from the launcher and on
servers. Moving, turning or scaling the object moves the whole road.

Moving objects in the **Explorer**: drag a row onto the top or bottom
half of another row to put it before or after it, onto the middle of a group
to put it inside, or below the list for the top level. Dragging a selected
row moves the whole selection. With several objects selected, the Properties panel
shows, hides, locks, unlocks, switches collision, resets or drops them all
at once. Each is one undo step.

While playing, E uses the interaction you stand in. Zones and checkpoints
react to players on foot and in cars. Interactions are on foot only unless
*Works from a vehicle* is ticked: then a driver presses E there too (a
drive-through, a garage door). Any teleport a driver gets (from an
interaction or a zone) takes the car along with the driver at the wheel and
its damage as it was, when the car has room at the destination; otherwise
the driver sees "No room for the car there" and stays. Servers check it as
well and refuse an on-foot interaction from a driver. Every playtest starts the gameplay fresh.

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
  `SetWorldObjectVisible`, `SetWorldHour`, `SetWorldWeather` and
  `ResetWorldInstance`.
- The server console's `world_reset [instance]` puts an instance (or all of
  them) back as the world starts.

Without the pack on the server, each player's game runs the gameplay on its
own, unsynchronised, as before.

## Files

A Build Mode world is a content pack folder:

```text
content/my-town/
  content.json        # name, id, licence, spawn, "world": { "layout": "layout.json" }
  layout.json         # the placed objects (this format)
  assets/<category>/  # models you imported (File → Import model)
  models/*.model.json # My Models: saved models, each a layout with one model group
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
- A node without `asset` is a group. Groups pass Visible, Locked and
  Collision down to their contents. A group's `transform` is its pivot:
  its contents' transforms are relative to it (position and rotation; a
  group has no scale of its own, so `scale` stays 1). A group without a
  pivot has no position or rotation. `"model": true` makes a click select
  the whole group. `Layout::world_transform` gives any object's place in the
  world, and `Layout::resolved` the whole layout in world space, which is
  what packs, playtests, servers and scripts play.
- Ids are stable and never reused, even after undo.
- An `interact` behaviour can have `"vehicles": true` to work for drivers
  too (left out: on foot only).
- `terrain` is optional: `{ "size": 64, "cells": 32, "heights": [...],
  "color": [0.36, 0.5, 0.27] }` with (cells + 1)² heights in metres, row by
  row from south to north (`sa_scene::terrain`). Up to 128 cells and
  ±200 m. Optional `"paint"`: cells² material indexes, rows south to north
  (0 Ground, 1 Grass, 2 Dirt, 3 Sand, 4 Rock, 5 Snow, 255 a hole).
- `road` is optional: `{ "points": [[-24, 0, 0], [0, 6, 0], [24, 0, 0]],
  "width": 8 }`, 2-256 points in the object's own metres (east, north, up),
  each coordinate within ±2000 m, at least 0.5 m long, width 1-40 m
  (`sa_scene::road`).
- `part` is optional: `{ "shape": "block", "size": [4, 2, 1],
  "color": [0.64, 0.64, 0.66], "material": "brick", "transparency": 0.2 }`
  (`sa_scene::part`). Shapes `block`, `ball`, `cylinder`, `wedge`,
  `corner_wedge`; materials `plastic`, `smooth`, `concrete`, `wood`,
  `brick`, `tiles`, `metal`, `glass`. Size 0.05-2048 m per side in the
  object's own axes, colour linear RGB 0-1, transparency 0-1. The object
  shows the part instead of its asset (`builtin:part`); a shape or
  material this SARE does not know is kept and drawn as a plastic block.
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
  message instead of being half read. Version 2 added group pivots; a world
  that uses none is still written as version 1, so older SARE versions
  open it.
- Saves are atomic (write to a temporary file, then rename). Autosave runs
  15 seconds after a change and when SARE closes; after a crash Build Mode
  offers to recover the autosave.

## Architecture

Build Mode reuses the existing engine; there is no second renderer, asset
pipeline or project format.

| Part | Where | Notes |
| --- | --- | --- |
| Layout format | `sa-scene::world_layout` | Serde types, validation, migration hook, atomic writes, Blender-to-glTF transform maths |
| Publish | `sa-editor::publish` | `to_server` copies the saved project into `content/.<id>-publishing`, checks it opens, swaps it in, and sets the one `content_pack` line in `server.cfg` |
| Local server test | `build_mode/local_test.rs` | `prepare` writes `content/<id>` (via `Project::save_as` with the same id), `resources/` and `test.cfg`; `sa-server +exec test.cfg` and `sa-runtime --play --content <id> --content-dir … --join 127.0.0.1:<port> --name "Player n"` run beside SARE; their output goes to Output through a channel |
| Physics | `sa-scene::physics`, `world_logic::Shared::settle` | `Physics::new` takes the solid, fixed objects (`place_layout`) as a `CollisionWorld` and each unanchored part's box; `settle` lands parts lowest first (so they stack) and starts a `Motion` with `fall` (gravity easing, sent as `g` in snapshots). `Logic::advance` and the server's loop call it |
| Command bar | `sa-editor::command_bar`, `build_mode/command.rs` | Runs the line in a `sa_lua::world` host built from the resolved layout (everything changeable), then turns its requests and changed attributes/tags into one `Command::Batch`; copies are pasted first so later requests find them |
| Arranging | `build_mode/arrange.rs` | `nudge`, `align`, `distribute` on world boxes from the GPU meshes (a group: everything in it); one `Command::Modify` each, written back relative to each object's group |
| Attributes and tags | `world_layout::Attribute`, `Node.attributes`/`tags`, `build_mode/attributes_ui.rs`, `sa-lua` world API | `Layout::effective_attributes`/`effective_tags` merge a node's groups'; `world_logic::script_objects` hands them to scripts; unknown kinds from a newer SARE are kept (`Attribute::Other`) and hidden from scripts |
| My Models | `sa-scene::saved_model`, `sa-editor::ops`, `build_mode/saved_models.rs` | `SavedModel::capture` takes a model out of the layout (pivot at the origin, ids from 1), `matches` compares a copy with the file; `AssetResolver` draws a saved model as one model (Toolbox picture, placement preview); `ops::place_saved` and `ops::update_copies` place copies and replace their contents as one undo step; a node's `saved_model` names its file |
| Parts | `sa-scene::part` | `model` builds a part's shape (origin in the middle of its base, faces outwards, closed), baked side shading and planar texture coordinates in metres; `texture` generates each material's pattern in code. The editor uploads one mesh per look (`gpu::mesh_key`), so parts that look the same share it and script colours tint it |
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

- My Models shared between worlds (today each world has its own), copies
  that keep some of their own changes when the others are updated, and
  saved models inside saved models (they are saved as plain models).
- Dragging a model's pivot in the world (today: type it in, or Centre
  pivot), and scaling a whole model to a size.
- Multiplayer: server checks of teleport destinations against the player's
  later position.
- More actions (sounds, items). World scripts react to zones and move
  objects today; see [World scripts](world-scripts.md) for what they cannot
  do yet.
- A wireframe view of the exact collision triangles (the Collision view
  outlines object bounds).
- Spot lights, shadows from lamps, and per-object shadow switches (lamps
  light their surroundings per pixel, without shadows).
- Terrain bigger than one object (place several side by side today), and
  textured materials (today: colours).
- Roads: junctions where roads meet, and road surfaces that follow the
  terrain under them by themselves (today: set the points' heights).
- Visual scripting (blocks instead of Lua), publishing worlds to servers.
- Vehicles: driving a newly imported vehicle in playtest without saving
  and playing the world, parked vehicles placed in the world, and boats.
