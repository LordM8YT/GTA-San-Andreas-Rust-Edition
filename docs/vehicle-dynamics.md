# Vehicle dynamics

The native car now uses a lightweight tire-slip model inspired by the heavier
driving feel requested for this project. This is an original approximation,
not the GTA IV or GTA V physics engine.

- Horizontal momentum is independent of body heading, allowing sideways slip.
- Steering moves gradually and steering lock decreases at higher speeds.
- Body rotation has inertia and a lateral acceleration limit.
- Tire grip opposes sideways motion; the handbrake reduces grip and slows the car.
- Front/rear axle slip produces lateral forces and yaw torque. Braking and
  cornering share a limited tire-grip budget.
- Four ground probes use each wheel's expected height on the tilted body.
  Damped suspension follows road height and grade; body pitch and roll respond
  to slopes, acceleration and cornering. Local and remote rendering converts
  independent forward/side grade angles into one rotation, avoiding excess
  side tilt on roads that are both sloped and banked.
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
prototype's default driving setup. Original cars use these defaults unless a resource supplies an original-car profile.
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
| `suspension_spring` | 140 | 40–250 | Height restoring acceleration coefficient, s⁻² |
| `suspension_damping` | 22 | 8–40 | Vertical velocity damping coefficient, s⁻¹ |
| `aerodynamic_drag` | 0.0035 | 0–0.02 | Quadratic speed resistance coefficient, m⁻¹ |

The existing Vehicle handling setting multiplies the resource's tire grip.
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
