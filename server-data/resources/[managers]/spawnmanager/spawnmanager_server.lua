-- FiveM's spawnmanager runs on the client. In SARE it runs on the server:
-- this server version picks the point and moves the player with SetEntityCoords.

local autoSpawn = true
local extra = {}

local function points()
  local list = {}
  for _, p in ipairs(exports.mapmanager:getSpawnPoints() or {}) do list[#list + 1] = p end
  for _, p in ipairs(extra) do list[#list + 1] = p end
  return list
end

local function spawnPlayer(player, point)
  local list = points()
  point = point or list[math.random(#list)]
  if not point then
    print('spawnmanager: no spawn points; start a map resource')
    return false
  end
  SetEntityCoords(player, point.x, point.y, point.z)
  SetEntityHeading(player, point.heading or 0.0)
  TriggerEvent('playerSpawned', player, point)
  TriggerClientEvent('playerSpawned', player, point)
  return true
end

exports('setAutoSpawn', function(enabled) autoSpawn = enabled end)
exports('spawnPlayer', spawnPlayer)
exports('addSpawnPoint', function(point)
  extra[#extra + 1] = point
  return #extra
end)
exports('removeSpawnPoint', function(index) extra[index] = nil end)

AddEventHandler('playerJoining', function()
  if autoSpawn then spawnPlayer(source) end
end)
