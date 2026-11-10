# Map Streaming & MLO Contribution Guide

This guide describes how to contribute to the ongoing GTA San Andreas map and MLO work without duplicating or conflicting with other contributors.

## Starting point

- Base branch: `feat/fivem-mlo-runtime`
- Current integration PR: [#95](https://github.com/LordM8YT/GTA-San-Andreas-Rust-Edition/pull/95)
- Do not base new work on `main` for this feature.
- Open contribution PRs against `feat/fivem-mlo-runtime`, not `main`.

Before starting, check the open PRs and Discord development thread. If a task touches the same files or subsystem as existing work, coordinate first.

## Split work into independent areas

Prefer small, focused contributions with clear ownership:

| Area | Main code locations | Example tasks |
|---|---|---|
| GTA SA formats | `tools/import-gta-sa.py`, `tools/tests/` | IPL/IDE parsing, binary format validation, importer fixtures |
| FiveM conversion | `tools/fivem_map.py`, `tools/import-fivem.py` | YMAP/YTYP parsing, conversion reports, supported metadata |
| MLO data model | `native/crates/scene/src/mlo.rs`, `mods.rs` | Rooms, portals, entity sets, validation and resource schema |
| Runtime streaming | `native/crates/runtime/src/` | World placement, streaming lifecycle, player room detection |
| Rendering and visibility | Relevant renderer and scene integration | Room/portal visibility, interior draw calls |
| Assets and collision | `native/crates/assets/`, scene collision code | DFF frames/pivots, TXD resolution, COL and door collision |
| Test maps | Test fixtures and map resource examples | Reproducible ppalace and other real-world compatibility cases |

These are contribution boundaries, not permission to create parallel implementations. Extend the existing pipeline wherever possible.

## Rules to avoid duplicated work

1. Claim or announce a task before starting it, including the files/subsystem you expect to touch.
2. Keep each PR focused on one area and describe what it changes and what remains unsupported.
3. Reuse existing data structures and parsers; do not introduce a second format or loader for the same job without discussing it first.
4. Add fixtures and tests for format changes, malformed input and regression cases.
5. If two contributors need the same files, agree on an owner or split the work into separate interfaces before coding.
6. Do not merge incomplete work into `main`. The integration branch stays the shared base until the full feature is validated.

## Definition of done

A parser passing tests does not mean the feature works in-game. Contributions should state which layer they cover: parsing, resource loading, streaming, rendering, collision or gameplay interaction.

The main compatibility target is the New Police Station (`ppalace`): correct model placement, textures, interior visibility, collision and doors. Keep claims limited to what has actually been tested.

## Native MLO manifest coordinates

A native `resource.json` MLO definition describes one placed MLO instance. Its
`position` and `rotation` are the instance transform in GTA SA world
coordinates; each entity's `position` and `rotation` are local to that
instance. `interior` selects the GTA SA streaming dimension (default `0`).
Room bounds and portal corners remain MLO-local.

```json
{
  "mlo": [{
    "name": "police_station",
    "position": [1030.0, -1987.05, 14.53],
    "rotation": [0.0, 0.0, 0.0, 1.0],
    "interior": 0,
    "entities": [{
      "model_id": 18697,
      "position": [0.0, 0.0, 0.0],
      "rotation": [0.0, 0.0, 0.0, 1.0],
      "room": 0
    }]
  }]
}
```

MLO entities are fed through the same model, TXD, DFF-atomic, map-door and
COL pipeline as ordinary map placements. Their batches retain MLO/entity
identity, and the renderer filters room-owned entities through the active room
and open-portal graph. Validate this behaviour and its performance with real
resources before treating the feature as production-ready.

## Pull request checklist

- [ ] Based on the latest `feat/fivem-mlo-runtime`
- [ ] Scope and touched subsystems are stated
- [ ] No duplicate parser/loader was introduced
- [ ] Relevant automated tests pass
- [ ] Unsupported cases and in-game test status are documented
- [ ] PR targets `feat/fivem-mlo-runtime`
