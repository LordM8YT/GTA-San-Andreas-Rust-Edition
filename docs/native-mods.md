# Native mod resources

The Rust free-roam runtime loads `mods/<resource>/mod.json` or `resource.json`.
FiveM-style category folders such as `mods/[vehicles]/my_car/` are also supported,
including up to four nested bracket categories. Load order is an alphabetical
walk through each category; resource folder names must be unique across categories.
If both manifests exist, `mod.json` takes priority. The local-resources menu lists detected enabled and disabled
resources. Set `enabled` to `true` or `false` in the manifest and restart to
change a resource's state. The launcher starts from the project root so the
project's `mods/` folder is found. Use `--no-mods` to disable mods and
`--mods-dir PATH` to select another root folder. A later resource replaces an
earlier definition with the same model ID.

## Formats and compatibility

This runtime supports its documented `mod.json` resource schema and the asset
formats described below: PC RenderWare DFF models, TXD textures (and PNG
texture overrides), COL collision files, placements, MTA:SA `.map` object
placements, and the documented
vehicle, player, and clothing resources. It does not load arbitrary original
San Andreas mods directly. Binary plugins, ASI/DLL mods, CLEO scripts, original
game-code or plugin APIs, and unconverted archives/workflows such as IMG/IPL
are not drop-in compatible. Convert assets into a supported resource or add
explicit format support; scripts and DLLs are not executed. FiveM asset resources and most GTA V APIs are not drop-in compatible. SARE's
separate server scripting layer supports a documented subset of the FiveM resource
and CfxLua workflow; see [framework compatibility](framework-compatibility.md).
The native asset loader uses its own JSON manifest and does not parse
`fxmanifest.lua`; the dedicated server reads fxmanifest metadata to manage server
resources. GTA V `.meta` files are not parsed, and arbitrary executable plugins
or CLEO scripts are not run.
The offline importer inspects resource manifests as text and converts supported assets;
see [GTA V / FiveM conversion](gta5-conversion.md). For classic San Andreas packages, use the
[GTA SA mod importer](gta-sa-conversion.md): it can read loose DFF/TXD/COL/IFP assets,
uncompressed VER2 IMG entries, selected IDE declarations, text IPL placements, standalone
binary IPL instance tables, and text IPL ENEX portals. It preserves interior IDs and creates
native interior_portals entries for supported ENEX transitions. It creates a native JSON
resource and a server-facing fxmanifest.lua; this is a conversion workflow, not direct
execution of old mod-loader code. Compressed IMG archives, automatic room-boundary discovery,
arbitrary binary IPL sections, ASI/DLL plugins and CLEO scripts remain unsupported. Use
explicit mlo metadata when a building needs room-based visibility.

## Create a resource

From the project root, with Python 3 installed:

```text
python tools/new-resource.py my_car --type vehicle
python tools/new-resource.py my_building --type map
python tools/new-resource.py my_character --type player
python tools/new-resource.py my_outfit --type clothing
```

The generator creates a disabled resource and never overwrites an existing
resource. `--mods-dir PATH` selects another mod directory. For example:

```text
mods/[vehicles]/my_car/
  resource.json
  README.md
  stream/                 # Your DFF/TXD assets
  data/README.md          # Optional authoring/source files
```

Add the assets listed in the generated README, register optional TXD/COL paths,
then set `enabled` to `true` and restart. Paths in the manifest are relative to
the resource root, for example `stream/car.dff`. Model registration and map
placements currently remain in `resource.json`; putting a GTA V metadata file
in `data/` does not make it load. A clothing template includes its base player
because clothes currently attach to a matching custom player skeleton.

