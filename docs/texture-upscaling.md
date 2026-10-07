# Local AI texture preview

The offline tool selects prominent opaque road, ground and building textures
around a map coordinate. It uses [Real-ESRGAN ncnn Vulkan](https://github.com/xinntao/Real-ESRGAN-ncnn-vulkan)
with its `realesrgan-x4plus` model, then produces 2x or 4x PNG overrides.
2x is the default. Periodic border padding gives the model context from the
opposite edge of repeating textures. The padding is removed after processing.

```powershell
.\tools\upscale-textures.ps1 -InstallEngine
```

The pinned Windows portable engine is stored in ignored `private-assets`.
Processing runs locally on Vulkan and never runs during gameplay or streaming.
The tool reads the user's original installation and writes an enabled resource
to `mods/local-upscaled-textures`. Restart the game to load it. To revert, set
`enabled` to `false` in that folder's `mod.json` and restart. Source and intermediate
PNGs remain beside the results for comparison. No original IMG/TXD is changed.
Generated game textures are ignored by Git and must remain local.

The initial preview selects up to 32 textures, each originally 64–512 pixels
per dimension. Transparent textures, text/sign atlases and animated materials
are excluded. AI output can smooth away grit or change tiny details; this is
a quality trial, not a complete remaster. Existing UVs, models and collisions
are unchanged. Twice the width and height means four times the decoded texture
memory; the tool reports the added memory and caps the preview at 192 MiB.
Other mods still share the runtime's 256 MiB resource budget.

For another region, use a separate resource output directory:

```powershell
.\tools\upscale-textures.ps1 -X 2000 -Y 1500 -Count 32 -Scale 2 -OutputDirectory .\private-assets\vegas-texture-preview
```

Place the generated resource directory under a mod root to activate it.
The tool refuses to replace an existing manifest, so a previous preview stays
available. Keep only one preview per region enabled when comparing versions.
