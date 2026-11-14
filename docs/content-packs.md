# Content packs: make your own SARE world

A **content pack** is a folder with your own world in it. SARE can run a pack
**without GTA San Andreas installed**: you build the world in Blender (or any
3D tool that exports glTF), drop the folder into `content/`, and press its
button in the launcher.

A pack holds one world (geometry, textures and collision) and, optionally,
customisable [characters](#characters) and drivable [vehicles](#vehicles).
Sounds and traffic come in later versions (see [the plan](STANDALONE.md)).
Without characters the player is SARE's blocky stand-in; without vehicles
F9 spawns SARE Kart.

## Quick start: play the example

1. Find the `content/` folder. It is next to `mods/`: in the release ZIP it
   is beside `sa-launcher`, and in a source checkout it is in the repository
   root. The launcher creates it the first time it starts.
2. Copy [`content/example-town`](../content/example-town) from the
   repository into it (the release ZIP does not ship the example yet).
3. Open the launcher. On **Home** you now see **Content packs: Example Town**.
   Press it. (Press **Refresh** if you added the folder while the launcher
   was open.)

From a terminal you can also run:

```text
sa-runtime --content example-town
sa-runtime --content path/to/my-pack
sa-runtime --content my-pack --content-dir D:/my-packs
```

## Make your own pack in 10 minutes

1. **Copy the example.** Copy `content/example-town` to `content/my-world`.
2. **Edit `content.json`.** Give it your own `id` (lowercase, no spaces),
   `name` and `license`:

   ```json
   {
     "id": "my-world",
     "name": "My World",
     "version": "1.0.0",
     "license": "CC0-1.0",
     "author": "Your name",
     "description": "One sentence about the world.",
     "world": {
       "scene": "world.glb",
       "spawn": [0, 0, 1]
     }
   }
   ```

3. **Build the world in Blender.** Model at real size: 1 Blender unit is
   1 metre. A player is about 1.8 m tall and a car about 4 m long.
4. **Export.** *File → Export → glTF 2.0 (.glb)*. Keep the defaults
   (**Format: glTF Binary**, **+Y Up** ticked). Save it over
   `content/my-world/world.glb`.
5. **Test.** Press **Refresh** and then **My World** in the launcher.

If anything is wrong, the game says why in its log (launcher: **Details**),
for example `world.spawn: the spawn point is not a standing position`.

## `content.json` reference

| Field | Required | Meaning |
| --- | --- | --- |
| `id` | yes | 1–64 characters: `a-z`, `0-9`, `-`, `_`. Used by `--content <id>` and later by servers. |
| `name` | yes | Shown in the launcher and as the area name in game. |
| `version` | yes | Your version, for example `1.0.0`. |
| `license` | yes | Who may share the pack, for example `CC0-1.0`, `CC-BY-4.0` or `MIT`. |
| `author` | no | You. |
| `description` | no | One sentence. |
| `world.scene` | yes | The drawn world: a `.glb` (or `.gltf`) path inside the pack. |
| `world.collision` | no | A separate collision model. When set, **only** this file is solid and `scene` is drawn only. |
| `world.spawn` | yes | Start point `[east, north, up]` in metres, in Blender coordinates. Put it on or just above solid ground. |
| `characters` | no | A `.glb` with customisable player characters, see [Characters](#characters). |
| `vehicles` | no | Up to 64 drivable vehicles, see [Vehicles](#vehicles). |

Unknown fields are refused, so a typo like `"verison"` gives a clear error
instead of being ignored. Paths must stay inside the pack folder.

## Coordinates

Use Blender's own axes: **X = east, Y = north, Z = up**, in metres. The
Blender origin (0, 0, 0) is the centre of your world. The exporter's
**+Y Up** option converts this to glTF for you; leave it on.

The game's map and compass treat Blender +Y as north.

## What gets drawn

- **Meshes** of the default scene, with all object transforms (move, rotate,
  scale, parenting) applied.
- **Colour:** the material's *Base Color*, multiplied by the mesh's colour
  attribute (vertex colours) if it has one.
- **Textures:** the *Base Color* image texture, **PNG only**, up to
  4096 × 4096. Pack it into the `.glb` (the default) or keep the `.png` next
  to a `.gltf` file. JPEG is not read yet: convert those images to PNG.
- **Transparency:** materials with *Blend Mode* Alpha Blend or Alpha Clip are
  drawn see-through and are not solid.
- **Lighting:** SARE bakes soft light from the normals when it loads the
  world. You do not need to bake lighting yourself, but painted vertex
  colours (for dirt or shadow) are kept.

Not read in the world: armatures/skins, animations, shape keys, emission,
normal maps and other material channels. They are ignored, not an error.

## Characters

`"characters": "characters.glb"` gives every player in the pack a body they
can dress. One file holds a male and a female rig and every style of hair
and clothing; the player's look picks one style per slot and a colour for
each. Servers store and share the look (`SetPlayerAppearance`, see
[SARE Box](sarebox.md)), so everyone sees the same person.

- **Rig:** bones are plain nodes (an armature exported with *Apply
  Modifiers*, no skin weights). Each mesh hangs under the bone it follows,
  so parts are rigid, like action figures.
- **Mesh names:** `<body>.<slot>.<style>.<part>`, for example `F.hair.3.0`
  or `M.top.2.sleeve_l`. Body is `M` or `F`; slot is `base` (always shown),
  `hair`, `top`, `pants` or `shoes`; style is a number from 0. A look asking
  for a style the file lacks gets style 0.
- **Colours:** materials named `skin`, `hair`, `top`, `pants` and `shoes`
  are tinted with the look's colours (make them white). Other materials
  (eyes, buttons) keep their colour.
- **Animations:** glTF animations named `idle`, `walk`, `run`, `sit`
  (driving) and `fall` (in Blender, one NLA track each). A missing clip
  shows the rest pose.
- **Facing:** the character looks along glTF +Z (Blender -Y) with its feet
  at height 0.

[`tools/blender/make_rp_characters.py`](../tools/blender/make_rp_characters.py)
builds the RP Town set from code and is a good starting point.

## Vehicles

Each entry of `vehicles` is one car:

```json
{ "model": "sedan", "name": "Sedan", "scene": "vehicles/sedan.glb",
  "color": [60, 90, 150], "seat": [-0.4, -0.05, -0.53],
  "handling": { "top_speed": 46.0, "acceleration": 6.0 } }
```

| Field | Required | Meaning |
| --- | --- | --- |
| `model` | yes | Name scripts use, e.g. `CreateVehicle("sedan", ...)`: 1–32 of `a-z`, `0-9`, `-`, `_`, unique in the pack. |
| `name` | yes | Shown in menus. |
| `scene` | yes | The car's `.glb` inside the pack. |
| `color` | no | Default paint, sRGB `[r, g, b]` 0–255. Servers can repaint with `SetVehicleCustomPrimaryColour`. |
| `seat` | no | Where the driver's feet go, `[right, forward, up]` in metres from the car's origin. |
| `handling` | no | Overrides of the car physics (`top_speed`, `acceleration`, `brake_deceleration`, `tire_grip`, `steering_lock`, …); omitted fields keep the defaults. |

The first vehicle is the car F9 places; all of them are in the car menu and
can be spawned by servers. In the `.glb`:

- The nose points along Blender +Y; the origin is the axle centre, and the
  lowest vertex sets the ground clearance. The collision box comes from the
  model's size.
- Meshes named `wheel_lf`, `wheel_rf`, `wheel_lb`, `wheel_rb` (left is
  Blender -X) are the wheels, with the object origin at the hub. Front
  wheels steer, all of them roll.
- Parts using a material named `paint` take the car's colour (make it
  white or the default colour). Glass with *Alpha Blend* is drawn
  see-through.

[`tools/blender/make_rp_vehicles.py`](../tools/blender/make_rp_vehicles.py)
builds the nine RP Town cars and writes their `vehicles` entries.

## Collision: what is solid

By default **every visible opaque mesh is solid**. Control this with object
names in Blender:

| Name starts with | Drawn | Solid | Use it for |
| --- | --- | --- | --- |
| *(anything else)* | yes | yes | Ground, buildings, ramps |
| `COL_` | no | yes | Invisible walls, simple boxes around detailed models |
| `NOCOL_` | yes | no | Grass, flowers, wires, small decoration |

A prefix on a parent object also applies to its children. For big worlds,
use a separate low-detail collision file instead and set
`world.collision`.

Tips for good collision:

- Keep floors flat and closed. A gap between two floor pieces is a hole.
- Walls should be closed boxes, not single planes, so fast cars cannot pass
  through them.
- Walkable slopes are up to about 45°; the example's ramp is 11°.
- Put `COL_` walls at the edge of the world so nobody falls off.

## Size and performance

A pack loads whole when the game starts (no streaming yet), so keep it
modest: tens of thousands of triangles run well on weak PCs; the hard limit
is 4 million. Big ground planes are fine. Join objects that share a material
(Ctrl+J in Blender): SARE draws one batch per texture, so fewer textures is
faster.

## Licences and what you may include

Only put files in a pack that you made or are allowed to share, and say so in
`license`. Never include files from GTA San Andreas or other Rockstar games,
or anything converted from them. Good sources of free assets are CC0
libraries such as Poly Haven and Kenney; keep their licence notes in the pack.

## The example pack

`content/example-town` is generated by
[`tools/make-example-pack.py`](../tools/make-example-pack.py) from code, so it
contains no third-party assets. It shows: a textured ground, roads, coloured
buildings, a jump ramp, stairs, `NOCOL_Flowers` you can walk through and
`COL_EdgeWalls` that stop you at the edge. Open `world.glb` in Blender
(*File → Import → glTF 2.0*) to see how it is built, then change it.

## The RP Town pack

`content/sare-rp-town` is a bigger example made in Blender by
[`tools/blender/make_rp_town.py`](../tools/blender/make_rp_town.py): a
roleplay town with a police station, hospital, bank, shops and garages,
plus customisable characters and nine cars. It is the map of the **SARE RP Town** server template; see
[its README](../server-data/resources/[sare-rp]/README.md).

## Testing without a window

`sa-runtime --probe-standalone --content my-world` loads the pack, checks
that the spawn point stands on solid ground and lets the player stand for ten
seconds, then prints the triangle and texture counts. With characters and vehicles
it also poses each animation and drives every car for a few seconds. It is quick and needs
no graphics card, so it also works in CI.
