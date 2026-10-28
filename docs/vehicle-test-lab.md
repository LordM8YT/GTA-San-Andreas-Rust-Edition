# Vehicle Test Lab

## Goal

Create a repeatable, developer-facing place to evaluate SARE's vehicle physics without changing normal freeroam gameplay. The lab should make regressions visible and let developers compare the classic vehicle model with the experimental 3D body.

The first executable milestone is a headless, procedural regression course in `native/crates/scene/examples/vehicle-test-lab.rs`. It exercises flat-ground suspension, an oblique wall impact, and ramp flight/landing. It is not yet a rendered in-game test map and does not include the telemetry overlay.

## First milestone: a small test course

Build a compact test area from original procedural geometry or project-owned assets. Do not include Rockstar game files or converted/extracted copies.

The first version should prioritize clear, isolated tests over visual polish:

1. **Suspension lane** — flat ground, repeated low bumps, one-wheel bump, shallow dip, and offset ramps to exercise wheel contact and body response.
2. **Ramp and landing lane** — small and large ramps, a long jump, and safe landing space for four-wheel, side, and roof landings.
3. **Grip pad** — a constant-radius circle and slalom cones for steering, lateral slip, handbrake, and repeatable cornering.
4. **Surface comparison** — equivalent marked lanes with distinct surface types only where the collision/material system can reliably provide different grip. Do not fake different grip values before surface identity is wired into physics.
5. **Collision lane** — broad wall, angled wall, and thin-wall cases, with a safe reset point and marked approach distance.
6. **Slope and rollover area** — uphill/downhill grades, side slopes, and a flat recovery area.

Keep the test course isolated from ordinary mission/freeroam content and make it opt-in for developers.

## Developer controls

The minimum useful controls are:

- Spawn or reset the test vehicle at a known start point.
- Select classic or experimental 3D physics when both are available.
- Restart the current test without restarting the game.
- Toggle a developer-only telemetry overlay.
- Show the current test name and a short instruction.

Avoid adding permanent player-facing UI or changing the default physics mode as part of this work.

## Telemetry

Start with values already available from the simulation; only add instrumentation where a trustworthy value can be exposed.

- Vehicle position, speed, and vertical speed.
- Body pitch/roll and whether the vehicle is airborne.
- Per-wheel ground contact and suspension compression, if exposed by the physics implementation.
- Frame time/FPS and physics integration step.
- Test elapsed time and reset count.

Label unavailable values as unavailable rather than estimating them. Keep telemetry developer-only.

## Repeatable checks

After the course exists, add deterministic tests for cases that can run without rendering:

- Suspension remains supported on flat ground and moderate slopes.
- A bump on one side produces a bounded body response.
- Airborne motion preserves vertical momentum and returns to ground support.
- A car hitting a wall does not tunnel through it at the tested speeds.
- The body can land on its side/roof without NaNs or unbounded spin.
- Results are stable across supported frame-step sizes.

Each check should state its setup, expected invariant, and tolerance. Avoid asserting that the physics exactly matches GTA or another game unless a measured reference and acceptance tolerance are documented.

## Current headless milestone

Run from the repository root:

```sh
cargo run -p sa-scene --example vehicle-test-lab
```

The example builds collision geometry from project-owned triangles at runtime, then runs fixed-step checks for flat-ground suspension, oblique wall response, and ramp launch/landing. A failed invariant returns an error and exits non-zero. These checks are useful as a quick local smoke test; they do not replace the full test suite or hardware testing. The example has been authored through the GitHub connector but has not yet been compiled in this environment.

## Suggested implementation order

1. Run the new `vehicle-test-lab` example and fix any compile or behavioral failures.
2. Expand the procedural course with bumps, side slopes, thin walls, and roof/side landing cases, reusing the existing vehicle test helpers where possible.
3. Add reset/spawn controls and a minimal developer-only telemetry overlay in-game.
4. Add repeatable non-rendering regression tests for the highest-risk collision and suspension cases.
5. Only after those are useful, add recorded runs/replay comparison and surface-grip tests.

## Acceptance criteria for the first usable version

- The lab is opt-in and does not alter normal gameplay or the default physics mode.
- The headless course uses project-owned/procedural geometry and adds no game assets.
- A developer can reset a vehicle and reproduce the same basic test setup.
- Telemetry identifies which physics mode is active and shows speed, body angles, and frame time.
- At least one automated regression test covers a previously fragile vehicle-physics case.
- CI and the project's required review process pass before merge.

## Out of scope for the first milestone

- DLSS/FSR/XeSS implementation.
- A polished public race track or player-facing tutorial.
- Replacing or merging the classic and experimental physics systems.
- Claims of cross-machine determinism before the simulation has been measured for it.
