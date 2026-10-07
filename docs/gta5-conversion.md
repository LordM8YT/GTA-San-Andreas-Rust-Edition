# GTA V / FiveM asset conversion (experimental)

The offline converter creates a native `resource.json` folder from **unencrypted
GTA V Legacy** YFT (cars), YDR (props), YDD (players/clothes/drawables) and YTD textures.
It does not run FiveM resources or their scripts. The game continues to load
native DFF/TXD assets; this tool converts models before starting the game.

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

Restart the game and press **F9** to spawn/enter the custom vehicle. Only one
custom vehicle resource can be enabled. Choose the normal `.yft` for a lighter
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

Restart to use the player. Enable only one player resource. The target DFF
supplies the bone hierarchy and bind matrices; only its animation IFP is copied
into this player package. Textures come from the converted character. The
converted player must fit our upright 1–2.5 m height requirement and all weighted
bones need matching tracks in the native idle/walk/run clips. Additional facial
bones need a deliberate mapping or model editing; automatic facial animation
and GTA V animations are not imported.

This replaces the controlled player. Ambient NPC spawning, model selection
during gameplay, per-ped AI and arbitrary FiveM script APIs are not implemented.
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
archetypes, MLO rooms/portals and interior streaming are not imported. Our native
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
python -m unittest discover -s tools/tests -v
dotnet build tools/gta5-extract/Gta5Extract.csproj -c Release -p:RestoreLockedMode=true
```

The native audit example checks converter output with the game's actual asset
decoders, including all referenced diffuse textures and skin bind-pose evaluation:

```powershell
cargo run --manifest-path native/Cargo.toml -p sa-assets --example audit_resource -- "mods/[vehicles]/skyline/stream/converted.dff" "mods/[vehicles]/skyline/stream/converted.txd"
```
