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
| A whole new map or level | **Content pack** with a `.glb` world | **No** | [Tutorial 5](#5-tutorial-your-own-world-no-gta-needed), [content packs](content-packs.md) |
| A custom car | **Mod** with a DFF/TXD vehicle and handling | Yes | [creating resources](creating-resources.md) |
| A player character or clothes | **Mod** with skinned DFF + IFP animations | Yes | [native mods](native-mods.md) |
| Different car handling | **Mod** with `original_vehicle_handling` | Yes | [vehicle dynamics](vehicle-dynamics.md) |
| A race or stunt map from MTA | Drop the MTA folder into `mods/` | Yes | [native mods → MTA maps](native-mods.md#mtasa-maps) |
| Commands, game modes, chat, instances | **Server resource** in Lua | Players: depends on the world | [Tutorial 6](#6-tutorial-a-server-command-in-lua), [server guide](server-guide.md) |
| On-screen UI and client logic | **Client script** in Lua | Depends on the world | [client scripts](client-scripts.md) |

**Mods** and **content packs** are data: models, textures and JSON. They never
run code. **Server resources** are Lua scripts that run on a server (or in a
sandbox on the player's PC for client scripts). SARE never runs ASI/DLL
plugins or CLEO scripts.

## 2. Tools you need

- **SARE** itself: the launcher from the
  [latest release](https://github.com/LordM8YT/GTA-San-Andreas-Rust-Edition/releases/latest).
- **Blender** (free, [blender.org](https://www.blender.org/)) for models and
  worlds. Any tool that exports **glTF 2.0 binary (`.glb`)** works too.
- **A text editor** for JSON and Lua, for example VS Code or Notepad++.
- **Python 3** (optional) to run the helper tools in `tools/`, such as
  `new-resource.py`, which creates a working starting point for you.

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

You will make a world that anyone can play without owning San Andreas.

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
| Material → Images | Automatic, with PNG textures |
| Animation | Off (not used yet) |

What SARE reads from a `.glb`: meshes, object transforms, Base Color colour
and PNG texture, vertex colours, and Alpha Blend/Clip. It ignores (without
errors) armatures, animations, shape keys, normal maps and other material
channels. JPEG textures are skipped: save your images as PNG.

## 8. Coordinates and placing things

- **On the San Andreas map** (mods): positions are GTA coordinates
  `[x, y, z]`: x east, y north, z up, in metres. Grove Street is around
  `[2495, -1665, 13.3]`. The pause menu map shows where you are.
- **In a content pack**: positions are Blender coordinates around your
  world's origin, `[east, north, up]`.
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
| My model is black or white | The texture is JPEG (use PNG), or the material has no Base Color texture. |
| I fall through my world | The ground must be a closed, opaque mesh not named `NOCOL_`; check `world.spawn` is above it. Test with `sa-runtime --probe-standalone --content my-world`. |
| An invisible wall blocks me | An object named `COL_…`, or a big transparent-looking mesh that is still opaque. |
| My pack is not in the launcher | `content.json` must be directly inside `content/<folder>/`, and `id`, `name`, `version` and `license` must be set. Press **Refresh**. |
| It runs slowly | Fewer materials (join objects), fewer triangles, smaller textures (2048 px is plenty). |

Still stuck? Ask on the SARE Discord with your log and your manifest.

## 13. Reference documents

- [Content packs](content-packs.md): standalone worlds, full `content.json` reference
- [Native mods](native-mods.md): `mod.json`/`resource.json`, models, vehicles, players, clothing, MLO interiors, MTA maps
- [Creating resources](creating-resources.md): folder layout and the resource generator
- [Vehicle dynamics](vehicle-dynamics.md): handling values
- [Server guide](server-guide.md) and [server API](server-hosting.md)
- [Client scripts](client-scripts.md)
- [GTA SA importer](gta-sa-conversion.md) and [GTA V converter](gta5-conversion.md)
- [Standalone plan](STANDALONE.md) and [platform roadmap](PLATFORM.md): what comes next
