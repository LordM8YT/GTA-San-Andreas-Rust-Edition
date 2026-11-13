# Creating resources (mods) for SARE

> New here? The [creator handbook](MODDING.md) has step-by-step tutorials. The quickest
> start is `python tools/new-resource.py my_crate --type prop`, which makes a working
> `.glb` prop you can open in Blender, or `--type world` for a standalone world.

SARE resources are the way to add supported assets to the Rust runtime. A resource is a self-contained folder with a JSON manifest and the files it uses.

There are currently **two different resource types**:

1. **Native asset resources** — models, textures, collision and placements loaded by the game runtime. These use `resource.json` or `mod.json`.
2. **Server script resources** — Lua scripts started by a dedicated server. These use `fxmanifest.lua`.

They can live in the same server resource folder, but they are not interchangeable. A converted GTA SA resource can contain both: resource.json registers native assets, while fxmanifest.lua lets the SARE server discover and ensure that folder. A native asset manifest does not run Lua, and an fxmanifest.lua does not register DFF models.

## What can be added today?

The current native runtime supports documented forms of:

- **World models and props:** PC RenderWare DFF models, with optional TXD textures and COL collision.
- **Map placements:** Place a supported model at a position in the San Andreas world. MTA:SA `.map` object placements are supported in the documented workflow.
- **Vehicles:** Register supported custom DFF/TXD vehicles with SARE vehicle handling values.
- **Player characters:** Supported skinned PC DFF player models, with compatible animation clips where needed.
- **Clothing:** Separate skinned clothing models that match the selected player's skeleton and bind pose.
- **Texture overrides:** PNG overrides for supported texture dictionary entries.

This is not a general-purpose loader for every GTA mod. ASI/DLL plugins, CLEO scripts and arbitrary original game scripts do not run as native SARE resources. SARE includes an offline importer for a subset of classic GTA SA packages, including loose DFF/TXD/COL/IFP assets, uncompressed VER2 IMG entries, selected IDE declarations and exterior text IPL placements. Binary IPLs, compressed IMG variants and unsupported game-data formats remain out of scope. See [Importing classic GTA San Andreas mods](gta-sa-conversion.md). GTA V/FiveM assets need to be converted with the experimental converter where supported.

## Import a classic GTA San Andreas mod

Use the offline [GTA SA mod importer](gta-sa-conversion.md) to convert supported Mod Loader-style folders or loose game assets into a native resource. The output includes both resource.json for the native asset loader and fxmanifest.lua for SARE's server resource manager. This is an import step; it does not execute ASI, DLL or CLEO code.

## 1. Generate a starter resource

From the repository root, with Python 3 installed, run one of:

```bash
python tools/new-resource.py my_prop --type prop          # working .glb prop, enabled
python tools/new-resource.py my-world --type world        # content pack in content/
python tools/new-resource.py my_vehicle --type vehicle
python tools/new-resource.py my_map --type map
python tools/new-resource.py my_character --type player
python tools/new-resource.py my_clothing --type clothing
```

The generator creates a disabled resource under `mods/[category]/name/`. It will not overwrite an existing folder. It creates a manifest, a `stream/` folder for runtime assets, and a README describing the starter files.

The generated resource is disabled initially. Add the required files, check the manifest paths and model IDs, then set `"enabled": true` and restart the game.

## 2. Understand the folder layout

For example:

```text
mods/
  [vehicles]/
    my_vehicle/
      resource.json
      README.md
      stream/
        vehicle.dff
        vehicle.txd
      data/
        README.md
```

- `resource.json` or `mod.json` describes the resource.
- `stream/` is a convenient place for model, texture and collision files. Manifest paths are relative to the resource root.
- `data/` can hold authoring/source files for your own workflow. The runtime does not automatically parse arbitrary files placed there.

The runtime also supports category folders with up to four nested bracket categories. Resource folder names must be unique across categories. Resources are discovered in alphabetical category order; later definitions can replace earlier definitions that use the same model ID.

## 3. A simple map-prop manifest

This example registers a custom building and places it in the San Andreas world:

