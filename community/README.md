# Community server list

`servers.json` is the public server list SARE Client shows under **Servers**.
The client downloads it from this repository, then asks every listed server
itself for its name, player count, world and protocol version. Nothing here is
typed in by hand except the name, address and an optional description, so the
numbers players see are always the server's own.

## Add your server

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

## Rules

- SARE is free: no paid access, paid perks, donations or sponsorships.
- No servers that use Rockstar or GTA logos or branding.
- One entry per server address.

Entries that break these rules, or that the client rejects (bad address, name
too long, `http://` website), are left out.
