# Project communication

- Write all changelogs and release notes in English.
- Keep Discord changelogs short and focused on the most important player-facing changes.
- Distinguish verified changes from builds or hardware testing that are still pending.
- Before pushing changes, use Ponytail Review and resolve any must-fix findings.

# Versioning

- Small bugfix releases increment the patch number, for example `0.33.1` → `0.33.2`.
- Larger feature updates increment the minor number and reset the patch number, for example `0.33.2` → `0.34.0`, then `0.35.0`.
- Assign versions per release, not per commit. Set the shared version in `native/Cargo.toml` and keep local package versions in `native/Cargo.lock` and current-version documentation in sync.
