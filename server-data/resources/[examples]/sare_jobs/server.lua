-- Uses the core only through `sare-core`, the name sare_core provides.
local Core = exports['sare-core']:GetCoreObject()
local pay = { taxi = 120, mechanic = 150 }

local function notify(src, message)
  TriggerClientEvent('chat:addMessage', src, { color = { 255, 200, 80 }, args = { 'Jobs', message } })
end

RegisterCommand('job', function(src, args)
  local player = Core.Functions.GetPlayer(src)
  if not player then return end
  local job = args[1]
  if not pay[job] then
    notify(src, 'Jobs: taxi, mechanic. Example: /job taxi')
    return
  end
  player.Functions.SetJob(job)
end, false)

RegisterCommand('work', function(src)
  local player = Core.Functions.GetPlayer(src)
  if not player then return end
  local job = Player(src).state.job
  if not pay[job] then
    notify(src, 'Pick a job first: /job taxi')
    return
  end
  player.Functions.AddMoney('cash', pay[job])
  local money = lib.callback.await('sare_core:getMoney', src)
  notify(src, ('You earned $%d as %s. Cash: $%d'):format(pay[job], job, money.cash))
end, false)

-- Runs for every job change, whichever resource made it.
AddStateBagChangeHandler('job', nil, function(bag, _key, value)
  local src = GetPlayerFromStateBagName(bag)
  if src ~= 0 and value ~= 'unemployed' then
    notify(src, ('You now work as %s. Use /work to earn money.'):format(value))
  end
end)
