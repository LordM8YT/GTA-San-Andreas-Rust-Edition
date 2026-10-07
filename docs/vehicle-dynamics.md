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
  to slopes, acceleration and cornering.
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
map surfaces around Grove Street; it is not a whole-map collision audit.

## Community vehicle tuning

Each entry in a native resource's `vehicles` array can include a `handling`
object. Omitted fields use the values below; omitting the object retains the
prototype's default driving setup. Original cars currently use these defaults.
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
and underlying model differ. Add the native values after asset conversion.
