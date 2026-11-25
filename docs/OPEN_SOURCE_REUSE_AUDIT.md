# Open-source code reuse audit (for SARE Studio 0.61 part 2)

Status: **audit only, awaiting review.** Nothing here has been implemented, no
dependency has been added and no engine code was changed. Every REUSE or
ADAPT item below needs approval before work starts.

Audited on 2026-11-25 against SARE `main` at 9ad5ccf (0.61.0, "SARE Studio").
Each candidate was cloned and its source read; dependency licenses were taken
from `cargo metadata` over the real crates.io dependency trees, not from
READMEs.

## Summary

SARE already has most of what the brief lists: command-based undo/redo,
a docked egui editor with gizmos, Properties with attributes and tags,
thumbnails, an asset catalog, terrain sculpt/paint/holes, loose-part physics,
world scripts, local server + 1–4 player testing, and publish-to-server. The
audit therefore found **few things worth importing and many worth only
studying**. The clear wins are:

1. **LibreTerrain's `terrain-field` + `terrain-sim` erosion** (MIT OR
   Apache-2.0, pure Rust, deps: rayon, serde, thiserror, libm) for a
   "Generate terrain" step on top of SARE's existing sculpting. **ADAPT**.
2. **`parry3d`** (Apache-2.0) for collision queries, and later **`rapier3d`**
   for tumbling/sliding loose parts, both behind a setting so weak PCs and
   the clean-room vehicle model are untouched. **ADAPT**, staged.
3. **`gltf` crate features SARE already depends on** for skins, animation and
   JPEG/data-URI textures in content packs. **REUSE** (no new top-level crate
   for skins/animation; one small decoder for JPEG).
4. **`egui-snarl`** (MIT OR Apache-2.0, already on egui 0.36) as the node
   editor for a terrain graph or visual scripting later. **REUSE**, only when
   such a feature is approved.
5. **`cargo-deny`** in CI (MIT OR Apache-2.0, a dev tool, not shipped) to
   enforce the license and advisory rules this audit describes. **REUSE**.

Everything else is INSPIRE (study, implement our own way) or AVOID. A
second pass (see "Additional sources") adds a network conditioner from
lightyear and net graphs from renet_visualizer for Test › Server.

## Verification of the named projects

| Project | Real? | What was verified |
|---|---|---|
| Frame Engine | **Yes, with a caveat.** | Found `github.com/outerframehq/frame-engine`, release 0.1.0, last commit 2026-10-30, **MPL-2.0**, Rust (wgpu 29, egui 0.35, rapier3d-f64 0.35, kira, rhai). One maintainer, self-described learning project. The name is generic: if Patrick meant a different "Frame Engine", this section does not cover it. No crate named `frame-engine` exists on crates.io. |
| LibreTerrain | **Yes.** | Site libreterrain.org; source at `gitlab.com/libreterrain/libreterrain` (cloned, workspace version 2026.3.0, edition 2024, MSRV 1.94) with a mirror at `github.com/EllisonDigital/LibreTerrain` (0 stars, 14 commits on the page). `github.com/LibreTerrain/LibreTerrain` could not be reached. **MIT OR Apache-2.0** (both license files present; workspace `license = "MIT OR Apache-2.0"`). Rust engine + Godot 4 editor via godot-rust. Not published on crates.io (`publish = false`). The site's version (2026.2), docs (2026.1) and repo (2026.3.0) disagree, a sign of a young project. |
| Godot | Yes | `godotengine/godot` @ 8f2e46b (2026-11-25), MIT ("Expat"). C++, so nothing can be linked; `thirdparty/` mixes many licenses (see `COPYRIGHT.txt`), including MPL-2.0 and OFL items. |
| Bevy | Yes | `bevyengine/bevy` @ 93af76a, 0.20.0-dev, MIT OR Apache-2.0. |
| Rapier / Parry | Yes | `dimforge/rapier` 0.36.1 @ 475caa3, `dimforge/parry` 0.31.1 @ b3c3584, both Apache-2.0. |
| Fyrox (added) | Yes | `FyroxEngine/fyrox` 2.0.0-rc.1 @ 5c46e5c, MIT. Added because it is the closest full Rust editor to SARE Studio. |

