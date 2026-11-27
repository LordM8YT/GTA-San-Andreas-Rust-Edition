# The SARE creator handbook

This handbook is the place to start if you want to make something for SARE:
a prop, a building, a whole world, a car, a character or a server game mode.
It explains what you can make today, which tools to use, and walks you
through your first project step by step. The detailed format references are
linked at each step.

SARE is a free engine and modding platform. It does not ship any game
content of its own from Rockstar: when you play on the San Andreas map, SARE
reads that map from **your own** installation. Your creations can sit on top
of that map, or be completely standalone worlds that run without any game
files at all.

## Contents

1. [What can I make?](#1-what-can-i-make)
2. [Tools you need](#2-tools-you-need)
3. [Where things go](#3-where-things-go)
4. [Tutorial: your first prop in 5 minutes](#4-tutorial-your-first-prop-in-5-minutes)
5. [Tutorial: your own world, no GTA needed](#5-tutorial-your-own-world-no-gta-needed)
6. [Tutorial: a server command in Lua](#6-tutorial-a-server-command-in-lua)
   - [6b. Your own RP job](#6b-your-own-rp-job)
7. [Blender export cheat sheet](#7-blender-export-cheat-sheet)
8. [Coordinates and placing things](#8-coordinates-and-placing-things)
9. [Collision: what is solid](#9-collision-what-is-solid)
10. [Bringing in existing mods](#10-bringing-in-existing-mods)
11. [Sharing your work](#11-sharing-your-work)
12. [Troubleshooting](#12-troubleshooting)
13. [Reference documents](#13-reference-documents)

## 1. What can I make?

| You want to make | Make a… | Needs GTA SA installed to play? | Guide |
| --- | --- | --- | --- |
| A prop, sign, building or ramp on the San Andreas map | **Mod** with a `.glb` (or DFF) model | Yes | [Tutorial 4](#4-tutorial-your-first-prop-in-5-minutes), [native mods](native-mods.md) |
| A whole new map or level, built inside SARE | **Content pack** made in **Build Mode** | **No** | [Tutorial 5](#5-tutorial-your-own-world-no-gta-needed), [Build Mode](build-mode.md) |
| A whole new map or level, modelled in Blender | **Content pack** with a `.glb` world | **No** | [Tutorial 5](#5-tutorial-your-own-world-no-gta-needed), [content packs](content-packs.md) |
| Your own car, truck or van for a SARE world or server | `.glb` vehicle in a content pack (`vehicles`) or a server resource (`addon_vehicles`) | **No** | [content packs → Vehicles](content-packs.md#vehicles), [addon vehicles](native-mods.md#addon-vehicles-gltf-cars-for-sare-servers) |
| A custom car | **Mod** with a DFF/TXD vehicle and handling | Yes | [creating resources](creating-resources.md) |
| A player character or clothes | **Mod** with skinned DFF + IFP animations | Yes | [native mods](native-mods.md) |
| Different car handling | **Mod** with `original_vehicle_handling` | Yes | [vehicle dynamics](vehicle-dynamics.md) |
| A race or stunt map from MTA | Drop the MTA folder into `mods/` | Yes | [native mods → MTA maps](native-mods.md#mtasa-maps) |
| Commands, game modes, chat, instances | **Server resource** in Lua | Players: depends on the world | [Tutorial 6](#6-tutorial-a-server-command-in-lua), [server guide](server-guide.md) |
| Logic inside your world: doors, lifts, scores, events | **World script** in Lua, written in Build Mode | **No** | [world scripts](world-scripts.md) |
| On-screen UI and client logic | **Client script** in Lua | Depends on the world | [client scripts](client-scripts.md) |

**Mods** and **content packs** are data: models, textures and JSON. They never
run code. **Server resources** are Lua scripts that run on a server (or in a
sandbox on the player's PC for client scripts). SARE never runs ASI/DLL
plugins or CLEO scripts.

## 2. Tools you need

- **SARE** itself: the launcher from the
  [latest release](https://github.com/LordM8YT/GTA-San-Andreas-Rust-Edition/releases/latest).
- **Nothing else to build a world**: SARE's own **Build Mode** editor places
  ready-made pieces (ground, roads, walls, stairs, houses, trees and more)
  and your own models.
- **Blender** (free, [blender.org](https://www.blender.org/)) for your own
  models, and for worlds you would rather model by hand. Any tool that exports **glTF 2.0 binary (`.glb`)** works too.
- **A text editor** for JSON and Lua, for example VS Code or Notepad++.
- **Python 3** (optional) to run the helper tools in `tools/`, such as
  `new-resource.py`, which creates a working starting point for you.
- **An AI assistant** (optional): SARE's creator bridge `sa-mcp` lets Claude
  and other MCP apps build Build Mode worlds and write Lua resources for you.
  See [Make worlds and scripts with an AI assistant](ai-creators.md).

You do **not** need RenderWare tools, a modding SDK, or a compiler.

## 3. Where things go

Next to the launcher (or in the repository root when you build from source):

```text
SARE/
  sa-launcher(.exe)
  sa-runtime(.exe)
  mods/                      ← mods for the San Andreas map
    [props]/my_crate/
      resource.json
      stream/prop.glb
  content/                   ← standalone worlds (content packs)
    my-world/
      content.json
      world.glb
  server-data/               ← only on a dedicated server
    server.cfg
    resources/[local]/my_script/
      fxmanifest.lua
      server.lua
```

- Folders in square brackets, like `[props]`, are just categories to keep
  things tidy. Use as many as you like.
- The launcher creates `mods/` and `content/` the first time it starts.
- A mod's manifest can be called `resource.json` or `mod.json`; they are the
  same format.

## 4. Tutorial: your first prop in 5 minutes

You will put a crate on Grove Street, then replace it with your own model.

**Step 1. Create the prop.** In the SARE folder, run:

```text
python tools/new-resource.py my_crate --type prop
```

This creates `mods/[props]/my_crate/` with a working `resource.json` and a
starter model `stream/prop.glb` (a 1 m crate). Without Python, create the
folder and file by hand:

```json
{
  "schema_version": 2,
  "enabled": true,
  "name": "my_crate",
  "models": [{ "id": 30000, "model": "stream/prop.glb" }],
  "placements": [{ "model_id": 30000, "position": [2495.0, -1665.0, 13.34] }]
}
```

**Step 2. See it.** Start free roam. The crate stands a few steps from where
you start. The pause menu's local-resources list shows `my_crate`.

**Step 3. Make it yours.** Open Blender, delete the default cube, then
*File → Import → glTF 2.0* and pick `stream/prop.glb`. Model your prop. Keep
the object's origin where it touches the ground, and model at real size
(1 unit = 1 metre).

**Step 4. Export.** *File → Export → glTF 2.0*, set **Format** to
**glTF Binary (.glb)**, keep **+Y Up** ticked, and save over
`stream/prop.glb`. Restart the game to see it.

**Step 5. Place more.** Add more entries to `placements`, each with its own
position. To add a second, different model, add another entry to `models`
with a new ID (30001, 30002, …) and its own `.glb`.

Tips:

- Pick IDs of 30000 and up, and keep them unique across your mods. An ID that
  already exists in San Andreas **replaces** that original object everywhere
  it appears, which is how you swap a vanilla prop for your own.
- Textures: use an *Image Texture* node on the material's **Base Color**
  with a PNG image. The exporter packs it into the `.glb`.
- Rotation: add `"rotation": [x, y, z, w]` (a quaternion, as in GTA IPL
  files) to a placement. `[0, 0, 0.7071, 0.7071]` turns it a quarter turn
  around the vertical axis.

## 5. Tutorial: your own world, no GTA needed

You will make a world that anyone can play without owning San Andreas. There
are two ways, and you can mix them: build it inside SARE with **Build Mode**
(fastest, no other tools), or model it in **Blender**.

### 5a. The fast way: Build Mode

**Step 1. Create the world.** Open the launcher. On **Home**, type a name
under **Build Mode** and press **Create world**. Build Mode opens with a
ground plate and a spawn point.

**Step 2. Place things.** The **Toolbox** on the left shows a picture of
every piece: ground and grass plates, roads, floors, walls, stairs, ramps,
houses, trees, crates, benches, lamp posts, plus the models of every content
pack you have installed. To place one:

- drag its picture into the world and let go, **or**
- click it, then click in the world. Every click places another one, so a
  row of trees is one click per tree. Right-click or Esc stops.

`[` and `]` turn the piece before you place it. With **Surface** on (the
default), pieces land on whatever is under the mouse, so a house can stand on
a platform. With **Snap** on, they line up on a 1 m grid (change the size in
the ribbon's Model tab).

**Step 3. Arrange.** Click an object to select it. Move (W), Rotate (E) and
Scale (R) give you handles to drag, or type exact numbers in the
**Properties** on the right. Ctrl+D duplicates, Delete removes, Ctrl+Z undoes
anything. The **Explorer** on the right lists every object: rename, group, hide
and lock them there.

**Step 4. Your own models.** Drag a `.glb` file from your file explorer onto
the SARE window, or use **Import** on the ribbon (or **File → Import model**) to pick it with your system's file picker. It is copied into your world,
so the world stays one folder you can share. Make the model in Blender as in
[Tutorial 4](#4-tutorial-your-first-prop-in-5-minutes).

**Step 5. Play it.** Press **F6** to walk around your world right away, and
F6 again to go back to editing. Move the green spawn marker with **Set spawn
at camera focus** in the Properties panel (click empty space or **Workspace** in the Explorer
first), or with **Set spawn** in the ribbon's Test tab.

**Step 6. Save.** Ctrl+S. Your work is also autosaved, and offered back if
SARE closes unexpectedly. The world is now a normal content pack in
`content/<your-world>/`: it shows up under **Content packs** on Home to play,
with an **Edit** button to open it in Build Mode again. Inside Build Mode, *File → New world* and *File → Open world* switch
worlds without going back to the launcher.

Every control and the file format are in the [Build Mode guide](build-mode.md).

### 5b. The Blender way

**Step 1. Create the pack.**

```text
python tools/new-resource.py my-world --type world
```

This makes `content/my-world/` with `content.json` and a starter
`world.glb`: a 200 m paved square with a block, a ramp and invisible walls at
the edge. (Or copy [`content/example-town`](../content/example-town).)

**Step 2. Play it.** Open the launcher. On **Home**, under **Content packs**,
press **My World** (press **Refresh** first if the launcher was already
open). Walk around, jump with Space, press F9 for SARE Kart and F to get in.

**Step 3. Build.** Import `world.glb` into Blender and build your map. Some
advice:

- Start with the ground and big shapes; test early and often.
- Blender X is east, Y is north, Z is up. The origin is the middle of your
  world.
- Join objects that share a material (Ctrl+J). Fewer materials means fewer
  draw calls and better speed on weak PCs.
- Keep it to tens of thousands of triangles for now; packs load whole.

**Step 4. Export** over `world.glb` exactly as for props, then press the
button in the launcher again.

**Step 5. Fill in `content.json`.** Set `name`, `author`, `description` and,
most important, `license`, and move `world.spawn` (`[east, north, up]`, in
metres) to where players should start.

**Mixing both:** press **Edit** next to your pack to open it in Build Mode.
Your Blender `world.glb` stays as the base, and the pieces you place are
saved beside it in `layout.json`.

The full format, limits and tips are in [content packs](content-packs.md).
Right now a pack holds a world; your own characters, vehicles and sounds in
packs are next on the [roadmap](STANDALONE.md).

## 6. Tutorial: a server command in Lua

Server resources use a FiveM-style layout, so many FiveM tutorials apply.

**Step 1.** In `server-data/resources/[local]/`, create a folder `hello` with
two files.

`fxmanifest.lua`:

```lua
fx_version 'cerulean'
game 'common'
lua54 'yes'

server_script 'server.lua'
```

`server.lua`:

```lua
RegisterCommand('hello', function(source, args)
  local name = GetPlayerName(source)
  TriggerClientEvent('chat:addMessage', -1, {
    color = { 120, 200, 255 },
    args = { 'Server', ('%s says hello!'):format(name) },
  })
end, false)
```

**Step 2.** Add `ensure hello` to `server.cfg`, or type `ensure hello` in the
server console. Join and type `/hello` in chat.

**Step 3.** Go further with the examples that ship with the server:
`[examples]/sare_vehicles` (spawn cars with `/car`, delete with `/dv`) and
`[examples]/sare_instances` (private copies of the world with `/instance`).

Read the [server guide](server-guide.md) for setup and the
[server API](server-hosting.md) for every supported function. Not every FiveM
or GTA V native exists; [framework compatibility](framework-compatibility.md)
lists what does.

### 6b. Your own RP job

On a SARE RP server (the **SARE RP Town** template) a job is one resource
folder, the way FiveM jobs are: the job, its places on the map and what
happens there all live in that folder. You never edit the core files.

**Step 1.** Make the job from the example courier job:

```text
python tools/new-resource.py fisher --type rp-job --resources-dir "<your server>/resources"
```

This creates `resources/[jobs]/fisher/` with `fxmanifest.lua`, `config.lua`,
`server.lua` and a README. It works at once: add `ensure fisher` to
`server.cfg` after `ensure [sare-rp]` and restart, take the job at City Hall,
go on duty at its depot, load a package and deliver it.

**Step 2.** Put the places on your map. `config.lua` has the positions:

```lua
Config.Locations = {
  duty = vector4(2352.50, -1806.00, 0.20, 0.0),   -- x, y, z, heading
  pickup = vector4(2375.00, -1815.00, 0.20, 0.0),
  dropoffs = { vector4(...), vector4(...) },       -- a list works too
}
```

These are world coordinates, like in FiveM scripts. Two ways to get them:

- **In game:** stand on the spot and type `/coords`. Chat shows
  `vector4(...)` with your heading; the server console prints it too, to copy.
- **In Build Mode:** select an object where the spot should be (a small cube,
  a sign, a door) and press **Copy position** in the Properties panel. The
  `vector4(...)` is now on your clipboard; paste it into `config.lua`. The
  heading is the object's Z rotation.

**Step 3.** Make it your job. `config.lua` holds the job's name, grades,
paycheck and pay. `server.lua` holds what happens: each place has an `onUse`
function that runs when a player presses **H** on it.

```lua
-- The job, with grades like QBCore's shared jobs.
exports.sarebox:AddJob('fisher', {
  label = 'Fisher', defaultDuty = false,
  grades = { [0] = { name = 'Deckhand', payment = 40 } },
})
-- Anyone may take it at City Hall's job centre.
exports.sare_rp_core:AddOpenJob('fisher')
-- A point on the map: [H] hint, blip, job and duty checks, and what it does.
exports.sare_rp_core:AddPlace('fisher_pier', {
  label = 'Fishing spot',
  coords = Config.Locations.pier,         -- vector3, vector4 or a list
  blip = 'Fishing', colour = 3,           -- optional map blip
  job = 'fisher', duty = true,            -- optional: only on-duty fishers
  radius = 3.0,                           -- optional, metres
  onUse = function(src, key, point)
    -- give fish, start a progress bar, pay... (see the courier's server.lua)
  end,
})
```

Helpers from `exports.sare_rp_core:GetRP()`: `Notify`, `Menu`, `Dialog`,
`Progress`, `Near`, `OnDuty`, `Coords`, `Distance`. Money and jobs come from
SARE Box: `exports.sarebox:GetCoreObject()` (see [SARE Box](sarebox.md)).

The job tools in `exports.sare_rp_jobs` do the parts most jobs share:

| Export | Does |
| --- | --- |
| `AddService(name, { label, colour, jobs })` | `/call <name>` (and `/911` for `police`) reaches on-duty players of those jobs |
| `AllowBills(job)` | The job's F2 menu gets **Bill nearest person**, and `/bill` works |
| `AddJobAction(job, { id, title, target, run })` | An entry in the job's **F2** menu; `run(src, target)` gets the nearest player when `target = true` |
| `DutyMenu(src, job, placeKey, extraOptions)` | The on/off duty menu for a duty point |
| `Gear(src, { item = count })` | Locker gear, once every ten minutes |
| `Call(src, service, text)`, `Nearest(src, radius)`, `Close(src, target, radius)` | Dispatch and distance checks |

The built-in jobs use exactly these, so they are full examples to copy:
`sare_job_police` (cuffs, search, fines, jail), `sare_job_ems` (treatment,
hospital), `sare_job_mechanic` and `sare_job_taxi`, each in
`server-data/resources/[sare-rp]/`.

The server checks everything: a player can only use a place they stand at,
with the right job and duty. Places go away when their resource stops, and
players keep a custom job even if its resource starts after they joined.

## 7. Blender export cheat sheet

*File → Export → glTF 2.0 (.glb/.gltf)*:

| Setting | Use |
| --- | --- |
| Format | **glTF Binary (.glb)** |
| Include → Limit to | *Selected Objects* if you only want part of the scene |
| Transform → +Y Up | **On** (default) |
| Data → Mesh → Apply Modifiers | On |
| Data → Mesh → UVs, Normals | On |
| Data → Mesh → Vertex Colors | On if you painted colours |
| Material → Images | Automatic (PNG or JPEG textures) |
| Animation | Off (not used yet) |

What SARE reads from a `.glb`: meshes, object transforms, Base Color colour
and PNG or JPEG texture, vertex colours, and Alpha Blend/Clip. It ignores (without
errors) armatures, animations, shape keys, normal maps and other material
channels.

## 8. Coordinates and placing things

- **On the San Andreas map** (mods): positions are GTA coordinates
  `[x, y, z]`: x east, y north, z up, in metres. Grove Street is around
  `[2495, -1665, 13.3]`. The pause menu map shows where you are.
- **In a content pack**: positions are Blender coordinates around your
  world's origin, `[east, north, up]`. Build Mode's Properties shows the same
  numbers (X east, Y north, Z up).
- **In server scripts** (RP jobs, spawn points): world coordinates. A
  content pack's origin is world `(2500, -1670, 0)`, so Build Mode's
  `X, Y, Z` is world `2500 + X, -1670 + Y, Z`. Build Mode's **Copy position**
  does this for you and copies a ready `vector4(...)`.
- **Inside a model**: the object's origin is the point that is placed.

## 9. Collision: what is solid

The same rules work for props and worlds made from `.glb` files:

| Object name in Blender | Drawn | Solid |
| --- | --- | --- |
| Anything | yes | yes |
| `COL_…` | no | yes |
| `NOCOL_…` | yes | no |
| Material with Alpha Blend/Clip | yes (see-through) | no |

Use `COL_` boxes as simple invisible walls or as a cheap collision shell
around a detailed model. Use `NOCOL_` for grass, wires and small decoration.
Mods can also use a `col` file instead, as for DFF models.

## 10. Bringing in existing mods

- **Classic GTA SA mods** (DFF/TXD/COL, IDE/IPL, Mod Loader folders): convert
  them once with the [GTA SA importer](gta-sa-conversion.md).
- **MTA:SA maps**: unzip the map folder (with its `meta.xml`) into `mods/`.
  See [MTA maps](native-mods.md#mtasa-maps).
- **FiveM / GTA V assets**: the experimental
  [GTA V converter](gta5-conversion.md) handles a subset.

Only bring in mods you have permission to use and share.

## 11. Sharing your work

- **Zip the folder** (`mods/[props]/my_crate` or `content/my-world`) and
  share it. Others unzip it into the same place.
- **On a server**, put mods in `server-data/resources/[local]/` and `ensure`
  them: players download them automatically when they join. Restart the
  server after changing shared models.
- **Licences**: say who may reuse your work. `CC0-1.0` (public domain) and
  `CC-BY-4.0` (credit required) are common for game assets. Credit the
  sources of anything you did not make yourself.
- **Never include** files from GTA San Andreas or other Rockstar games, or
  anything converted, extracted or upscaled from them. Those belong to their
  owners, and SARE always reads them from each player's own copy.

## 12. Troubleshooting

Start with the log: the newest `runtime-*.log` in the `logs` folder of SARE's
settings folder. Errors that stop the game from starting are also shown in
the launcher.

| Problem | What to check |
| --- | --- |
| My mod is not in the local-resources list | Is `"enabled": true`? Is the folder directly in `mods/` or inside `[category]` folders (up to four levels)? Is the JSON valid (no trailing commas)? |
| `unknown field` error | A typo in a field name. Compare with the examples here. |
| My model is invisible | Did you export **glTF Binary**? Is it at the right place and height? Are faces inside out (Blender: *Mesh → Normals → Recalculate Outside*)? |
| My model is black or white | The material has no Base Color texture, or the texture is not PNG or JPEG (WebP, KTX2). |
| I fall through my world | The ground must be a closed, opaque mesh not named `NOCOL_`; check `world.spawn` is above it. Test with `sa-runtime --probe-standalone --content my-world`. |
| An invisible wall blocks me | An object named `COL_…`, or a big transparent-looking mesh that is still opaque. |
| My pack is not in the launcher | `content.json` must be directly inside `content/<folder>/`, and `id`, `name`, `version` and `license` must be set. Press **Refresh**. |
| Build Mode shows a magenta cube | That object's model is missing or broken. The **Errors and warnings** tab at the bottom says why; the model's file may have been moved or renamed. |
| A dropped or imported model is not accepted | Only `.glb` (glTF Binary) works for import; the Console tab says what is wrong with the file. |
| It runs slowly | Fewer materials (join objects), fewer triangles, smaller textures (2048 px is plenty). |

Still stuck? Ask on the SARE Discord with your log and your manifest.

## 13. Reference documents

- [Build Mode](build-mode.md): the world editor, its controls and `layout.json`
- [Content packs](content-packs.md): standalone worlds, full `content.json` reference
- [Native mods](native-mods.md): `mod.json`/`resource.json`, models, vehicles, players, clothing, MLO interiors, MTA maps
- [Creating resources](creating-resources.md): folder layout and the resource generator
- [Vehicle dynamics](vehicle-dynamics.md): handling values
- [Server guide](server-guide.md) and [server API](server-hosting.md)
- [Client scripts](client-scripts.md)
- [AI assistants (sa-mcp)](ai-creators.md): let an AI build worlds and scripts through MCP
- [GTA SA importer](gta-sa-conversion.md) and [GTA V converter](gta5-conversion.md)
- [Standalone plan](STANDALONE.md) and [platform roadmap](PLATFORM.md): what comes next
