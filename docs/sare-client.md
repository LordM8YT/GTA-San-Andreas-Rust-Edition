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
status. Friends only shows a friend online when the directory says so; Messages
explains that private messages are not built yet. The
profile uses the local player name from Settings.

## Pages

Home has a featured banner (worlds with their own cover first) and shelves.
Every tile opens the experience page: cover, description, About (type,
version, licence, folder) and Play, Continue (San Andreas with a saved
position), Set up, Edit in Build Mode and Open folder.

Create lists templates: Blank world plus the bundled example packs
(`experience::TEMPLATES`). A template is copied into `content/<new id>` with
the chosen name, no author and version 0.1.0, then opened in Build Mode; the
template itself is never changed. Templates stay playable from their page and
servers can still use them as content packs, but they are not listed as games.

## Social and discovery plan

SARE has no service of its own yet, so this is built in steps that each work
on their own:

1. **Public server list (done).** `community/servers.json` in this repository
   (see [its README](../community/README.md)). Servers asks every listed
   server directly for its status with `sa_net::query` (`Message::Info`,
   answered before any join and without taking a slot). `SARE_SERVER_LIST`
   points the client at another URL or a local file. Hosts older than this
   build drop the query and show as offline.
2. **Download the world on join (done).** A server on a content pack
   (`sets content_pack`, or a player hosting in a pack) shares that pack's
   folder: `Share::add_world` lists its files (scenes, textures, JSON, Lua,
   text; hidden files and other types are left out; up to 1024 files and
   256 MiB). A joining player who is in another world asks with
   `WorldQuery`, fetches the inventory by hash, checks every path and hash,
   and stores the world in `server-worlds/<id>-<hash>/` beside the mod cache,
   reusing identical files from their own copy. SARE then restarts in that
   folder. The player's own `content/` is never written to; the eight most
   recent server worlds are kept. Older servers that do not share their world
   fall back to the player's own copy, as before. Known gap: a player who
   already has a pack with the same id open joins with that copy, even if the
   server's differs.
3. **World catalog in Discover (done).** `community/worlds.json` lists shared
   worlds as ZIP links with size and SHA-256 (`world_catalog.rs`,
   `SARE_WORLD_LIST` overrides the source). Discover shows them under *From
   the community*; Install downloads, checks the hash, unpacks into
   `content/.<id>-installing` (no paths outside the folder, 256 MiB, 1024
   files), checks `content.json` and moves it to `content/<id>` with a
   `.sare-catalog` marker. Only marked folders are ever replaced (Update).
   Next: cover images for worlds that are not installed yet.
4. **SARE ID and friends (done).** `sa_net::directory`: a SARE ID derived
   from `identity.key` with its own domain (servers never see it), a friend
   code `XXXX-XXXX-XXXX` (60-bit hash of the key, Crockford base32, forgiving
   input), and friends kept in `launcher.json`. Friends shows the code, Add
   friend, each friend's status and Join (when they joined a server by IP).
   `friends.rs` sends a signed heartbeat about once a minute.
5. **Online service (`sa-directory`, built).** In-memory only, answers with
   friends who are online and added the player back. See
   [sare-directory.md](sare-directory.md) for the API, privacy and hosting.
   The official instance runs on Render's free plan at
   `https://sare-directory.onrender.com` (`friends::DEFAULT_DIRECTORY`); the
   setting or `SARE_DIRECTORY` points elsewhere.
   Next: private messages, invites, and live server heartbeats.

## Next steps

1. Covers for the Gameplay Sample and the testbed (in-engine captures).
2. Recently played and favorites for worlds, stored in the launcher config.
3. Optional categories for content packs (manifest fields must stay readable
   by older clients, which reject unknown fields).
