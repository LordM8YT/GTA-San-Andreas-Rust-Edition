# Automatic client updates

Download and extract a complete Windows/Linux client ZIP once, into a writable
folder outside the original San Andreas installation. Subsequent packaged
launchers check GitHub automatically at startup and every 30 minutes while open.
They download and verify updates in the background, save client settings, wait
until the game closes, install and restart. An offline/check failure leaves the
current client playable. No account or GitHub token is needed on clients.

Settings > Downloads > Client updates can disable automatic checks/downloads/restarts.
Help and that settings section also provide manual check/download and install
controls. With automatic updates disabled, press Install after a manual download.
Source/Cargo builds do not self-update; use Git/Cargo for development.
The launcher's main Home screen continues to use real installation/session status;
download/verification progress also appears in its footer.

## Publishing

The existing `native.yml` workflow builds/tests/packages Windows and Linux. A
push to `main` publishes only after both jobs succeed, using the narrowly scoped
`contents: write` permission in the publishing job. PRs/other branches cannot publish.
An advanced main commit cancels older builds, and publishing checks main's SHA.

Each release uses `client-<full commit SHA>` and contains the two platform ZIPs.
Assets upload to a draft before it becomes latest. A rerun preserves an already
published release's assets; an interrupted draft can be completed by rerunning CI.
A failed build publishes no update. Tags must not be moved or published assets
replaced; fix forward with a new main commit.

## Verification and install boundaries

- HTTPS requests target this repository's latest published GitHub Release.
  Only expected commit tags/platform filenames and repository asset URLs are accepted.
- The download must match the size and SHA-256 digest returned by the GitHub
  Releases API. Missing hashes, truncated/oversized data and mismatches fail closed.
- A bounded ZIP preflight rejects traversal, Windows reserved names, links,
  unexpected paths, case-colliding duplicates and oversized entries/expansion.
- `sare-build.json` identifies the commit/platform and hashes every packaged file.
  Every file is verified before installation and again after replacement.
- Only the four program binaries, the build manifest and legal/support notices
  may change. Settings, saves, local `mods/`, server cache and original game files
  are outside the update allowlist. Source/original game folders are refused.
- A copy of the currently trusted launcher performs installation after the GUI
  exits. An exclusive client-folder lease waits for other launchers to close;
  the runtime profile lock also prevents updating an active game.
- Backups and a persisted rollback journal live under `.sare-update/`. Individual
  replacements are atomic after a flushed backup is created. File/permission
  failures roll back changed files. An interrupted transaction is rolled back by
  the copied worker on next launcher startup; it is not silently retried in place.

This authenticates transport through GitHub HTTPS and detects changed bytes; it
is **not** independent release signing or Windows Authenticode. Repository/release
write access is trusted and must be protected. No downloaded PowerShell/batch
installer or arbitrary update endpoint is executed.

## Recovery and checks

Help shows the last installation result, also saved as
`.sare-update/last-result.txt`. Keep the most recent backup until the new build
is working. Backups currently remain on disk for manual recovery; there is no
automatic updater-backup cleanup yet. Do not delete a pending journal or backup
while installation/recovery is running. If recovery cannot finish, extract a
fresh complete ZIP into a new client folder; profile settings/cache remain separate.

Unit tests cover release source/hash checks, short/oversized/corrupt downloads,
ZIP validation, settings/mod preservation, shared launcher leases, interrupted
transaction recovery and actual locked-file rollback on Windows.
`tools/test-client-update.ps1` exercises the real copied updater worker and
restarted native launcher from isolated packaged clients, including running-game
protection. `tools/test-launcher.ps1` checks native layouts and screenshot sizes.

Local Windows validation on 8 October 2026 passed: 152 workspace Rust tests,
Clippy with warnings denied, formatting, release binaries, and 50 Python tests
(49 passed, one unavailable privileged-symlink fixture skipped). The native updater
smoke at `native/target/update-smoke-20261008-163449/` installed a complete package,
restarted/captured the launcher, retained settings/local mods, and refused to
replace files while the runtime profile was locked. All installed file hashes
matched after both cases. GitHub publication and Linux packaging are checked
separately by the workflow.

Client release ZIPs contain no mods, including project-owned examples. Server resources are separate opt-in downloads; updates preserve user-installed mods and cache. See [installation contents](client-package.md).