```json
{
  "schema_version": 2,
  "enabled": true,
  "name": "My building",
  "models": [
    {
      "id": 30000,
      "dff": "stream/building.dff",
      "txd": "stream/building.txd",
      "col": "stream/building.col"
    }
  ],
  "placements": [
    {
      "model_id": 30000,
      "position": [2500, -1670, 12.35],
      "rotation": [0, 0, 0, 1]
    }
  ]
}
```

Choose a model ID that is not already used by another resource. Positions use San Andreas world coordinates; rotations use the IPL quaternion convention. A placement can also reference an existing model ID without declaring a new model.

**Important map limitation:** this workflow adds models and placements inside the current San Andreas world coordinate space. It does not yet provide a complete standalone-world pipeline for creating an entirely separate planet/city with its own world bounds, streaming system, navigation and map infrastructure. Custom-world support is a larger engine capability, not something a manifest alone enables.

## 4. Vehicles

A vehicle resource registers a DFF model and optional TXD, plus supported handling values:

```json
{
  "schema_version": 2,
  "enabled": true,
  "name": "My custom car",
  "vehicles": [
    {
      "name": "My Coupe",
      "dff": "stream/car.dff",
      "txd": "stream/car.txd",
      "handling": {
        "acceleration": 8.5,
        "brake_deceleration": 12.0,
        "tire_grip": 5.0
      }
    }
  ]
}
```

Up to 32 custom vehicles can be registered. Select and spawn them in-game using `/cars` or **F7**. The current vehicle simulation uses a fixed passenger-car shape; it is not a full implementation of every GTA vehicle type, wheel setup, damage model or animation.

## 5. Player characters and clothing

A player resource uses a skinned PC DFF with Skin/HAnim data:

```json
{
  "schema_version": 2,
  "enabled": true,
  "name": "My character",
  "player": {
    "dff": "stream/ped.dff",
    "txd": "stream/ped.txd",
    "ifp": "stream/ped.ifp"
  }
}
```

The skeleton must use supported bone IDs and weights. If the original player animation set is not compatible, provide supported custom ANP3 clips. Up to 16 player resources can be registered; choose a character with `/peds` or **F4**.

Clothing is attached to a compatible player skeleton. Its bone IDs, model coordinates and bind pose must match the player. Clothing is not a universal accessory that can be attached to any arbitrary ped. The wardrobe is opened with **F6**.

See [Native mod resource format](native-mods.md) for the complete constraints, limits, examples and supported animation details.

## 6. Texture overrides

PNG overrides can replace supported texture dictionary entries using a `dictionary:texture` key:

```json
{
  "texture_overrides": {
    "cj_bins:miamibin": "textures/my_texture.png"
  }
}
```

PNG files can be up to 4096 × 4096. The full resource schema and current memory/file limits are documented in [Native mod resources](native-mods.md).

## 7. Use assets on a dedicated server

Place native resources inside the server's `resources/` directory, commonly under `resources/[local]/`. Start a resource by name in `server.cfg`:

```cfg
ensure my_vehicle
```

Or start a whole category:

```cfg
ensure [local]
```

The resource's native manifest controls whether the asset resource is enabled. The server prepares the enabled native files for joining clients, who download and cache required files before entering the session. Clients verify the inventory and file hashes.

**Restart the server after changing shared native assets.** The server's asset inventory is prepared for joining players; changing it while running can leave the current inventory out of date. Script resources can be restarted separately.

Only share assets you created or have permission to redistribute. SARE does not grant rights to Rockstar or other third-party content. Do not upload original game files or converted copies of Rockstar assets to a server or repository.

## 8. What about scripts?

Lua scripts are a separate resource type. A server script resource uses `fxmanifest.lua`, for example:

```lua
fx_version 'cerulean'
game 'common'

server_script 'server.lua'
client_script 'client.lua'
```

SARE's server supports a documented subset of FiveM-style server Lua APIs, and client scripts run in a sandbox with SARE's supported client API. FiveM scripts that rely on GTA V natives, NUI, OneSync or unsupported services need adaptation. Read [server hosting](server-hosting.md), [client scripts](client-scripts.md) and [framework compatibility](framework-compatibility.md).

## Related documentation

- [Server guide](server-guide.md)
- [Full native resource format and limitations](native-mods.md)
- [Dedicated server API and configuration](server-hosting.md)
- [Client scripts](client-scripts.md)
- [Experimental GTA V/FiveM asset conversion](gta5-conversion.md)
