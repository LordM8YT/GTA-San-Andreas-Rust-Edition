# Owned ped and clothing conversion fixture

These CodeWalker-style YDD XML meshes are generated from our own native ped,
jacket and hat demos. Their source skeleton is shifted two metres sideways;
the explicit `bones.json` mapping brings each weighted vertex back into the
native target bind pose. They contain no original Rockstar or downloaded assets.
The root manifest is inert text for the resource scanner. XML files are offline
conversion fixtures, not binary assets that can be streamed directly in FiveM.

From the repository root, import a complete character:

```powershell
python tools/import-fivem.py examples/fivem-skinned-models --kind player --model stream/character.ydd.xml --base-player mods/native-ped-demo/ped.dff --base-ifp mods/native-ped-demo/ped.ifp --bone-map examples/fivem-skinned-models/bones.json --out "mods/[peds]/converted-character" --enable
```

Or import the two clothes onto the native demo player:

```powershell
python tools/import-fivem.py examples/fivem-skinned-models --kind clothing --model stream/hat.ydd.xml --model stream/jacket.ydd.xml --base-player mods/native-ped-demo/ped.dff --base-ifp mods/native-ped-demo/ped.ifp --bone-map examples/fivem-skinned-models/bones.json --out "mods/[peds]/converted-clothes" --enable
```

Restart to register the new resources. F8 selects the player; F6 toggles the
imported hat/jacket on the clothing resource's base player. Disable either
resource if you want just one choice. Output directories must be new.

This mapping belongs to these simple owned meshes. Downloaded characters need
their own source bone mapping, matching native animation tracks and fitted
meshes. The importer does not reproduce GTA V freemode component metadata,
body masks, facial animation, cloth simulation or FiveM scripts.

The test-only .NET `MapFixture` helper can encode these XML dictionaries to raw
Legacy YDD for extractor round trips. Its reader uses the
[CodeWalker YDD API](https://github.com/dexyfex/CodeWalker/blob/master/CodeWalker.Core/GameFiles/FileTypes/YddFile.cs);
vertex layouts/byte weights follow
[CodeWalker drawable serialization](https://github.com/dexyfex/CodeWalker/blob/master/CodeWalker.Core/GameFiles/Resources/Drawable.cs).
