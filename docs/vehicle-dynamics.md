# Vehicle dynamics

The native car now uses a lightweight tire-slip model inspired by the heavier
driving feel requested for this project. This is an original approximation,
not the GTA IV or GTA V physics engine.

- Horizontal momentum is independent of body heading, allowing sideways slip.
- Steering moves gradually and steering lock decreases at higher speeds.
- Body rotation has inertia and a lateral acceleration limit.
- Tire grip opposes sideways motion; the handbrake reduces grip and slows the car.
- Front/rear axle slip produces lateral forces and yaw torque. Each axle shares
  one grip budget between cornering and the engine or brake force it carries,
  so rear-drive cars step out under power and front-heavy braking pushes wide.
- Steering into a slide wins back steering lock, so slides can be caught at speed.
- Four ground probes use each wheel's expected height on the tilted body.
  Damped suspension follows road height and grade; body pitch and roll respond
  to slopes, acceleration and cornering. Local and remote rendering converts
  independent forward/side grade angles into one rotation, avoiding excess
  side tilt on roads that are both sloped and banked.
- Suspension only pushes against the road; it cannot pull a rising car down.
  Ramp launches and fast hill crests preserve vertical momentum and fall under
  gravity, then regain suspension support on landing.
- Opposite throttle brakes before engaging reverse. Engine acceleration decreases
  near top speed, with rolling resistance and quadratic aerodynamic drag.
- Airborne cars retain momentum but cannot accelerate or steer through tire grip.
- Physics integrates at intervals no greater than 1/120 second. Existing swept
  body collision, ground contact, safe spawning and exit checks remain active.

W / RT accelerates. S / LT brakes, then reverses. Space / LB is the handbrake.
The existing Vehicle handling setting adjusts tire grip, not engine power.
Exiting the car or reaching the streamed map boundary clears sideways momentum.

Suspension and body pitch/roll are approximations with fixed contact spacing,
not independent simulated wheel bodies. This model does not yet simulate wheel
articulation/animation, gear changes, deformation, different road surfaces or physical collision
impulses. Solid obstacles still stop the car rather than producing a crash response.

Automated checks cover braking and reverse, handbrake slip, frame-rate consistency,
airborne traction, wall clearance, spawning and custom car compatibility, plus
uphill/downhill support, banked roads, crests and falling beyond suspension travel.
The read-only `sa-scene` example `vehicle-slopes` also checks sloping original
map surfaces around Grove Street, including virtual rendered wheel points at
four headings; it is not a whole-map collision audit. The latest run covered
212 orientations, with maximum body-height error 0.020 m and virtual rendered
wheel-height error 0.018 m. Wheel bodies remain rigid parts of the model, so
these checks do not claim independent tire/suspension simulation.

## Community vehicle tuning

Each entry in a native resource's `vehicles` array can include a `handling`
object. Omitted fields use the values below; omitting the object retains the
prototype's default driving setup. Original cars take their values from the
player's own handling.cfg (see below) unless a resource supplies an original-car profile.
Unknown field names, nonfinite numbers and out-of-range values reject the
resource instead of silently ignoring typos. Restart after editing a resource.

| Field | Default | Allowed range | Meaning |
| --- | ---: | ---: | --- |
| `acceleration` | 6.2 | 0.5–20 | Forward engine acceleration, m/s² |
| `reverse_acceleration` | 3.8 | 0.5–10 | Reverse engine acceleration, m/s² |
| `brake_deceleration` | 10.5 | 1–25 | Opposite-throttle braking, m/s² |
| `top_speed` | 48 | 5–90 | Forward engine cutoff, m/s; resistance lowers actual speed |
| `reverse_speed` | 10 | 1–25 | Reverse engine cutoff, m/s |
| `tire_grip` | 4.6 | 1–12 | Per-axle lateral acceleration budget, m/s² |
| `steering_lock` | 0.55 | 0.1–0.9 | Low-speed steering angle, radians |
| `steering_rate` | 1.7 | 0.3–4 | Steering movement rate, radians/s |
| `steer_fade_speed` | 16 | 8–40 | Speed, m/s, at which steering lock has fallen to half |
| `suspension_spring` | 140 | 40–250 | Height restoring acceleration coefficient, s⁻² |
| `suspension_damping` | 22 | 8–40 | Vertical velocity damping coefficient, s⁻¹ |
| `aerodynamic_drag` | 0.0035 | 0–0.02 | Quadratic speed resistance coefficient, m⁻¹ |
| `traction_loss` | 0.82 | 0.5–1 | Share of rear grip kept while sliding |
| `front_grip` | 1 | 0.4–1.6 | Front grip multiplier; the rear gets `2 - front_grip` |
| `drive` | 0 | 0–2 | Driven axle: 0 front, 1 rear, 2 both |
| `brake_front` | 1 | 0–1 | Share of braking taken by the front tyres |
| `engine_lag` | 0 | 0–1.5 | Seconds the engine takes to answer the throttle |
| `yaw_inertia` | 1 | 0.5–4 | How slowly the body starts and stops turning |
| `front_weight` | 0.5 | 0.35–0.65 | Share of weight on the front axle; it also sets each axle's distance from the centre of mass on a 2.7 m wheelbase, so a nose-heavy car runs wide |
| `cg_offset` | 0 | −0.5–0.5 | Metres added to the centre-of-gravity height |
| `handbrake` | 1 | 0–1 | 0 disables the handbrake |
| `air_control` | 0 | 0–4 | Mid-air turning from the steering, rad/s² |

