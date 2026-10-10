local Core = SareBox

exports('GetCoreObject', function() return Core end)
exports('GetPlayer', Core.Functions.GetPlayer)
exports('GetPlayerByCitizenId', Core.Functions.GetPlayerByCitizenId)
exports('GetPlayers', Core.Functions.GetPlayers)

-- Spawning: saved characters return to where they left, new ones use the
-- map's spawn points.
AddEventHandler('onResourceStart', function(resource)
  if resource ~= GetCurrentResourceName() then return end
  exports.spawnmanager:setAutoSpawn(false)
  -- A restart logs everyone in again.
  for _, src in ipairs(GetPlayers()) do Core.Functions.Login(tonumber(src)) end
end)

AddEventHandler('onResourceStop', function(resource)
  if resource ~= GetCurrentResourceName() then return end
  for _, src in ipairs(Core.Functions.GetPlayers()) do Core.Functions.Logout(src) end
  -- On server shutdown spawnmanager may already be stopped.
  if GetResourceState('spawnmanager') == 'started' then exports.spawnmanager:setAutoSpawn(true) end
end)

AddEventHandler('playerJoining', function()
  local src = source
  local player = Core.Functions.Login(src)
  local data = player.PlayerData
  if data.position then
    SetEntityCoords(GetPlayerPed(src), data.position.x, data.position.y, data.position.z)
    SetEntityHeading(GetPlayerPed(src), data.position.heading or 0.0)
  else
    exports.spawnmanager:spawnPlayer(src)
  end
  if not data.license then
    Core.Functions.Notify(src, 'Your game did not send a player identity, so this character is not saved.', 'warning')
  end
  TriggerEvent('sarebox:server:playerLoaded', player)
  TriggerClientEvent('sarebox:client:playerLoaded', src, data)
end)

AddEventHandler('playerDropped', function()
  Core.Functions.Logout(source)
end)

-- The game asks for its data after its client script starts (also after a
-- resource restart, when playerJoining does not run again).
RegisterNetEvent('sarebox:server:requestPlayerData', function()
  local player = Core.Functions.GetPlayer(source)
  if player then TriggerClientEvent('sarebox:client:playerLoaded', source, player.PlayerData) end
end)

RegisterNetEvent('sarebox:server:toggleDuty', function()
  local player = Core.Functions.GetPlayer(source)
  if not player then return end
  player.Functions.SetJobDuty(not player.PlayerData.job.onduty)
  Core.Functions.Notify(source, player.PlayerData.job.onduty and 'You are on duty.' or 'You are off duty.')
end)

-- Paychecks and periodic saves.
CreateThread(function()
  local minutes = 0
  while true do
    Wait(60000)
    minutes = minutes + 1
    for _, src in ipairs(Core.Functions.GetPlayers()) do
      local player = Core.Functions.GetPlayer(src)
      local job = player.PlayerData.job
      if minutes % Config.PaycheckMinutes == 0 and job.onduty and job.payment > 0 then
        player.Functions.AddMoney('bank', job.payment, 'paycheck')
        Core.Functions.Notify(src, ('Paycheck: $%d'):format(job.payment), 'success', job.label)
      else
        player.Functions.Save()
      end
    end
  end
end)

RegisterCommand('id', function(src)
  if src == 0 then return end
  local license = GetPlayerIdentifierByType(src, 'license') or 'none'
  TriggerClientEvent('chat:addMessage', src, { color = { 120, 200, 255 }, args = { 'SARE Box', ('Server ID %d, %s'):format(src, license) } })
end, false)
