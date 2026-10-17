What this change does: Cars preserve upward momentum off ramps and hill crests, and unloaded tires no longer apply driving forces. The change also aligns slope collision rules, improves depth precision and controller input, and adds separate Intel and Apple Silicon Mac client packaging and updates.

Reviewed with the installed Ponytail Review skill, against local changes on top of `c2f19c46`. This is a skill-guided review, not a separate external review service. Expected load: one native client running physics and rendering each frame, within the project's existing 20-player session limit. The review covered the diff, connected physics/input/rendering code, package and updater paths, CI configuration, and relevant tests.

**Must fix — resolved**

1. **Unloaded tires still changed speed during takeoff** (`native/crates/scene/src/vehicle.rs`, `Car::integrate`)
   - **What this is:** Four wheel probes find nearby road surfaces. Their results control tire grip and suspension support.
   - **Problem:** A rising car could have zero suspension support while a road was still within probe reach. Throttle, brakes and steering could therefore change its motion in the air. In the reproduced braking case, speed fell to 9.095 m/s instead of the coasting car's 9.969 m/s.
   - **Fix:** Use the same positive suspension load to enable tire grip. Reuse the sampled road plane and support-force calculation for suspension and grip. A new test covers throttle, braking, handbrake and steering during takeoff.
   - **If we skip it:** Jumping cars retain a short period of artificial road control after losing support.

No remaining must-fix findings were found in the reviewed changes. Existing car damage, door animation, shadow rendering, wet-road reflections, and Windows/Linux server packaging were preserved while integrating the fixes with the latest branch.

Verification:

- The new takeoff test failed when its fix was temporarily removed, then passed with the fix restored.
- All 305 Rust workspace tests passed.
- Python tests completed: 58 run, one skipped, no failures.
- Clippy passed for the workspace and all targets with warnings treated as errors.
- The Windows release build passed.
- The final GPU smoke test passed driving, braking, exit, animated re-entry and parked-car checks. Its timing was updated to wait for the current door animations.
- The original-map slope audit passed 212 vehicle orientations; maximum body-height error was 0.020 m and rendered wheel error was 0.018 m.
- Git confirmed the local base matches `origin/main` at `c2f19c46`.

Verdict: Ship.

Not checked: Actual macOS CI builds and gameplay, physical Xbox button/axis operation, and the absence of flickering or collision defects everywhere on the map. The Mac packaging tests use fixtures; they do not prove a Mac binary builds or runs. No push or release was performed.
