# Native launcher redesign â€” 8 October 2026

The Rust/egui launcher uses a 220-point desktop sidebar, full-width content with
28-point margins, a single free-roam hero action and three equal-height cards.
Compact windows use a navigation dropdown. Page content scrolls separately from
the status footer; build hash and network protocol are under Help.

## Visual direction

Forest background `#0c1c16`, sidebar `#091711`, surface `#152b21`, warm gold
`#dfb778`, offwhite `#efe9dd` and muted text `#a5b8a9`. The existing OFL gothic
wordmark remains; other text uses egui's bundled proportional sans-serif.
Wrapped card text is left-aligned to avoid stretched word spacing. Original
outline icons, 6-point controls and 10-point cards/hero share hover/focus styling.
The native window requests the OS dark theme; decorations remain platform-controlled.

Composition references: [Envato dashboard](https://elements.envato.com/game-launcher-dashboard-ui-design-G8YGJVB)
and [GameHub on Dribbble](https://dribbble.com/shots/27155163-GameHub-Esports-Game-Launcher-Game-Library-UI-UX-Design).
No reference artwork, branding or external UI assets are bundled.

## Optional player artwork

Settings â†’ Home artwork â†’ Choose image accepts a local PNG. Configuration stores
its path; no image is copied into Git, uploaded or added to server resources.
An original abstract green background works without artwork. Missing/invalid
images fall back to it, with recovery guidance in Settings. Use default background
removes the selection.

Decoding checks file size (24 MiB), pixel count (16 megapixels) and decoder
allocation limit (64 MiB). RGB, RGBA, grayscale and grayscale-alpha are supported.
Images are center-cropped with a dark gradient. Saved area labels are approximate;
the actual checkpoint and launch behavior remain unchanged.

## Verification

- 145 workspace Rust tests passed, including local-image validation and actual
  sidebar traversal with controller-translated Tab/Enter/Escape events.
- Formatting, workspace Clippy with warnings denied and release launcher build passed.
- Eleven actual native window captures passed, including PNG dimension checks:
  1280Ã—720, 1920Ã—1080, 2560Ã—1080, 1920Ã—1080 at 200% DPI (960Ã—540 logical),
  all five pages, compact Home, compact Settings at 200% DPI and invalid installation.
- Local captures: `native/target/launcher-ui-20261008-161243/`.
  Optional artwork testing uses the user's supplied gameplay capture; it remains
  outside Git. Saved checkpoints are fixtures in isolated test profiles.

Compact/high-DPI layouts may require scrolling; the footer and navigation remain
separate. Existing installation, server, favorites, resource/cache, diagnostic
and settings workflows remain in the native application. No server listings or
online counts are fabricated. Physical controller hardware and Linux window
decorations were not manually retested.
