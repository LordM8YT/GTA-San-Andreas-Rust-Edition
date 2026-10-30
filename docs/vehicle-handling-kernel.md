# The car now runs on GTA:SA's own model

The car used to use a model written for this project: metres per second, one
parabola of engine acceleration against a `top_speed` cutoff, a friction circle
and four ground probes. It is now driven by the original game's automobile
kernel, ported to Rust as `native/crates/handling`.

This document is the map between the two: where the code lives, how the tuning
fields translate, and how to check the result against a recording.

## Where it lives

| File | What it is |
| --- | --- |
| `crates/handling/src/handling.rs` | `tHandlingData`: the handling record, flags, suspension geometry |
| `crates/handling/src/transmission.rs` | `cTransmission`: gear table and `CalculateDriveAcceleration` |
| `crates/handling/src/suspension.rs` | `CAutomobile::ProcessSuspension`: springs, then dampers |
| `crates/handling/src/wheel.rs` | `CVehicle::ProcessWheel`: grip, braking, skid states, anti-dive |
| `crates/handling/src/physical.rs` | `CPhysical`: forces, effective mass, friction, integration |
| `crates/handling/src/vehicle.rs` | One tick, in the game's order |
| `crates/scene/src/vehicle.rs` | `Car`: keeps its public surface, runs the kernel, speaks to the world |

The kernel has no dependencies and no engine types. It takes a handling record,
four wheel contacts and one tick of controls, and advances a body; everything
about the world stays in `scene`.

## One tick

`Car::step` splits the frame into substeps of at most 1/120 s, and each substep
does the same thing:

1. cast the four suspension lines through `CollisionWorld::segment_hit`, which
   returns the hit point and the surface normal;
2. turn the contact into a normalised compression: `clearance` below the model
   origin is half compression, and the kernel's convention runs 0 fully
   compressed to 1 fully extended;
3. `Vehicle::tick` with `ctimer_step = 50 * dt`, so a 1/120 s frame is 0.4167 of
   a GTA tick;
4. keep the chassis out of the ground;
5. push out of walls and take the impact.

The kernel itself runs, in order: pedal resolution, nitro, airborne control
torque, friction accumulators, gravity, drag, angular air resistance, wheel
contact preparation, suspension, gearbox, steering lock and its speed limiter,
wheel pairs, tyre temperature, rest-stop, integration.

## How the tuning fields translate

The resource-facing `Handling` keeps its fields and their ranges, so existing
profiles still load. They are now *targets* that are solved back onto GTA's
per-tick parameters rather than used directly:

| Field | Meaning now |
| --- | --- |
| `mass` (new) | kilograms, carried into the kernel as-is; per-tick forces scale with it |
| `turn_mass` (new) | the scalar GTA uses in place of an inertia tensor |
| `drive` (new) | 4 = all wheels, 2 = rear, 1 = front; comes from the handling row |
| `acceleration` | first-gear launch in m/s^2, converted to `engineAcceleration`: `a / (0.16 * firstGearMultiplier)` |
| `top_speed` | drag-limited top speed; the drag coefficient is solved by bisection until the kernel's own gear walk lands there |
| `brake_deceleration` | stop rate in m/s^2; GTA applies braking per wheel pair, so the stored value is `a / 5000` |
| `tire_grip` | scales GTA's `tractionMultiplier` and `surfaceAdhesiveLimit` about the values a saloon uses |
| `suspension_spring` | scales `suspensionForceLevel`; solved so the car rests at half travel, which keeps `clearance` the ride height the models were built around |
| `suspension_damping` | scales `suspensionDampingLevel` |
| `gears` | gear count, as before |
| `steering_lock` | degrees for `steeringLock` |

`handling.rs::from_line` now also takes mass, turn mass and drive type from the
row instead of discarding them.

## Checking it against a recording

`crates/scene/examples/dm-replay.rs` loads a DM map's collision, loads an
`mta-tas-dm` recording, starts the car from the recording's first frame and
replays only the recorded keys, then compares position frame by frame. Nothing
from the recording is fed back into the simulation.

```
cargo run --release -p sa-scene --example dm-replay -- \
    ../../browser-three/dist/maps/dm-skynet-vol6-profeta.json \
    ../../profeta_slip.tas 3000 "F:/GTA San Andreas"
```

The map is culled to the recorded path (80 m), because a full DM map is half a
million triangles. The fourth argument is optional and makes the replay use the
recorded model's own `handling.cfg` row.

## What the replay currently shows

On Profeta the car follows the recording to 5.3 m through the opening jump and
then stops, at a constant height with zero speed, against `plasticsgate1`
instances: 20 m by 9 cm decorative gates placed over the start. The recorded car
is 0.7 m lower and passes underneath. That is a collision-import question - which
instances are solid, and how thin geometry is treated - rather than a dynamics
one, and it is the next thing to fix before the map test means anything.

## Known gaps

- **Surfaces.** Everything is one material: no per-surface adhesion or wet grip
  yet. The kernel has the hooks (`surfaceAdhesiveLimit` per contact) but the
  collision world does not carry materials.
- **Moving entities.** Car-to-car contact still goes through the old
  `collide()` push rather than the kernel's collision response.
- **Body contacts.** The chassis is kept out of the ground by a push along
  world up, not by the game's sphere contacts against the collision mesh, so a
  hard landing is a little softer than the original.
- **Tunnelling at speed.** One push per frame; a very fast car can still pass
  through a thin wall between two ticks.


CI validation is required before this kernel is merged into the current main branch. The parity fixtures and known limitations above are part of the acceptance criteria.
