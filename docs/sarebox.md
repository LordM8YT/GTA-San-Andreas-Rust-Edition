# SARE Box

SARE Box is a small roleplay framework that ships with the server, built the
way QBCore and Qbox are: one core resource with a player object per player,
used by other resources through exports and events.

**SARE Box is not Qbox.** It is SARE's own framework with a similar shape
(`GetCoreObject`, `PlayerData`, `Functions.AddMoney`, shared jobs). Qbox
resources do not run on it, and SARE does not run Qbox itself yet: Qbox needs
oxmysql, OneSync entities, NUI and GTA V natives, which SARE does not have.
See [framework compatibility](framework-compatibility.md) for the exact state.

## Install and start

**New server.** On the first start without a `server.cfg`, `sa-server` asks
which server to create. Pick **SARE Box**, or pass `+set sv_template sarebox`
when there is no console. The template runs `ensure [sarebox]`, which starts
every resource in `resources/[sarebox]`:

- `sarebox_core`: characters, money, jobs, paychecks, the HUD and the F1 player
  menu.
- `sarebox_admin`: admin commands and the F11 admin menu.
- `sarebox_taxi`: the taxi job (see [Taxi job](#taxi-job)).

**Existing server.** The automatic updater replaces only `server/`, never your
`server-data/`. When you update to this version, also copy
`server-data/resources/[sarebox]/` from the new server ZIP over yours (keep a
copy of your `shared/config.lua` if you changed it). New binaries with old
SARE Box files still run, but players get new characters until the files are
updated; an admin can give the old ones back afterwards with
`sarebox_migrate` (see [Moving old characters](#moving-old-characters)).

To leave out a resource, move its folder out of `[sarebox]`, or replace
`ensure [sarebox]` with `ensure sarebox_core` and `ensure sarebox_admin`.

## Player identity

Every player's game has a secret key (`identity.key` beside its settings).
Every dedicated server has its own key, `server.key` next to `server.cfg`,
created on the first start. When a player joins:

1. The game sends a random number with its hello.
2. The server answers with its public key, a random number of its own and a
   signature over both numbers and the player's server ID.
3. The game checks that signature, derives a key for this server only from its
   secret and the server's public key, and signs the server key, both numbers,
   its server ID and its own public key.
4. The server checks that signature. The player's public key for this server
   becomes their `license:` identifier.

Signatures are Ed25519 (`ed25519-dalek`). Because both random numbers are new
on every connection, a recorded answer cannot be replayed. Because the game's
key is derived from the server's key, the same player has a different license
on every server, and a server cannot sign in as the player anywhere else.

What this does not protect against: SARE traffic is not encrypted. Someone who
can sit between the player and the server (or a relay operator) for the whole
session could pass the handshake through and inject traffic afterwards. Play
on networks and relays you trust.

**Back up `server.key`.** It is what makes licenses stable. A new key gives
every player a new license (their characters stay saved but are not found).
Older games that do not sign still connect, but get no license: they play as
guests and nothing is saved.

## Moving old characters

Before this version, the license was an unsigned hash of the player's secret
and the server's `identity.salt`. Every client that connected learned the
salt, and licenses are visible (console, `/id`, `server.cfg`), so an old
license proves nothing: anyone could repeat one.

When `identity.salt` still exists next to `server.cfg`, the game also reports
the old license it had on this server, as a `legacy:` identifier. It is a
claim, not a proof, so it never grants ACL rights. What SARE Box does with a
claim on a saved character is set with `sarebox_migration` in `server.cfg`:

| Setting | What happens | When to use it |
| --- | --- | --- |
| `admin` (default) | The player plays as an unsaved guest. The console prints the claim with the old character's name. An admin checks it is really that person and runs `sarebox_migrate <server id>`, or `sarebox_migrate <server id> reject` to give them a new character. | Public servers |
| `auto` | The first signed player who claims an old character gets it, as the old system worked. Someone who knows another player's old license could take that character. | Small private servers where everyone knows each other |
| `off` | Old characters are ignored; everyone starts new. | Fresh starts |

```
set sarebox_migration admin
```

A moved character is linked to the new license and the old license is
removed, so it cannot be claimed twice. `sarebox_migrate <id>` also works for
a player who already got a new character (for example when new binaries ran
with old SARE Box files); their new character is then unlinked, not deleted.
Delete `identity.salt` once everyone has moved; after that no claims are made.

**Admins need their new license.** `add_principal identifier.license:...`
lines with old licenses no longer match. Join, run `/id` (or read the console
line "Player ... joined, license:..."), and replace the line in `server.cfg`.
For the current session only: `add_principal player.<id> group.admin`.

## When data is saved

Characters live in resource KVP, in `kvp/sarebox_core.json` next to
`server.cfg`: name, citizen ID, cash, bank, job and grade, metadata and the
last position.

| Change | Saved in memory | On disk |
| --- | --- | --- |
| Money, job, metadata, a new character | at once | within about one second |
| Position | every minute, when the player leaves, when `sarebox_core` stops | within about one second after that |
| On duty / off duty | not saved: each job's `defaultDuty` applies on join | – |
| `quit`, Ctrl+C, SIGTERM, `restart sarebox_core` | everything | at once |

Disk writes are atomic: the server writes `sarebox_core.json.tmp`, syncs it,
keeps the previous file as `sarebox_core.json.bak` and renames the new file
into place. A crash or power loss leaves either the old or the new file. If
the file cannot be read at start (damaged disk, manual edit), it is moved
aside as `sarebox_core.json.corrupt-<time>`, the backup is loaded, and the
console says so; the damaged file is never overwritten.

**What a crash can lose:** changes from about the last second, and up to a
minute of movement (players return to where they were at the last save).
Money is never paid twice by a reconnect: a character loads once per
connection, and leaving or rejoining does not repeat paychecks or taxi fares.

## In the game

- **F1** or `/playermenu`: money, job, citizen ID, duty on or off, HUD on or off.
- **J** or `/taxi`: the taxi menu.
- `/hud` hides or shows the money and job display.
- `/id` shows your server ID and license.
- On-duty players get their job's pay every 10 minutes.

## Admins

Admin commands are restricted. `server.cfg` gives them to `group.admin`; add
yourself by license (see [Player identity](#player-identity)):

```
add_ace group.admin command allow
add_principal identifier.license:0123abcd... group.admin
```

| Command | What it does |
| --- | --- |
| `/admin` or **F11** | Player list with go to, bring, give money, set job and kick, plus announcements |
| `/givemoney <id> <cash\|bank> <amount>` | Adds money |
| `/setmoney <id> <cash\|bank> <amount>` | Sets money |
| `/setjob <id> <job> [grade]` | Sets job and grade |
| `/goto <id>`, `/bring <id>` | Teleports |
| `/kick <id> [reason]` | Disconnects a player |
| `/announce <text>` | Notification for everyone |
| `sarebox_migrate <id> [reject]` | Approves or rejects a claim on an old character |

The server checks every action, also those from the F11 menu: the player
needs `command.<name>` in the ACL. Amounts must be whole dollars from 1 to
`Config.MaxMoney` (one billion by default); fractions, negative numbers,
`nan`, `inf` and text are refused. Every admin action is printed on the
console with who did it (`[admin] Name (3, license:...): givemoney 4 cash 50`).

## Jobs

Jobs live in `sarebox_core/shared/config.lua` with grades and pay, like
QBCore's shared jobs: `unemployed`, `taxi`, `mechanic`, `police` and
`ambulance` (EMS) to start with. Add your own there and restart the resource.

For a complete roleplay server built on SARE Box (identity, bank, inventory,
shops, police/EMS/taxi/mechanic, dealer and garage) on its own map, pick the
**SARE RP Town** template (`+set sv_template sare-rp`); see
[its README](../server-data/resources/[sare-rp]/README.md).

## Taxi job

`sarebox_taxi` is a complete small job that shows how a SARE Box job is built.
SARE has no NPC passengers yet, so a run is a drive to a dispatch point in
your car, not passenger transport.

1. Open the taxi menu with **J** or `/taxi`. Unemployed players can pick
   **Sign up as a taxi driver** (the server sets the job; players with another
   job are refused).
2. Get in a car and pick **Take a dispatch**. The server picks a point in Los
   Santos 250 to 3000 m away and the fare: $40 plus $0.12 per metre in a
   straight line, at most $450.
3. A yellow blip marks the point on the radar and the pause map, and a
   yellow GPS line shows the way along the roads. The side of the screen
   shows the distance and the direction.
4. Stop within 12 m of the point in a car. The server sees it within half a
   second and pays the fare into your cash once. The next dispatch is
   available 20 seconds later.

"In a car" is what the server can see: driving your personal car, or riding
in another player's car as a passenger.

The server decides everything: the game can only ask to start, cancel or sign
up. It never sends a place, an amount or "I arrived". The server ends a run
unpaid when the player's position jumps further than a car can drive (a
teleport), arrives faster than 50 m/s on average over the straight-line
distance, takes longer than 10 minutes, goes off duty, changes job, leaves,
or when `sarebox_taxi` or `sarebox_core` restarts. Runs are kept in memory
only, so a restart cancels open runs (the players are told) and nothing is
paid for them. The fare is saved like any money change.

All numbers and the dispatch points are in `sarebox_taxi/config.lua`.

### Testing the taxi job

1. Start a SARE Box server and join it.
2. Press **J**, pick **Sign up as a taxi driver**. The HUD shows
   `Taxi - Recruit`.
3. Get in your car (pick a car in the game menu, then **F** to get in),
   press **J** and pick **Take a dispatch**.
4. Follow the yellow GPS line to the yellow blip. Within 12 m, stop:
   "Arrived at ... Paid $..." and the cash on the HUD goes up by the fare.
5. Try to break it: take a dispatch and teleport there with `/goto` from an
   admin, or press **J** and cancel, or go off duty in **F1**: no money.

`cargo test -p sa-server` runs the same flow automatically against the real
resources, with a real client connection and the client scripts in the game's
sandbox (`taxi_runs_are_started_checked_and_paid_by_the_server_once`).

## Restarting resources

`restart sarebox_core` (or `ensure`) stops the resources that depend on it
first (`sarebox_admin`, `sarebox_taxi`, and any of yours with
`dependency 'sarebox'`), then starts the core and those resources again.
`stop sarebox_core` stops them too. Players stay logged in: they are saved
on stop and loaded again on start. In the game, stopping a resource removes
its menus, dialogs, progress bars and text, so a restart leaves no stale menu
and no second HUD.

## For resource authors

```lua
-- fxmanifest.lua: dependency 'sarebox'
local Core = exports.sarebox:GetCoreObject()

RegisterCommand('tip', function(src, args)
  local player = Core.Functions.GetPlayer(src)
  if player and player.Functions.RemoveMoney('cash', 5, 'tip') then
    Core.Functions.Notify(src, 'Thanks!', 'success')
  end
end)

AddEventHandler('sarebox:server:playerLoaded', function(player)
  print(player.PlayerData.citizenid, player.PlayerData.job.name)
end)
```

Server: `Core.Functions.GetPlayer(src)`, `GetPlayerByCitizenId`, `GetPlayers`,
`GetJob`, `Notify(src, text, type, title)`. Player:
`player.PlayerData` (`citizenid`, `license`, `name`, `money`, `job`,
`metadata`), `player.Functions.AddMoney/RemoveMoney/SetMoney/GetMoney`,
`SetJob(name, grade)`, `SetJobDuty`, `SetMetaData/GetMetaData`, `Save`.
Money functions return `false` for invalid amounts and when a balance would
go below zero. State bags on `Player(src).state`: `cash`, `bank`, `job`,
`citizenid`.

Events: `sarebox:server:playerLoaded`, `sarebox:server:moneyChanged`
(`src, account, amount, reason`), `sarebox:server:onJobUpdate`; on the client
`sarebox:client:playerLoaded`, `sarebox:client:setPlayerData`,
`sarebox:client:onJobUpdate`. Client export:
`exports.sarebox_core:GetPlayerData()`.

The player object you get through an export is a copy of `PlayerData` at the
time of the call, with functions that act on the live player; call
`GetPlayer` again for fresh values.

Rules that keep a job safe, as `sarebox_taxi` does: decide amounts, targets
and completion on the server; accept no amount or position from a client
event; close a task before paying for it; and end open tasks on
`playerDropped` and `onResourceStop`.

## Known limitations

- Movement is decided by each player's game. The server checks positions for
  plausibility (taxi), but a modified game can still drive "legally" without
  driving. There is no server-side physics.
- No database: KVP files hold up to 8 MB per resource, enough for thousands
  of characters, but there is no query language or multi-server sharing.
- One character per license; no character selection screen.
- No inventory, phone, garages or NPC passengers in SARE Box itself; the
  SARE RP Town scripts (`[sare-rp]`) add inventory, a phone item, a car
  dealer, garages and drivable server cars.
- Traffic is not encrypted (see [Player identity](#player-identity)).
