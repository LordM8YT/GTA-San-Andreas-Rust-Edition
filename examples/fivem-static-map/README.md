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
