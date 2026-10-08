# Offline free-roam checkpoints

Normal free roam saves one local checkpoint on menu visits, normal exit and
before multiplayer preparation. It stores outdoor feet position in original
SA coordinates, camera angles, selected car/ped names and up to 16 wardrobe
toggles. Main-menu **Continue free roam** resumes this checkpoint on the next
launch. You resume on foot; the selected car is placed nearby if there is safe
space. Vehicle speed, exact parking, NPCs and server progress are not saved.

The first region is loaded around the saved location before the window opens.
The current map must provide supported standing ground, wall clearance and
headroom; ground height must remain within 0.75 m of the saved floor. Invalid
locations fall back to Grove Street. Removed or ambiguous catalog labels keep
the current default model. Wardrobe toggles restore by unique name only when
the saved ped is available. Names do not identify resource versions or guarantee
identical mod content after an author updates a pack.

Interior visits, free-fly mode, airborne players, loading destinations and
multiplayer leave the previous checkpoint intact. Joining records the offline
state first; server resources and positions do not overwrite it. Opening menus
and quitting are the checkpoints, rather than continuous background autosaves.
An abrupt crash can lose play since the last checkpoint.

Windows: `%LOCALAPPDATA%/SAFreeroam/progress.json`. Other platforms use the
settings directory (`$XDG_CONFIG_HOME/sa-freeroam` or `~/.config/sa-freeroam`).
Linux runtime validation is still pending. Files are bounded to 16 KiB, checked
for supported schema/finite values and replaced through a synced temporary
file in the same directory. Invalid files are ignored on load. Failed writes
show a message and retain the previous file; retries are throttled. Multiple
normal instances using one profile can still replace each other's checkpoints.

Use `--no-save` to disable reading and writing checkpoints for a run, or
`--save-file C:/MyProfiles/freeroam.json` for a separate local profile. These
options do not change graphics/settings storage. Normal probes, model/cutscene
previews and smoke tests do not touch checkpoint files.

After building the release runtime:

```powershell
./tools/test-progress.ps1
```

The four-start Vulkan test uses isolated copied owned car/clothing demos and
a private save file. It checks an exact position/model/wardrobe round trip,
safe fallback after removing the mod and rejection of an unsupported saved
height. `--smoke-save --save-file PATH` is its special isolated test mode.
Owned collision tests cover changed floors, real low roofs and missing ground;
file tests cover replacement and invalid updates preserving the last file.
