# Original archive and world metadata verified

Historical metadata milestone. See runtime-test-findings.md for the later
rendered Grove Street test and broader geometry/texture support.

The user's private gta3.img was retrieved in 38 parts, each at most 24 MiB.
Every part's size and SHA-256 matched the private transfer manifest. The
reassembled 937,680,896-byte file matched the complete-file SHA-256:
`efeb84043c37053b53cabb50702ab6d36fcab6620fefffdc19e3156bd590b46c`.
The VER2 directory contains 16,297 entries and no invalid sector ranges.
The existing model decoder successfully reads CJ_WASTEBIN and CJ_BINS directly
from this archive: 66 vertices, 60 triangles, original Miamibin texture.
No original executable is loaded or used.

## Placement loader

`tools/load_world.py` reads classic SA text IPL `inst` records and streamed
binary IPL instance records inside the IMG. It resolves numeric model IDs
through static IDE definitions and checks corresponding DFF/TXD entries.
It keeps position, quaternion, interior and LOD references. LOD indices remain
source-local; the probe does not build a cross-file LOD graph or render a world.
It selects by placement origin, not geometry bounds, and does not apply
time-of-day visibility, streaming, occlusion, texture parenting or collisions.

Binary IPL header: magic `bnry`, instance count at byte 4, instance offset at
byte 28. Each 40-byte instance contains seven float32 values (position XYZ,
quaternion XYZW), then signed model ID, interior and LOD index. Bounds,
finite values and approximately unit quaternions are checked. Other sections
are ignored. Format details were cross-checked with the developer's reference:
https://github.com/Parik27/DragonFF/blob/master/gtaLib/map.py
No implementation code was copied from that project.

The private Los Santos dataset currently includes 21 LA IDE/IPL files and six
generic IDE files. It produces 3,822 static definitions and 14,305 placements
from 61 text/streamed IPL sources. A radius of 120 game units around XY
(2500, -1670), the initial Grove Street probe region, selects 211 exterior
placements. All 211 resolve to definitions with DFF and TXD entries present.
This includes both detailed and LOD objects, so it is not a final visible-object
count. The command prints aggregate counts only and writes no assets or cache:

```
python tools/load_world.py --game-dir "E:\GTA San Andreas\Grand Theft Auto San Andreas"
```

## Remaining renderer work

The narrow first-model decoder supports one material and DXT1 textures.
Many detailed world models use multiple materials; some nearby dictionaries
include unsupported texture formats. A probe of the 87 distinct LA-defined
models in the region found 30 model/dictionary pairs accepted by the current
decoder, predominantly LOD assets. It deliberately rejects unsupported data.
The world has not yet been rendered. Next work is multi-material geometry,
additional native texture formats, placement transforms and world draw batches.
The existing WebGL single-model viewer still needs a visual GPU/input check
on the user's computer. The project remains Python/WebGL; Rust is not yet used.

Original IMG files, extracted assets, transfer parts and placement data are
private development inputs. None are included in the downloadable source ZIP.
