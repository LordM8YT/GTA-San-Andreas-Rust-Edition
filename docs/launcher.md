# SARE native launcher

The forest-green desktop redesign adds a sidebar and optional local PNG gameplay
artwork under Settings > Appearance > Home artwork. See [design and validation](launcher-redesign.md).

Build from the repository: `cargo build --release --workspace --manifest-path native/Cargo.toml`.
Start `start-sare.cmd`, or run `native/target/release/sa-launcher` directly.
A complete review package contains compiled launcher/runtime/server/relay binaries;
end users do not need Cargo. Direct runtime development startup still works.

The launcher uses native egui/wgpu, the existing OFL street font and one English UI.
Free roam is the main action. Settings validates your chosen original installation
read-only: required DAT registrations, IMG directory/default models and IFP decoding.
Optional missing interiors/sound/textures are named separately. A valid selection
is persisted outside the original installation. No gta_sa.exe process is started.

Play together uses the existing relay directory and native join-code/direct-IP
protocol. Favorites persist locally (including room codes; protect your profile).
Join starts a separate runtime through structured OS arguments, with `--play`.
The runtime completes its existing preflight/download/cache/model upload before
automatically entering the admitted session. Auto downloads can be disabled.
Resource preparation also works when starting the runtime directly. Hosting remains
in the runtime Multiplayer menu or the existing headless server.

Settings, version/build ID and launch contract live in `sa-client`. Normal runtime
instances share an OS profile lock; duplicate starts are rejected. The launcher
tracks its child and reloads settings after it exits. Logs are local, under the
profile's logs folder. Help displays the last observed GPU/backend and a bounded,
conservatively redacted report. Paths, credentials and invitation identifiers can
remove whole log lines; reports are editable and never uploaded automatically.
Log rotation and automatic crash upload are not implemented.

Resources distinguishes local manifests, active session resources (shown in-game
with verified byte size), and cached downloads. Local status is manifest inspection,
not a claim that DFF/TXD assets have rendered. Cache preview lists pack names/size
and total disk usage including reusable blobs. Inventory recorded does not replace
join-time hash checks. Cleanup rejects active session leases/download locks,
changed previews, unknown entries, links/reparse points and paths outside its root.
Cleanup removes all currently unused cache packs/blobs, not original or local mods.
Any active server session conservatively protects the whole cache.

Tab / Shift-Tab / Enter / Escape navigate. D-pad / A / B map to corresponding
launcher actions; existing in-game controller flow is retained. UI uses a scrollable
body and native DPI handling. Hardware controller, native folder picker, ultrawide
and Linux GPU behavior still require manual tests unless separately recorded.

F3 in gameplay shows a 120-frame mean/p99, CPU update/streaming work, CPU render and
present time, resident process memory and retained offline-world status. This is
not GPU timestamp data, VRAM usage or CPU utilization. Session swaps log resident
memory; existing incremental upload logs record transfer bytes/slices and texture
reuse. No speculative streaming or physics rewrite is part of this change.

Offline checkpoints now attempt every 60 seconds in addition to menu/exit/join.
The existing safe-ground validation and multiplayer separation are preserved.

Packaged clients automatically check and download tested GitHub releases, then
replace program files and restart once the game closes. Disable this under
Settings > Downloads > Client updates, or use the manual download/install controls there or
under Help. Source builds do not overwrite the checkout. See [update pipeline,
verification and recovery](client-updates.md).

CI publishes a separate release per main-branch commit after both platforms pass.
Packages contain allowlisted binaries, build/file hashes and legal notices, with
no original game assets. Missing dependency notices fail packaging.

Developer screenshot capture uses `SARE_SCREENSHOT_TO` inside an isolated
`SARE_CONFIG_DIR`, with `SARE_PREVIEW_PAGE=settings|multiplayer|resources|help`,
optional `SARE_PREVIEW_COMPACT=1` and `SARE_PREVIEW_SCALE=2`. It uses wgpu viewport
capture and exits after saving; destination files must not already exist.

See [current launcher polish and verification](launcher-polish.md), and
[initial validation results](launcher-validation.md).
