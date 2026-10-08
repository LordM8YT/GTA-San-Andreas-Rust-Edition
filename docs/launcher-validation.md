# Launcher validation - 8 October 2026

The initial validation was performed locally; the user subsequently authorized
committing and pushing the complete project changes. No release, public relay or
Steam integration is deployed by this change. Original game files were read only.

## Automated checks on Windows

- `cargo fmt --all -- --check`.
- `cargo test --workspace`: 143 tests passed, including launcher argument/locking,
  stale installation/browser results, controller focus/activation/back, atomic
  configuration/checkpoint validation, cache leases/stale previews/writer locks,
  real Windows junction rejection and gameplay RTT.
- `cargo clippy --workspace --all-targets -- -D warnings`.
- Release workspace binaries and examples built successfully.
- Python discovery: 50 tests, 49 passed and 1 skipped (host cannot create the
  privileged symlink fixture). Actual Windows junction tests passed separately.
- Packaging tests check the explicit file allowlist, incomplete runtime rejection,
  pinned license text sources/cache, unknown-host rejection and preservation of the
  previous ZIP when required notices are unavailable.

## Practical native tests

On this Windows host with NVIDIA RTX 3070 / Vulkan:

- Original installation validation returned ready, without modifying its files.
- Eight native launcher captures cover all five pages, an invalid installation,
  compact window and 200% UI scaling. Narrow navigation uses a dropdown; the body
  scrolls separately from status. Visual inspection caught and corrected clipping.
- The exact shared `Launch::command` started the real runtime twice with a profile
  path containing spaces and `&`; first play and checkpoint continuation passed.
- Four-start checkpoint smoke passed: exact position/car/ped/wardrobe restoration,
  removed resource fallback, unsafe height rejection and the periodic save schedule.
- Two local Vulkan clients passed direct player-hosted multiplayer and relay-backed
  dedicated hosting with auto-entry, custom appearance, passenger seats and audio.
  Both modes restored the offline world/local resources after disconnect.
- The subsequent relay join downloaded **0 bytes** and reused **7 files** from the
  preceding test's cache. These are same-PC tests, not different-network validation.
- Nine-region Vulkan streaming tour passed, including Las Venturas and Mount Chiliad.
- Windows review ZIP built, passed CRC inspection and includes all required dependency
  and bundled font notices. Its four binaries match the build; original assets,
  local settings, cache and private logs are excluded by the packaging allowlist.

## Measurements and remaining work

The isolated nine-region tour logged background region loads around 0.38-1.82 s;
GPU-upload CPU slices reached approximately 15.6 ms despite the soft 2 ms budget.
Individual driver work is not interruptible. Existing texture reuse was observed.
No speculative streaming/collision/physics rewrite was made on this evidence.

During the two-client resource-switch stress test, one client's resident memory
was approximately 503 MiB before preparation, 838 MiB after the server-world swap
and 809 MiB immediately after offline restoration. These are process working-set
samples, not VRAM or a leak diagnosis. Retaining the offline world trades memory
for reliable disconnect restoration. F3 now exposes frame/CPU/working-set metrics;
GPU timestamp measurements and finer allocation profiling remain future work.

Manual native folder-picker interaction, physical controller hardware, ultrawide,
Linux GPU/input execution and different-PC/network/NAT tests remain unverified.
Windows/Linux CI packaging is defined; hosted CI results are separate from the
local validation recorded here. There is no published release source to version-check, public
relay deployment, signed updater or automatic report upload.