## What SARE already has (so we do not rebuild it)

| Area | SARE today (path) |
|---|---|
| Undo/redo | `native/crates/editor/src/history.rs`: `Command` (Insert/Remove/Modify/Info) with apply/revert, labels, `LIMIT = 1000`, saved-marker/dirty, gizmo drags recorded as one step; command bar lines are one step each (`command_bar.rs`). |
| Editor UI | egui 0.36 in `native/crates/runtime/src/build_mode/` (`panels.rs` ribbon + closable docks, `viewport.rs` gizmo/pick/snap, `attributes_ui.rs`, `script_ui.rs`, `arrange.rs`, `pacing.rs` idle frame-rate cap). |
| Assets | `editor/src/catalog.rs` (`AssetProvider` trait: built-ins, project folder, packs, saved models), `editor/src/thumbnail.rs` (CPU thumbnails), `scene/src/gltf_model.rs` (glTF meshes, PNG only; **no skins, animation, morph targets, JPEG/KTX2, data URIs**). |
| Terrain | `scene/src/terrain.rs`: one height grid per layout object, `raise`/`smooth`/`flatten`/`paint` brushes, holes; `Terrain::MAX_CELLS = 128` per side. **No generation, erosion, LOD or chunking.** |
| Physics | `scene/src/collision.rs` (8 m grid, capsule player vs triangles, step height), `scene/src/physics.rs` (unanchored parts fall and stack; "not yet: tumbling, sliding, being pushed"), `scene/src/vehicle/rigid.rs` (clean-room experimental 3D car). |
| Animation/audio | IFP animation from the player's install (`assets/src/ifp.rs`, `runtime/src/animation.rs`); audio on kira 0.12 (`crates/audio`). |
| Multiplayer testing | `build_mode/local_test.rs` (local server + up to 4 player windows, logs in Output), `crates/net`, `crates/server`. |
| Publishing | `editor/src/publish.rs` (copy saved world into a server's `content/<id>/`, set `content_pack` in `server.cfg`); release ZIPs + notices via `tools/package-client.py`. |
| AI/automation | `crates/mcp` (sa-mcp MCP server for worlds and Lua). |

## Candidates by priority area

Classification: **REUSE** = depend on or copy as is (license-compatible);
**ADAPT** = port/trim with notices kept; **INSPIRE** = study, write our own;
**AVOID** = not worth it or unacceptable risk.

### 1. Asset management and importing

| Candidate | Source | Class | Notes |
|---|---|---|---|
| `gltf` 1.4.1 (already a SARE dep) | crate features + `gltf::Skin`, `gltf::animation` | **REUSE** | SARE uses `gltf` with `default-features = false, ["utils","names"]`. Skins, animation channels and morph targets are already in the crate's data model; SARE just doesn't read them. Adding `KHR_texture_transform`/data-URI handling is our own code. Zero new dependencies. |
| `zune-jpeg` 0.5 | crates.io | **REUSE** (small) | MIT OR Apache-2.0 OR Zlib, pure Rust, MSRV 1.77. Lets packs use JPEG textures. Alternative: `image` crate's `jpeg` feature (bigger). Pin a stable release; 0.5.16 is an `-rc` at audit time. |
| `ktx2` 0.5 | crates.io | **AVOID for now** | Apache-2.0, reader only (no Basis transcoder). GPU-compressed textures only pay off with a transcoder; revisit if pack sizes become a problem. |
| Bevy asset processor | `crates/bevy_asset/src/processor/{mod.rs,process.rs,log.rs}`, `meta.rs` | **INSPIRE** | Good ideas: per-asset `.meta` sidecar with import settings, hash-based "processed" cache, transaction log so a crash mid-import is recovered. Code is welded to `bevy_ecs`/`bevy_reflect`; porting is not practical. |
| Godot importer | `editor/import/3d/resource_importer_scene.cpp`, `scene_import_settings.cpp`, `editor/import/resource_importer_texture.cpp` | **INSPIRE** | The `.import` sidecar + reimport-on-change flow and per-mesh import options (collision generation, LOD, scale) are the model to copy for a "glTF import settings" dialog. C++, not linkable. |
| Fyrox resource manager | `fyrox-resource/` | **INSPIRE** | Async loading with typed handles; SARE's catalog + streaming already cover what we need. |

### 2. Terrain generation and editing

| Candidate | Source | Class | Notes |
|---|---|---|---|
| LibreTerrain `terrain-field` | `crates/terrain-field/src/{grid.rs,noise.rs,ops.rs,seed.rs}` (≈2.2k lines total) | **ADAPT** | `GridSpec`/`Grid` (vertex-aligned, world-metre grids, resolution-independent), `noise::{perlin, simplex, value, cellular, fbm, ridged, billow, warped_fbm}`, `ops::{gaussian_blur, gradient, slope_degrees, distance_to}`. Deps: rayon, serde, serde_json, thiserror, libm, all MIT/Apache. No `unsafe` in field/sim. Edition 2024 / MSRV 1.94 is fine with SARE's latest-stable CI. |
| LibreTerrain `terrain-sim` erosion | `crates/terrain-sim/src/erosion/{mod.rs,thermal.rs,hydraulic.rs,hardness.rs}`, `hydrology/{flow.rs,routing.rs}` | **ADAPT** | `thermal::solve` (talus diffusion, Musgrave et al. 1989) and `hydraulic::solve` (stream-power law, Priority-Flood routing, Braun & Willett 2013). File headers state "clean-room, from the published methods" and cite papers, which matches SARE's rules. Deterministic for any thread count, so a server and client generate the same terrain from a seed. |
| LibreTerrain `terrain-nodes`, `terrain-core` graph/eval | `crates/terrain-nodes/`, `crates/terrain-core/src/{graph.rs,eval.rs,cache.rs}` | **INSPIRE** | 60-node graph evaluator with caching and tiled builds; too large (≈18k lines) for the first step. Revisit with a node editor (see `egui-snarl`). |
| LibreTerrain `terrain-godot`, `terrain-mcp` | `crates/terrain-godot/`, `crates/terrain-mcp/` | **AVOID** / **INSPIRE** | Godot bindings are irrelevant. `terrain-mcp` is a useful reference for exposing terrain tools in sa-mcp. |
| Fyrox terrain LOD | `fyrox-impl/src/scene/terrain/quadtree.rs` (540 lines), `geometry.rs` | **INSPIRE** | Needed only if terrains grow past `MAX_CELLS = 128`. A quadtree LOD over chunks is simple enough to write ourselves against SARE's renderer. |
| Godot terrain | none in core (plugins only) | — | Nothing to take. |
| `noise` crate 0.9 (noise-rs) | crates.io | **AVOID** | Apache-2.0/MIT, but LibreTerrain's `noise.rs` already covers it with world-metre, seed-hashed sampling and fewer deps. |

Integration shape: vendor the two crates' needed files into a new
`native/crates/terrain-gen/` (or as modules in `sa-scene`) with the original
MIT/Apache headers and a `THIRD_PARTY.md` entry, then add a "Generate" panel
that fills SARE's existing `Terrain` heights (one undo step through
`history::Command::Modify`). Generation runs once in the editor; the result is
saved in `layout.json`, so play and servers pay nothing at runtime.

