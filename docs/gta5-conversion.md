# GTA V / FiveM asset conversion (experimental)

The offline converter creates a native `resource.json` folder from **unencrypted
GTA V Legacy** YFT (cars), YDR (props), YDD (players/clothes/drawables) and YTD textures.
It does not run FiveM resources or their scripts. The game continues to load
native DFF/TXD assets; this tool converts models before starting the game.

## Import a resource folder

`tools/import-fivem.py` inspects an extracted FiveM resource with a root
`fxmanifest.lua` or legacy `__resource.lua`. Inspection lists model files,
scripts, metadata and unsupported features without executing Lua:

Reference: [Cfx resource manifest documentation](https://docs.fivem.net/docs/scripting-reference/resource-manifest/).

```powershell
python tools/import-fivem.py C:/Downloads/my-car-pack
python tools/import-fivem.py C:/Downloads/my-car-pack --kind vehicles --out "mods/[vehicles]/my-car-pack" --enable
```

Batch import converts YFT cars or YDR props, including their CodeWalker XML
exports, into one native resource. Matching YTD textures are found by basename,
preferring the same folder; ambiguous dictionaries stop the import. Embedded
and adjacent DDS textures follow the existing converter. A car's base YFT is
preferred when its `_hi` variant is also present, preserving its fragment
children; repeat `--model stream/name.yft` to choose exact relative model paths.
Raw assets take precedence over adjacent XML exports of the same asset.

For cars whose model and texture dictionary names differ, select their metadata:

```powershell
python tools/import-fivem.py C:/Downloads/my-car-pack --vehicles-meta data/vehicles.meta --out "mods/[vehicles]/my-car-pack" --enable
```

Repeat `--vehicles-meta` for up to 16 exact relative metadata paths. The reader
uses `InitDatas/Item` (also lowercase `item`) and `modelName`/`txdName` associations,
as represented by [CodeWalker's vehicle metadata reader](https://github.com/dexyfex/CodeWalker/blob/master/CodeWalker.Core/GameFiles/FileTypes/VehiclesFile.cs).
Every selected YFT must have a matching model entry; duplicate names, missing
local dictionaries and ambiguous dictionaries stop the import. Each XML file
is limited to 1 MiB and 512 entries, with DTD/entities rejected. `_hi` fragments
match the base model name. An explicit `--texture MODEL=YTD` pair takes precedence,
including when replacing a dictionary dependency from the GTA V base game.
Metadata is snapshotted and hashed for the report, then only the resolved
native textures are shared with players. GTA V handling, seats, audio, flags,
tuning and GXT display labels are not applied. Parent texture relationships
are not merged; include a self-contained dictionary or use an explicit override.

To tune a converted car for this engine, add a native JSON profile inside the
source resource and select it explicitly:

```json
{"acceleration":8.5,"brake_deceleration":12,"tire_grip":5,"steering_lock":0.6,"suspension_spring":160}
```

```powershell
python tools/import-fivem.py C:/Downloads/my-car-pack --native-handling stream/car.yft=native-tuning.json --out "mods/[vehicles]/my-car-pack" --enable
```

Repeat `--native-handling MODEL=JSON` for selected cars; one profile can be shared
by several models. Each file is limited to 16 KiB. The reader rejects duplicate
or unknown fields, nonnumeric/nonfinite values and values outside the native
engine's limits. Omitted fields keep native defaults. The snapshot hash and
selected values are recorded in the import report; the values become the car's
`handling` object in `resource.json`, so server downloads and cached packs retain
them. Source JSON files themselves are not distributed.

These are native tuning values, with speed in metres per second, acceleration
and braking in metres per second squared, and steering angles in radians.
Spring, damping, grip and drag use this engine's model. This option does not
translate GTA V `handling.meta`, its mass, gearbox or suspension units. See
[native vehicle tuning](vehicle-dynamics.md) for fields and limits. The community
can adjust the emitted `resource.json` afterwards and restart to load changes.

For props, specify a preview location in original San Andreas world coordinates:

```powershell
python tools/import-fivem.py C:/Downloads/my-props --kind props --position 2500 -1670 12.35 --model-id 31000 --out "mods/[maps]/my-props"
```

Props receive sequential model IDs and preview placements three metres apart.
This does **not** import their original YMAP/ IPL placement; review/edit the
native placements and choose model IDs that do not conflict with other mods.
Output is disabled unless `--enable` is given. Restart to register the pack.

The output directory must be new and outside the source resource. Input is
limited to 4,096 entries, 12 directory levels, 128 MiB per file and 512 MiB total;
links and Windows junctions are rejected. An isolated data snapshot is converted
and the completed output is published together, so failed conversions leave no
partial resource. Native file/model/server-sharing limits also apply: at most
32 cars or 64 props, 16 MiB per converted DFF/TXD, 64 shared files including
the manifest and 128 MiB total assets. Select fewer models if those limits are hit.

`data/fivem-import-report.json` records source hashes, converted geometry and
missing textures. Lua/JS/C# scripts, NUI/HUD pages, handling/vehicle metadata
and MLO rooms/portals are reported or listed but never applied. Explicit
`vehicles.meta` selection imports only the model-to-texture association above. A limited
YBN collision subset can be explicitly selected as described below.
Manifest inspection is text-based and cannot interpret
dynamic declarations. Peds and clothes use the explicit rig/mapping
workflow below, now also available through folder import. This is an asset import path, not a FiveM runtime.

The folder importer was tested with owned multi-model XML fixtures, atomic
failure/overwrite and Windows-junction rejection checks. The previously tested
free Skyline's raw YFT/YTD files were also imported from an isolated resource
folder and then downloaded, loaded, driven and restored through a dedicated
relay session with two Vulkan clients. The model-to-texture metadata reader was
also tested with the same raw car, a renamed YTD and owned metadata: both
clients downloaded the converted pack, drove/rode in the car and restored
the offline world. Third-party assets remain local.

## Material approximation

Known glass shader file names, including hashed CodeWalker names, now become
50% transparent native materials. This fixes vehicle glass being imported as
opaque simply because its render bucket was 0 or 1. Ordinary bodywork and lights
keep their previous opaque behavior; unknown shaders retain the existing bucket
fallback. The identifiers follow [CodeWalker's glass shader classification](https://github.com/dexyfex/CodeWalker/blob/master/CodeWalker/Rendering/ShaderManager.cs).

This is a fixed-opacity approximation. GTA V reflection, refraction, tint,
breakage and lighting behavior are not reproduced. Alpha layers use the current
native renderer and may still have sorting artifacts. Re-import existing packs
to regenerate their materials; the changed assets produce a new server cache
fingerprint. Original San Andreas materials are unaffected.

## Player and clothing resource folders

`--kind player` imports one selected YDD character into a native player
resource. `--kind clothing` imports up to 16 selected YDD files as individually
named wardrobe toggles on one native base player. Both require explicit
`--base-player`, `--base-ifp` and `--bone-map` paths. The target rig and mapping
are read into the isolated conversion snapshot; reports include their hashes.
Native animation/base files are shared once within a clothing pack.

```powershell
python tools/import-fivem.py C:/Downloads/my-ped --kind player --model stream/character.ydd --base-player C:/MyNativePed/ped.dff --base-ifp C:/MyNativePed/ped.ifp --bone-map C:/MyNativePed/bones.json --out "mods/[peds]/character" --enable
python tools/import-fivem.py C:/Downloads/my-clothes --kind clothing --model stream/shirt.ydd --model stream/hat.ydd --base-player C:/MyNativePed/ped.dff --base-ifp C:/MyNativePed/ped.ifp --bone-map C:/MyNativePed/bones.json --out "mods/[peds]/clothes" --enable
```

Add `--skeleton stream/source.yft.xml` for a source skeleton without an embedded
one. This must be an exact relative CodeWalker model XML path inside the source
resource. `--base-txd` optionally supplies the native clothing base player's
textures. Converted characters use their own converted textures.

When drawable and texture dictionary names differ, repeat an explicit relative
pair such as `--texture stream/shirt.ydd=stream/shirt_diff_000_a_uni.ytd`.
Pairs must reference selected models and included YTD/YTD XML files. Without
an explicit pair, the existing basename matching/adjacent DDS workflow applies.

One player per output resource is supported; select one with `--model` when
several YDDs are present. All drawable items within each selected YDD are merged;
extract/edit a dictionary first if its items represent alternate variants.
This does not interpret freemode component slots, texture variants, body masks,
facial animation or cloth simulation. Mesh fitting and weighted-bone mapping
remain author work; the native loader also checks height and animation tracks.
See the [owned ped/clothing example](../examples/fivem-skinned-models/README.md).
Owned XML and raw Legacy YDD round-trip tests exercise native skin decoding.

## Static map extensions from YMAP or XML

`--kind map` can preserve HD static placements from one **Legacy YMAP** file
or **CodeWalker YMAP XML** export instead of using the prop preview grid:

```powershell
python tools/import-fivem.py examples/fivem-static-map --kind map --ymap stream/demo.ymap.xml --model-id 31200 --out "mods/[maps]/static-demo" --enable
```

The owned example contains two orange blocks near Grove Street, one rotated
90 degrees. Its source XML is an importer fixture, not FiveM binary game data.
For a binary map, use `--ymap stream/name.ymap`; the pinned CodeWalker extractor
converts it internally. Binary maps require the .NET SDK; XML-only import does not.
Use `--offset X Y Z` to move an imported extension into the San Andreas map;
positions otherwise retain the source world coordinates. The importer resolves
archetype names or GTA name hashes against the selected YDR basenames, assigns
native model IDs and preserves inverse quaternion rotations. Lower-detail LOD
entities are omitted and listed in the report to avoid duplicate geometry.

Only unit-scale static `CEntityDef` HD/orphan-HD entities are supported. Missing
models, ambiguous names/hashes, invalid rotations, non-unit scales and MLO
instances stop the whole import with a clear error. Include the custom models;
references to GTA V base-game props are not resolved. Optional static YTYP
aliases are supported as described below.
Only unencrypted Legacy RSC7 maps are accepted; Gen9/Enhanced, RPF and escrow
remain unsupported. Bake scale into the mesh before import. Rooms, portals,
entity sets, light/audio metadata, navigation,
doors and LOD streaming relationships are not recreated.
The native loader derives static collision from the converted mesh unless an
explicit model-local collision file is selected.

Add `--ytyp stream/types.ytyp` or `--ytyp stream/types.ytyp.xml` when
archetype names differ from their drawable filenames. Repeat the option for
up to 16 files. Only `CBaseArchetypeDef` with `ASSET_TYPE_DRAWABLE` is supported:
`name`/name hashes resolve through `assetName` to the selected YDR model.
A declared `textureDictionary` selects the matching YTD by name/hash even when
its basename differs from the YDR. Include that dictionary in the resource;
missing or ambiguous dictionaries fail the import. Without a declaration the
existing basename/embedded-texture workflow applies.

Every archetype in the selected YTYP files must reference a selected custom YDR.
Duplicate/conflicting aliases, time/MLO archetypes, drawable dictionaries,
extensions and composite entities are rejected. Bounds, flags and
`physicsDictionary` are not translated; converted mesh collision remains the
fallback. YTYP alias support does not reproduce MLO rooms, portals, doors,
lights or animation. See the alias variant in the [owned map example](../examples/fivem-static-map/README.md).
The parser limits metadata to 4096 archetypes/entities; each emitted native
resource is limited to 2000 placements. Binary YTYP uses the same bounded Legacy
extractor; XML-only imports need no .NET runtime.

Static archetype format reference: [CodeWalker YtypFile](https://github.com/dexyfex/CodeWalker/blob/master/CodeWalker.Core/GameFiles/FileTypes/YtypFile.cs).
Owned tests cover XML/name hashes, binary YTYP round trips, a YTD with a different
basename, and atomic rejection of unsupported definitions/missing dictionaries.

### Explicit YBN collision

The folder importer accepts unencrypted Legacy `.ybn` and CodeWalker
`.ybn.xml`. Only `Composite`, `Geometry`/`GeometryBVH` containing `Triangle`
polygons, and standalone `Box` bounds are supported. Unsupported spheres,
capsules, cylinder/disc/cloth bounds and non-triangle geometry polygons reject
the entire import. Convert unsupported shapes to triangle meshes in your
modelling workflow first. GTA V materials, flags, margins, BVH acceleration
and dynamic collision behavior are not reproduced.

For a collision file authored **in the model's local coordinates**, pair it
with a selected drawable. The resulting native COL follows that model's
YMAP rotation and translation. This works for `--kind map` and `--kind props`:

```powershell
python tools/import-fivem.py examples/fivem-static-map --kind map --ymap stream/demo.ymap.xml --collision stream/demo_block.ydr.xml=stream/demo_block.ybn.xml --out "mods/[maps]/collision-demo" --enable
```

Repeat `--collision MODEL=YBN` for distinct selected models. There is no
filename-based guess: a standalone map YBN often already contains **world
coordinates**, and attaching it to a drawable would incorrectly transform it
again. For those files use `--world-collision stream/world.ybn` with
`--kind map`; repeat for up to 16 files. The importer bakes geometry centers
and nested child transforms, rebases each file around its bounds, and creates
an invisible native bounds model/COL placement. `--offset` translates it
once, alongside the map. Reusing one YBN in both modes is refused.

Each collision file is limited to 65,535 triangles and 65,536 output vertices;
composite depth is limited to 16 and nodes to 4,096. World collision's
horizontal half-bounds diagonal must be at most 1,600 m to remain compatible
with native region selection; split wider files. Native file/pack/placement
budgets also apply. No scripts or original collision archives are bundled.
The import report records each collision's space, triangle count and world
origin/offset. Review alignment before enabling a converted pack.

Owned tests check raw YBN round trips, nested transforms, native COL decoding,
box placement/rotation, unsupported-shape rejection and atomic failure. The
owned world-ramp audit checks actual streamed support under a standing player
and all four vehicle contact points. See the additional fixture commands in
the [map example](../examples/fivem-static-map/README.md).

Format references: [CodeWalker YbnFile](https://github.com/dexyfex/CodeWalker/blob/master/CodeWalker.Core/GameFiles/FileTypes/YbnFile.cs)
and [bound/transform XML structures](https://github.com/dexyfex/CodeWalker/blob/master/CodeWalker.Core/GameFiles/Resources/Bounds.cs).

Rotation convention reference: [CodeWalker YmapEntityDef](https://github.com/dexyfex/CodeWalker/blob/master/CodeWalker.Core/GameFiles/FileTypes/YmapFile.cs).
Tests cover translated positions, rotated placements, hash lookup, skipped LODs
and atomic rejection of unsupported entities. The owned example was downloaded
and rendered by two Vulkan clients through a dedicated relay session.
The binary route was checked by encoding our owned fixture to RSC7 and comparing
its extracted placements/rotations with the XML import, including rejection of
bad headers and excessive declared memory. This is not a test of every external map.

## Requirements and a vehicle

Use Python 3.11+ and the .NET 9 SDK. The first binary conversion builds our small
extractor and restores the pinned CodeWalker.Core 1.0.3 NuGet dependency. Its
third-party notices are in `tools/gta5-extract/THIRD-PARTY-NOTICES.txt` and are
copied alongside the extractor. Upstream source: [CodeWalker](https://github.com/dexyfex/CodeWalker/tree/baca4c1af822b7df6f6547c939e396beaabcf855). Exported CodeWalker XML plus DDS textures can
be converted with Python alone. No GTA V executable is needed.

Run from the repository root:

```powershell
python tools/convert-gta5.py C:/Downloads/car/skyline.yft --textures C:/Downloads/car/skyline.ytd --type vehicle --out "mods/[vehicles]/skyline" --enable
```

Restart the game and press **F9** to spawn/enter the custom vehicle. Use `/cars` or F7 to select among enabled vehicles. Choose the normal `.yft` for a lighter
model or `_hi.yft` for its high-detail geometry. Output directories must be new;
existing resources are never overwritten. Omit `--enable` to prepare a disabled
resource for review. `--scale` and `--flip-v` are available for rigid models.

The converter uses the highest available drawable LOD, bakes pristine physics
children, and instances a shared wheel mesh for ordinary four-wheel cars.
Wheels and doors are static. Diffuse textures and vertex colours are preserved;
GTA V paint, reflection, dirt, normal-map and other shaders are approximated.
Missing shared textures receive an explicit neutral material colour. Supply a
DDS folder with the shared textures through `--textures` to improve the result.
DXT1/3/5 and 32-bit RGB DDS are supported; BC7/DX10 must be converted first.
`data/conversion-report.json` lists counts, missing textures, limitations, and
the source SHA-256. Each resulting DFF/TXD is limited to 16 MiB; reduce the model
or textures in an editor if it exceeds this limit.

Vehicle physics uses our existing passenger-car body. FiveM handling.meta,
vehicles.meta, carcols, damage, tuning parts, lights and wheel animation are not
imported. This is a usable asset conversion path, not exact GTA V rendering or
full FiveM compatibility. Enhanced/Gen9, escrow/encrypted resources and RPF
archives are not supported.

## Custom player peds

`--type player` converts a complete skinned character into a native player
resource. It uses the converted mesh as the player, rather than adding it as
clothing. Supply the target native skeleton/animation and explicit bone map,
as for clothing below. A YDD with an embedded skeleton needs no `--skeleton`;
otherwise export the matching source YFT skeleton as CodeWalker XML.

```powershell
python tools/convert-gta5.py C:/Downloads/ped/custom.ydd --textures C:/Downloads/ped/custom.ytd --type player --skeleton C:/Downloads/ped/source.yft.xml --base-player C:/MyNativePed/ped.dff --base-ifp C:/MyNativePed/ped.ifp --bone-map C:/MyNativePed/bones.json --out "mods/[peds]/custom-player" --enable
```

Restart to register the player. Use `/peds` or F8 to select an enabled player resource. The target DFF
supplies the bone hierarchy and bind matrices; only its animation IFP is copied
into this player package. Textures come from the converted character. The
converted player must fit our upright 1–2.5 m height requirement and all weighted
bones need matching tracks in the native idle/walk/run clips. Additional facial
bones need a deliberate mapping or model editing; automatic facial animation
and GTA V animations are not imported.

This replaces the controlled player. Nearby idle peds can be spawned from the Peds menu, with a limit of eight.
Navigation/combat AI and arbitrary FiveM script APIs are not implemented.
Custom peds are still hidden while driving. Use our `native-ped-demo` as an
example of a compatible target rig; its simple skeleton is not a universal
mapping for every downloaded character.

## Clothing and skeletons

Clothing is a separate skinned DFF attached to a native base player. It needs a
compatible native DFF/IFP, an explicit source-to-target bone map, and the source
skeleton. GTA V multiplayer clothes do not automatically fit CJ: body shape,
bind pose and bone IDs differ. Fit the garment in a modelling tool and map
**every weighted bone**. The converter remaps weights and transforms vertices
from the source bind pose into the selected native bind pose. It copies the
base hierarchy and inverse bind matrices so native animation can drive it.
Normals use the inverse transpose. Scaling clothing after retargeting is refused;
fit it before conversion instead.

A map JSON maps actual GTA V bone names (or numeric tags as strings) to integer
HAnim IDs in the chosen native base player. For example, `{"source_bone": 32}`
is only a schema example; ID 32 must exist and represent the same body part in
your chosen base. Missing weighted mappings fail without publishing a resource.
Do not map unrelated limbs merely to suppress an error.

```powershell
python tools/convert-gta5.py C:/Downloads/outfit/garment.ydd --textures C:/Downloads/outfit/garment.ytd --type clothing --skeleton C:/Downloads/rig/mp_player.yft.xml --base-player C:/MyNativePed/ped.dff --base-ifp C:/MyNativePed/ped.ifp --base-txd C:/MyNativePed/ped.txd --bone-map C:/MyNativePed/bones.json --out "mods/[clothing]/outfit" --enable
```

An embedded skeleton can be used instead of `--skeleton`. The external skeleton
must be CodeWalker XML from the correct source character, not an unrelated rig.
The base IFP supplies native idle/walk/run animations; GTA V animations are not
converted. This creates a player resource with one garment; merge additional
converted `clothes` entries into the same player manifest. Press **F6** to toggle
clothing. Body masking, cloth simulation and automatic body fitting are pending.

## Props and interiors

`--type map --model-id 30000 --position 2500 -1670 12.35` converts drawable
geometry and places it in the SA world. YMAP placements, YBN collision, YTYP
archetypes, MLO rooms/portals and interior streaming are not imported by this
single-model converter. Use the folder importer above for static YMAP/YTYP
placement and alias conversion. Our native
opaque-triangle collision fallback applies. A complete MLO therefore needs
manual assembly and separate support for its metadata.

## What was tested

A public [FiveM-Civ-Car-Pack](https://github.com/PLOKMJNB/FiveM-Civ-Car-Pack)
Nissan Skyline GT-R by YCA-y97y was downloaded locally for testing. Normal YFT
plus YTD conversion produced 25 meshes, 22,164 vertices, 17,478 triangles and six
textures, including four static wheel instances. Native DFF/TXD decoding and the
Vulkan driving/braking/exit/re-entry smoke test passed. Several shared GTA V
textures were absent, so the report identifies their replacement materials.
The downloaded and converted third-party model is not committed or redistributed.

Full player retargeting and clothing retargeting were tested using our own jacket expressed in CodeWalker XML
with an intentionally shifted source rig. Its converted vertices match the
native garment and its native skin/bind matrices decode. A complete player
conversion registers the converted mesh directly and preserves the base clips;
its Vulkan walk/run/idle and camera-toggle smoke tests passed. The converted jacket
also passed the Vulkan walk/run/idle and wardrobe hide/show tests. This does **not** prove
arbitrary downloaded FiveM clothes fit or animate correctly; each rig needs its
own mapping and fit check.

Run automated checks:

```powershell
cargo build --manifest-path native/Cargo.toml -p sa-assets --example audit_resource
dotnet build tools/tests/map-fixture/MapFixture.csproj -c Release -p:RestoreLockedMode=true
python -m unittest discover -s tools/tests -v
dotnet build tools/gta5-extract/Gta5Extract.csproj -c Release -p:RestoreLockedMode=true
```

The native audit example checks converter output with the game's actual asset
decoders, including all referenced diffuse textures and skin bind-pose evaluation:

```powershell
cargo run --manifest-path native/Cargo.toml -p sa-assets --example audit_resource -- "mods/[vehicles]/skyline/stream/converted.dff" "mods/[vehicles]/skyline/stream/converted.txd"
```
