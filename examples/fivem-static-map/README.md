# Owned static map import example

These XML meshes and placements were created for this project. They contain no
third-party game assets or scripts. The orange block has deliberately simple
normals/materials; it tests geometry, placements and rotation, not final visuals.

From the repository root:

```powershell
python tools/import-fivem.py examples/fivem-static-map --kind map --ymap stream/demo.ymap.xml --model-id 31200 --out "mods/[maps]/static-demo" --enable
```

Restart the game to see two blocks near Grove Street. Choose a different starting
model ID if 31200 is already in use. Add `--offset X Y Z` to relocate the map.
Omit `--enable` to prepare a disabled resource for review. This source folder is
CodeWalker XML for our offline importer; FiveM itself needs compiled game files.

Legacy binary `.ymap` is also supported through the .NET extractor; select its
relative path with `--ymap`. The fixture stays XML so it can be inspected easily.

See [conversion limits](../../docs/gta5-conversion.md#static-map-extensions-from-ymap-or-xml).

The alias variant exercises static YTYP definitions: its map refers to
`demo_block_alias`, while the drawable is named `demo_block.ydr.xml`.
Use a separate output directory:

```powershell
python tools/import-fivem.py examples/fivem-static-map --kind map --ymap stream/demo-alias.ymap.xml --ytyp stream/demo.ytyp.xml --model-id 31200 --out "mods/[maps]/static-alias-demo" --enable
```

Enable one variant at a time to avoid duplicate placements/model IDs. The YTYP
contains only static asset names; no MLO rooms, portals or behavior is included.

## Explicit collision fixtures

`demo_block.ybn.xml` contains a model-local box matching the orange drawable.
Pair it explicitly to make native COL follow both placements, including the
rotated block:

```powershell
python tools/import-fivem.py examples/fivem-static-map --kind map --ymap stream/demo.ymap.xml --collision stream/demo_block.ydr.xml=stream/demo_block.ybn.xml --model-id 31200 --out "mods/[maps]/collision-demo" --enable
```

`demo_world.ybn.xml` is a separate **invisible audit ramp**, elevated west of
the blocks at GTA (2460, -1670), from height 18 to 20 m. It deliberately tests
world-space collision independently of visible mesh fallback. It is not a
finished visible map addition. Prepare it in a separate output for the audit:

```powershell
python tools/import-fivem.py examples/fivem-static-map --kind map --ymap stream/demo.ymap.xml --collision stream/demo_block.ydr.xml=stream/demo_block.ybn.xml --world-collision stream/demo_world.ybn.xml --model-id 31200 --out "mods/[maps]/collision-audit" --enable
cargo run --manifest-path native/Cargo.toml -p sa-scene --example collision-import -- "E:/GTA San Andreas/Grand Theft Auto San Andreas" mods
```

Enable only this variant while auditing. The example checks both boxes, world
floor placement, standing clearance and four vehicle contacts on the ramp.
For an actual custom map, match the world collision to its visible geometry.
Legacy raw YBN is accepted too; the test helper compiles owned XML to validate
raw `Geometry`, `GeometryBVH`, nested `Composite` and `Box` round trips.
