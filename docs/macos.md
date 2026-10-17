# macOS

## Packaged clients

The build workflow prepares separate `SARE-macos-aarch64-test.zip` (Apple Silicon)
and `SARE-macos-x86_64-test.zip` (Intel) client packages. Extract the matching ZIP
into a writable folder and run `./sa-launcher` from Terminal. Packaged updates
use the launcher's architecture, including under Rosetta. These executables are
unsigned, without an app bundle or notarization. The new packaging jobs still
need a successful macOS CI run before their packages can be published.

## Building from source

The native runtime uses wgpu with Metal, winit for windows, Kira/CPAL for audio (CoreAudio), and gilrs for controllers. It reads original game data directly; it does not launch the Windows game executable or require Wine for rendering.

Install Rust plus your original PC San Andreas installation:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
bash start-sare-macos.sh --game-dir "/path/to/Grand Theft Auto San Andreas"
```

Alternatively set `GTA_SA_DIR` to the original installation directory. The launcher will ask for the game directory on first launch. `start-freeroam-macos.sh` is the macOS counterpart for direct runtime startup.

Original installation paths are resolved case-insensitively, including Windows path separators in GTA manifests. Use a Metal-capable GPU. Settings are stored in `~/Library/Application Support/SAFreeroam/settings.json`, or `$XDG_CONFIG_HOME/sa-freeroam/settings.json` if XDG_CONFIG_HOME is set.

Metal has been exercised on Apple Silicon Macs. Compilation and basic startup are verified; gameplay testing continues. `--renderer auto`, `--renderer vulkan` (unavailable on macOS) and `--renderer dx12` (Windows-only) override the saved renderer for startup. Changing the renderer in Display settings requires restarting.
