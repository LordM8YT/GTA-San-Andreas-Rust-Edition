# Reference parity

GTA:SA's automobile model here is a port, and a port can be wrong in ways that
only show up after a few seconds of driving. This directory pins it to an
independent implementation instead of to an opinion.

## What is compared

`golden.txt` holds state sampled from **infernus-physics**, a Python port of the
same GTA functions that is checked against native game telemetry (its own suite
has ~300 tests comparing per-tick state against recorded `CAutomobile` frames).
Each row is the state that reference produces for a fixed scenario: a seed pose,
a flat ground plane and a control schedule.

The scenarios cover the paths that decide how a car feels:

| scenario    | what it exercises |
|-------------|-------------------|
| `settle`    | springs, dampers, wheel counts, rest stop |
| `accel`     | gear table, drive-acceleration inertia, drag, tyre grip (900 ticks) |
| `brake`     | brake force and bias from 30 m/s |
| `corner`    | steady-state cornering grip and turn mass |
| `slalom`    | steering ramp, load transfer, repeated direction changes |
| `handbrake` | handbrake torque, rear lock, slide |
| `reverse`   | reverse gear band and the negative gas pedal |
| `air`       | gravity, air drag and angular air resistance with no contact |

`cargo test -p sa-handling --test reference_parity` replays them and requires
agreement within **1 cm** in position and 5e-3 raw velocity units. Measured: the
port lands inside 1 mm over the full 18 seconds of `accel`, and 5.2 mm after
12 seconds of steady cornering — the residue of f32 summation order, not of a
different model. Any real disagreement is orders of magnitude larger (the last
one was 150 m).

## Reproducing the goldens

The goldens are generated, not hand-written:

1. `ref_run.py` drives the Python reference (`infernus-physics`, located with
   `INFERNUS_ROOT`) over the same `scenarios/*.txt` and writes one JSONL per
   scenario into `out/`.
2. `make_golden.py` samples ticks 1–900 of those into `golden.txt`.
3. `diff.py` (with `PARITY_SIDE=rs` or `scene`) reports the first tick at which
   either Rust side leaves a scenario's reference dump.

`INFERNUS.txt` is the exact `handling.cfg` row used for both sides, so the test
needs no game installation.

## Checking the scene layer

The kernel can be perfect while the layer above it is not: contacts, units and
input signs all live there. `cargo run --release -p sa-scene --example
scene-parity -- <scenario.txt> <out.jsonl> <GTA folder>` runs the same scenarios
through `sa_scene::vehicle::Car` against a real triangle mesh, and the JSONL
compares against the same reference dumps. That is what caught the frame-rate
dependent substep and the bounding-box suspension geometry.
