# GTA:SA's car camera, and where SARE's differs

SARE already has a car chase camera. In the driving branch of
`crates/runtime/src/main.rs` it is:

```rust
let elevation = (0.45 - self.pitch).clamp(-0.1, 1.4);
let behind = Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos());
let offset = (-behind * elevation.cos() + Vec3::Y * elevation.sin()) * 7.8;
self.position = world.clip_camera(car.position + Vec3::Y, car.position + Vec3::Y + offset);
```

so: a 7.8 m boom, elevation 0.45 rad by default, a look-at target 1 m above the
car origin, mouse orbit with a 1.5 s recenter, and a single clipped ray for
occlusion. That is the same *shape* as San Andreas. What follows is what the
original does differently, with the numbers needed to close the gap, and why
some tempting "fixes" are traps.

As in the handling document, *fitted* marks a number that was matched against
recorded telemetry rather than read out of the binary.

## The model GTA uses: a polar boom, not an offset

GTA keeps three numbers as camera state and computes the eye from them:

- `beta` — horizontal orbit angle relative to a smoothed heading (0 = directly
  behind the car)
- `alpha` — elevation of the boom above the look-at target
- `distance` — boom length, derived from the car's collision size

```
target = carPosition + worldUp * targetHeight
dir    = (normalize(-cos(beta) * F_chase + sin(beta) * R_chase) * cos(alpha))
       + worldUp * sin(alpha)
eye    = target + dir * distance
```

SARE's version is the same equation with `beta = 0` (folded into `self.yaw`),
`alpha = 0.45 - pitch`, and `distance = 7.8`. The structural difference is that
GTA smooths `beta` and `alpha` as state; SARE's orbit *is* the mouse-driven yaw
and pitch plus a recenter, so a mouse flick moves the eye directly and the car's
own yaw is followed by the separate recenter blend.

## The numbers

For the Infernus (model 411), from the US 1.0 binary and the SACarCam table:

```
rearOffset   = 7.7721
eyeHeight    = 1.2789416        // above the car origin
targetHeight = 0.49163935355306165
fov          = 70 degrees
defaultAlpha = atan2(eyeHeight - targetHeight, rearOffset) = 0.10096 rad
defaultDist  = hypot(rearOffset, eyeHeight - targetHeight)  = 7.8119
```

`CCam::Process_FollowCar_SA` (0x5245B0) scales the boom with the car:

```
fDist      = 2 * |col.min.y|          // collision length
fVehHeight = col.max.z
fCamDist   = fCamBase + tweakDist * fDist        // tweakDist = 1
fHgtAdj    = fVehHeight * 1.3 - 0.4              // cars row of the table
if fHgtAdj > 0: target.z += fHgtAdj; fCamDist += fHgtAdj
```

So a Roadtrain gets a boom roughly twice as long as an Infernus. With a fixed
7.8 m boom a Roadtrain's camera ends up inside the cab.

`CARCAM_SET`, cars row (0x8CC600), entries this implementation uses:

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

## Measured delta against the current implementation

| Quantity | SARE now | GTA SA | Note |
| --- | ---: | ---: | --- |
| boom length | 7.8 m, fixed | 7.81 m (Infernus), scaled by car | same for a saloon, wrong for long vehicles |
| look-at target | 1.0 m above origin | 0.49 m | camera aims higher than the game |
| eye height | 1.0 + 7.8·sin(0.45) = **4.39 m** | **1.28 m** | the biggest framing difference |
| eye behind | 7.8·cos(0.45) = 7.02 m | 7.77 m | slightly closer and a lot higher |
| FOV | settings default | 70 degrees | cosmetic, differs per user setting |
| recenter | after 1.5 s, ease 5/s | ~1 s mouse hold, then servo | close enough to leave alone |
| occlusion | one clipped ray | one ray + footprint ignore + speed gate | see below |
| car-size scaling | none | length and height | matters for trucks and long cars |

