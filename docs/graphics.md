# Graphics and renderer settings

The graphics menu now has Display, Graphics, Gameplay, Interface and Audio
categories. Mouse, keyboard and Xbox D-pad navigation use the same settings.
Each row explains its effect; the system panel shows the actual adapter,
graphics backend, scene resolution and display resolution. Preferences apply
live and persist, except renderer changes which require restart.

## Working features

- Vulkan and DirectX 12 selection; automatic selection supports Vulkan,
  DirectX 12 and Metal where those backends are available. OpenGL is not exposed.
- 50–150% scene resolution, with HUD and menus composited at display resolution.
- AMD FSR 1: an FP32 WGSL adaptation of the official EASU and RCAS algorithms.
  It uses clamped direct texture loads and exact reciprocals instead of packed
  gathers and approximate intrinsics. It has not been certified by AMD or
  compared bit-for-bit against the reference implementation.
- FSR filtering happens after scene anti-aliasing and tone mapping, in sRGB
  space. EASU operates when upscaling; at native resolution or supersampling,
  linear scaling is followed by RCAS. A Linear option bypasses FSR.
- FXAA, adjustable sharpening, a small bright-pass bloom filter, exposure,
  saturation, vignette and outdoor distance haze. Fog is disabled indoors.
- A linear RGBA16F intermediate scene target and filmic display mapping.
  The final display remains SDR; this is not HDR monitor output.
- Performance (67%), Balanced (83%), Quality (100%) and Ultra (150%) presets.
  These are project presets, not the standard FSR quality-mode names. Changing
  individual image controls labels the preset Custom. Gameplay, display and
  audio preferences are preserved when changing graphics presets.
- VSync, borderless fullscreen, FOV and frame rate limits
  (Unlimited, 30, 60, 90, 120, 144, 165 or 240 FPS).
- Master, music and effects levels through the actual Kira buses; zero mutes.
  Original radio playback is still pending.

Original SA vertex lighting and textures remain the scene material basis.
The post-processing pass does not replace them with PBR materials or add
dynamic shadows. No performance improvement percentage is claimed.

## DLSS and temporal FSR

DLSS, FSR 2/3, frame generation and ray tracing are visibly unavailable. The
project currently lacks per-object motion vectors, temporal history/jitter,
reactive masks for transparency and the native SDK integration needed to
reconstruct frames. The real depth buffer and separate scene/display targets
are foundations, not completed support.

A future temporal implementation needs previous camera and animated object
transforms, motion output, disocclusion handling and reset logic on teleport,
interior travel and resolution changes. DLSS additionally needs native Vulkan
or DirectX interop and NVIDIA SDK initialization/capability checks.

## Validation

`--smoke-graphics --renderer vulkan --capture-dir <directory>` renders seven
modes with live resolution changes: native FSR/RCAS, 67% FSR, 50% FSR, 67%
Linear, 150% supersampling, native Linear with effects disabled, and 83% FSR.
The test runs the real GPU pipelines and verifies target dimensions. It does
not establish image-quality parity with commercial games or temporal stability.
Smoke tests do not overwrite the user's saved settings.

## Settings navigation

Settings use a fixed category sidebar, a scrolling list of controls, and a
fixed description/system panel. The mouse wheel moves only the control list;
the selected row is revealed on keyboard/controller navigation, rather than
forcing its scroll position on every frame. Each control has separate decrease
and increase buttons. Arrow keys/D-pad select and adjust values, Enter/A
adjusts the selected value, and Escape/B returns. Up from the first control
selects the category; left/right then switches categories.

## Neighbourhood upload

Map decoding and old-region cleanup run on the background world worker.
Replacement textures and vertex buffers are uploaded incrementally, with
256 KiB packets, at most 64 operations per frame, and a soft 2 ms / 4 MiB
per-frame budget. Individual driver calls can exceed the time budget. The
previous region remains usable until the replacement's graphics, collision
and water can be installed together. Static vertices upload directly from
their packed representation, without a temporary float copy.

Initial startup still uploads its first region synchronously. This change
targets streaming stalls, not overall rendering cost or draw-call reduction.
Streaming still replaces whole neighbourhoods rather than retaining individual
objects across smaller chunks. GPU texture/mesh reuse across overlapping
regions is a future improvement.
On the local RTX 3070/Vulkan nine-region smoke tour, the Desert upload changed
from one 139.44 ms CPU upload to 87 slices with an 11.83 ms maximum CPU slice.
Across all streamed destinations, the largest slice was 14.71 ms in the final
tour; an earlier tuning run reached 27.15 ms. These are
CPU upload timings, not total frame times or a guarantee of hitch-free play.
`--smoke-stream` also exercises a continuous 1.2 km return route at a fixed
speed, with no teleport and assertions against camera jumps.

Sources and licensing:

- [AMD FSR 1 source](https://github.com/GPUOpen-Effects/FidelityFX-FSR).
  The original header and MIT license are retained in
  `native/crates/runtime/vendor/fsr1/`.
- [AMD temporal FSR integration requirements](https://gpuopen.com/manuals/fidelityfx_sdk/techniques/super-resolution-temporal/).
- [NVIDIA DLSS programming guide](https://raw.githubusercontent.com/NVIDIA/DLSS/main/doc/DLSS_Programming_Guide_Release.pdf).
- [Linux startup and validation status](linux.md).