### 3. Editor UX and property inspection

| Candidate | Source | Class | Notes |
|---|---|---|---|
| Godot inspector | `editor/inspector/editor_inspector.cpp`, `editor_properties*.cpp`, `multi_node_edit.cpp`, `editor_sectioned_inspector.cpp` | **INSPIRE** | Multi-object editing with mixed values, collapsible sections, property drag-scrub, pinned/default reset. SARE already edits several parts at once; copy the "mixed value" display and "reset to default" button. |
| Bevy inspector | `crates/bevy_inspector/src/{details_panel.rs,entity_tree.rs,resource_list.rs}` | **INSPIRE** | Reflect-driven, built on `bevy_ui`/`bevy_feathers`; not usable from egui. |
| Fyrox inspector | `fyrox-ui/src/inspector/` (`mod.rs` 1958 lines, `editors/`) | **INSPIRE** | Reflection-based property editors; useful reference for a generic `Reflect`-like trait if SARE later wants scripted components in Properties. |
| `transform-gizmo-egui` 0.11 | crates.io | **AVOID** | MIT OR Apache-2.0 and on egui 0.36, but SARE has its own gizmo (`build_mode/viewport.rs`), and it would pull glam 0.32 + mint. |
| `egui_dock` 0.22 | crates.io | **AVOID** | MIT; needs egui 0.37 (SARE is on 0.36.2) and SARE's ribbon/dock layout already works. |
| Frame Engine editor | `crates/frame-editor/src/lib.rs` (14.5k lines, single file) | **INSPIRE only** | MPL-2.0 and monolithic; nothing isolated to take. |

