# Importing classic GTA San Andreas mods

SARE has an offline importer for classic San Andreas mod folders and Mod Loader-style folders. It converts supported data assets into a native SARE resource. It does not patch the user's game installation and never executes code from a mod package.

## Supported input

- Loose PC RenderWare DFF, TXD, COL and IFP files.
- Uncompressed GTA SA VER2 IMG archives containing those assets.
- Text IDE model declarations in the objs, tobj, anim and cars sections.
- Text IPL inst placements for the exterior world.
- Vehicle model-to-texture names from cars IDE rows, plus a best-effort conversion of matching passenger-car lines in handling.cfg into SARE's native handling parameters.

The importer creates resource.json, a FiveM-style fxmanifest.lua, a stream/ folder and a README. SARE's native runtime reads resource.json; the fxmanifest lets the SARE server discover and start the same resource through its existing resource manager. The two manifests have different jobs and are both required for this combined workflow.

## Import a mod

Run from the repository root with Python 3:

    python tools/import-gta-sa.py "C:\Downloads\my-car-mod" --out "mods/[vehicles]/my-car-mod" --kind vehicles

For a map or object pack with IDE/IPL files:

    python tools/import-gta-sa.py "C:\Downloads\my-map-mod" --out "mods/[maps]/my-map-mod" --kind map --game-dir "C:\Games\GTA San Andreas"

--game-dir is optional. When supplied, the importer reads only IDE files registered in data/default.dat and data/gta.dat, plus TXD directory names from the original gta3.img and gta_int.img archives. It does not extract or copy original game assets. This lets it retain map placements that reference the base game and lets vehicles reuse a TXD already present in the player's own installation.

For a standalone prop without IDE/IPL metadata, choose the model and its world position:

    python tools/import-gta-sa.py "C:\Downloads\prop-pack" --out "mods/[maps]/my-props" --kind props --model crate --position 2500 -1670 12.3

For one custom player model:

    python tools/import-gta-sa.py "C:\Downloads\ped-mod" --out "mods/[peds]/my-ped" --kind player --model myped

Available kinds are auto, vehicles, map, props and player. Auto detects a car pack from a cars IDE section, otherwise a map from object IDE declarations. For mixed or unusual packages, specify the kind and use --model repeatedly to select exact basenames. --name changes the displayed resource name. Resources are disabled by default; --enable opts into enabling the generated resource immediately.

Review the import report, generated resource.json, model IDs and placements before enabling a resource. Put the output under the client's mods/ directory for offline use. For a dedicated SARE server, put it under server-data/resources/[category]/<resource-name>/ and add ensure <resource-name> to server.cfg; keep both manifests at the resource root. SARE shares the native manifest's referenced assets with joining players.

## Limits and compatibility

- IMG support is read-only and currently accepts uncompressed VER2 entries. Compressed entries are reported and skipped.
- Map placement import currently reads text IPL inst rows for exterior (interior 0) placements. Binary IPL files and interior placements are reported/skipped; the importer does not convert binary streaming IPLs.
- IDE support is intentionally limited to the documented sections. Unsupported IDE sections and other game data files are not silently translated.
- Custom vehicle registration currently uses SARE's passenger-car physics. Matching passenger-car lines in handling.cfg are translated approximately into SARE's smaller native handling model; this is not a one-to-one import of every GTA handling field. Bike, boat, train, helicopter, plane and trailer handling is not converted into equivalent vehicle simulation.
- A custom player DFF must already match SARE's supported PC skin/skeleton and animation requirements. Merely having a DFF extension does not make a model compatible.
- The importer copies source assets; the SARE runtime still validates and decodes them when the resource loads. A successful conversion report is not a guarantee that every model is renderable.
- The native runtime has resource, file-size, model, placement and memory budgets. Split large packs into smaller resources when those limits are reached.

This is not a drop-in loader for every GTA SA mod. ASI/DLL plugins, CLEO scripts, arbitrary executable patches, dynamic scripts, shaders, audio patches, binary IPLs, compressed IMG variants and unsupported game-data formats are not run or converted. In particular, do not put unknown executables in a resource expecting SARE to load them.

## FiveM-style server resources

Every generated resource contains both resource.json and fxmanifest.lua. The native manifest registers San Andreas models and placements; the fxmanifest declares a SARE server resource that can be placed in a category folder and selected with ensure. This makes imported SA assets usable by SARE's current server-side resource lifecycle and native asset-sharing path.

It does not make GTA V/FiveM assets or scripts automatically compatible. Existing SARE FiveM-style support for fxmanifest.lua, Lua server/client resources, dependencies, exports, events and selected CfxLua syntax is documented in framework-compatibility.md. GTA V YFT/YDR/YMAP/META, NUI and GTA V native calls need their own supported conversion/runtime implementation; a FiveM resource manifest alone cannot provide that.

## Asset ownership

Use mods you created or have permission to use. Keep Rockstar game assets and extracted/converted copies on your own machine. Do not commit, upload or redistribute original game files, converted game assets or content you do not have permission to share. SARE is an independent project and is not affiliated with Rockstar Games or Take-Two Interactive.
