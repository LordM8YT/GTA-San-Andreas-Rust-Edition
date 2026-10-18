# Project communication

- Write all changelogs and release notes in English.
- Keep all built-in player-facing UI labels, hints and error messages in English.
- Keep Discord changelogs short and focused on the most important player-facing changes.
- Distinguish verified changes from builds or hardware testing that are still pending.
- Before pushing changes, use Ponytail Review and resolve any must-fix findings.

# Versioning

- Small bugfix releases increment the patch number, for example `0.33.1` → `0.33.2`.
- Larger feature updates increment the minor number and reset the patch number, for example `0.33.2` → `0.34.0`, then `0.35.0`.
- Assign versions per release, not per commit. Set the shared version in `native/Cargo.toml` and keep local package versions in `native/Cargo.lock` and current-version documentation in sync.

# UI quality and maintainability

- Treat player-facing UI as a cohesive, polished game interface, not a quick reskin. Establish the layout, hierarchy, typography, spacing, interaction states, and visual language before expanding implementation. Keep all player-facing text in English.
- Do not call a UI change finished based on code edits or mockups alone. Run the relevant formatting, tests, lint/build checks, and inspect the actual in-game/launcher result when the environment allows it. Clearly state what was and was not verified.
- Keep UI code approachable for Jasper and other contributors: prefer small, clearly named modules and reusable components; keep styling/tokens separate from screen-specific behavior; document non-obvious decisions; avoid needless abstractions, duplicated styling, giant files, and AI-specific or opaque workflows. A human developer should be able to locate, understand, and safely modify each screen.
- Make changes in reviewable steps, preserve existing controls and behavior unless explicitly intended, and explain key design/architecture decisions in the pull request.
