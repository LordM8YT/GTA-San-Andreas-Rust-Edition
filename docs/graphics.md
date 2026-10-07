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

Sources and licensing:

- [AMD FSR 1 source](https://github.com/GPUOpen-Effects/FidelityFX-FSR).
  The original header and MIT license are retained in
  `native/crates/runtime/vendor/fsr1/`.
- [AMD temporal FSR integration requirements](https://gpuopen.com/manuals/fidelityfx_sdk/techniques/super-resolution-temporal/).
- [NVIDIA DLSS programming guide](https://raw.githubusercontent.com/NVIDIA/DLSS/main/doc/DLSS_Programming_Guide_Release.pdf).
- [Linux startup and validation status](linux.md).
