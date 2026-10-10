# SARE Box

SARE Box is a small roleplay framework that ships with the server, built the
way QBCore and Qbox are: one core resource with a player object per player,
used by other resources through exports and events. It is SARE's own
framework, not Qbox; see [framework compatibility](framework-compatibility.md)
for what running Qbox itself would need.

## Starting a SARE Box server

On the first start without a `server.cfg`, `sa-server` asks which server to
create. Pick **SARE Box**, or pass `+set sv_template sarebox` when there is no
console. The template runs `ensure [sarebox]`, which starts:

- `sarebox_core`: characters, money, jobs, paychecks, the HUD and the F1 player
  menu.
- `sarebox_admin`: admin commands and the F11 admin menu.

## Players and saving

A player's game creates a secret key (`identity.key` beside its settings) and
sends the server a hash of it with the server's salt (`identity.salt` next to
`server.cfg`). That hash is the player's `license:` identifier: the same
player gets the same license on this server every time, and a different one on
other servers. It is not a signed proof of identity yet: a malicious server
could learn a player's license for another server by copying that server's
salt. Deleting `identity.key` gives the player a new identity.

Characters are saved per license with resource KVP in
`kvp/sarebox_core.json`: name, citizen ID, cash, bank, job and grade, duty,
metadata and the last position. They load when the player joins and save on
every money or job change, every minute, and when the player leaves.

## In the game

- **F1** or `/playermenu`: money, job, citizen ID, duty on or off, HUD on or off.
- `/hud` hides or shows the money and job display.
- `/id` shows your server ID and license.
- On-duty players get their job's pay every 10 minutes.

## Admins

Admin commands are restricted. `server.cfg` gives them to `group.admin`; add
yourself by license (the console prints it when you join, and `/id` shows it):

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

## Jobs

Jobs live in `sarebox_core/shared/config.lua` with grades and pay, like
QBCore's shared jobs: `unemployed`, `taxi`, `mechanic` and `police` to start
with. Add your own there and restart the resource.

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
State bags on `Player(src).state`: `cash`, `bank`, `job`, `citizenid`.

Events: `sarebox:server:playerLoaded`, `sarebox:server:moneyChanged`
(`src, account, amount, reason`), `sarebox:server:onJobUpdate`; on the client
`sarebox:client:playerLoaded`, `sarebox:client:setPlayerData`,
`sarebox:client:onJobUpdate`. Client export:
`exports.sarebox_core:GetPlayerData()`.

The player object you get through an export is a copy of `PlayerData` at the
time of the call, with functions that act on the live player; call
`GetPlayer` again for fresh values.
