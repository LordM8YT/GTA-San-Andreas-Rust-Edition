local Core = SareBox

exports('GetCoreObject', function() return Core end)
exports('GetPlayer', Core.Functions.GetPlayer)
exports('GetPlayerByCitizenId', Core.Functions.GetPlayerByCitizenId)
exports('GetPlayers', Core.Functions.GetPlayers)
exports('GetJob', Core.Functions.GetJob)
exports('AddJob', Core.Functions.AddJob)
exports('AddJobs', Core.Functions.AddJobs)

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

local function loadPlayer(src)
  local player = Core.Functions.Login(src)
  local data = player.PlayerData
  if data.position then
    SetEntityCoords(GetPlayerPed(src), data.position.x, data.position.y, data.position.z)
    SetEntityHeading(GetPlayerPed(src), data.position.heading or 0.0)
  else
    exports.spawnmanager:spawnPlayer(src)
  end
  if data.pendingMigration then
    Core.Functions.Notify(src, ('Your character from before the update (%s) waits for an admin to confirm it is yours. You play as a guest until then; nothing is saved.')
      :format(data.pendingMigration), 'warning')
  elseif not data.license then
    Core.Functions.Notify(src, 'Your game did not send a player identity, so this character is not saved. Update the game.', 'warning')
  end
  TriggerEvent('sarebox:server:playerLoaded', player)
  TriggerClientEvent('sarebox:client:playerLoaded', src, data)
end

AddEventHandler('playerJoining', function()
  loadPlayer(source)
end)

-- Approve or reject a claim on a character from before signed identities.
RegisterCommand('sarebox_migrate', function(src, args)
  local function say(text)
    if src == 0 then print(text) else Core.Functions.Notify(src, text, 'inform', 'Admin') end
  end
  local target = tonumber(args[1])
  local claim = target and Core.Pending[target]
  local replace = false
  if not claim and target and args[2] ~= 'reject' then
    -- A player who already got a new character (for example while the
    -- server ran new binaries with old SARE Box files) can still get the
    -- old one back; the new one is unlinked, not deleted.
    local license = GetPlayerIdentifierByType(target, 'license')
    local old, citizenid = Core.Functions.LegacyClaim(target)
    if license and old then
      claim, replace = { license = license, legacy = old, citizenid = citizenid }, true
    end
  end
  if not claim then
    say('Usage: sarebox_migrate <server id> [reject]. That player claims no old character.')
    return
  end
  print(('[admin] %s: sarebox_migrate %d %s (character %s)'):format(src == 0 and 'console' or GetPlayerName(src) .. ' (' .. src .. ')',
    target, args[2] or 'approve', claim.citizenid))
  Core.Pending[target] = nil
  if args[2] == 'reject' then
    Core.Functions.NewCharacter(target, claim.license)
    Core.Functions.Notify(target, 'An admin did not confirm the old character. You start a new one.', 'warning')
    say(('Rejected; %s gets a new character.'):format(GetPlayerName(target)))
  elseif Core.Functions.MigrateCharacter(claim.license, claim.legacy, claim.citizenid, replace) then
    Core.Functions.Notify(target, 'Your old character is back.', 'success')
    say(('Moved character %s to %s.'):format(claim.citizenid, GetPlayerName(target)))
  else
    say('The old character was already moved, or this player already has a character.')
    return
  end
  Core.Functions.Logout(target)
  loadPlayer(target)
end, true)

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
