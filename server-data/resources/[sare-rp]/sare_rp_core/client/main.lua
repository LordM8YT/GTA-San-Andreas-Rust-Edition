-- SARE RP core (game): map blips, the "[H] Bank" hint near places, the
-- interaction key and temporary dispatch blips. The server checks everything.
local blips, nearLabel = {}, nil

local function addBlip(point, label, colour)
  local blip = AddBlipForCoord(point.x, point.y, point.z)
  SetBlipColour(blip, colour or 0)
  SetBlipAsShortRange(blip, true)
  BeginTextCommandSetBlipName('STRING')
  AddTextComponentString(label)
  EndTextCommandSetBlipName(blip)
  blips[#blips + 1] = blip
end

local function points(value)
  return value.x and { value } or value
end

local function refreshBlips()
  for _, blip in ipairs(blips) do
    if DoesBlipExist(blip) then RemoveBlip(blip) end
  end
  blips = {}
  for key, place in pairs(RPConfig.Places) do
    if place.blip and RPLocations[key] then
      for _, point in ipairs(points(RPLocations[key])) do addBlip(point, place.blip, place.colour) end
    end
  end
end

-- Places other resources added on the server (AddPlace), sent whole each
-- time one is added or removed.
local builtin = { places = {}, locations = {} }
for key, place in pairs(RPConfig.Places) do builtin.places[key] = place end
for key, value in pairs(RPLocations) do builtin.locations[key] = value end
local added = {}
RegisterNetEvent('sare_rp:client:places', function(list)
  if type(list) ~= 'table' then return end
  for key in pairs(added) do
    RPConfig.Places[key], RPLocations[key] = builtin.places[key], builtin.locations[key]
  end
  added = {}
  for _, entry in ipairs(list) do
    local spots = type(entry) == 'table' and type(entry.key) == 'string' and RPPlaces.points(entry.points)
    if spots and type(entry.place) == 'table' and type(entry.place.label) == 'string' then
      local place = entry.place
      RPConfig.Places[entry.key] = {
        label = place.label, blip = type(place.blip) == 'string' and place.blip or nil,
        colour = tonumber(place.colour), radius = tonumber(place.radius),
      }
      RPLocations[entry.key] = #spots == 1 and spots[1] or spots
      added[entry.key] = true
    end
  end
  refreshBlips()
end)

CreateThread(function()
  refreshBlips()
  TriggerServerEvent('sare_rp:server:places')
  while true do
    Wait(250)
    local here = GetEntityCoords(PlayerPedId())
    local label = nil
    for key, place in pairs(RPConfig.Places) do
      local radius = place.radius or RPConfig.InteractRadius
      for _, point in ipairs(RPLocations[key] and points(RPLocations[key]) or {}) do
        local dx, dy = here.x - point.x, here.y - point.y
        if dx * dx + dy * dy <= radius * radius and math.abs(here.z - point.z) < 3.0 then label = place.label end
      end
    end
    if label ~= nearLabel then
      nearLabel = label
      if label then UI.showTextUI(('[%s] %s'):format(RPConfig.InteractKey, label)) else UI.hideTextUI() end
    end
  end
end)

RegisterCommand('rp_interact', function()
  TriggerServerEvent('sare_rp:server:interact')
end)
RegisterKeyMapping('rp_interact', 'Use what is in front of you', 'keyboard', RPConfig.InteractKey)

-- 911 calls and service calls: a blip at the caller for two minutes.
RegisterNetEvent('sare_rp:client:dispatchBlip', function(data)
  if type(data) ~= 'table' or not tonumber(data.x) then return end
  local blip = AddBlipForCoord(data.x + 0.0, data.y + 0.0, (data.z or 0) + 0.0)
  SetBlipColour(blip, tonumber(data.colour) or 1)
  BeginTextCommandSetBlipName('STRING')
  AddTextComponentString(tostring(data.label or 'Call'))
  EndTextCommandSetBlipName(blip)
  SetTimeout(120000, function()
    if DoesBlipExist(blip) then RemoveBlip(blip) end
  end)
end)