### 4. Undo/redo and command history

SARE's `editor/src/history.rs` is already a proper command pattern and is
**better than two of the candidates**: Frame Engine pushes a full clone of the
world per step (`frame-editor/src/lib.rs`, `push_undo`, ~line 6083), and Bevy
has no editor undo.

| Candidate | Source | Class | Notes |
|---|---|---|---|
| Godot `UndoRedo` merge modes | `core/object/undo_redo.h` (`MergeMode::{MERGE_DISABLE, MERGE_ENDS, MERGE_ALL}`), `editor/editor_undo_redo_manager.cpp` (per-scene histories) | **INSPIRE** | Merging slider/colour scrubbing into one step is **already done** in SARE (`BuildMode::preview` + `commit_preview` in `build_mode/mod.rs`, committed when the pointer and focus are free, `panels.rs` ~line 2603). The idea left is separate histories per open document if Studio opens several worlds. |
| Fyrox `CommandTrait` | `editor/src/command/mod.rs` (`is_significant`, `finalize`) | **INSPIRE** | "Insignificant" commands (selection) that don't mark the file dirty; `finalize` to free resources when a command drops off the stack. |
| `undo` crate 0.52 | crates.io | **AVOID** | MIT OR Apache-2.0, small, but replacing working code adds risk for no gain. |
| Small fix spotted | `history.rs` `self.undo.remove(0)` | — | O(n) at the 1000-step cap; a `VecDeque` would be O(1). Cosmetic. |

### 5. Physics and collision

| Candidate | Source | Class | Notes |
|---|---|---|---|
| `parry3d` 0.31.1 | `src/shape/{trimesh.rs,heightfield3.rs,compound.rs}`, `src/partitioning/bvh/`, `src/query/{ray,shape_cast,contact}` | **ADAPT** (as a dependency) | Apache-2.0; deps all MIT/Apache/Zlib. A BVH over triangle meshes and a native heightfield shape would replace the hand-made 8 m grid in `scene/src/collision.rs` for ray picks and shape casts, and gives exact terrain collision. Uses `glamx` → glam 0.33 (SARE core is on glam 0.30; `sa-audio` already pulls 0.33), so conversions go through arrays at the boundary. |
| `rapier3d` 0.36.1 | `src/pipeline/physics_pipeline/`, `src/dynamics/`, `src/control/character_controller.rs` (`KinematicCharacterController::move_shape`, line 297) | **ADAPT** (staged, opt-in) | Apache-2.0, 61 transitive crates, MSRV 1.86. Fits Build Mode loose parts ("tumbling, sliding, being pushed" in `physics.rs`). Must run on the server as authority and send results, because float determinism across Windows/Linux/macOS clients is only "enhanced", not guaranteed (`enhanced-determinism` feature). Keep a cap on active bodies and a setting to fall back to today's fall-and-stack for weak PCs. |
| Rapier character controller | `src/control/character_controller.rs` | **INSPIRE** | SARE's player controller is tuned for SA feel; study autostep/slope/snap-to-ground logic, don't swap. |
| Rapier vehicle | `src/control/ray_cast_vehicle_controller.rs` | **AVOID** | SARE's clean-room handling.cfg model and `vehicle/rigid.rs` are deliberate; replacing them breaks the agreed feel work. |
| Frame Engine physics | `crates/frame-engine/src/physics.rs` | **INSPIRE** | A readable example of wrapping rapier for static/dynamic/kinematic entities and a server tick; MPL-2.0, so no copying without approval. |

