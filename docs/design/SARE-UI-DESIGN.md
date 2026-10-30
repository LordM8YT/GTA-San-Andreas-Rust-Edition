# SARE visual identity

**Direction:** SARE — San Andreas, Reimagined  
**Tagline:** SAME CITY. NEW STORIES. SARE.

This document is the implementation reference for the shared visual language across the launcher, in-game menus, HUD, notifications and dialogs.

## Colour tokens

| Token | Hex | Use |
|---|---|---|
| Midnight | `#0B0F14` | Main background |
| Charcoal | `#1A1F26` | Panels and cards |
| Slate | `#2E3540` | Raised surfaces, borders and controls |
| Gold | `#D4AF37` | Primary accent, focus and active navigation |
| Sand | `#F5E9C8` | Primary text |
| Muted | `#A6A9AD` | Secondary text and metadata |
| Success | `#8FCBA0` | Positive status only |

The shared Rust tokens live in `native/ui/theme.rs`. Do not introduce screen-specific near-duplicates of these values without a clear reason.

## Typography

- Display / wordmark: the bundled Archivo Black title face.
- Interface and body: egui's readable default sans-serif until a bundled Inter font is licensed and added.
- Use short uppercase labels for navigation and metadata; reserve large display type for screen titles.

## Visual principles

1. The world is the hero. Use cinematic San Andreas scenery behind primary navigation, with a dark left-side vignette for legibility.
2. Keep surfaces dark, restrained and slightly translucent. Avoid bright blocks and generic developer-tool panel styling.
3. Gold indicates focus, selection or a primary action; it is not a general-purpose text colour.
4. Use consistent spacing, thin separators and subtle transitions rather than heavy borders or excessive decoration.
5. Keep the interface keyboard- and controller-accessible, with visible focus and clear input hints.
6. Preserve gameplay, networking, settings, save and launcher behaviour while changing presentation.

## Screen direction

- **Main menu:** cinematic world background; Play, Multiplayer, Settings and Exit are the dominant destinations. Secondary game tools remain reachable without competing with these actions.
- **Pause menu:** compact GTA-inspired navigation with Resume as the clear primary action; map and gameplay tools remain one step away.
- **HUD:** minimal, edge-anchored information. Keep the radar readable and avoid covering the world.
- **Launcher:** same palette and typography, with a strong home/launch area, server discovery, resources, settings and update status.
- **Dialogs and notifications:** consistent dark surfaces, clear severity, concise copy and gold focus treatment.

## Implementation order

1. Shared tokens and interaction styling.
2. In-game main menu and pause navigation.
3. Launcher home and navigation.
4. HUD, notifications and dialogs.
5. Screenshot review at 16:9, ultrawide and compact window sizes, followed by keyboard/controller and regression tests.
