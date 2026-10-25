# GTA:SA automobile handling, for SARE's car model

SARE's current car is a temporary approximation, not the target implementation.
The project goal is a faithful remaster/port: preserve GTA:SA vehicle behaviour while
replacing the original engine implementation with modern Rust code. The original
game is the behavioural specification; SARE's existing model must not be treated
as the reference when it disagrees with GTA:SA. This document records the reference
behaviour and the gaps that must be closed in `crates/scene/src/vehicle.rs`.

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

SARE currently has a per-body spring/damper rather than GTA:SA's four-wheel
suspension processing. This is a fidelity gap to port, not an optional enhancement.
The port must preserve GTA's spring/damper order, contact-point velocity and
force/moment calculations rather than substitute a visually similar approximation.

## Transmission

`ConvertDataToGameUnits` builds the gear table once per handling record:

```
engineAcceleration *= 0.4                       // at load
engineAccelGame = engineAcceleration / 2500
maxFlat: start at km/h * 0.277778/50, subtract 0.01
         until maxFlat^2 * drag/2 / 1000 <= engineAccelGame / 6
maxVelocity = maxFlat * 1.2                     // 4WD/2WD cars
engineAccelGame /= 4 for 4WD, else 2            // after maxFlat calculation
engineAccelerationSI = engineAccelGame * 50^2
gears 1..N: band(g) = g * (maxVelocity - half) / n + half, half = maxVelocity / 2n
gear 0 is reverse, with maxVelocity = -maxFlat * 0.3 (minimum magnitude 0.2)
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

The original state latches: once any wheel is `Spinning`, `Skidding` or `Fixed`,
the process-global `bAlreadySkidding` is set, traction loss stops applying for
subsequent wheels that tick, and the tyre-temperature penalty is skipped. SARE now
keeps per-wheel states and mirrors this ordering with a per-car per-tick latch; it
does not yet reproduce the original process-global storage or tyre temperature.

`ProcessWheel` also applies anti-dive by changing the force direction used for
turn-force calculation (full multiplier when braking, half when driving). SARE's
loaded-handling path now applies that contact-point turn-force correction, but
the body's pitch and roll still use an approximate shared spring/damper model;
that is not yet a complete suspension/anti-dive port.

## Port status and remaining fidelity gaps

The current main branch has begun replacing the old axle model for vehicles loaded
from GTA:SA handling.cfg. This is a staged port, not yet full behavioural parity.

Implemented in the current code:
- Preserve original mass, turn mass, centre of mass, drag, traction multiplier/loss/bias,
  drive type, engine type/inertia, brake bias/ABS, steering lock and suspension values.
- Convert maxVelocity, engine acceleration, brake deceleration and reverse speed
  using GTA's unit conversion and drag-limited maxFlat calculation.
- Use GTA's gear-band and shift-window fractions, separate reverse gear, handling-flag
  acceleration boost, and the two per-car transmission inertia smoothing values.
- Use the sign-preserving squared steering input, steering-lock conversion and
  tarmac speed limiter.
- Use a per-wheel correction model for loaded handling: drive layout, traction and
  brake bias, wheel state, traction-loss clipping, handbrake, anti-dive moment,
  centre-of-mass offset, effective mass and turn force.
- Use GTA's velocity-based linear drag formula for loaded handling records.

Still required before claiming a faithful port:
1. **Per-wheel suspension forces:** replace the shared body spring/damper with GTA's
   four spring passes followed by four damper passes, using collision normals,
   contact-point velocities, surface velocity and the original force clamps.
2. **Rigid-body state:** store linear and angular velocity as vectors; preserve GTA's
   model-origin/centre-of-mass distinction, integrate turn forces on all axes, and
   re-orthonormalise the body basis in the original order. The current body still
   uses scalar yaw plus approximate pitch/roll.
3. **Real collision surfaces:** feed actual material adhesion, wetness, wheel normals
   and contact entities into steering and wheel forces. The current GTA wheel branch
   uses a provisional tarmac adhesion value and estimated suspension compression.
4. **Wheel lifecycle and special cases:** port wheel-count persistence, tyre temperature,
   burst tyres, burnout, NOS, rear-wheel steering, off-road flags and vehicle-specific
   exceptions. The current four contact probes are not the original suspension lines.
5. **Low-speed and airborne rules:** port the rest-stop snap, air resistance/torque,
   wheel contact grace period, ramps, landing response and exact collision ordering.
6. **Reference tests:** compare telemetry against GTA:SA/MTA for acceleration traces,
   shift points, coast-down, braking distance, turning radius, countersteering, wheel
   lift, suspension travel and landings. Unit tests alone cannot prove parity.

The reference implementation used while porting is the open-source GTA Reversed
decompilation, especially cHandlingDataMgr::ConvertDataToGameUnits,
cTransmission::CalculateDriveAcceleration, CAutomobile::ProcessControlInputs,
CAutomobile::ProcessCarWheelPair, CVehicle::ProcessWheel,
CAutomobile::ProcessSuspension and CPhysical::ApplyTurnForce. The target is the
original game's observable behaviour, not a redesign of the physics.

## Compatibility rules

- Preserve GTA's gameplay and vehicle behaviour; do not "improve" or rebalance it.
- Convert units at the boundary, but retain the original formulae and ordering.
- Keep modern rendering, resource handling and performance improvements separate
  from simulation behaviour.
- Treat any deviation as a bug unless a known original-game quirk is deliberately
  reproduced and covered by a reference test.

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