### 6. UI editor (for creators building in-game UI)

| Candidate | Source | Class | Notes |
|---|---|---|---|
| Godot Control/anchors | `scene/gui/control.cpp` (anchors, offsets, containers) | **INSPIRE** | Anchor + margin layout is the model creators know. |
| Fyrox UI editor | `editor/src/ui_scene/` | **INSPIRE** | A WYSIWYG UI scene editor in Rust; architecture only. |
| `bevy_ui` / `bevy_feathers` | `crates/bevy_ui*` | **AVOID** | ECS-bound; SARE's script UI (`crates/lua/src/ui.rs`, `runtime/src/script_ui.rs`) already exists. |

### 7. Animation and audio

| Candidate | Source | Class | Notes |
|---|---|---|---|
| `gltf` skins/animation | (see §1) | **REUSE** | Lets content packs bring rigged characters and animated props without SA files. |
| Bevy animation | `crates/bevy_animation/src/{gltf_curves.rs,graph.rs,transition.rs,blendable.rs}` | **INSPIRE** | Clean reference for sampling glTF curves (step/linear/cubic spline) and a blend graph. ECS-bound. |
| Fyrox ABSM | `fyrox-animation/src/machine/` | **INSPIRE** | Animation state machine with layers and masks; good model for a later "animation graph" editor. |
| Audio | kira 0.12 already | **AVOID** new audio libs | Fyrox-sound and bevy_audio add nothing SARE needs. |

### 8. Multiplayer testing and debugging

| Candidate | Source | Class | Notes |
|---|---|---|---|
| Bevy Remote Protocol | `crates/bevy_remote/src/lib.rs` (JSON-RPC 2.0 over HTTP) | **INSPIRE** | SARE's sa-mcp already plays this role for editing; the idea to borrow is a read-only live "inspect a running server" endpoint for `local_test`. |
| Bevy dev tools | `crates/bevy_dev_tools/src/{fps_overlay.rs,frame_time_graph/,infinite_grid.rs}` | **INSPIRE** | Frame-time graph and net stats overlay for the 1–4 player test windows. |
| `puffin` 0.20 (+ `puffin_egui`) | crates.io | **ADAPT** (optional, dev builds) | MIT OR Apache-2.0, MSRV 1.92. In-app profiler with an egui viewer; keep behind a feature so release builds are unaffected. |
| Network conditioner | lightyear `conditioner.rs` (see Additional sources) | **ADAPT** | Add simulated latency/loss/jitter to `local_test` (a small relay in `crates/net`). |
| Frame Engine net | `crates/frame-engine/src/net/mod.rs` | **AVOID** | A minimal TCP slice; SARE's networking is further along. |

### 9. Publishing and content packaging

