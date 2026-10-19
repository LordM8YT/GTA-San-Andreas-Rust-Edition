# Native Freeroam — Development Status, October 8, 2026

The goal is freeroam on the original map with local custom resources. Missions
are not a priority. The original installation is read; gta_sa.exe is not run.

Custom vehicle resources can supply bounded native handling values for engine,
brakes, grip, steering and suspension. Server packs include this tuning and
respawning preserves it; see [vehicle dynamics](vehicle-dynamics.md).

## Launcher (local review change)

Native `sa-launcher` now provides installation validation, offline start/Continue,
relay browser/join codes, favorites, shared settings, log/report access and safe
unused-cache cleanup. Runtime remains independently launchable. See [launcher](launcher.md).

## Local saves

The offline position, camera, selected car, ped and outfit are saved when you
open a menu, every 60 seconds at a safe position, on normal exit and before
multiplayer. The main menu shows “Continue free roam” when a valid checkpoint
has been loaded. On the next launch, the area around that position is loaded
and checked for floor, wall and head clearance before resuming.
If the location has no safe ground, you start at Grove Street. Removed or
ambiguous model names fall back to the current defaults; clothing selections
only apply to the same ped.

This saves one local offline session, without missions or server progress.
You start on foot, and the selected car is placed in a clear space nearby;
speed and exact parking position are not saved. Interiors, free-fly camera
mode, jumping, streaming and multiplayer do not overwrite the last safe
checkpoint. A crash can lose progress since the last safe save. See
[saving and testing](progress.md).

## Multiplayer prototype

One player hosts, and up to 19 others connect using an IP address and port.
Open Multiplayer from the main/pause menu, F5 or /mp. Player positions and
vehicle movement are shared; selected cars, peds and up to 16 clothing
selections per ped are also synchronized from the session's shared resource
list. Personal cars remain visible after the driver exits, with separate
visibility for the car and its owner. Passenger seats are reserved by the
host: G requests a seat in another player's car, and F exits again. NPCs,
switching cars and shared vehicle collisions are not synchronized.
See [setup and limitations](multiplayer.md). Direct internet hosting requires
a forwarded TCP port (7777 by default); automatic NAT traversal is not
implemented. Relay mode provides a server browser, public/private rooms and
join codes. Both sides connect outbound to a reachable relay, without port
forwarding on the game host. Start the relay with `start-relay.cmd`; no public
service is configured. The prototype uses unencrypted TCP and should be tested
on a trusted LAN/VPN. Native server mods are downloaded and cached before
joining, with version checks and restoration of local resources after
disconnecting. See the [roadmap](roadmap.md) for the next milestone and caching.

## Startup and controls

Double-click `start-sare.cmd`, select the installation and press Play in the launcher.
Developers first build with `cargo build --release --workspace --manifest-path native/Cargo.toml`.
Direct startup is also supported through `native/target/release/sa-runtime.exe`.

- WASD: walk. Shift: run. Space: jump. Click inside the window to enable mouse control.
- Xbox controller (default mapping): left stick moves/steers, right stick looks around;
  A jumps/confirms, X runs, Y enters/exits a car, RT accelerates, LT brakes/reverses,
  LB applies the handbrake. Start opens the pause menu, View/Back opens the map,
  the D-pad navigates and B goes back.
  The controller can be connected while the runtime is running; keyboard and mouse
  continue to work at the same time. No vibration or custom bindings yet.
- In deep water: WASD moves along the surface, Space gives an upward boost.
- P: walking mode/free-fly camera. Q/E: down/up in free-fly mode. Esc: pause menu.
- M: map menu. Use the mouse, arrow keys/Enter or Xbox D-pad/A in menus.
- /cars or F7: select and spawn a car. /peds or F4: change the player character.
  T: chat. F8: console.
  /mods shows the resource list, using the server's list while connected.
  Press /, type the command and press Enter. These menus are also available
  from the pause menu.
  Choose Spawn nearby or S in the peds menu to place a character in the world
  (up to eight), with an idle animation and no combat/walking AI.
- F9: spawn the selected car in front of you and get in. F: exit / enter near the car.
- In a car: W/S accelerate and brake/reverse, A/D steer, Space applies the handbrake.
- R: load Grove Street and return there.
- 1: Grove Street; 2: downtown Los Santos; 3: Santa Maria Beach.
- 4: LS airport; 5: countryside; 6: San Fierro.
- 7: Las Venturas; 8: desert; 9: Mount Chiliad.

