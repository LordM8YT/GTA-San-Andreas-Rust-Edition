# SA Runtime — Grove Street Test

## Latest free-roam build — October 7, 2026

Run **start-freeroam.cmd** to launch the native version. It starts in walk mode
and streams nearby neighborhoods as you move. Use **1–9** to travel to Grove
Street, downtown Los Santos, the beach, the airport, the countryside, San
Fierro, Las Venturas, the desert, and Mount Chiliad. Press **P** to toggle
free-fly mode and **R** to return to Grove Street.

Missions are not currently a priority. The Rust runtime supports native mods
with custom DFF models, TXD/PNG textures, COL collision, map placements, and
selected vehicle and player resources. See [free-roam status](docs/freeroam-status.md)
and [native mod documentation](docs/native-mods.md) for current controls,
capabilities, and compatibility details.

## Player-hosted multiplayer

Open **Multiplayer**, press **F5**, or use **/mp** to host/join a session with
up to **20 players including the host**. Player and driving poses synchronize;
remote players currently use Grove Street and Taxi models. Direct IP/LAN
connections are supported; internet hosting requires TCP port forwarding.
Custom assets, NPCs, shared vehicle collisions and passengers are not yet
replicated. See [multiplayer setup and limits](docs/multiplayer.md).

## Menus, HUD, and settings

The main and pause menus provide destination selection, controls, wardrobe,
interiors, local-resource status, and settings. HUD options include a custom
radar/minimap and an in-car speedometer. Display, mouse, movement, and vehicle
handling settings are saved under `%LOCALAPPDATA%/SAFreeroam/settings.json`.

The radar and overview map use the original `radar00.txd`–`radar143.txd` tiles
from `models/gta3.img` in the selected San Andreas
installation, positioned from the game's world coordinates. It rotates with the
player's heading, supports adjustable zoom, and overlays the runtime's travel
markers. If the original radar texture dictionary is unavailable, the runtime
does not substitute a fabricated map. The local-resource menu
lists detected enabled and disabled resources; edit `enabled` in each `resource.json` or `mod.json`
and restart to change their state.

Original menu sound effects now load directly from the installation. The audio
backend also supports ordinary music files and spatial effects. Footstep/engine
events and original radio archives are still pending; see
[native audio](docs/native-audio.md). Press **I** to visit the three available
interiors and **R** or a map destination to return outside.

## Mod compatibility

The native runtime loads documented `resource.json` / `mod.json` resources and supported assets;
it is not a drop-in loader for arbitrary original San Andreas mods. Existing
binary plugins, scripts, IDE/ IPL/IMG workflows, and mods that depend on the
game executable or its plugin APIs are not expected to work without conversion
or dedicated support. Read [native mod documentation](docs/native-mods.md)
before preparing a resource. An experimental [GTA V / FiveM asset converter](docs/gta5-conversion.md)
can prepare Legacy cars, props and explicitly mapped clothing as native resources.



Graphics settings now include FSR 1 EASU/RCAS, FXAA, render scale, image controls,
quality presets, renderer selection and frame limits. See [graphics](docs/graphics.md).
Vulkan is available on Windows; Linux startup and CI build checks are included,
with Linux gameplay validation still pending: [Linux](docs/linux.md).
DLSS and temporal FSR are not integrated yet.