| Candidate | Source | Class | Notes |
|---|---|---|---|
| Godot export presets | `editor/export/editor_export.cpp`, `editor_export_preset.cpp`, `editor_export_platform_pc.cpp`, `dedicated_server_export_plugin.cpp` | **INSPIRE** | Presets per target and a "dedicated server" export that strips client-only data map well to "publish to server" vs "share pack". |
| Frame Engine pack format | `crates/frame-engine/src/pack.rs` | **INSPIRE** | Single-file pack with index at the end and per-file compression. MPL-2.0: idea only. SARE should keep zip (already a dependency) for transparency to modders. |
| Frame Engine third-party notices | `crates/frame-editor/src/licences.rs` | **INSPIRE** | Builds `THIRD-PARTY-LICENSES.txt` from `cargo tree` + registry sources. SARE's `tools/package-client.py` already does a version of this. |
| `cargo-deny` 0.22 | crates.io (dev tool) | **REUSE** | MIT OR Apache-2.0. A `deny.toml` allowing MIT/Apache-2.0/BSD/Zlib/ISC/Unicode/OFL (fonts) and failing on GPL/AGPL/unknown, plus RustSec advisories, in CI. Bevy ships a `deny.toml` worth reading. Not shipped to players. |
| `cargo-about` 0.9 | crates.io (dev tool) | **AVOID for now** | Overlaps with `package-client.py`'s notices. |

## License compatibility assessment

SARE is MIT. Compatible inbound licenses for code we copy or link:
MIT, Apache-2.0, BSD-2/3, Zlib, ISC, Unicode-3.0, CC0, and dual
"MIT OR Apache-2.0".

| Project | License | Copy code into SARE? | Depend on it? |
|---|---|---|---|
| LibreTerrain | MIT OR Apache-2.0 | Yes, keep headers, add to `THIRD_PARTY.md` | Not on crates.io; vendor |
| Rapier / Parry | Apache-2.0 | Yes with NOTICE; prefer depending | Yes |
| Bevy | MIT OR Apache-2.0 | Yes in principle; not practical (ECS-bound) | Not recommended |
| Fyrox | MIT | Yes in principle; INSPIRE only | No |
| Godot | MIT (engine); `thirdparty/` mixed | Engine code is C++, so ideas only. Never copy from `thirdparty/` without checking `COPYRIGHT.txt` (contains MPL-2.0, OFL, CC-BY-4.0 items) | No |
| Frame Engine | **MPL-2.0** | **Not without Patrick's approval.** MPL is file-level copyleft: copied files must stay MPL-2.0 and their source must stay available. Allowed in an MIT project, but it splits licensing and contradicts "keep SARE simple MIT". Also "Frame Engine" is a trademark (`TRADEMARKS.md`). | No |
| egui-snarl / puffin / zune-jpeg / cargo-deny | MIT OR Apache-2.0 (zune-jpeg also Zlib) | — | Yes |

SARE's own tree today (`cargo metadata --locked` on `native/`, 506 crates): all permissive except the MPL-2.0 audio crates above and dual-licensed crates with a copyleft *option* (we take the permissive side). No GPL/AGPL-only crate.

Transitive dependency check of the candidates (cargo metadata, normal + build deps):

