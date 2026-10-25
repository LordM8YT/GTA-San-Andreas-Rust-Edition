# GTA:SA's car camera, for SARE

While driving, SARE places the view at `self.position` and looks at the car
(`view_position` in `crates/runtime/src/main.rs`), so the camera sits inside the
car. San Andreas is third person: a boom about 7.8 m behind and 1.3 m above the
car, orbiting it. This document describes that camera precisely enough to
implement it, and why it is built the way it is.

As in the handling document, *fitted* marks a number that was matched against
recorded telemetry rather than read out of the binary.

## The idea: a polar boom, not a spring arm

GTA's car camera keeps three numbers as state:

- `beta` — horizontal orbit angle relative to a smoothed heading (0 = directly
  behind the car)
- `alpha` — elevation of the boom above the look-at target
- `distance` — boom length, derived from the car's collision size

Every frame the eye is **computed** from them, not simulated:

```
target = carPosition + worldUp * targetHeight
dir    = (normalize(-cos(beta) * F_chase + sin(beta) * R_chase) * cos(alpha))
       + worldUp * sin(alpha)
eye    = target + dir * distance
```

Smoothing angles instead of positions is the whole trick: the eye never cuts a
corner through a wall, never bobs with the physics rate, and the framing is
identical at any frame rate.

## Numbers, and how they scale with the car

For the Infernus (model 411), from the US 1.0 binary and the SACarCam table:

```
rearOffset   = 7.7721
eyeHeight    = 1.2789416
targetHeight = 0.49163935355306165
fov          = 70 degrees
defaultAlpha = atan2(eyeHeight - targetHeight, rearOffset) = 0.10096 rad
defaultDist  = hypot(rearOffset, eyeHeight - targetHeight)  = 7.8119
```

`CCam::Process_FollowCar_SA` (0x5245B0) scales the boom with the car rather than
using a fixed length:

```
fDist      = 2 * |col.min.y|          // collision length
fVehHeight = col.max.z
fCamDist   = fCamBase + tweakDist * fDist        // tweakDist = 1
fHgtAdj    = fVehHeight * 1.3 - 0.4              // cars row of the table
if fHgtAdj > 0: target.z += fHgtAdj; fCamDist += fHgtAdj
```

A Roadtrain is roughly twice as long and tall as an Infernus; with a fixed boom
the camera ends up inside the cab.

`CARCAM_SET`, cars row (0x8CC600):
`{1.3, 1.0, 0.4, 10, 15, 0.5, 1.0, 1.0, 0.85, 0.2, 0.075, 0.05, 0.8, 0.785398,
1.5533431}`. The entries this implementation uses:

| Entry | Value | Meaning |
| --- | --- | --- |
| `[5]` | 0.5 | elevation servo blend base: `alpha += (1 - 0.5^dt) * (target - alpha)` |
| `[6]` | 1.0 | cap on the servo step per timestep, radians |
| `[8]` | 0.85 | angular-speed decay of the yaw follower |
| `[9]` | 0.2 | maximum yaw speed per timestep, radians |
| `[10]` | 0.075 | lateral-velocity influence on the target yaw |
| `[11]` | 0.05 | clamp on that per-step nudge |
| `[12]` | 0.8 | manual-input multiplier |
| `[13]` | 0.785398 | lower elevation limit, -pi/4 |
| `[14]` | 1.5533431 | upper elevation limit, ~89 degrees |

## State machine

**Heading.** The boom is attached to a smoothed copy of the car's heading, not
the live one. Attach it to the live heading and every yaw wiggle whips the
camera. The follower is a critically damped filter:

```
omega      = sqrt(-ln(0.85))              // 0.403
maxError   = 2 * 0.2 / omega              // displacement limit, not velocity
displ      = -clamp(error, +-maxError)
decay      = exp(-omega * step)
delta      = displ * (decay - 1) + (speed + omega * displ) * step * decay
speed'     = (speed - omega * (speed + omega * displ) * step) * decay
if |delta| >= |error| and same sign: delta = error, speed' = 0
```

The last two lines are the crossing guard: a car that rotates faster than the
boom can follow parks the boom exactly behind it instead of ringing.

