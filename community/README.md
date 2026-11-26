# Community lists

This folder holds the two public lists SARE Client reads: servers
(`servers.json`) and shared worlds (`worlds.json`). Both are changed with a
pull request.

## Servers

`servers.json` is the public server list SARE Client shows under **Servers**.
The client downloads it from this repository, then asks every listed server
itself for its name, player count, world and protocol version. Nothing here is
typed in by hand except the name, address and an optional description, so the
numbers players see are always the server's own.

### Add your server

1. Run `sa-server` on a machine players can reach: a public IP or DNS name and
   TCP port 7777 (or your `endpoint_add_tcp` port) open in the firewall and
   router. See [server hosting](../docs/server-hosting.md).
2. Open a pull request that adds one entry to `servers`:

   ```json
   {
     "name": "Grove Street RP",
     "address": "play.example.org:7777",
     "description": "Roleplay, English, whitelist-free",
     "website": "https://example.org"
   }
   ```

   - `name`: up to 48 characters.
   - `address`: `host:port`; the host may be an IP or a DNS name.
   - `description` (optional): up to 200 characters.
   - `website` (optional): an `https://` link, up to 200 characters.
3. A maintainer merges it when the server answers. Servers that stay offline
   for a long time are removed again.

Test your entry before the pull request: save a copy of the list and start the
client with `SARE_SERVER_LIST=/path/to/servers.json`.

### Rules

- SARE is free: no paid access, paid perks, donations or sponsorships.
- No servers that use Rockstar or GTA logos or branding.
- One entry per server address.

Entries that break these rules, or that the client rejects (bad address, name
too long, `http://` website), are left out.

## Worlds

`worlds.json` lists worlds (content packs) anyone can install from
**Discover › From the community** with one click. SARE Client downloads the
ZIP, checks its size and SHA-256 against the list, checks the world's
`content.json`, and puts it in the player's `content/` folder. A world the
player made or copied in themselves with the same id is never replaced; a
newer catalog version replaces an older catalog install.

### Share your world

In SARE Studio, *Workspace › Sharing › Export world* does steps 1 and 3
for you: it makes the ZIP, its size and SHA-256, and copies the entry below
with only the download link left to fill in.

1. Zip the world folder (the folder with `content.json`; the ZIP may hold the
   folder itself or its contents). Up to 256 MiB, 1024 files.
2. Upload the ZIP somewhere that serves it over `https://` without a login,
   for example as a file on a GitHub release of your own repository.
3. Get its size in bytes and SHA-256 (`sha256sum world.zip` on Linux and
   macOS, `Get-FileHash world.zip` in PowerShell).
4. Open a pull request that adds an entry:

   ```json
   {
     "id": "hill-race",
     "name": "Hill Race",
     "author": "Your name",
     "description": "A short downhill race with jumps.",
     "version": "1.0.0",
     "license": "CC-BY-4.0",
     "download": "https://github.com/you/hill-race/releases/download/v1.0.0/hill-race.zip",
     "sha256": "…64 hex digits…",
     "bytes": 4200000
   }
   ```

   `id` must match the `id` in the world's `content.json`. For an update,
   change `version`, `download`, `sha256` and `bytes` together.

Worlds follow the same rules as servers, and must not contain files from
Rockstar games or anything you do not have the right to share.
