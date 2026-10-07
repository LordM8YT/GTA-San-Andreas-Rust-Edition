# Linux / Vulkan

The native runtime uses wgpu with Vulkan, winit for windows, Kira/CPAL for
audio and gilrs for controllers. It reads original game data directly;
it does not launch the Windows game executable or require Wine for rendering.

On Debian/Ubuntu, install Rust plus the development libraries:

```sh
sudo apt-get install pkg-config libasound2-dev libudev-dev libdbus-1-dev libxkbcommon-dev
bash start-freeroam.sh --game-dir "/path/to/Grand Theft Auto San Andreas"
```

Alternatively set `GTA_SA_DIR` to the original installation directory.
File names must retain the casing expected by the original archives and data
files. Use a Vulkan-capable GPU driver. Settings are stored in
`$XDG_CONFIG_HOME/sa-freeroam/settings.json`, or
`~/.config/sa-freeroam/settings.json` if XDG_CONFIG_HOME is unset.
The menu title uses the bundled OFL font; missing Windows fonts fall back to
the bundled egui text font.

Vulkan has been exercised on Windows with an NVIDIA RTX 3070. A Linux CI job
checks compilation, tests and linting without original game assets. Linux
window presentation, audio devices, controller mappings and gameplay still
need an actual Linux machine smoke test; compilation alone does not prove them.

`--renderer auto`, `--renderer vulkan` and `--renderer dx12` override the saved
renderer for startup. DirectX 12 requires Windows. OpenGL is not exposed in
this build. Changing the renderer in Display settings requires restarting.