- `rapier3d` 0.36.1: 61 crates, all MIT/Apache/Zlib/Unlicense-or-MIT/Unicode. No copyleft.
- `egui-snarl` 0.12: 81 crates (mostly egui's tree, which SARE already has). Dual-licensed items with a copyleft *option* (`self_cell`: Apache-2.0 OR GPL-2.0-only; `r-efi`: MIT OR Apache-2.0 OR LGPL-2.1-or-later) are fine; we take the permissive side. Both are already in SARE's egui tree.
- `noise` 0.9: 7 crates, all MIT/Apache.
- `transform-gizmo-egui`, `egui_dock`: permissive, but AVOID for technical reasons above.
- LibreTerrain field/sim: rayon, serde, serde_json, thiserror, libm, all MIT/Apache.

None of the candidates contain Rockstar code or assets. LibreTerrain's erosion
files say they are clean-room implementations of published papers; that
matches SARE's rules.

## Integration complexity and maintenance

| Item | Class | Effort | Maintenance | Weak-PC impact |
|---|---|---|---|---|
| glTF skins/animation + JPEG in packs | REUSE | M (1–2 weeks) | Low; `gltf` is stable | None unless used |
| LibreTerrain field + erosion → "Generate terrain" | ADAPT | M (≈1 week) | Medium: vendored, one upstream maintainer; we own the copy | Editor-time only; zero at runtime |
| Undo `VecDeque` + insignificant commands | INSPIRE | S (½ day) | Low | None |
| Inspector mixed values / reset | INSPIRE | S | Low | None |
| cargo-deny in CI | REUSE | S (½ day) | Low | None |
| puffin profiler (dev feature) | ADAPT | S | Low | None in release |
| Network conditioner + net graphs in local test | ADAPT | S–M | Low | None |
| parry3d for picking + terrain heightfield collision | ADAPT | M–L (2–3 weeks) | Medium: Dimforge releases breaking versions often (0.x) | Likely faster than the grid for big worlds; must benchmark |
| rapier3d for loose parts (opt-in) | ADAPT | L (3–4 weeks) | Medium–high | Real cost; cap bodies, setting to disable |
| Terrain LOD/chunks | INSPIRE | L | Medium | Positive for big terrains |
| egui-snarl node graph | REUSE | M, only with a feature that needs it | Low–medium | None |

## Additional sources (second pass)

Cloned and read on 2026-11-25 after Patrick asked for more places to take
from.

| Candidate | Source | License | Class | Notes |
|---|---|---|---|---|
| lightyear link conditioner | `cBournhonesque/lightyear` @ 2026-11-24, `crates/io/link/src/conditioner.rs` (489 lines) | MIT OR Apache-2.0 | **ADAPT** | Receive-side latency, jitter and Gilbert–Elliott packet loss. Uses `bevy_reflect` and lightyear's `ReadyBuffer`; strip those and it becomes a small module for Test › Server ("simulate a bad connection"). The rest of lightyear (Bevy-based replication) is **INSPIRE** for prediction/interpolation. |
| renet visualizer | `lucaspoffo/renet` @ 2026-10-30, `renet_visualizer/src/{lib.rs,circular_buffer.rs}` (≈450 lines) | MIT OR Apache-2.0 | **ADAPT** | egui 0.36 graphs of RTT, packet loss and bandwidth per client, same egui version as SARE. Tied to `renet::NetworkInfo`; swap that for SARE's own stats. The `renet` transport itself is **AVOID** (SARE has its own net). |
| bevy-inspector-egui | `jakobhellermann/bevy-inspector-egui` @ 2026-10-17, `crates/bevy-inspector-egui/src/{inspector_egui_impls/,egui_utils.rs,reflect_inspector/}` | MIT OR Apache-2.0 | **INSPIRE** | The best egui property-editor widgets around (vectors, colours, enums, collections), but built on Bevy 0.20 reflection. Copy small widget ideas by hand. |
| Ambient | `AmbientRun/Ambient` @ 2025-01-07, `crates/intent/src/{lib.rs,logic.rs,registry.rs}`, `crates/asset_cache/src/lib.rs`, `crates/package_semantic_native/`, `crates/deploy/` | MIT OR Apache-2.0 | **INSPIRE** | Rust/WASM multiplayer creation platform, close to Patrick's Roblox+FiveM vision. Its "intents" are server-side undo/redo per user in a shared editing session, the model for collaborative Studio editing. **Paused indefinitely** (README), so never depend on it. |
| oxidized_navigation | `TheGrimsey/oxidized_navigation` @ 2025-11-14, `crates/oxidized_navigation/src/{heightfields.rs,regions.rs,contour.rs,mesher.rs,detail_mesh.rs,query.rs}` | MIT OR Apache-2.0 | **ADAPT** (later) | Pure-Rust Recast-style navmesh building and path queries, for NPCs in content-pack worlds (SA path nodes only exist for the SA map). Tied to Bevy 0.16 and an old parry3d; pull the algorithm files out. |
| meshopt (meshoptimizer bindings) | `gwihlidal/meshopt-rs` 0.7.0 @ 2026-10-17, `src/{simplify.rs,optimize.rs,clusterlod.rs}`; C++ `zeux/meshoptimizer` as git submodule | MIT OR Apache-2.0 (bindings), MIT (meshoptimizer) | **REUSE** (when needed) | Mesh simplification for automatic LODs of imported glTF, and vertex-cache optimisation. Builds C++ with `cc` (bindgen is optional), like mlua already does. Good for weak PCs. |
| notify 9 | `notify-rs/notify` @ 2026-11-23 | CC0-1.0 | **REUSE** (when needed) | File watching for hot-reloading assets and scripts while Studio is open. |

Checked by license only, not cloned; all **AVOID** for code (ideas are fine):

- **MTA:SA** and **Veloren**, **OpenMW**: GPL-3.0.
- **Luanti (Minetest)**: engine LGPL-2.1+, so copied code would have to stay LGPL.
- **Blender**: GPL. Use it as a tool only (SARE already does for glTF).
- **symphonia** (audio decoding): MPL-2.0. Note: SARE **already depends** on it unmodified through kira (`symphonia*` 0.6.1, plus `audio_thread_priority` and `triple_buffer`, also MPL-2.0). Using MPL crates unmodified from crates.io is allowed in an MIT program; copying their files into SARE is not. The `deny.toml` in step 1 should list these as named exceptions.
- **FiveM/Cfx.re**: proprietary platform terms. SARE copies its conventions, never its code.

## Roadmap proposal (0.61 part 2)

Patrick wants this in **0.61 part 2**, so the work is one release series in
small steps instead of five minor versions. Each step is its own PR, merged
only when CI is green and after Patrick approves that step's scope.

1. **Safety net and quick wins.** `cargo-deny` + `deny.toml` in CI;
   `THIRD_PARTY.md` listing vendored code. Undo: selection as insignificant,
   `VecDeque`. Inspector: "mixed" display and reset
   to default.
2. **Test tools.** Network conditioner (from lightyear) and net graphs (from
   renet_visualizer) in Test › Server; optional puffin overlay in dev builds.
3. **Assets.** glTF skins, animation clips and morph targets in content
   packs; JPEG and data-URI textures (`zune-jpeg`); import settings sidecar
   (scale, collision on/off) inspired by Godot/Bevy `.meta`.
4. **Terrain generation.** Vendor LibreTerrain `terrain-field` + erosion
   into a SARE crate with notices; "Generate" panel (noise type, seed, octaves,
   thermal and hydraulic erosion steps) writing into the existing `Terrain`
   as one undo step; sa-mcp tool for it.
5. **Collision.** `parry3d` behind an internal switch for editor picks
   and terrain heightfield collision; benchmark against `collision.rs` on a
   weak-PC profile before making it default.
6. **Physics.** Opt-in `rapier3d` for loose parts (server-authoritative, body
   cap, setting to turn off).

If part 2 gets too big, steps 5–6 are the natural cut to move to 0.62.

Later, only on demand: automatic LODs with meshopt, asset hot reload with
notify, NPC navmesh from oxidized_navigation, collaborative editing modelled
on Ambient's intents, terrain node graph with `egui-snarl`, terrain LOD,
animation graph editor, UI editor.

## Not done in this audit

- No code was changed and no dependency was added.
- `github.com/LibreTerrain/LibreTerrain` was unreachable; the GitLab source
  and the EllisonDigital GitHub mirror were used.
- Rapier/parry performance on weak PCs was not measured; that is the first
  task of step 5.
- Security review of new crates is limited to license and maintenance
  checks here; `cargo-deny`'s RustSec check (step 1) covers advisories.
