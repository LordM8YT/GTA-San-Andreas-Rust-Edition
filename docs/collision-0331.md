# SARE 0.33.1: slope collision and overlapping terrain

## Collision

On-foot horizontal movement tested the player's body rim for overhead surfaces
without excluding the ground supporting the feet. On a downhill slope, the uphill
rim could hit that road surface and reject every horizontal move. At the foot of
a ramp, a neighboring ramp triangle could similarly block the move onto flat ground.

Movement now reuses the support-aware clearance check already used by standing
and vehicle-exit placement. It excludes nearby walkable planes at or below the
feet, including adjacent planes within the existing step-height tolerance. Real
overhead surfaces retain their body-clearance check. This changes shared on-foot
collision behavior, not a coordinate-specific map patch or vehicle dynamics.

## Rendering

The loader previously suppressed an immediate LOD parent when detail was selected,
but could still emit a coarser ancestor in a multi-level IPL chain. Farther away,
both the intermediate and coarse LOD could also be emitted. Their overlapping
surfaces can fight for the same depth values.

The loader now suppresses every ancestor of selected geometry. It retains the
coarse fallback when no child is selected and leaves unrelated groups unchanged.
Ancestor traversal visits each ID at most once, including malformed cycles. The
existing view distances, collision geometry and renderer depth settings are kept.

## Verification

- The walking regression failed on the original code: a downhill player remained
  at the starting position. It now covers both directions, four grades and both
  triangle windings, checking grounded movement and road height.
- A ramp-to-flat regression exposed and now covers the separate seam case.
- Existing wall, low-roof, doorway, jump, step and vehicle-exit tests still pass.
- A generated three-level terrain fixture runs through the actual area loader.
  Disabling ancestor suppression reproduces two overlapping placements; the fixed
  loader emits one near and far, and keeps a lone coarse fallback.
- Tests also cover unrelated LOD groups and terminating cyclic metadata.

No original assets or captured game image are included. The screenshot location,
other reported roads and the desert still need in-game retesting on the new build.
This fixes two reproduced mechanisms; it does not establish that all original-map
collision or z-fighting problems are resolved. Platform CI remains required.