The area finishes loading before teleporting. A background task loads the next
neighborhood after approximately 140 meters. The current area is retained
while this happens. Movement is stopped at the safety boundary if loading has
not finished in time. The window title shows FPS, GTA coordinates, mode and
loading status. Failed map loads retain the current area and wait five seconds
before retrying. If the background loader stops, the loading indicator is
cleared and an error message appears in the menu.

## Implemented

- Original 2DFX sign text is loaded from DFF files and drawn using `roadsignfont`
  from the installation's `models/particle.txd`. Lines, colors, arrows and symbols
  are preserved. All visible characters share one texture and draw batch and
  are excluded from the collision map. The original installation yielded
  489 text fields across 207 models; the Vulkan test confirms readable text
  at the bridge near Grove Street. This covers dynamic sign text; other
  missing building textures need to be investigated separately.
  `--smoke-signs --capture-dir <directory>` captures a test image of a nearby sign.
- Original RenderWare texture animations are used on scrolling signs, neon
  signs and waterfalls. 21 tracks are indexed once at startup; base
  geometry and collisions are retained while the animated materials' UVs
  are updated. Animation data also follows the map through background loading
  and GPU upload.
  The first UV channel is supported; full MatFX/dual-texture lighting and
  timed day/night objects are still not implemented.
  `--smoke-neon --capture-dir <directory>` tests the scrolling Las Venturas sign.
  Format reference: [RenderWare UV animations](https://github.com/aap/librw/blob/master/src/uvanim.cpp).

- A main menu inspired by San Andreas, adapted for freeroam, with the world as its background.
- Pause, a map with nine destinations, settings, controls and a local mod overview
  showing the status of enabled and disabled resources.
- Settings for field of view, mouse, fullscreen, VSync, HUD, radar/minimap,
  speedometer, vehicle handling and flight speed are saved in
  `%LOCALAPPDATA%/SAFreeroam/settings.json`. Menus stop player movement.
- The radar/minimap uses the original radar tiles (`radar00.txd` through
  `radar143.txd` in `gta3.img`), positioned using San Andreas world coordinates.
  As in GTA V, it is a rectangle in the lower-left corner that rotates with
  the camera, keeps the player near the bottom and zooms out with speed.
  Out-of-range blips sit on the edge. If the original file is missing, no
  fabricated map is displayed.
- The pause map (M) works like GTA V's: drag or use WASD to pan, scroll or
  PgUp/PgDn to zoom, with the legend on the right and destinations on the left.
  Right-click sets and removes a waypoint, C centers on you again and Backspace
  clears the waypoint. The GPS line follows vehicle roads from the game's
  `nodes*.dat` files and reroutes after a wrong turn; without them, it is a
  straight line. The speedometer appears while the player is driving.
  Both can be disabled separately in the settings.
- Losing window focus opens the pause menu. The map menu also supports keys 1–9.
- `--smoke-menus` renders the menu pages and can save rendered images.

- Indexing of 44,771 outdoor placements in this installation.
- Fixed an IPL bug: only the lowest eight bits are the interior ID. Flags above
  those bits must not cause outdoor terrain to be discarded.
- Actual transformed model bounds are used to select neighborhood content.
- Objects from IDE `anim` are included as static geometry. Their movement
  is not yet implemented.
- IDE `txdp` links texture dictionaries to their parents. The child's textures
  take priority; cycles and excessively long chains are stopped. Dictionaries
  are cached during a single area load with a 64 MiB limit.
- Original `water.dat` is read as triangles/quads, with visibility flags
  and height interpolation. Water is rendered using the original `waterclear256`
  from the local `particle.txd`; its geometry is excluded from normal floor
  collision.
- Basic buoyancy and surface movement prevent sinking to the seabed.
  Waves, diving and swimming animations are not yet included.
- `--probe-water` checks buoyancy using original coastal geometry.
- Parent models referenced by IPL LOD links are filtered out of the detailed view.
- Distant LOD is displayed up to 2.5 km away. An IPL LOD group is displayed
  either entirely in detail or as its LOD model, never partly; this previously
  caused a ring of holes in the ground at the edge of the detailed area.
  Distant models never participate in walking collision.
  The camera's far clipping plane is 3.5 km.
- Objects with interior ID 13 (“visible everywhere”) are now displayed outdoors.
  This covers 284 placements, including roads, traffic lights and LOD buildings
  in downtown Los Santos that were previously missing and left a large hole.
- `--smoke-view eyeX,eyeY,eyeZ,targetX,targetY,targetZ --capture-dir <directory>`
  captures an image from a fixed camera in the starting area to check the map.
- An area that reaches the geometry budget (30,000 placements / 4 million triangles)
  still loads: nearest details first, with the most distant scenery omitted.
- The next area is centered ahead of a moving car (up to 120 m), so the car
  does not reach the edge of the old area before the new one is ready.
- Original COL1/2/3/4 collision containers are decoded. A total of 8,255 original
  models were indexed. Triangles are used directly; boxes and spheres are triangulated.
- Visual geometry is used as a collision fallback for models without COL.
- First-person walking mode starts automatically. Recovery is provided after
  falling far below the map. P keeps free-fly mode available for exploration
  and debugging.
- Native mods: DFF, TXD, PNG, optional COL, placements and model replacement.
- DFF files with a leading UV animation dictionary are read with supported
  UV animation. Invalid UV coordinates receive a fallback value; invalid
  spatial data is rejected.
  A metadata audit decoded 12,955 out of 12,955 DFF files from the local
  `gta3.img`. This metadata audit demonstrates geometry loading, not support
  for every material effect.
- GPU uploads do not retain CPU copies of static, non-animated geometry.
- Neighborhood streaming reuses GPU textures from the active area while the
  same unchanged resource loader is still in use. Server/mod changes receive
  separate uploads so identical texture names with different images are not
  mixed up. CPU images are released incrementally within the upload time budget.
  A before/after test on RTX 3070/Vulkan along the same 1.2 km route with seven
  area changes reduced total uploads from 2871.6 to 871.5 MiB (about 70 percent).
  The number of upload frames dropped from 1100 to 417. This measures reduced
  work, not a guaranteed FPS or the absence of all driver/loading stutters.
- One active drivable car, selected from the vehicle menu's original and custom
  models, with a follow camera, terrain contact, wall collision and entry/exit.
  `--smoke-car` checks driving, braking, exiting and re-entering.
  The car's dynamic geometry is retained when neighborhoods change.
  Parked and paused frames now reuse the car's GPU geometry until it moves
  or rotates; model and session changes force a new update. The Skyline test
  went from 54.01 MiB to 0 MiB of repeated vehicle mesh uploads across
  30 parked frames. This measures eliminated work, not a specific FPS increase.
- Ground contact is measured from each wheel's expected height on the tilted car.
  The suspension also follows changes in road height while driving, so the
  lower axle alone does not pull the car down into the road. Uphill/downhill
  slopes, side slopes, crests and drops are tested using dedicated collision
  surfaces.
  The read-only check `sa-scene --example vehicle-slopes` checked 106 directions
  on sloped original surfaces around Grove Street, with a maximum deviation
  of 1.9 cm from the expected vehicle height. This is not an audit of the
  entire map.
- Separate fragment passes for alpha cutout and blending.
- `--smoke-tour`: a hidden native GPU test through nine areas.
- `--smoke-stream`: a continuous round trip of approximately 1.2 km using
  normal free-fly movement keys. Checks multiple neighborhood changes,
  finite coordinates and that installing a scene does not move the camera
  unexpectedly.
- Jumps are stopped by overhead ceilings, including hits at the body's outer edge.
  Pausing with a zero time step does not change the jumping/gravity state either.
- Small steps up to 38 cm are checked at the body's leading edge before wall
  collision resolution. Small drops are also walkable; adjacent ground within
  one step of the feet is not mistaken for a low roof. Sideways entry is blocked
  if there is not enough clearance for the body's full height.
- Collision tests cover three short stairs in both directions, walking off a
  platform and landing on a lower floor, and separate walkable levels for a
  bridge deck and the tunnel below it.
- `--probe-walk`: four directions from Grove Street using original collision,
  1200 physics steps per direction. Logs actual progress and ground contact.
- `--probe-curbs-ramps`: samples ground-height transitions around all nine
  smoke-tour destinations and walks across selected curb-like edges and
  ramp-like slopes in both directions, including diagonal curb approaches.
  Blocked samples report map coordinates for follow-up; sampled terrain alone
  does not confirm that every candidate is a sidewalk or intentional ramp.
- `--capture-dir PATH` used with a smoke test saves private rendered images.

## Verification

The development loop uses `cargo test --workspace`, `--probe-world` and
`--smoke-tour`. The GPU test waits for successful rendering, loads nine areas
and checks finite player positions. Images are stored privately under
`native/target/captures/`. The Grove Street image has been visually inspected.

Selected results before/after fixing IPL flags, measured using 81 ground
probes per area: countryside 27 to 81, Las Venturas 62 to 81, desert 11 to 74.
This demonstrates ground support at the sampled points, not that all roads,
bridges or stairs are free of errors. Some samples may lie on water or steep
surfaces. The first nine-area test took approximately 0.2–1.1 seconds per CPU load.

The IPL regression test failed as expected against the old filtering.
A separate generated room is tested through the native mod loader and
collision system: the player can walk through the doorway and stand on
the floor. Test data contains no game assets.

## Remaining work

Vehicle physics is still basic: simple suspension and tilt are implemented,
but further tuning, wheel animation, damage and traffic remain.
One active custom DFF/TXD car can replace the F9 car through `vehicles`
in the mod format. The follow camera is tested against walls, ceilings and terrain.
F9 tries several vehicle placements with ground support under the footprint,
body clearance and headroom; obstructed rotation during turns is rejected.
Exiting tries several sides and requires a floor, body clearance and an
unobstructed path out. This uses the same COL/visual fallback geometry as
other collision, with a limited number of samples.
The player now has a visible `fam1` character with idle, walk and run animations
in third person. V switches between first and third person. The camera
collides with the map. AI for other peds, proper jumping/swimming/vehicle-seat
animations and transition blending remain to be implemented. Multiplayer is
available as a prototype; personal parked cars, passenger reservations and
custom appearances are shared. Switching cars and shared collision
simulation remain to be implemented.
The asset layer now has a separate Skin/HAnim loader and CPU skinning with
bone weights and inverse bind matrices. The bind pose has been checked
against 265 original peds in the local archive. This is now used by player
rendering, and one custom skinned player character can replace it through
`player` in the mod format.
Separate skinned clothing is supported through `player.clothes` with compatible
HAnim bones and bind matrices. The wardrobe (F6) lets you toggle imported
clothing items during gameplay. Body-part masks and assembly of original
CJ components remain to be implemented. Outfits are not yet saved between
game sessions.
ANP3 clips can now be sampled and mapped to the skeleton by bone ID. All
294 clips in the local `ped.ifp` have been read and checked with five valid
sample poses per clip. Walk, idle, run and sprint match all 32 tracks to the
skeleton. Live idle and run animations have been visually checked in GPU
images. `--smoke-ped` tests 20 meters of walking/running, stopping and camera
switching with a controlled time step.
FiveM/GTA V resources are not loaded directly. An experimental
[converter](gta5-conversion.md) can prepare supported Legacy models as
native resources. Custom DFF rooms can be placed with collision, but this
is not a complete GTA V MLO/portal/interior engine.
The original interior archive is now also read. Separate room loading and
600 steps of walking/jumping have been checked in CJ's house, Sweet's house
and Madd Dogg's mansion. The interior menu (I) and returning via R/the map
have been GPU-tested. Doorway entrances remain to be implemented; see
`native-interiors.md` for verification and limitations.

The map has detail streaming with preliminary distant LOD. Waves, diving,
timed objects, some RenderWare effects, transparency sorting and more
accurate physics remain to be implemented. Models may be skipped with an
explanation in the console, and some materials receive a fallback color.
COL spheres use a triangle approximation, and body collision uses a limited
number of samples instead of a full capsule sweep.

The next milestone is driving together with friends in your own cars. See
[priorities and criteria](roadmap.md). Streaming, performance and further
vehicle physics tuning remain ongoing quality requirements.

Format references: [COL](https://gtamods.com/wiki/Collision_File),
[IPL flags](https://gtaundergroundmod.com/pages/ug-mp/documentation/dl/map-dl/ipl/inst).

Driving physics now uses inertia, gradual steering and lateral tire grip.
See [vehicle dynamics](vehicle-dynamics.md) for details and limitations.

The graphics menu has separate categories, quality presets and working FSR 1.
See [graphics status](graphics.md) and [Linux/Vulkan](linux.md).
DLSS and temporal FSR are still not integrated.

Original engine audio and local footsteps are integrated. Engines can also
be heard from nearby multiplayer cars, including the car you are riding in
as a passenger. Volume settings apply to these sounds. Collision sounds,
material-dependent footsteps and original radio remain to be implemented;
see [audio status](native-audio.md).

Vehicle physics uses axle-based tire forces, shared grip for braking/cornering,
four ground contacts and damped suspension/body movement. Steering direction
with keyboard and controller has been corrected. Damage, animated wheels and
full rigid-body collision are still not simulated.

A local sample pack of 32 textures upscaled 2x has been created and tested
in Vulkan. This is not an upscale of the entire map or an FPS measurement.
The [texture-upscaling tool](texture-upscaling.md) creates PNG overrides as a
separate native mod; the original files are not modified. Generated game
textures remain local and are not included in the Git repository.
