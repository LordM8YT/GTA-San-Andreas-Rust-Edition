**SARE 0.54: gameplay in Build Mode 🚪 (pending release)**

🚪 **Doors that open with E**, doors and pads that teleport you somewhere else, and trigger zones that show messages or change the time and weather.
🏁 **Checkpoint courses** with a timer: drop the checkpoints in and they number themselves.
🧭 Give any object a behaviour in the Properties panel, and see every zone and teleport link in the viewport.
🎮 Try it all in **Gameplay Sample**, in Build Mode with F6 Playtest or from the launcher. No GTA needed.

🧰 **A familiar editor:** Build Mode now has a ribbon with Home, Model, Test and View tabs, plus Toolbox, Explorer, Properties and Output panels. Insert a Part, copy and paste, Ctrl+1 to 4 for the tools and F5 to play.
💻 **Runs on slower PCs:** the editor switches to light graphics on integrated graphics, or pick it yourself under View → Performance.
⛰️ **Terrain and lights:** sculpt hills with the new Sculpt tool, and lamps glow at night.
🌐 **Multiplayer worlds:** host your Build Mode world and everyone shares the same doors, time and weather. The server decides, and your Lua scripts can react to zones and races.

✅ Tested on Windows with automatic runs in Build Mode, from the launcher and joined to a local server, plus server tests with several players; builds pass on Windows, Linux and macOS.

---

**SARE 0.45 + 0.46: make your own stuff, no GTA needed 🛠️**

🌍 **Your own worlds.** Build a map in Blender, export it as `.glb`, drop it in the new `content/` folder, and it gets its own Play button in the launcher. It runs **without GTA San Andreas installed**, so anyone can play it.
🏙️ **Example Town** is included as a ready-made world to copy and change.
🧱 **Blender props in mods.** Benches, ramps, signs and buildings can now be `.glb` files straight from Blender. You don't need RenderWare tools, and the textures come inside the file.
🚧 **Simple collision.** Name an object `COL_something` for an invisible wall, or `NOCOL_something` for things you can walk through, like grass and flowers.
⚡ **Starter templates.** `python tools/new-resource.py my_crate --type prop` or `--type world` creates something that works right away, so you can open it in Blender and start editing.
📖 **New creator handbook** (docs/MODDING.md) with step-by-step tutorials for your first prop, your first world and your first server command.
🧪 **SARE Testbed.** Walk, jump and drive SARE Kart in a built-in test world with no game files at all.
🖥️ **For server owners:** routing buckets (separate instances) and script-spawned cars, peds and objects (`CreateVehicle`, `DeleteEntity` and friends). Servers can also share `.glb` mods with players.

✅ Tested automatically and rendered on Linux. 🙏 Please try it on Windows and tell us what breaks!

---

**SARE 0.34.0 — native interface refresh (pending release)**

- A new look for the launcher, main menu and pause menus, with consistent SARE styling.
- A quieter gameplay HUD: grouped clock, ammo and online status, plus a clearer speedometer.
- Better small-window layouts, visible selection states and English interface text.

Native interface previews and regression checks are available in the PR. Platform CI
and original-map gameplay checks are still required before release.