This takes inspiration from the [FiveM resource workflow](https://docs.fivem.net/docs/scripting-reference/resource-manifest/)
and [stream folder layout](https://docs.fivem.net/docs/assets-manual/beginner-series/part-4/).

Original game assets are read from the user's installation. Do not redistribute
Rockstar assets or converted/upscaled copies in project releases. Share your
own assets and manifests instead.


```json
{
  "schema_version": 2,
  "enabled": true,
  "name": "My building",
  "models": [
    {
      "id": 30000,
      "dff": "building.dff",
      "txd": "building.txd",
      "col": "building.col",
      "map_doors": [
        { "frame": "security_door_leaf", "door": 0 },
        { "frame": "garage_gate", "door": 1, "pivot": [0.0, 0.0, 0.0] }
      ]
    }
  ],
  "placements": [
    { "model_id": 30000, "position": [2500, -1670, 12.35], "rotation": [0, 0, 0, 1] }
  ],
  "exclude_model_ids": [],
  "texture_overrides": { "cj_bins:miamibin": "textures/my.png" }
}
```

Positions use the original GTA XYZ coordinates. Quaternions use the IPL convention.
Choose an unused ID, such as one starting at 30000. An existing ID replaces the
original model at its existing map placements. Model files must be local to the
resource folder. DFF files without textures do not need `txd`.

Rigid PC RenderWare DFFs are supported for map models. Skinned PC DFFs are
supported for `player`, as described below. Native and multimorph geometry are
not supported. TXD files use the D3D9 decoders. PNG images can be up to
4096 × 4096. A COL file must contain exactly one model. Without a COL file,
visible opaque triangles are used as a collision fallback. With a COL file,
its collision geometry is used. The loader preserves map DFF atomics and named
door pivots. Common GTA SA door/gate frame names are detected automatically; press
**E** within 3 metres of a detected hinge to open or close it. For a non-standard
frame, add map_doors to that model as shown above. frame must exactly match an
atomic frame name reported by the map DFF audit; door is 0-3; optional pivot
overrides the DFF-local hinge position. Opening a door releases matching vertical
COL faces inside the moving mesh bounds while keeping floors and ceilings solid.
This is a geometry-based match, so validate the actual doorway and collision in-game.

Native mlo metadata can describe SA-compatible interior instances, entities,
rooms, and open/closed portal visibility. It does not decode GTA V binary MLO assets
or infer room boundaries from a DFF. Keep exterior shell models as unassigned
entities and put separate interior geometry models in rooms. Each entity's
model_id must be declared in the same resource; its position and rotation are
local to the MLO instance.

`placements` can reference an original model ID without a `models` entry.
Texture overrides use the `dictionary:texture` key format. Legacy manifest
settings are not part of the native schema.

A GTA SA importer-generated interior_portals entry uses world-space coordinates
and the target interior ID; supported ENEX records also create the reverse exit
side. For room-by-room MLO visibility, provide explicit room metadata:

```json
{
  "mlo": [{
    "name": "police_station",
    "position": [1030.0, -1987.05, 14.53],
    "rotation": [0.0, 0.0, 0.0, 1.0],
    "interior": 0,
    "entities": [
      { "model_id": 30000, "room": null },
      { "model_id": 30001, "position": [0.0, 0.0, 0.0], "room": 0 }
    ],
    "rooms": [{
      "name": "lobby",
      "min": [-8.0, -6.0, -1.0],
      "max": [8.0, 6.0, 5.0],
      "entities": [1]
    }],
    "portals": []
  }]
}
```

Room bounds and portal corners are MLO-local. Use a separate shell entity (room
omitted or null) for exterior geometry and assign only interior geometry to
room indices. Portal records connect room_from and room_to with local-space
polygon corners; their open field defaults to true. The importer can embed
an explicit JSON sidecar with tools/import-fivem.py --mlo-metadata; it does not
guess rooms from GTA V assets or legacy SA DFF geometry.

`mods/native-room-demo` contains a custom DFF room with a door, floor, ceiling,
walls, and bench. Set `enabled` to `true` to place it on Grove Street. It is
disabled by default so you can choose its placement.

These are data resources; DLLs and scripts are not executed. Ped and vehicle IDs
do not automatically provide AI, skeletal animation, clothing, or vehicle
physics. FiveM files are not loaded directly, and the FiveM script API is not supported.
An experimental [GTA V / FiveM asset converter](gta5-conversion.md) prepares
Legacy YFT/YDR/YDD/YTD models as native resources.

Runnable custom vehicles are registered separately:

```json
{
  "schema_version": 2,
  "enabled": true,
  "name": "My car",
  "vehicles": [{
    "name": "My coupe",
    "dff": "car.dff",
    "txd": "car.txd",
    "handling": { "acceleration": 8.5, "brake_deceleration": 12.0, "tire_grip": 5.0 }
  }]
}
```

The optional vehicle `name` supplies its label in `/cars` (1–48 printable
characters). Omit it to keep the existing resource-name/path labels. Conversion
tools preserve source model names as labels.

Up to 32 custom vehicles can be registered. One vehicle is active at a time.
Use `/cars` or F7 to select and spawn; F9 respawns the selected vehicle. `txd` can be omitted for an untextured DFF.
All textures must be included in the resource TXD; automatic lookup in the
original `vehicle.txd` is available only for the standard taxi. Use a typical
passenger car with original SA scale and model axes. Physics uses a fixed
passenger-car shape, without wheel animation or damage. Optional native handling
values adjust each custom vehicle, as described in [vehicle dynamics](vehicle-dynamics.md). Tire slip, axle forces, handbrake grip reduction,
four-point ground contact, suspension and body pitch/roll are simulated.
Manifest-only `original_vehicle_handling` resources can tune installed original
models without sharing their DFF/TXD files; see [vehicle dynamics](vehicle-dynamics.md).

`mods/native-car-demo` is a simple custom blue car with no original assets.
Set `enabled` to `true` in its `mod.json` and restart to try it. The demo is
disabled by default so normal startup continues to use the taxi.

A custom player character is registered with `player`:

```json
{
  "schema_version": 2,
  "enabled": true,
  "name": "My player character",
  "player": { "dff": "ped.dff", "txd": "ped.txd", "ifp": "ped.ifp" }
}
```

Up to 16 player resources can be registered. `/peds` or F4 selects the controlled
player. Use “Spawn nearby” or S to place an idle ped in the world (maximum eight).
Spawned peds have no navigation/combat AI; remove them from the Peds menu. The model must have PC Skin/HAnim
data with bone IDs and weights. `txd` can be omitted for untextured geometry.
`ifp` can be omitted when the skeleton matches the original `anim/ped.ifp`;
otherwise, provide custom ANP3 clips named `idle_stance`, `walk_player`, and
`run_player`. Every bone with vertex weights must have a track in each clip.
Tracks are currently matched by bone ID, not name, so weighted player bones
using ID -1 with name-only binding are not supported. The model must be upright
and between 1 and 2.5 meters tall in its idle pose. Physics still uses a fixed
player body; skeleton size does not change the collision shape.

`mods/native-ped-demo` contains a custom skinned character and custom clips.
Set `enabled` to `true` and restart to try it. The demo is disabled by default.
This replaces the entire player character. The [GTA V converter](gta5-conversion.md#custom-player-peds)
also supports `--type player` with an explicit bone map and compatible native rig.
The character is currently hidden
while driving.

Separate skinned clothing items can be added to the player resource:

```json
"player": {
  "dff": "ped.dff",
  "txd": "ped.txd",
  "clothes": [{ "name": "Jakke", "dff": "jacket.dff", "txd": "jacket.txd", "enabled": true }]
}
```

Up to 16 clothing items are supported. Each item must have Skin/HAnim data
and the same bone IDs, model coordinates, and bind pose as the player. Palette
bone order may vary; the loader matches bones by ID. Bind matrices must match
within 0.01. Clothing follows the player's idle, walk, and run animations and
can use its own TXD. Untextured clothing does not need a TXD. Body geometry
remains under clothing, so items must be fitted to avoid clipping; body-part
masks are not yet supported. Press F6 to open the wardrobe. Items can be
toggled during gameplay with the mouse or arrow keys and Enter. `name` is an
optional display name, and `enabled` selects the initial state (default: true).
Outfits are not yet saved between sessions. Original CJ component assembly and
FiveM clothing are not loaded directly.

`mods/native-clothing-demo` shows a separate red jacket and yellow hat on our
custom demo character. It is disabled by default. Select a player demo through `/peds` after enabling its resource.

Filer begrenses til 16 MiB hver, totalt 512 modeller, 512 teksturoverstyringer
og 256 MiB mod-ressurser. Ressurser kan ikke referere til filer utenfor egen
mappe. Del egne assets; originaldata leses fra brukerens installasjon.
Dekodede spillerteksturer har en egen grense på 128 MiB.

## glTF models from Blender (`.glb`)

A map model can be a glTF binary file instead of a DFF, so you can make props
and buildings in Blender without any RenderWare tools:

```json
{
  "enabled": true,
  "name": "My bench",
  "models": [{ "id": 30100, "model": "bench.glb" }],
  "placements": [{ "model_id": 30100, "position": [2495, -1665, 13.5] }]
}
```

- Export with *File → Export → glTF 2.0*, **Format: glTF Binary (.glb)** and
  **+Y Up** ticked. Blender axes stay as they are: X east, Y north, Z up, in
  metres. The object's origin is the placement point.
- The `.glb` carries its own textures (PNG only), so leave out `txd` and
  `original_txd`. Materials use their Base Color and Base Color texture;
  Alpha Blend/Clip materials are see-through and not solid.
- Collision works as in content packs: opaque meshes are solid, objects
  named `COL_…` are invisible but solid, and `NOCOL_…` objects are drawn but
  not solid. A `col` file, if you add one, replaces all of that.
- `"model"` is another name for `"dff"`; both accept `.dff` and `.glb`.
  `map_doors` works only with DFF models for now.
- Servers share `.glb` files with players like DFFs (players need SARE
  0.46.0 or newer).

The same format builds whole worlds that run without San Andreas: see
[content packs](content-packs.md).

## MTA:SA maps

Only load and share maps you are allowed to use. Servers send map files and
their models to every player, so do not host maps that contain models taken
from other Rockstar games.

The quickest way: unzip a downloaded MTA map resource into `mods/` (or a
category folder such as `mods/[maps]/`), so that its `meta.xml` sits directly
inside the resource folder, for example `mods/[maps]/stuntpark/meta.xml`. A
folder with a `meta.xml` and no `mod.json` loads as an enabled resource with
the `.map` files named by `<map src="...">`; the `<info name="...">` becomes the
resource name. Its client scripts are read for model replacements (see below)
but never run. To disable it, add a
`mod.json` containing `{"enabled": false}`, or move the folder out of `mods/`.

A native resource can also list MTA `.map` files itself in `mta_maps`:

```json
{
  "schema_version": 2,
  "enabled": true,
  "name": "Stunt park",
  "mta_maps": ["stuntpark.map"]
}
```

Up to 16 maps per resource and
20,000 objects in total are loaded. SARE reads:

- `<object>`: `model`, `posX`/`posY`/`posZ`, `rotX`/`rotY`/`rotZ` in degrees
  (MTA's object rotation order: Z, then X, then Y), `interior`, and a uniform
  `scale`. As in MTA, scale changes only the drawn model: collision keeps the
  model's normal size, and scale 0 hides the model but keeps it solid. Objects
  with broken attributes are skipped one by one.
- `<spawnpoint>`: becomes a teleport target. Type `/spawns` in chat (T) to list
  them and `/spawns 1` to go to the first one. Most MTA maps are built away from
  the normal roads, often out at sea, so this is the way to reach them.
- `<racepickup>`: `type` `nitro`, `repair` or `vehiclechange` (with
  `vehicle`, a vehicles.ide ID), position and `respawn` in milliseconds (5 s
  when missing). Drive through one to take it: nitro fills a 10 s tank that
  burns while you hold fire or left Ctrl, repair fixes the car, and vehicle
  change swaps your car for that model, keeping speed and heading. Pickups work
  in single player and for every player on a server; each player takes them
  with their own car. Up to 256 per map.
- `<removeWorldObject>`: removes original placements of `model` and `lodModel`
  whose position lies within `radius` of `posX`/`posY`/`posZ`, in `interior`
  or in every interior when it is missing or -1.

Objects with `collisions="false"` are drawn without collision. Objects with a
model ID that SARE does not know are skipped. `alpha`, `doublesided`, `frozen`
and `dimension` are ignored. Vehicles,
peds, markers, race checkpoints and other elements are skipped.

MTA maps load their custom models through client scripts. SARE never runs
those scripts, but reads them as text and recognises two common forms:

- tables whose fields name a `.dff`, `.txd` or `.col` file and a model ID, such
  as `{ dffFile = "road.dff", txdFile = "road.txd", modelID = 18450 }`;
- `engineLoadDFF/TXD/COL("file")` results, directly or through a local
  variable, passed to `engineReplaceModel`, `engineImportTXD` or
  `engineReplaceCOL` with a literal model ID.

These become `mta_models` entries in the generated manifest. A model whose
files are missing or fail to decode is skipped and logged; the rest of the
resource still loads. A new DFF without its own TXD keeps the replaced model's
textures, and a TXD alone replaces the textures of the original model. IDs or
paths computed at run time, shaders, sounds and other script behaviour are not
supported. For any model the scripts hide, register its files in `models` with
the model ID the map uses. The loader counts what it skipped in the console
log.

When a server shares the resource, the `.map` files go to joining players with
the other resource files. A drop-in MTA folder is shared with a generated
`mod.json` in place of its `meta.xml`.

## Multiplayer resource sharing

Hosting takes an immutable snapshot of enabled native resources and their
referenced files. Guests prepare the same ordered resource set before joining,
downloading missing content into a SHA-256 cache. Their local mods and original
installation remain separate; disconnect restores the offline world. The
automatic-download choice is saved with settings.

See [resource limits and cache behavior](multiplayer.md#native-server-resource-preparation)
and [dedicated hosting](server-hosting.md). Selected custom cars, peds and
clothing toggles synchronize through the shared session catalog. Remote peds
use their selected rig and independent outfit state. Personal cars remain
visible after exit while their owner stays connected. Exchanging cars,
passengers and spawned NPC replication remain future work.
