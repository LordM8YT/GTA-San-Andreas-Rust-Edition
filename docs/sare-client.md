# SARE Client

The launcher (`native/crates/launcher`, egui) is turning into SARE Client: one
place to discover, play, manage and create worlds. San Andreas free roam is one
experience among others, not the identity of the app.

## Structure

- `shell.rs`: persistent sidebar (`SECTIONS`, add a page there), compact header
  (search, player, settings), experience cards, empty states and the original
  outline icons. Presentation only; it returns what the player clicked.
- `experience.rs`: one model for everything the client can show and start:
  the player's San Andreas installation, the built-in testbed and content packs
  from `content/`. Each kind maps to the launcher's existing launch path, so
  there is no second launch or install system.
- `pages.rs`: Discover, Library, Create, Profile, Friends and Messages.
  Home, Servers, Mods, Settings and Help are the existing pages.

## Data rules

Nothing is invented. No player counts, ratings, friend activity or online
status. SARE has no accounts or social backend yet, so Friends and Messages
explain that and point to what works today (servers and join codes). The
profile uses the local player name from Settings.

## Next steps

1. Home: featured banner and card rows built from the experience list.
2. Experience detail page (cover, description, creator, version, licence,
   Play/Edit), ready for server lists and update history later.
3. Optional cover image and categories for content packs (manifest fields
   must stay readable by older clients, which reject unknown fields).
4. Recently played and favorites for worlds, stored in the launcher config.
