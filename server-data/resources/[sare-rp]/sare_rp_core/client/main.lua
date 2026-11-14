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

CreateThread(function()
  for key, place in pairs(RPConfig.Places) do
    if place.blip and RPLocations[key] then
      for _, point in ipairs(points(RPLocations[key])) do addBlip(point, place.blip, place.colour) end
    end
  end
  local radius = RPConfig.InteractRadius
  while true do
    Wait(250)
    local here = GetEntityCoords(PlayerPedId())
    local label = nil
    for key, place in pairs(RPConfig.Places) do
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
