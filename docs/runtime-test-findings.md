# Grove Street runtime test — 2026-09-30

This milestone supersedes the earlier world-metadata-only findings.

The standalone rigid DFF loader supports frame hierarchies, multiple geometries,
atomics, material reuse and per-triangle material references. World placement
uses the conjugated, normalized IPL quaternion, followed by translation and
coordinate conversion (X,Z,-Y). IPL orientation reference:
https://github.com/INU-ez/INU_Tools-GTA-Blender/blob/main/INU_tools/ops/ipl_import.py

The native TXD loader handles D3D9 DXT1/BC1, DXT3/BC2, DXT5/BC3, A8R8G8B8 and
X8R8G8B8. Required mip lengths are checked; the viewer regenerates mipmaps.
BGRA/XRGB layout reference:
https://learn.microsoft.com/en-us/windows/win32/direct3d9/d3dformat
No implementation code was copied from either reference.

For XY center (2500,-1670), radius 120, the private dataset produces 211 exterior
placements. Removing LOD-prefixed models yields 169 detailed placements,
67 model types, 29,833 triangles, 185 textures and 185 draw batches. Start is
GTA XYZ (2490,-1665,15.5), looking toward CJ's house.

The loopback-only server registers explicit routes. Float32 draw buffers store
position, UV, normal and prelight/material RGBA: 48 bytes per vertex. Assets
remain in memory. No conversion cache, executable or DLL is used.
Data-only mods support settings, PNG overrides, hidden IDs and extra placements.
The optional Paint demo's checker PNG was generated for this project.

## Validation

- 35 synthetic Python tests passed: source protection, geometry/frame/material
  handling, IPL transforms, alpha interpolation, BGRA channels, mip sizes and
  mod path containment.
- The original scene was built. All HTTP buffer lengths matched vertex counts.
- The exact GLSL from world.js compiled/linked under EGL/OpenGL ES with Mesa
  llvmpipe. Actual served buffers and textures rendered from street level and
  an elevated overview. Both were visually inspected; no GL errors occurred.
  This is software offscreen validation, not a browser screenshot or GPU benchmark.
- A Node harness ran the actual JavaScript with simulated DOM/GL interfaces and
  real localhost responses. It checked ready state, forward/vertical movement,
  reset, menu pause and drag-look. Real pointer-lock behavior is not covered.
- Paint demo enabled in a private test copy produced 170 placements. The bin
  material received the new checker PNG; served bytes matched the override.
  The original archive still matched its pre-test SHA-256.

Windows startup and the complete interactive browser flow remain to be tested
on the user's computer. A local Chromium download failed; no successful real
browser test is claimed. Deliverables contain no original asset files,
placement metadata, transfer pieces or converted original textures.

## Remaining scope

This is an explorable renderer test with a first-person fly-camera, no collision,
player, vehicles, physics, AI or missions. Rendering has original prelight,
optional directional shading, fog and approximate blended-batch sorting.
It lacks original light behavior, material plugins, timed visibility, full
LOD/streaming, texture inheritance, billboards, animated geometry, water and
shadows. Selection uses placement origins, so the map edge may have holes.
Python/WebGL was retained for this minimal test; Rust remains the planned
native runtime foundation after this asset path is tested locally.
