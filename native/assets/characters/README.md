# SARE default character

`sare-default.glb` is the player character every world without its own
`characters` uses (built into `sa-runtime` with `include_bytes!`). It is the
SARE RP Town set, generated from code by
[`tools/blender/make_rp_characters.py`](../../../tools/blender/make_rp_characters.py):
CC0, no third-party art. Regenerate it with that script
(`--out native/assets/characters/sare-default.glb`) and keep it in step with
`content/sare-rp-town/characters.glb`.