**Elevation.** The target elevation follows the car's pitch and flight path
through an asymmetric lag (*fitted*): attack 0.2 s when the pitch magnitude
grows, release 0.3 s when it relaxes. The input is a blend between car attitude
and flight-path angle, weighted by how far apart they are, saturating at 0.26 rad
(*fitted*). The elevation itself then moves with the `[5]`/`[6]` position servo
above, clamped to `[-pi/4, 1.5533431]`.

That asymmetric lag matters on crests: the nose drops long before the car stops
rising, and the game keeps looking up at the still-rising car.

**Manual look.**

- Mouse: `beta += -dx * fov * 0.0125 * 0.0042`, `alpha += -dy * ...`, then a
  **1 second hold** (50 timesteps at 50 Hz) during which the auto-chase is
  suppressed. After the hold the chase takes beta back behind the car.
- Analog side look (Numpad 4/6 in the original): a quadratic stick rate of
  2.5 rad/s that stops at +/-90 degrees. It cannot orbit the boom.
- Look up/down: a hitch-capped step, with the upward influence halved, and the
  automatic elevation target **frozen** while the key is held.

## Occlusion

One centre ray, not a cone:

1. Cast from `carPosition + carUp * 0.75` toward the desired eye.
2. Ignore hits closer than the car's own footprint (~2.8 m for the Infernus,
   scaled by length). Road and ramp surfaces under the bumper would otherwise
   collapse the boom onto the exhaust.
3. Pull the eye to `hit - 0.2 m`, never closer than `minDistance`.
4. **Only map objects block while the car is slow**: `CameraVehicleModeSpecialCases`
   (0x50CDE0) enables object collision only when move speed is <= 0.2 GTA units,
   which is 10 m/s. Above that, a thin ray grazing a track slab must not move the
   camera at speed.

Two things that look like improvements and are not, both measured on a replay
corpus:

- A ring of parallel rays: in curved tunnels the ring alternately clips and the
  boom yo-yos (26 jumps over one loop window against 0 with a single ray).
- Smoothing the shared pose to hide judder: it also steers the occlusion ray, so
  framing swims by 50-100x. Smooth the angles, pass the pose through untouched.

There is a separate guard for a camera that ends up *inside* geometry, which a
segment-capped ray cannot see (a ray that ends inside solid rock reports clear):
probe +/-x, +/-y, +/-z from the resolved eye within a metre; if five or more hit,
walk the eye back toward the car until it breathes. Proximity alone never
triggers it.

The near plane is driven by a separate set of diagonal clearance probes
(`ImproveNearClip`, 0x516B20); they adjust the near plane and never move the eye.

## Fitting it into SARE

SARE uses Y up and -Z forward; GTA uses Z up and +Y forward. The conversion is a
sign flip and an axis swap, and it is worth keeping in one place:

```
gta(x, y, z) -> sare(x, z, -y)        // position
```

Suggested shape, all of it independent of the renderer:

```rust
pub struct CarCamera {
    beta: f32, beta_speed: f32,
    alpha: f32,
    heading: f32,        // smoothed, radians about the Y axis
    pitch: f32,          // lagged elevation input
    mouse_hold: f32,     // seconds remaining
}

pub fn update(&mut self, car: &CameraCar, input: &CameraInput, dt: f32) -> CameraPose {
    // target, polar boom, servos; eye and target returned in world space
}
```

The camera needs only: car position, orientation, velocity, and the model's
collision length and height. It owns no physics and must not write to the car.

Suggested checks, in the same style as the rest of `sa-scene`:

- at rest, the eye sits `rearOffset` behind and `eyeHeight` above the car
  origin, with the target `targetHeight` above it, at 70 degrees FOV;
- a car spinning at 3 rad/s brings the boom back behind it within a second and
  never overshoots the target heading;
- a wall 5 m behind the car shortens the boom; a wall 2 m behind it (inside the
  footprint) does not;
- the mouse freezes beta for 1 s and it returns afterwards;
- at speed above 10 m/s, map geometry does not move the camera.

## Provenance

The framing, the servo constants and the occlusion classification are read out
of the US 1.0 build. The pitch-lag time constants, the flight-path blend angle
and the saturating yaw chase were fitted against a 21,291-frame MTA:SA camera
oracle and should be treated as tunable rather than exact.
