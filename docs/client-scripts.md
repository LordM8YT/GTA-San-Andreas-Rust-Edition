# Client scripts and script UI

`client_script` and `shared_script` entries run on players' machines, like
FiveM. The server packs each resource's client Lua (with `@other/file.lua`
includes and the text files listed under `files`) into one bundle, sends its
SHA-256 when the resource starts and when a player joins, and the game
downloads the bundle and checks the hash before running it. Restarting a
resource restarts it on every client; stopping it stops it there.

```lua
fx_version 'cerulean'
game 'common'
shared_script '@sare_lib/init.lua'
server_script 'server.lua'
client_script 'client.lua'
files { 'data/*.json' }   -- readable with LoadResourceFile on the client
```

## Sandbox

Client scripts come from whichever server the player joins, so they run in a
sandbox:

- Only `table`, `string`, `math`, `utf8`, `coroutine` and `os.clock`/`os.time`.
  No `io`, `os.execute`, `require`, `package`, `dofile`, `loadfile` or `debug`.
- `load` accepts text only; binary chunks are rejected.
- 64 MiB of memory per resource, at most 64 client resources.
- Any single call into Lua (a frame, an event, a command) is stopped after
  250 ms, so an endless loop cannot freeze the game.
- `LoadResourceFile` reads only files in the downloaded bundles.
- `TriggerServerEvent` payloads are limited to 4 KiB, like other events.
- `SetEntityCoords`/`SetEntityHeading` only move the local player.

## Client API

Same as the server where it applies: `AddEventHandler`, `RegisterNetEvent`,
`TriggerEvent`, `TriggerServerEvent`, `RegisterCommand`, `ExecuteCommand`,
`RegisterKeyMapping`, `CreateThread`/`Wait`/`SetTimeout`, exports between client
resources, function references, `json`, `vector3`, `promise`, CfxLua syntax.
Natives: `PlayerId`, `PlayerPedId`, `GetPlayerServerId`, `GetPlayerFromServerId`,
`GetPlayerPed`, `GetPlayerName`, `GetActivePlayers`, `NetworkIsPlayerActive`,
`DoesEntityExist`, `IsPedAPlayer`, `GetEntityCoords`, `GetEntityHeading`,
`IsPedInAnyVehicle`, `SetEntityCoords`, `SetEntityHeading`, `GetGameTimer`,
`GetResourceState`, `GetCurrentResourceName`, `LoadResourceFile`.
Coordinates are San Andreas world coordinates. Only events registered with
`RegisterNetEvent` can be triggered by the server.

`RegisterKeyMapping('cmd', 'Label', 'keyboard', 'F2')` works for keys the game
does not use itself: F1, F2, F11, F12, B, C, H, J, K, L, N, O, U, Y, Z, Tab,
Home, End, Insert, Delete, Page Up and Page Down.

## Script UI

There are no web pages (NUI). Scripts describe UI with text and choices, and
the game draws it with its own widgets, so a server cannot run HTML or
JavaScript on players' machines. The API follows ox_lib:

```lua
UI.notify({ title = 'Bank', description = 'Deposited $250', type = 'success' })

UI.registerContext({
  id = 'shop',
  title = 'Shop',
  options = {
    { title = 'Bread', description = '$5', serverEvent = 'shop:buy', args = { item = 'bread' } },
    { title = 'Deposit', onSelect = function()
        local values = UI.inputDialog('Deposit', {
          { type = 'number', label = 'Amount', required = true },
          { type = 'select', label = 'Account', options = { { value = 'cash' }, { value = 'bank', label = 'Bank' } } },
          { type = 'checkbox', label = 'Receipt' },
        })
        if values then TriggerServerEvent('bank:deposit', values[1], values[2]) end
      end },
    { title = 'Closed', disabled = true },
  },
})
UI.showContext('shop')

if UI.progressBar({ label = 'Repairing', duration = 5000 }) then
  -- finished; false when the player pressed X
end
UI.showTextUI('[E] Open door')
UI.hideTextUI()
```

`UI.inputDialog` and `UI.progressBar` wait for the player, so call them from a
thread, a command or a menu option. Options can use `onSelect`, `event`,
`serverEvent`, `args` and `menu` (opens another registered menu). Menus and
dialogs free the mouse while open; Esc closes them.

Server scripts can show the same UI without client Lua:

```lua
TriggerClientEvent('sare:ui:notify', src, { title = 'Jobs', description = 'Hired', type = 'success' })
TriggerClientEvent('sare:ui:context', src, { title = 'Garage', options = {
  { title = 'Infernus', serverEvent = 'garage:spawn', args = 'infernus' },
} })
TriggerClientEvent('sare:ui:text', src, '[E] Garage')   -- nil hides it
TriggerClientEvent('sare:ui:dialog', src, 7, 'Name', { 'First name', 'Last name' })
TriggerClientEvent('sare:ui:progress', src, 8, { label = 'Loading', duration = 2000 })
```

Dialogs and progress bars answer with `sare:ui:dialogResult` and
`sare:ui:progressResult` (`token, value`); register them with `RegisterNetEvent`.
Text is plain, without markup, and limited to 256 characters per field.

## Example

`resources/[examples]/sare_jobs/client.lua` opens a job menu with **F2** or
`/jobs`. Its options run `/job` on the server, and "Work a shift" shows a
progress bar before `/work`. Start it with `ensure sare_jobs`.
`cargo test -p sa-server` downloads and runs these client scripts through a real
connection.
