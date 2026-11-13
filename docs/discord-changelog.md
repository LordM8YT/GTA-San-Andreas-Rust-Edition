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
