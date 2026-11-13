-- Server-owned vehicles: the server creates them, every player in the same
-- routing bucket sees them, and they are deleted when this resource stops.
local spawned = {} -- player -> vehicle handle
local MAX_PER_PLAYER = 1

local function notify(src, message)
  TriggerClientEvent('chat:addMessage', src, { color = { 255, 200, 80 }, args = { 'Vehicles', message } })
end

local function deleteFor(src)
  local vehicle = spawned[src]
  if vehicle and DoesEntityExist(vehicle) then DeleteEntity(vehicle) end
  spawned[src] = nil
end

-- /car [model]: a vehicle a few metres in front of you, in your instance.
RegisterCommand('car', function(src, args)
  if src == 0 then return end
  local model = args[1] or 'infernus'
  if not model:match('^[%w_%-]+$') then
    notify(src, 'Use a model name, for example /car infernus')
    return
  end
  if MAX_PER_PLAYER == 1 then deleteFor(src) end
  local ped = GetPlayerPed(src)
  local at = GetEntityCoords(ped)
  local heading = GetEntityHeading(ped)
  local rad = math.rad(heading)
  local vehicle = CreateVehicle(model, at.x - math.sin(rad) * 4.0, at.y + math.cos(rad) * 4.0, at.z, heading, true, true)
  if vehicle == 0 then
    notify(src, 'The server could not create that vehicle.')
    return
  end
  SetEntityRoutingBucket(vehicle, GetPlayerRoutingBucket(src))
  Entity(vehicle).state:set('owner', src, false)
  spawned[src] = vehicle
  notify(src, ('Spawned %s (entity %d).'):format(model, vehicle))
end, false)

-- /dv: delete your vehicle.
RegisterCommand('dv', function(src)
  if spawned[src] then
    deleteFor(src)
    notify(src, 'Vehicle deleted.')
  end
end, false)

AddEventHandler('playerDropped', function()
  deleteFor(source)
end)

-- Vehicles follow their owner into another instance.
AddEventHandler('sare_instances:changed', function(src, bucket)
  local vehicle = spawned[src]
  if vehicle and DoesEntityExist(vehicle) then SetEntityRoutingBucket(vehicle, bucket) end
end)
