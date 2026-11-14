-- Admin extras for SARE RP. SARE Box's sarebox_admin already has /admin
-- (F11), /givemoney, /setmoney, /setjob, /goto, /bring, /kick and /announce.
-- These commands are restricted: the player needs command.<name> in the ACL,
-- which server.cfg gives group.admin.
local Core = exports.sarebox:GetCoreObject()
local RP = exports.sare_rp_core:GetRP()

local function reply(src, text)
  if src == 0 then print(text) else RP.Notify(src, text, 'inform', 'Admin') end
end
local function log(src, text)
  print(('[admin] %s: %s'):format(src == 0 and 'console' or ('%s (%d)'):format(GetPlayerName(src), src), text))
end

local function placeNames()
  local names = {}
  for key, value in pairs(RPLocations) do
    if value.x then names[#names + 1] = key end
  end
  table.sort(names)
  return names
end

-- /tp <x> <y> <z>  or  /tp <place>   (San Andreas world coordinates)
RegisterCommand('tp', function(src, args)
  if src == 0 then return end
  local x, y, z = tonumber(args[1]), tonumber(args[2]), tonumber(args[3])
  local point
  if x and y then
    point = { x = x, y = y, z = z or 1.0 }
  elseif args[1] and RPLocations[args[1]] and RPLocations[args[1]].x then
    point = RPLocations[args[1]]
  else
    return reply(src, 'Usage: /tp <x> <y> [z] or /tp <place>: ' .. table.concat(placeNames(), ', '))
  end
  SetEntityCoords(GetPlayerPed(src), point.x, point.y, point.z)
  log(src, ('tp %.1f %.1f %.1f'):format(point.x, point.y, point.z))
end, true)

-- /giveitem <id> <item> [count]
RegisterCommand('giveitem', function(src, args)
  local target, item, count = tonumber(args[1]), args[2], tonumber(args[3] or 1)
  if not target or not Core.Functions.GetPlayer(target) or not RPConfig.Items[item or ''] then
    local names = {}
    for name in pairs(RPConfig.Items) do names[#names + 1] = name end
    table.sort(names)
    return reply(src, 'Usage: /giveitem <id> <item> [count]. Items: ' .. table.concat(names, ', '))
  end
  if exports.sare_rp_inventory:AddItem(target, item, count) then
    log(src, ('giveitem %d %s %d'):format(target, item, count))
    reply(src, ('Gave %s %dx %s'):format(RP.Name(target), count, item))
  else
    reply(src, 'They cannot carry that.')
  end
end, true)

-- /clearinventory <id>
RegisterCommand('clearinventory', function(src, args)
  local target = tonumber(args[1])
  local player = target and Core.Functions.GetPlayer(target)
  if not player then return reply(src, 'Usage: /clearinventory <id>') end
  player.Functions.SetMetaData('inventory', {})
  log(src, 'clearinventory ' .. target)
  reply(src, 'Inventory cleared.')
end, true)

-- /dvall: delete every RP vehicle that is out (owners can take theirs out again).
RegisterCommand('dvall', function(src)
  local count = 0
  for _, handle in ipairs(GetAllVehicles()) do
    if Entity(handle).state.plate then
      DeleteEntity(handle)
      count = count + 1
    end
  end
  log(src, 'dvall ' .. count)
  reply(src, ('Deleted %d vehicle(s).'):format(count))
end, true)

-- /coords: where you stand, in world and Blender coordinates (for map edits
-- and new places in tools/blender/make_rp_town.py). Anyone may use it.
RegisterCommand('coords', function(src)
  if src == 0 then return end
  local c = RP.Coords(src)
  local text = ('World %.2f, %.2f, %.2f  |  heading %.0f  |  Blender %.2f, %.2f, %.2f'):format(
    c.x, c.y, c.z, GetEntityHeading(GetPlayerPed(src)), c.x - 2500.0, c.y + 1670.0, c.z)
  TriggerClientEvent('chat:addMessage', src, { color = { 120, 200, 255 }, args = { 'Coords', text } })
end, false)
