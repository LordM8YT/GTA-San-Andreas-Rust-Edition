-- SARE RP core (server): which location a player stands at, server-side
-- dialogs, and small helpers the other RP resources use through exports.
local Core = exports.sarebox:GetCoreObject()
local RP = {}

local function flat(a, b)
  local dx, dy = a.x - b.x, a.y - b.y
  return math.sqrt(dx * dx + dy * dy)
end
RP.Distance = flat

function RP.Coords(src)
  return GetEntityCoords(GetPlayerPed(src))
end

-- The location key and point a player stands at, or nil. The server decides
-- this from the position it sees; the game only asks "use what is here".
function RP.PlaceAt(src)
  local here = RP.Coords(src)
  local best, bestKey, bestDistance
  for key, value in pairs(RPLocations) do
    local points = value.x and { value } or (type(value[1]) == 'table' and value or {})
    if RPConfig.Places[key] then
      for _, point in ipairs(points) do
        local d = flat(here, point)
        if d <= RPConfig.InteractRadius + 0.5 and math.abs(here.z - point.z) < 3.0 and (not bestDistance or d < bestDistance) then
          best, bestKey, bestDistance = point, key, d
        end
      end
    end
  end
  return bestKey, best
end

function RP.Near(src, key, radius)
  local value = RPLocations[key]
  if not value then return false end
  local points = value.x and { value } or value
  local here = RP.Coords(src)
  for _, point in ipairs(points) do
    if flat(here, point) <= (radius or RPConfig.InteractRadius + 0.5) and math.abs(here.z - point.z) < 3.0 then return true end
  end
  return false
end

-- Players within `radius` metres of `src` in the same routing bucket
-- (including `src` itself).
function RP.PlayersNear(src, radius)
  local here = RP.Coords(src)
  local bucket = GetPlayerRoutingBucket(src)
  local list = {}
  for _, id in ipairs(GetPlayers()) do
    id = tonumber(id)
    if GetPlayerRoutingBucket(id) == bucket and flat(here, RP.Coords(id)) <= radius then list[#list + 1] = id end
  end
  return list
end

function RP.Notify(src, text, kind, title)
  Core.Functions.Notify(src, text, kind or 'inform', title or 'SARE RP')
end

-- The character's roleplay name (set at first join), or the player name.
function RP.Name(src)
  local player = Core.Functions.GetPlayer(src)
  local id = player and player.PlayerData.metadata.identity
  if type(id) == 'table' and id.firstname then return id.firstname .. ' ' .. id.lastname end
  return GetPlayerName(src) or ('Player ' .. tostring(src))
end

-- Whole dollars from 1 to SARE Box's limit, or nil.
function RP.Amount(value)
  local n = tonumber(value)
  if not n or n ~= n or n == math.huge or n < 1 or n ~= math.floor(n) or n > Core.Config.MaxMoney then return nil end
  return math.tointeger(n)
end

function RP.OnDuty(src, job)
  local player = Core.Functions.GetPlayer(src)
  local data = player and player.PlayerData.job
  return data ~= nil and data.name == job and data.onduty == true
end

function RP.OnDutyPlayers(jobs)
  local list = {}
  for _, src in ipairs(Core.Functions.GetPlayers()) do
    for _, job in ipairs(jobs) do
      if RP.OnDuty(src, job) then
        list[#list + 1] = src
        break
      end
    end
  end
  return list
end

-- Server dialogs. `callback(values)` gets the list of values, or nil when
-- the player cancelled. Each dialog has its own token, so a reply can only
-- answer the dialog the server opened for that player.
local Dialogs, Progress, nextToken = {}, {}, 4100000
function RP.Dialog(src, heading, rows, callback)
  nextToken = nextToken + 1
  Dialogs[nextToken] = { src = src, callback = callback, at = GetGameTimer() }
  TriggerClientEvent('sare:ui:dialog', src, nextToken, heading, rows)
  return nextToken
end
RegisterNetEvent('sare:ui:dialogResult', function(token, values)
  local src = source
  local dialog = Dialogs[tonumber(token)]
  if not dialog or dialog.src ~= src then return end
  Dialogs[tonumber(token)] = nil
  local ok, err = pcall(dialog.callback, type(values) == 'table' and values or nil)
  if not ok then print(('^1[sare_rp_core] dialog callback failed: %s^7'):format(err)) end
end)
AddEventHandler('playerDropped', function()
  local src = source
  for token, dialog in pairs(Dialogs) do
    if dialog.src == src then Dialogs[token] = nil end
  end
  for token, entry in pairs(Progress) do
    if entry.src == src then Progress[token] = nil end
  end
end)

-- A progress bar on the player's screen. `callback(true)` when it finished,
-- `callback(false)` when the player cancelled it.
function RP.Progress(src, label, ms, callback)
  nextToken = nextToken + 1
  Progress[nextToken] = { src = src, callback = callback, done = GetGameTimer() + ms - 250 }
  TriggerClientEvent('sare:ui:progress', src, nextToken, { label = label, duration = ms })
  return nextToken
end
RegisterNetEvent('sare:ui:progressResult', function(token, finished)
  local src = source
  local entry = Progress[tonumber(token)]
  if not entry or entry.src ~= src then return end
  Progress[tonumber(token)] = nil
  -- A modified game cannot finish early: the server keeps the time.
  local ok, err = pcall(entry.callback, finished == true and GetGameTimer() >= entry.done)
  if not ok then print(('^1[sare_rp_core] progress callback failed: %s^7'):format(err)) end
end)

-- A server menu: options are { title, description, serverEvent, args, disabled }.
function RP.Menu(src, title, options)
  TriggerClientEvent('sare:ui:context', src, { id = 'sare_rp_menu', title = title, options = options })
end

-- Third-person action text, as /me shows it, to everyone close by.
function RP.Me(src, action)
  for _, id in ipairs(RP.PlayersNear(src, RPConfig.Chat.me)) do
    TriggerClientEvent('chat:addMessage', id, { color = { 200, 150, 255 }, args = { '* ' .. RP.Name(src), action } })
  end
end

-- The game presses the interaction key: find what the player stands at and
-- let the resource that owns that kind of place answer.
RegisterNetEvent('sare_rp:server:interact', function()
  local src = source
  if not Core.Functions.GetPlayer(src) then return end
  local key, point = RP.PlaceAt(src)
  if not key then return RP.Notify(src, 'There is nothing to use here.') end
  local place = RPConfig.Places[key]
  if place.job then
    local job = Core.Functions.GetJob(place.job)
    local name = job and job.label or place.job
    local current = Core.Functions.GetPlayer(src).PlayerData.job
    if current.name ~= place.job then return RP.Notify(src, ('This is for %s only.'):format(name), 'error') end
    if place.duty and not current.onduty then return RP.Notify(src, 'Go on duty first.', 'error') end
  end
  if Player(src).state.cuffed then return RP.Notify(src, 'You are in handcuffs.', 'error') end
  TriggerEvent('sare_rp:interact', src, key, point)
end)

for name, fn in pairs(RP) do exports(name, fn) end
exports('GetRP', function() return RP end)
