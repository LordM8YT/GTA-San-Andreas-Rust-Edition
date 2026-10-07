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
texture overrides), COL collision files, placements, and the documented
vehicle, player, and clothing resources. It does not load arbitrary original
San Andreas mods directly. Binary plugins, ASI/DLL mods, CLEO scripts, original
game-code or plugin APIs, and unconverted archives/workflows such as IMG/IPL
are not drop-in compatible. Convert assets into a supported resource or add
explicit format support; scripts and DLLs are not executed. FiveM resources
and the FiveM script API are also incompatible. The resource folder workflow
can be similar, but the engine still uses SA asset formats and its own JSON
manifest. `fxmanifest.lua`, GTA V `.meta` files and executable scripts are not parsed.

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
    { "id": 30000, "dff": "building.dff", "txd": "building.txd", "col": "building.col" }
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
its collision geometry is used. This supports custom buildings and enterable
rooms, but is not GTA V MLO import.

`placements` can reference an original model ID without a `models` entry.
Texture overrides use the `dictionary:texture` key format. Legacy manifest
settings are not part of the native schema.

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
  "vehicles": [{ "dff": "car.dff", "txd": "car.txd" }]
}
```

Only one custom vehicle can be active at a time. It replaces the F9 taxi and
uses the same driving controls. `txd` can be omitted for an untextured DFF.
All textures must be included in the resource TXD; automatic lookup in the
original `vehicle.txd` is available only for the standard taxi. Use a typical
passenger car with original SA scale and model axes. Physics uses a fixed
passenger-car shape, without custom handling, wheel animation, or damage.

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

Only one player resource can be active. The model must have PC Skin/HAnim
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
This replaces the entire player character. The character is currently hidden
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
custom demo character. It is disabled by default. Enable only one of the player
demos at a time.

Filer begrenses til 16 MiB hver, totalt 512 modeller, 512 teksturoverstyringer
og 256 MiB mod-ressurser. Ressurser kan ikke referere til filer utenfor egen
mappe. Del egne assets; originaldata leses fra brukerens installasjon.
Dekodede spillerteksturer har en egen grense på 128 MiB.