Engine and brake force are capped at 2.7 × `tire_grip`, because tyres can't
push harder than they grip; a profile that raises braking past that needs more
`tire_grip` too. The existing Vehicle handling setting multiplies the resource's tire grip.
Tuning does not change wheelbase, tire spacing or collision shape. Ground
support cancels gravity before suspension damping, avoiding an artificial
ride-height offset that previously depended on the integration interval.

Handling travels in the server's immutable native resource manifest, so clients
load the same car setup along with its model. Editing it changes the resource
fingerprint; restart the host/server to publish the updated pack. This remains
client-simulated driving, without server authority or shared collision impulses.
The importer does not translate GTA V/FiveM `handling.meta`: its physical units
and underlying model differ. Add the native values after asset conversion, or
select an explicit native JSON profile during [FiveM folder import](gta5-conversion.md)
with `--native-handling MODEL=JSON`.

To tune installed original models without redistributing their assets, use a
manifest-only resource:

```json
{
  "schema_version": 2,
  "enabled": true,
  "name": "My driving setup",
  "original_vehicle_handling": {
    "taxi": { "acceleration": 7.4, "brake_deceleration": 12.0 },
    "infernus": { "acceleration": 9.5, "top_speed": 60.0 }
  }
}
```

Keys are lowercase model basenames (1–32 letters, digits or underscores), without
paths/extensions. Each resource supports 32 profiles, with 64 distinct profiles
across loaded resources. Later resources replace earlier profiles for the same
model; omitted fields in the winning profile use defaults. This changes cars
already loaded into the catalog and does not register additional original models
or affect custom DFF cars. Current original choices are Taxi, Infernus and Admiral.

The disabled [native handling demo](../mods/native-handling-demo/README.md)
provides a starting point. Server guests receive only its resource manifest and
load the models from their own installations. Disconnecting restores their
local car/tuning setup. Profiles are native values; original handling.cfg and
FiveM handling.meta are not automatically translated.

## Handling from your handling.cfg

Original cars read their row of `data/handling.cfg` from the player's own
install at load time. SARE uses only the column meanings listed in that file's
header. The units and every scale are SARE's own, tuned by watching how the
cars drive, not taken from the game's code. Mod profiles still replace the
whole result.

| handling.cfg column | SARE value |
| --- | --- |
| max velocity (read as km/h) | flat-out speed is 0.9 × that; the engine pull is solved so drag and rolling resistance balance there |
| engine acceleration, inertia, mass | `acceleration` = 0.34 × accel ÷ (1 + inertia/50) × (1500/mass)^0.3; `engine_lag` = 0.012 s × inertia |
| drag multiplier | `aerodynamic_drag` = 0.0004 × drag |
| traction multiplier, loss, bias | `tire_grip` = 6.0 × √(traction/0.75); `traction_loss` = 0.4 + 0.55 × loss; `front_grip` = 1 + 1.2 × (bias − 0.5) |
| drive type, brake decel, brake bias | `drive`; `brake_deceleration` = 1.2 × decel; `brake_front` = bias |
| mass, turn mass | `yaw_inertia` = turn mass ÷ (3.6 × mass) |
| centre of mass y, z | `front_weight` = 0.5 + y/2.7; `cg_offset` = z |
| steering lock (degrees) | `steering_lock`; `steering_rate` 3.5 rad/s and `steer_fade_speed` 22 m/s for every row |
| handling flags, NO_HANDBRAKE | `handbrake` = 0 |

