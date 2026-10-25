# GTA:SA automobile handling, for SARE's car model

SARE's car is an original approximation, and a good one: a lateral-slip model
with a friction circle, independent momentum and a suspension that cannot pull a
car down. This document is not a proposal to replace it with a port of GTA:SA.
It is a map of what the original game actually does, which parts of it are worth
stealing, and how those parts map onto `crates/scene/src/vehicle.rs`.

The numbers and formulas here come from the US 1.0 build, cross-checked against
recorded MTA:SA telemetry by an independent reimplementation. Every claim that
was fitted to recorded data rather than read out of the binary is marked
*fitted*.

## Units first: GTA does not use metres per second

This is the single biggest source of "I ported the formula and it behaves
nothing like the game".

| GTA quantity | Unit | Conversion to SARE |
| --- | --- | --- |
| `m_vecMoveSpeed` | GTA units **per physics tick** | `* 50` for m/s at 50 Hz |
| `handling.cfg` `maxVelocity` | km/h | `* 0.277778 / 50` for GTA units |
| `handling.cfg` `engineAcceleration` | raw | `* 1/2500` for GTA units |
| gravity | `0.008` per tick | `* 50 * 50` = 20 m/s² at 50 Hz |
| `m_vecTurnSpeed` | radians per tick | `* 50` for rad/s |
| steering lock | degrees in the file | `.to_radians()` |
| `CTimer::GetTimeStep()` | 1.0 at 50 Hz, 0.5 at 100 Hz | not seconds; a scale |

Note the gravity figure: GTA's 0.008 per tick is about twice Earth gravity, which
is why San Andreas cars feel heavy and landings are abrupt. If you ever map the
handling stack into SARE's units, do not carry metric gravity across.

## The tick, in the game's order

The order is the specification. Every one of these steps is small; the fidelity
lives in running them in this sequence, once, per tick.

1. clear per-tick flags (`HasContacted`, `HasHitWall`, bogged-down)
2. `ProcessControlInputs`: resolve pedals from speed, ramp steering by `dt/5`
   toward the stick, detect burnout (both pedals held near standstill)
3. `NitrousControl`: boost/recharge, **before** the drivetrain
4. airborne control torque, gated on **last** tick's wheel contact
5. fold friction accumulators into velocity, clear them
6. gravity, then drag as `(1 - 0.0005 * dragMult * |v|)^dt`, then angular air
   resistance `* 0.99` once per call (not raised to `dt`)
7. normalise suspension compression per wheel, advance `m_WheelCounts`
8. `ProcessSuspension`: all springs, then all dampers
9. `cTransmission::CalculateDriveAcceleration`
10. steering angle `radians(lock) * rawSteer^2`, scaled by the speed limiter
11. rear-steer history, **before** the limiter is applied to the front angle
12. per wheel pair: bias, brake, tyre temperature, burnout torque, `ProcessWheel`
13. rear tyre temperature drift, rest-stop snap
14. integrate once: `position += v * dt`, rotate basis, re-offset the centre of
    mass, re-orthonormalise

## The rigid body, and why it is so simple

GTA has **one scalar inertia**, `turnMass`, for every axis. There is no inertia
tensor, no gyroscopic term and no constraint solver.

```
ApplyMoveForce(F)      : v += F / mass
ApplyTurnForce(F, p)   : w += ((p - com) x F) / turnMass
GetSpeed(p)            : v + w x (p - com)
EffectiveMass(p, d)    : 1 / (|(p - com) x d|^2 / turnMass + 1 / mass)
```

Three consequences that are easy to miss:

- A contact point's effective mass is not the car's mass. It is much lower off
  the centre of mass, which is why a wheel at the corner can apply a large
  correction without launching the car.
- Friction accumulates in separate `frictionMoveSpeed`/`frictionTurnSpeed`
  fields and is folded in at step 5, not applied directly. If you add friction
  straight into velocity, contact response changes character.
- The stored position is the **model origin**, not the centre of mass. After
  rotating the basis you also have to offset the origin by the rotated COM arm,
  or a spinning car pivots around its nose.

`CMatrix::Reorthogonalise` is not Gram-Schmidt: it rebuilds
`up = right x forward`, `right = forward x up`, `forward = up x right`. That
order is observable.

## Suspension

Two passes over the four wheels. Springs:

```
limit = (1 - compression) * min(dt, 3) * mass * forceLevel * axleBias * 0.016
F     = normal * limit
```

Dampers, using the contact speed **after** the springs of the same tick:

```
dir   = normal.z > 0.35 ? -normal : worldDown
dtc   = clamp(min(dt, 3) * dampingLevel, -0.25, 0.25)
speed = -(dtc * (pointSpeed - surfaceVelocity) . dir)   // then clipped so it
                                                        // never adds speed
F     = EffectiveMass(point, dir) * speed
F     = min(F, 0.999 * |springLimit|)
```

`axleBias` is `suspensionBias` at the front and `1 - suspensionBias` at the rear.
The `0.999 * springLimit` clamp is what stops a hard landing from injecting
energy, and the `normal.z > 0.35` switch is what makes a car settle on a slope
instead of skating.

SARE has a per-body spring/damper rather than four independent wheels. If you
want the next step up in fidelity, per-wheel load is the one to take: it gives
body roll, diagonal load transfer and lift-off oversteer without changing the
tire model.

## Transmission

`ConvertDataToGameUnits` builds the gear table once per handling record:

```
engineAcceleration *= 0.4                       // at load
accel = engineAcceleration * (1/2500) / (4 for 4WD, else 2)
maxFlat: start at km/h * 0.277778/50, subtract 0.01 until
         maxFlat^2 * drag/2 / 1000 <= accel / 6
maxVelocity = maxFlat * 1.2                     // 4WD/2WD cars
gears: band(g) = g * (maxVelocity - half) / n + half, half = maxVelocity / 2n
       up(g)   = 0.6667 * (band(g) - band(g-1)) + band(g-1)
       down(g) = 0.42   * (band(g-1) - band(g-2)) + band(g-2)
```

Two details that cost days in the reference implementation:

- The decompiled source prints the gear-change window as `maxVelocity/gears/3`.
  The binary loads `0.6667` from `.rdata`, and `1 - 0.6667` is what reproduces
  the settled idle value.
- `CalculateDriveAcceleration` carries two private values across ticks (`a6`,
  `a7` in the source). They are the smoothed inertia multiplier applied to the
  engine acceleration, and a car that has been parked on the ground since map
  load has a **settled** value, not zero. Spawning from zero makes the first
  second of acceleration wrong.

The per-gear multiplier is `((1 - (g-1)/(n-1))^2 * 3 + 1) * 0.4`, with `5` or `4`
instead of `3` when the handling flags say so, and reverse uses `4.5`. Nitro
doubles it, including in reverse.

## Wheels

`ProcessWheel` is the grip model, and it is not a spring. It picks a target
velocity correction along two axes and clips it against adhesion:

```
adhesion *= dt                   // this line has no min(dt, 3) clamp
braking = brake != 0; driving = !braking && thrust != 0
if driving:      fwd = thrust;  right = clamp(-lateralSpeed/ground, +-adhesion)
else if moving:  fwd = -forwardSpeed/ground, clamped by the brake or, when
                 coasting with no pedal, by wheelFriction/mass
if fwd^2 + right^2 > adhesion^2:
    state = Spinning if driving and 0.3*adhesion < |fwd| else Skidding
    scale both axes to adhesion * tractionLoss / length
```

The state latches: once any wheel is `Spinning`, `Skidding` or `Fixed`, the
process-global `bAlreadySkidding` is set, traction loss stops applying, and the
tyre-temperature penalty is skipped. That latch is what makes a slide *dig in*
instead of instantly recovering.

`ProcessWheel` also applies anti-dive: the braking or driving part of the force
is applied at the contact point with a reduced, differently-directed moment
(`0.4x` when braking, `0.2x` when driving), which is why GTA cars do not
nose-dive under hard braking. SARE already approximates this with the
`acceleration * grip * 0.012` pitch term, so the shapes are compatible.

## What SARE should take, in order of value per line of code

1. **Gear bands and shift points** — done in this change, opt-in through the
   `gears` field. A gearbox is the most noticeable missing behaviour in the
   current model, and it costs about 40 lines.
2. **Per-wheel normal load** — the suspension formula above, feeding the friction
   circle per axle instead of one global budget. This is what makes a car lean,
   lift a wheel and lose rear grip on a crest.
3. **The skid latch** — one boolean per car, cleared when all wheels are
   `Normal`. It turns a slide into something with a beginning and an end.
4. **Per-surface adhesion** — GTA reads an adhesive limit per material plus a
   wet multiplier. SARE has one grip value per car, so asphalt, grass and sand
   all feel the same.
5. **The rest-stop snap** — below 0.0045 GTA units of speed with no pedal and
   ground contact, velocity is zeroed and yaw is frozen. It removes the
   perpetual crawl at idle.

## What SARE does better, and should keep

- A real friction circle. GTA's per-wheel clipping is cruder and produces the
  famous "tank slapper" behaviour at the limit.
- Independent heading and momentum, so a car can actually slide sideways. GTA
  achieves this indirectly, through turn mass and wheel states.
- Metric units. Do not adopt GTA's per-tick velocities; convert the constants
  into SARE's units instead, as the gear change does.
- A step capped at 1/120 s. GTA is fixed at 50 Hz and its handling is
  inextricably tuned to that; SARE's model stays consistent across frame rates.

## Appendix: constants worth knowing

| Constant | Value | Where |
| --- | --- | --- |
| gravity | 0.008 per tick | `CPhysical::ProcessControl` |
| linear drag factor | 0.0005 | multiplied by `handling` drag coefficient |
| angular air resistance | 0.99 per call | `CPhysical::ApplyAirResistance` |
| spring coefficient | 0.016 | `ProcessSuspension` |
| damping window clamp | 0.25 | `ProcessSuspension` |
| damping vs spring clamp | 0.999 | `ProcessSuspension` |
| handbrake torque | 20000 | `ProcessCarWheelPair` |
| wheel-count persistence | 4 ticks at 50 Hz | `m_WheelCounts` |
| rest stop speed | 0.0045 | `ProcessControl` |
| front/ rear bias form | `2b` / `2 - 2b` | brakes, traction, suspension |
| nitro multiplier | 2.0, inertia 0.5 | `CalculateDriveAcceleration` |
| nitro active rate | 1/1000 per tick | `NitrousControl` |
| nitro recharge rate | 1/100 per tick | `NitrousControl` |

Function anchors, for reading alongside the decompilation: `ProcessControl`
0x6B1880, `ProcessControlInputs` 0x6AD690, `ProcessSuspension` 0x6AFB10,
`ProcessCarWheelPair` 0x6A4EC0, `ProcessWheel` 0x6D6C00,
`CalculateDriveAcceleration` 0x6D05E0, `ApplyMoveForce` 0x542B50,
`ApplyTurnForce` 0x542A50, `ApplyFriction` 0x5483D0, `ApplyMoveSpeed` 0x547B80,
`ApplyTurnSpeed` 0x542E20, `CMatrix::Reorthogonalise` 0x59B6A0.
