-- The player's own data, a small HUD and the F1 player menu.
local PlayerData = nil
local hud = true

local function money(n) return ('$%d'):format(math.floor(n or 0)) end

local function drawHud()
  if not hud or not PlayerData then
    UI.hideTextUI()
    return
  end
  local job = PlayerData.job
  UI.showTextUI(('%s  |  Bank %s\n%s - %s%s'):format(
    money(PlayerData.money.cash), money(PlayerData.money.bank),
    job.label, job.grade.name, job.onduty and '' or ' (off duty)'))
end

RegisterNetEvent('sarebox:client:playerLoaded', function(data)
  PlayerData = data
  drawHud()
  UI.notify({ title = 'SARE Box', description = ('Welcome, %s'):format(data.name), type = 'success' })
end)

RegisterNetEvent('sarebox:client:setPlayerData', function(data)
  PlayerData = data
  drawHud()
end)

RegisterNetEvent('sarebox:client:moneyChanged', function(account, amount)
  UI.notify({
    title = account == 'bank' and 'Bank' or 'Cash',
    description = (amount >= 0 and '+' or '-') .. money(math.abs(amount)),
    type = amount >= 0 and 'success' or 'warning',
    duration = 2500,
  })
end)

exports('GetPlayerData', function() return PlayerData end)

RegisterCommand('hud', function()
  hud = not hud
  drawHud()
end)

RegisterCommand('playermenu', function()
  if not PlayerData then return end
  local job = PlayerData.job
  UI.registerContext({
    id = 'sarebox_player',
    title = PlayerData.name,
    options = {
      { title = 'Cash ' .. money(PlayerData.money.cash), description = 'Bank ' .. money(PlayerData.money.bank), disabled = true },
      { title = ('%s - %s'):format(job.label, job.grade.name), description = ('Citizen ID %s'):format(PlayerData.citizenid), disabled = true },
      { title = job.onduty and 'Go off duty' or 'Go on duty', serverEvent = 'sarebox:server:toggleDuty' },
      { title = hud and 'Hide HUD' or 'Show HUD', onSelect = function() ExecuteCommand('hud') end },
    },
  })
  UI.showContext('sarebox_player')
end)
RegisterKeyMapping('playermenu', 'Player menu', 'keyboard', 'F1')

AddEventHandler('onClientResourceStart', function(resource)
  if resource == GetCurrentResourceName() then TriggerServerEvent('sarebox:server:requestPlayerData') end
end)
