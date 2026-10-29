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

## Three layers of testing

### 1. Deterministic CI regressions

Run on every change using the existing workspace CI:
- Unit tests for simulation and game-state rules.
- Procedural scene tests, including
  `cargo run -p sa-scene --example vehicle-test-lab`.
- Existing import, packaging, and workspace tests.

These prove that defined invariants remain true; by themselves they do not
prove parity with the original game.

### 2. Black-box behavior comparisons

For a small, repeatable scenario, run the same input sequence in an owned GTA SA
installation and SARE. Record normalized observations at named checkpoints.
A future SARE telemetry/export path should emit the candidate trace; the GTA SA
reference can initially be recorded manually from visible behavior or permitted
player-side measurements. No injection, decompilation, or reading game memory
is required by this comparator.

The trace comparator is deliberately data-only. It compares the reference
measurements with a candidate JSON trace and applies explicit tolerances from
the reference file.

Run it with:

```powershell
python tools/compatibility_trace.py path\to\reference.json path\to\sare.json
```

Reference trace shape:

```json
{
  "schema_version": 1,
  "scenario_id": "vehicle/straight-line",
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

### 3. Human-reviewed gameplay checks

Some behaviors cannot yet be compared reliably by a headless test: mission
flow, animation timing, vehicle handling in traffic, police response, streaming,
and visual presentation. Keep a short reproducible checklist with screenshots
or notes stored outside the repository where appropriate. Convert repeated
findings into layers 1 or 2 when the simulation exposes trustworthy telemetry.

## First scenario backlog

Start with isolated, easy-to-repeat scenarios and add measured references
before setting expected values:

1. **Vehicle on flat ground:** idle settling, straight-line acceleration and
   braking.
2. **Vehicle handling:** fixed-speed circle, slalom, handbrake turn, and
   recovery after a bump.
3. **Collision:** low/high-speed wall impact and thin-wall tunnelling.
4. **Character movement:** idle, walk, sprint, jump, fall and landing.
5. **World interaction:** enter/exit a vehicle, pickup collection, wanted-level
   transitions, and arrest/busted flow.
6. **Streaming and stability:** repeat a route and compare missing assets,
   state transitions, frame time and crashes.

Do not compare a different game build, vehicle, handling setup, framerate cap,
or input timing as if it were the same experiment. Record those conditions and
run multiple captures to estimate normal variation.

## Current status and limitations

- The existing Vehicle Test Lab is a useful procedural smoke test for SARE's
  own physics invariants.
- `tools/compatibility_trace.py` and its unit tests provide a first comparator
  for manually or automatically captured traces.
- SARE does not yet have a complete, standardized telemetry recorder for every
  scenario, and no verified GTA SA reference baselines are included here.
  Building those capture paths and collecting clean measurements are follow-up
  work. Until then, this is a framework foundation, not a claim of parity.