The eye-height gap is what people feel as "the camera looks down on the car".
Running the runtime's own `--smoke-car` capture with only those two numbers
changed turns the current high, looking-down framing into the game's low boom
that sits just above the roof. I left the stills out of the tree because they
contain original map and vehicle art; happy to share them in the conversation.

Two lines close it:

```rust
let elevation = (0.10 - self.pitch).clamp(-0.1, 1.4);          // was 0.45
let target = car.position + Vec3::Y * 0.49;                    // was Vec3::Y
self.position = world.clip_camera(target, target + offset);
```

That puts the eye at `0.49 + 7.8·sin(0.10) = 1.27 m` above the car origin and
7.76 m behind it, which is the Infernus framing to within 2 cm. It is a
deliberate change of feel, so it belongs behind a setting if it lands at all.

## State machine (for reference)

**Heading.** The boom is attached to a smoothed copy of the car's heading, not
the live one, because attaching it to the live heading makes every yaw wiggle
whip the camera. The follower is critically damped:

```
omega      = sqrt(-ln(0.85))              // 0.403
maxError   = 2 * 0.2 / omega              // displacement limit, not velocity
displ      = -clamp(error, +-maxError)
decay      = exp(-omega * step)
delta      = displ * (decay - 1) + (speed + omega * displ) * step * decay
speed'     = (speed - omega * (speed + omega * displ) * step) * decay
if |delta| >= |error| and same sign: delta = error, speed' = 0
```

The last two lines are the crossing guard: a car rotating faster than the boom
can follow parks it exactly behind instead of ringing.

**Elevation.** The target follows the car's pitch and flight path through an
asymmetric lag (*fitted*): attack 0.2 s when the pitch magnitude grows, release
0.3 s when it relaxes. The input is a blend of car attitude and flight-path
angle, saturating at 0.26 rad (*fitted*). The elevation then moves with the
`[5]`/`[6]` position servo, clamped to `[-pi/4, 1.5533431]`.

This matters on crests: the nose drops long before the car stops rising, and
the game keeps looking up at the still-rising car.

**Manual look.** Mouse: `beta += -dx * fov * 0.0125 * 0.0042` and likewise for
`alpha`, then a one second hold (50 timesteps at 50 Hz) before the auto-chase
resumes. Analog side look: a quadratic stick rate of 2.5 rad/s that stops at
+/-90 degrees; it cannot orbit the boom. Look up/down: a hitch-capped step with
the upward influence halved, freezing the automatic elevation target while held.

## Occlusion

GTA casts one centre ray:

1. from `carPosition + carUp * 0.75` toward the desired eye;
2. ignoring hits closer than the car's own footprint (~2.8 m for the Infernus,
   scaled by length), so road under the bumper does not collapse the boom;
3. placing the eye at `hit - 0.2 m`, never closer than `minDistance`;
4. and **only letting map objects block while the car is slow**:
   `CameraVehicleModeSpecialCases` (0x50CDE0) enables object collision only when
   move speed is <= 0.2 GTA units, which is 10 m/s.

`clip_camera` already does (1) and a version of (3). It does not do (2) or (4),
so a kerb, ramp lip or near slab behind the car can pull the camera in at speed.
Adding the footprint test is small: reject hits closer than 2.8 m before using
them. The speed gate is a design choice for a game with script objects
everywhere; GTA needs it because every MTA map object would otherwise grab the
boom at speed.

Two things that look like improvements and are not, both measured on a replay
corpus:

- A ring of parallel rays: in curved tunnels the ring alternately clips and the
  boom yo-yos (26 jumps over one loop window against 0 for a single ray).
- Smoothing the shared pose to hide judder: it also steers the occlusion ray, so
  framing swims by 50-100x. Smooth the angles, pass the pose through untouched.

The near plane is driven by separate diagonal clearance probes
(`ImproveNearClip`, 0x516B20); they adjust the near plane and never move the eye.

## Provenance

The framing, the servo constants and the occlusion classification are read out
of the US 1.0 build. The pitch-lag time constants, the flight-path blend angle
and the saturating yaw chase were fitted against a 21,291-frame MTA:SA camera
oracle and should be treated as tunable rather than exact.
