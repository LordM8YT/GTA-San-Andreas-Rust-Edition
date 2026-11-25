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

## Look

Type is Inter (SIL Open Font License, `crates/launcher/assets/fonts`, licence
text beside the files); the SARE wordmark stays Archivo Black. Icons come from
Phosphor through the `egui-phosphor` crate (MIT). Icon fonts live in their own
font families because Inter has glyphs in the same private-use range, so put
icons in buttons as a separate part: `Button::new((shell::icon(..), "Label"))`.
Colours are in `shell::palette`: neutral greys, gold only for the main action
and the active page. A content pack can ship `cover.png` (16:9) as its card
image; without one it gets a generated gradient.

## Data rules

Nothing is invented. No player counts, ratings, friend activity or online
status. SARE has no accounts or social backend yet, so Friends and Messages
explain that and point to what works today (servers and join codes). The
profile uses the local player name from Settings.

## Pages

Home has a featured banner (worlds with their own cover first) and shelves.
Every tile opens the experience page: cover, description, About (type,
version, licence, folder) and Play, Continue (San Andreas with a saved
position), Set up, Edit in Build Mode and Open folder.

## Next steps

1. Covers for the Gameplay Sample and the testbed (in-engine captures).
2. Recently played and favorites for worlds, stored in the launcher config.
3. Optional categories for content packs (manifest fields must stay readable
   by older clients, which reject unknown fields).
