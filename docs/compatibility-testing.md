# Clean-room compatibility testing

## Goal

Make SARE's observable behavior closer to GTA: San Andreas while keeping the
implementation independent. Treat this as a measured compatibility project,
not a source-porting project.

## Non-negotiable clean-room rules

- Never copy, translate, port, or derive implementation details from the GTA SA
  executable, decompiled/disassembled output, or code from reverse-engineering
  projects.
- Never commit Rockstar game files, extracted assets, screenshots containing
  copyrighted assets as test fixtures, or binary dumps.
- Use a legally owned local GTA SA installation only as a black-box behavior
  reference. Record inputs, starting conditions, observable outputs, game
  version, settings, and uncertainty. Keep proprietary files on the player's
  machine.
- Use public file-format documentation and SARE-owned procedural fixtures for
  implementation and CI tests.
- Do not label a behavior as "GTA-compatible" until there is a documented
  reference observation and a tolerance/acceptance rule.

## Horizon-inspired testing approach

Horizon describes a differential harness as running the same inputs through
its implementation and the original game, then comparing observable outputs.
SARE can adopt that testing pattern, but not copy Horizon's code or Minecraft's
implementation. GTA SA does not currently expose the same convenient probe
surface in SARE, so the first stage is a registry of reproducible scenarios,
a trace format, and validation. Reference data must be captured and a reliable
SARE telemetry/export path must be built before a scenario can prove parity.

The registry is `tools/compatibility_harnesses.json`. It currently specifies
repeatable scenarios for:

- Vehicle idle settling, acceleration, braking, constant-radius turning,
  wall impacts, and ramp/landing.
- Player walking, sprinting, jumping, and landing.
- Entering/exiting vehicles and collecting pickups.
- Wanted-level transitions.
- Repeated-route streaming and frame-time/memory observations.

List the registered scenarios:

```powershell
python tools/compatibility_harnesses.py list
```

Validate the registry:

```powershell
python tools/compatibility_harnesses.py validate
```

Capture a real SARE candidate trace for the currently instrumented procedural
flat-ground vehicle scenario:

```powershell
python tools/compatibility_harnesses.py capture-sare vehicle/flat-idle --output .local/compatibility/vehicle-flat-idle-sare.json
```

The command builds/runs the existing headless Vehicle Test Lab, preserves its
normal pass/fail checks, and writes measured spawn and settled checkpoints in
the shared trace format. It currently supports only `vehicle/flat-idle`;
other scenario definitions remain specifications until their probes are
instrumented. This output is a SARE candidate trace, **not** a GTA SA reference
trace and not proof of compatibility.

Compare a captured reference trace and a SARE trace for a scenario:

```powershell
python tools/compatibility_harnesses.py compare vehicle/straight-acceleration reference.json sare.json
```

Both trace files use the JSON format below. The reference trace defines
per-metric tolerances; the candidate trace supplies measurements from SARE.
Boolean state checks must match exactly. The comparator fails for missing
checkpoints, missing metrics, non-finite numbers, or values outside tolerances.

## Trace format

Example shape only:

```json
{
  "schema_version": 1,
  "scenario_id": "vehicle/straight-acceleration",
  "capture": {
    "game_version": "record the version actually tested",
    "notes": "starting conditions and input sequence"
  },
  "tolerances": {
    "speed_mps": 1.0,
    "position_x_m": 0.5
  },
  "checkpoints": [
    {
      "id": "after_2s",
      "values": {
        "speed_mps": 20.0,
        "position_x_m": 40.0,
        "grounded": true
      }
    }
  ]
}
```

The candidate file uses the same `schema_version`, `scenario_id`, checkpoint
IDs and `values`, but does not need `tolerances`. Numeric measurements require
a tolerance for that metric in the reference. Boolean states (for example,
grounded/not grounded) must match exactly. Example numbers above explain the
format only; they are **not measured GTA SA data and must not be used as a
compatibility baseline**.

## Three layers of testing

### 1. Deterministic CI regressions

Run on every change using the existing workspace CI:
- Unit tests for simulation and game-state rules.
- Procedural scene tests, including
  `cargo run -p sa-scene --example vehicle-test-lab`.
- Registry schema tests and existing import, packaging, and workspace tests.

These prove that defined invariants remain true; by themselves they do not
prove parity with the original game.

### 2. Black-box behavior comparisons

For a small, repeatable scenario, run the same input sequence in an owned GTA SA
installation and SARE. Record normalized observations at named checkpoints.
The first references can be captured manually from visible behavior or
permitted player-side measurements. No injection, decompilation, or reading
game memory is required by the comparator.

### 3. Human-reviewed gameplay checks

Some behaviors cannot yet be compared reliably by a headless test: mission
flow, animation timing, vehicle handling in traffic, police response, streaming,
and visual presentation. Keep a short reproducible checklist with screenshots
or notes stored outside the repository where appropriate. Convert repeated
findings into layers 1 or 2 when the simulation exposes trustworthy telemetry.

## Rules for reliable comparisons

- Match the exact GTA SA version, vehicle, handling setup, scene, weather,
  traffic conditions, frame-rate cap, starting position, and input timing.
- Capture multiple runs where normal variation is expected; define tolerances
  from observed repeatability rather than guessing them.
- Record which side produced each trace and the tool/build version.
- Keep reference values separate from source code and never fabricate a
  baseline to make a test pass.
- Report missing coverage honestly. A passing harness proves only the metrics
  and checkpoints it actually compares.

## Current status and limitations

- The Vehicle Test Lab is a procedural smoke test for SARE's own physics
  invariants.
- The compatibility trace comparator and registry tests run through the
  existing Python test suite in CI.
- The 11 registered scenarios are specifications, not captured GTA SA
  references. They are marked `spec_only` / `not_captured` until measurements
  are collected.
- SARE still needs standardized telemetry/export and scenario drivers to
  produce candidate traces automatically. Until that exists and real reference
  traces are captured, this is a test framework foundation, not a claim of
  compatibility.