Each result is clamped to the range in the table above. Cars from handling.cfg
also get `air_control` 1.5 rad/s². Reverse runs at up to a quarter of flat-out
speed (at most 12 m/s) with 60% of the forward pull. Gears, ABS, burnouts and
surface types are not modelled yet.

## Air control and burnouts

Cars from handling.cfg can be steered in mid-air. The Up/Down arrows,
numpad 8/2 or the left stick tip the nose down or up, A/D turn the car, and
Space + A/D (or LB + stick) roll it instead. W stays off the nose so holding
the gas over a jump doesn't tip the car. The car holds the tilt it has until the driver changes it,
up to 0.8 rad, and levels onto its wheels again after landing. Hand-tuned
profiles with `air_control` 0 keep levelling out in the air as before.

Holding both pedals (W + S, or RT + LT) below 3 m/s is a burnout. The brakes
hold the car while the engine revs and the wheels spin, leaving tyre marks,
and steering pivots it slowly on the spot. Letting go of the brake launches
the car with the engine already at full throttle, which matters most for cars
with a lot of engine inertia.

## Experimental 3D car body

Gameplay settings → Car physics → 3D (experimental) drives your own car with
`vehicle/rigid.rs` instead of the flat model. It is built from general
rigid-body and raycast-suspension physics with SARE's own constants. It reads
the same `Handling`.

- The body is a box with the car's size. Its turning inertia is scaled by
  `yaw_inertia`, and `front_weight` moves the centre of mass toward an axle.
- Each wheel looks straight down for the road. Its spring and damper
  (`suspension_spring`, `suspension_damping`, shared by four wheels) carry
  the car, so it squats, dives, leans and leaves the ground over crests.
- Tyres use the same grip budget as the flat model: per-axle grip,
  `traction_loss` once sliding, drive and brake shares, the handbrake and
  burnouts. Their forces act at half the ride height, as if through stiff
  anti-roll bars, so hard cornering leans the car without flipping it.
- Gravity acts on the whole body. Cars roll down hills when nobody holds the
  brake, and they lose speed climbing.
- Steps up to 0.35 m (kerbs, seams between road pieces) are left to the
  wheels; only taller things count as walls. Tyres push and brake 1.6 times
  harder than they corner, so cars climb steep hills.
- Walls push the body out and take an impulse with a little bounce and
  scraping friction. A glancing hit slides along the wall and spins the car
  instead of stopping it dead.
- Body corners that touch the ground hold the car up. A car can tip over,
  slide on its side and come to rest on its roof.
- Mid-air controls tip, turn and roll the whole body.
- A car lying on its side or roof, moving slower than 3 m/s, rocks with the
  steering (28 rad/s² of roll) until it tips back onto its wheels.

Other players still see your car with the flat pose angles, so a car on its
roof shows on its side for them.

## Tyre smoke

`runtime/src/tyre_smoke.rs` puffs grey smoke from the rear tyres. A burnout
smokes at full strength, and a slide smokes once the car slips sideways
faster than 4 m/s. There are up to 96 camera-facing puffs, each lasting 1.6 s,
in one dynamic buffer drawn with the blended pipeline and a generated soft
round texture.

## Stationary mesh updates

The local car now reuses its transformed GPU geometry while its position and
body angles remain unchanged, including parked and paused frames. Movement,
rotation and model/resource changes still update the buffers. The same pose
change test is used for remote parked cars. Switching/restoring session worlds
invalidates the local cache so different car meshes cannot reuse stale geometry.

A Vulkan check with the locally imported Skyline measured 30 redundant updates
(54.01 MiB) over 30 parked frames before the change and zero afterwards. Driving,
braking, exit/re-entry and multiplayer model changes/offline restoration were
checked separately. This measures avoided CPU/GPU work, not a guaranteed FPS.
Use `--smoke-car --smoke-idle-car` to run the route and assert parked updates
remain zero. Third-party models are kept outside the repository.

The chase camera eases toward the car's heading (about 0.2 s time constant)
instead of copying every yaw correction, and the radar heading follows it while
driving. Moving the mouse looks around; the view settles back behind the car.
