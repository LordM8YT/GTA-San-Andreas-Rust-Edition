-- Jobs for SARE RP Town. Everything is decided on the server: who is on
-- duty, who is close enough, how much a fine or bill is, and where a
-- prisoner may stand.
local Core = exports.sarebox:GetCoreObject()
local RP = exports.sare_rp_core:GetRP()
local Inventory = exports.sare_rp_inventory

local function notify(src, text, kind, title) RP.Notify(src, text, kind, title or 'Jobs') end
local function job(src)
  local player = Core.Functions.GetPlayer(src)
  return player and player.PlayerData.job
end
local function label(name)
  local data = Core.Functions.GetJob(name)
  return data and data.label or name
end

-- The nearest other player within `radius` metres, in the same instance.
local function nearest(src, radius)
  local here, best, id = RP.Coords(src), nil, nil
  for _, other in ipairs(RP.PlayersNear(src, radius)) do
    local d = RP.Distance(here, RP.Coords(other))
    if other ~= src and Core.Functions.GetPlayer(other) and (not best or d < best) then best, id = d, other end
  end
  return id
end

local function close(src, target, radius)
  return target and Core.Functions.GetPlayer(target) and GetPlayerRoutingBucket(src) == GetPlayerRoutingBucket(target)
    and RP.Distance(RP.Coords(src), RP.Coords(target)) <= radius
end

-- Dispatch ----------------------------------------------------------------
local Services = {
  police = { jobs = { 'police', 'ambulance' }, label = '911', colour = 1 },
  ems = { jobs = { 'ambulance' }, label = 'EMS', colour = 1 },
  taxi = { jobs = { 'taxi' }, label = 'Taxi', colour = 5 },
  mechanic = { jobs = { 'mechanic' }, label = 'Mechanic', colour = 5 },
}
local LastCall = {}
local function call(src, service, text)
  local info = Services[service]
  if not info then return end
  text = tostring(text or ''):gsub('[%c]', ' '):sub(1, 160)
  if text == '' then text = 'needs help' end
  local now = GetGameTimer()
  if LastCall[src] and now - LastCall[src] < 15000 then return notify(src, 'Wait a moment before calling again.', 'warning', 'Phone') end
  LastCall[src] = now
  local units = RP.OnDutyPlayers(info.jobs)
  if #units == 0 then return notify(src, ('Nobody is on duty for %s right now.'):format(info.label), 'error', 'Phone') end
  local at = RP.Coords(src)
  for _, unit in ipairs(units) do
    TriggerClientEvent('chat:addMessage', unit, { color = { 255, 90, 90 }, multiline = true,
      args = { ('[%s] %s (%d)'):format(info.label, RP.Name(src), src), text } })
    notify(unit, ('%s: %s'):format(RP.Name(src), text), 'warning', info.label .. ' call')
    TriggerClientEvent('sare_rp:client:dispatchBlip', unit, { x = at.x, y = at.y, z = at.z, colour = info.colour,
      label = ('%s: %s'):format(info.label, RP.Name(src)) })
  end
  notify(src, ('Your call reached %d unit(s).'):format(#units), 'success', 'Phone')
  print(('[sare_rp_jobs] %s call from %s (%d): %s'):format(info.label, RP.Name(src), src, text))
end
AddEventHandler('sare_rp_jobs:call', call)
AddEventHandler('playerDropped', function() LastCall[source] = nil end)

RegisterCommand('911', function(src, args)
  if src ~= 0 then call(src, 'police', table.concat(args, ' ')) end
end, false)
RegisterCommand('call', function(src, args)
  if src == 0 then return end
  local service = (args[1] or ''):lower()
  if not Services[service] or service == 'police' then
    return notify(src, 'Usage: /call <taxi|mechanic|ems> <message>, or /911 <message>', 'inform', 'Phone')
  end
  table.remove(args, 1)
  call(src, service, table.concat(args, ' '))
end, false)

-- Bills and fines -----------------------------------------------------------
local Bills, nextBill = {}, 0
local function bill(src, target, amount, reason)
  amount = RP.Amount(amount)
  if not amount or amount > 100000 then return notify(src, 'Use a whole amount from $1 to $100000.', 'error') end
  if target == src then return notify(src, 'You cannot bill yourself.', 'error') end
  if not close(src, target, 5.0) then return notify(src, 'Stand next to them (5 m).', 'error') end
  nextBill = nextBill + 1
  reason = tostring(reason or 'services'):gsub('[%c]', ' '):sub(1, 80)
  Bills[nextBill] = { from = src, to = target, amount = amount, reason = reason, job = job(src).name, expires = GetGameTimer() + 120000 }
  RP.Menu(target, ('Bill: $%d'):format(amount), {
    { title = ('%s (%s): %s'):format(RP.Name(src), label(job(src).name), reason), disabled = true },
    { title = 'Pay by card', serverEvent = 'sare_rp_jobs:server:payBill', args = { id = nextBill, account = 'bank' } },
    { title = 'Pay cash', serverEvent = 'sare_rp_jobs:server:payBill', args = { id = nextBill, account = 'cash' } },
    { title = 'Refuse', serverEvent = 'sare_rp_jobs:server:payBill', args = { id = nextBill, account = 'refuse' } },
  })
  notify(src, ('Sent a $%d bill to %s.'):format(amount, RP.Name(target)), 'success')
end

RegisterNetEvent('sare_rp_jobs:server:payBill', function(args)
  local src = source
  local entry = type(args) == 'table' and Bills[tonumber(args.id) or 0]
  if not entry or entry.to ~= src then return end
  Bills[tonumber(args.id)] = nil
  if GetGameTimer() > entry.expires then return notify(src, 'That bill expired.', 'error', 'Bill') end
  if args.account == 'refuse' then
    notify(entry.from, ('%s refused your bill.'):format(RP.Name(src)), 'warning', 'Bill')
    return notify(src, 'Bill refused.', 'inform', 'Bill')
  end
  local account = args.account == 'cash' and 'cash' or 'bank'
  local payer, biller = Core.Functions.GetPlayer(src), Core.Functions.GetPlayer(entry.from)
  if not payer.Functions.RemoveMoney(account, entry.amount, 'bill: ' .. entry.reason) then
    notify(entry.from, ('%s could not pay the bill.'):format(RP.Name(src)), 'warning', 'Bill')
    return notify(src, 'Not enough money.', 'error', 'Bill')
  end
  if biller then biller.Functions.AddMoney('bank', entry.amount, 'bill paid by ' .. payer.PlayerData.citizenid) end
  notify(src, ('Paid $%d.'):format(entry.amount), 'success', 'Bill')
  if biller then notify(entry.from, ('%s paid your $%d bill.'):format(RP.Name(src), entry.amount), 'success', 'Bill') end
end)

local function fine(src, target, amount, reason)
  amount = RP.Amount(amount)
  if not RP.OnDuty(src, 'police') then return end
  if not amount or amount > 25000 then return notify(src, 'Fines are $1 to $25000.', 'error', 'Police') end
  if not close(src, target, 5.0) then return notify(src, 'Stand next to them (5 m).', 'error', 'Police') end
  local player = Core.Functions.GetPlayer(target)
  reason = tostring(reason or 'traffic offence'):gsub('[%c]', ' '):sub(1, 80)
  if player.Functions.RemoveMoney('bank', amount, 'fine: ' .. reason) or player.Functions.RemoveMoney('cash', amount, 'fine: ' .. reason) then
    notify(target, ('You were fined $%d: %s'):format(amount, reason), 'warning', 'Police')
    notify(src, ('%s paid a $%d fine.'):format(RP.Name(target), amount), 'success', 'Police')
    RP.Me(src, ('writes %s a $%d ticket'):format(RP.Name(target), amount))
    print(('[sare_rp_jobs] fine %s -> %s: $%d (%s)'):format(RP.Name(src), RP.Name(target), amount, reason))
  else
    notify(src, ('%s cannot pay $%d.'):format(RP.Name(target), amount), 'error', 'Police')
  end
end

-- Jail ------------------------------------------------------------------------
local Jailed = {} -- [server id] = { ends = game timer, citizenid }

local function teleport(src, point)
  local ped = GetPlayerPed(src)
  SetEntityCoords(ped, point.x, point.y, point.z)
  SetEntityHeading(ped, point.heading or 0.0)
end

local function saveJail(src)
  local entry = Jailed[src]
  local player = Core.Functions.GetPlayer(src)
  if not player then return end
  local left = entry and math.max(0, math.ceil((entry.ends - GetGameTimer()) / 1000)) or 0
  player.Functions.SetMetaData('jail', left > 0 and left or nil)
end

local function jail(target, seconds)
  local player = Core.Functions.GetPlayer(target)
  if not player then return end
  Jailed[target] = { ends = GetGameTimer() + seconds * 1000, citizenid = player.PlayerData.citizenid }
  Player(target).state:set('cuffed', false, true)
  teleport(target, RPLocations.jail_cell)
  saveJail(target)
  notify(target, ('You are in jail for %d minute(s).'):format(math.ceil(seconds / 60)), 'warning', 'Jail')
end

local function release(target, text)
  if not Jailed[target] then return end
  Jailed[target] = nil
  saveJail(target)
  teleport(target, RPLocations.jail_release)
  notify(target, text or 'You are free to go.', 'success', 'Jail')
end

CreateThread(function()
  local tick = 0
  while true do
    Wait(1000)
    tick = tick + 1
    for src, entry in pairs(Jailed) do
      local player = Core.Functions.GetPlayer(src)
      if not player or player.PlayerData.citizenid ~= entry.citizenid then
        Jailed[src] = nil
      elseif GetGameTimer() >= entry.ends then
        release(src, 'You served your time.')
      elseif RP.Distance(RP.Coords(src), RPLocations.jail_cell) > RPConfig.Jail.radius then
        teleport(src, RPLocations.jail_cell)
        notify(src, 'You cannot leave the cell.', 'error', 'Jail')
      elseif tick % 15 == 0 then
        saveJail(src)
      end
    end
  end
end)

AddEventHandler('sarebox:server:playerLoaded', function(player)
  local src = player.PlayerData.source
  local left = tonumber(player.PlayerData.metadata.jail)
  if left and left > 0 then jail(src, math.min(left, RPConfig.Jail.maxMinutes * 60)) end
end)
AddEventHandler('playerDropped', function()
  local src = source
  if Jailed[src] then saveJail(src) end
  Jailed[src] = nil
end)
AddEventHandler('onResourceStop', function(resource)
  if resource ~= GetCurrentResourceName() then return end
  for src in pairs(Jailed) do saveJail(src) end
end)
AddEventHandler('onResourceStart', function(resource)
  if resource ~= GetCurrentResourceName() then return end
  for _, src in ipairs(Core.Functions.GetPlayers()) do
    local player = Core.Functions.GetPlayer(src)
    local left = tonumber(player.PlayerData.metadata.jail)
    if left and left > 0 then jail(src, left) end
  end
end)

-- Police actions --------------------------------------------------------------
local Police = {}
function Police.cuff(src, target)
  if not close(src, target, 3.0) then return notify(src, 'Nobody within 3 m.', 'error', 'Police') end
  local cuffed = not Player(target).state.cuffed
  if cuffed and Inventory:GetItemCount(src, 'handcuffs') < 1 then return notify(src, 'You need handcuffs from the lockers.', 'error', 'Police') end
  Player(target).state:set('cuffed', cuffed, true)
  RP.Me(src, cuffed and ('puts handcuffs on %s'):format(RP.Name(target)) or ('takes the handcuffs off %s'):format(RP.Name(target)))
  notify(target, cuffed and 'You are handcuffed. Shops, banks and lockers are closed to you.' or 'Your handcuffs are off.', 'warning', 'Police')
end
function Police.search(src, target)
  if not close(src, target, 3.0) then return notify(src, 'Nobody within 3 m.', 'error', 'Police') end
  local inv = Inventory:GetInventory(target) or {}
  local options = { { title = ('%s carries:'):format(RP.Name(target)), disabled = true } }
  for name, count in pairs(inv) do
    local item = RPConfig.Items[name]
    if item then
      options[#options + 1] = item.illegal
        and { title = ('%s x%d (illegal)'):format(item.label, count), description = 'Confiscate',
              serverEvent = 'sare_rp_jobs:server:confiscate', args = { target = target, item = name } }
        or { title = ('%s x%d'):format(item.label, count), disabled = true }
    end
  end
  if #options == 1 then options[2] = { title = 'Nothing', disabled = true } end
  RP.Me(src, ('searches %s'):format(RP.Name(target)))
  RP.Menu(src, 'Search', options)
end
RegisterNetEvent('sare_rp_jobs:server:confiscate', function(args)
  local src = source
  if type(args) ~= 'table' or not RP.OnDuty(src, 'police') then return end
  local target, item = tonumber(args.target), args.item
  if not close(src, target, 3.0) or not RPConfig.Items[item] or not RPConfig.Items[item].illegal then return end
  local count = Inventory:GetItemCount(target, item)
  if count > 0 and Inventory:RemoveItem(target, item, count) then
    RP.Me(src, ('confiscates %dx %s'):format(count, RPConfig.Items[item].label))
    notify(target, ('The police took your %s.'):format(RPConfig.Items[item].label), 'warning', 'Police')
  end
end)

-- EMS treatment: uses a medkit or a bandage and takes a few seconds.
local function treat(src, target)
  if not close(src, target, 3.0) then return notify(src, 'Nobody within 3 m.', 'error', 'EMS') end
  local supply = Inventory:GetItemCount(src, 'medkit') > 0 and 'medkit' or (Inventory:GetItemCount(src, 'bandage') > 0 and 'bandage')
  if not supply then return notify(src, 'You need a medkit or a bandage from the EMS lockers.', 'error', 'EMS') end
  RP.Me(src, ('starts treating %s'):format(RP.Name(target)))
  RP.Progress(src, 'Treating ' .. RP.Name(target), 6000, function(done)
    if not done then return notify(src, 'Treatment stopped.', 'warning', 'EMS') end
    if not close(src, target, 4.0) or not Inventory:RemoveItem(src, supply, 1) then return end
    RP.Me(src, ('finishes treating %s'):format(RP.Name(target)))
    notify(target, ('%s treated your injuries.'):format(RP.Name(src)), 'success', 'EMS')
  end)
end

-- The F2 job menu --------------------------------------------------------------
local function ask(src, heading, rows, fn)
  RP.Dialog(src, heading, rows, function(values) if values then fn(values) end end)
end

RegisterNetEvent('sare_rp_jobs:server:menu', function()
  local src = source
  local data = job(src)
  if not data then return end
  local options = { { title = ('%s - %s%s'):format(data.label, data.grade.name, data.onduty and '' or ' (off duty)'), disabled = true } }
  local add = function(title, action, description)
    options[#options + 1] = { title = title, description = description, serverEvent = 'sare_rp_jobs:server:action', args = action }
  end
  if data.onduty and data.name == 'police' then
    add('Cuff / uncuff nearest person', 'cuff')
    add('Search nearest person', 'search')
    add('Fine nearest person', 'fine')
    add('Jail nearest person', 'jail', 'They are taken to the cell at the station')
    add('Release a prisoner', 'unjail')
  elseif data.onduty and data.name == 'ambulance' then
    add('Treat nearest person', 'treat', 'Uses a medkit or a bandage')
  end
  if data.onduty and (data.name == 'ambulance' or data.name == 'mechanic' or data.name == 'taxi') then
    add('Bill nearest person', 'bill')
  end
  if #options == 1 then options[2] = { title = 'Nothing for your job here. Go on duty at your workplace.', disabled = true } end
  RP.Menu(src, 'Job', options)
end)

RegisterNetEvent('sare_rp_jobs:server:action', function(action)
  local src = source
  local data = job(src)
  if not data or not data.onduty then return end
  local police, target = data.name == 'police', nearest(src, 3.0)
  if action == 'cuff' and police then return Police.cuff(src, target) end
  if action == 'search' and police then return Police.search(src, target) end
  if action == 'treat' and data.name == 'ambulance' then return treat(src, target) end
  if action == 'unjail' and police then
    return ask(src, 'Release a prisoner', { { type = 'number', label = 'Server ID', required = true } }, function(v)
      local id = tonumber(v[1])
      if not Jailed[id] then return notify(src, 'That person is not in jail.', 'error', 'Police') end
      release(id, ('%s released you.'):format(RP.Name(src)))
      notify(src, 'Released.', 'success', 'Police')
    end)
  end
  if not target then return notify(src, 'Nobody within 3 m.', 'error') end
  if action == 'fine' and police then
    return ask(src, 'Fine ' .. RP.Name(target), {
      { type = 'number', label = 'Amount ($)', required = true },
      { type = 'input', label = 'Reason', required = true },
    }, function(v) fine(src, target, v[1], v[2]) end)
  end
  if action == 'jail' and police then
    return ask(src, 'Jail ' .. RP.Name(target), {
      { type = 'number', label = ('Minutes (1-%d)'):format(RPConfig.Jail.maxMinutes), required = true },
    }, function(v)
      local minutes = tonumber(v[1])
      if not minutes or minutes < 1 or minutes > RPConfig.Jail.maxMinutes or minutes ~= math.floor(minutes) then
        return notify(src, ('Use 1 to %d minutes.'):format(RPConfig.Jail.maxMinutes), 'error', 'Police')
      end
      if not close(src, target, 5.0) then return notify(src, 'They are not with you any more.', 'error', 'Police') end
      jail(target, minutes * 60)
      RP.Me(src, ('books %s for %d minute(s)'):format(RP.Name(target), minutes))
      print(('[sare_rp_jobs] %s jailed %s for %d min'):format(RP.Name(src), RP.Name(target), minutes))
    end)
  end
  if action == 'bill' and (data.name == 'ambulance' or data.name == 'mechanic' or data.name == 'taxi') then
    return ask(src, 'Bill ' .. RP.Name(target), {
      { type = 'number', label = 'Amount ($)', required = true },
      { type = 'input', label = 'What for', required = true },
    }, function(v) bill(src, target, v[1], v[2]) end)
  end
end)

-- Chat commands for the same actions, with a server ID.
local function idCommand(name, jobName, fn)
  RegisterCommand(name, function(src, args)
    if src == 0 or not RP.OnDuty(src, jobName) then return notify(src, ('Only on-duty %s can do that.'):format(label(jobName)), 'error') end
    local target = tonumber(args[1])
    if not target or target == src then return notify(src, ('Usage: /%s <server id> ...'):format(name), 'inform') end
    fn(src, target, args)
  end, false)
end
idCommand('cuff', 'police', function(src, target) Police.cuff(src, target) end)
idCommand('search', 'police', function(src, target) Police.search(src, target) end)
idCommand('fine', 'police', function(src, target, args) fine(src, target, args[2], table.concat(args, ' ', 3)) end)
idCommand('jail', 'police', function(src, target, args)
  local minutes = tonumber(args[2])
  if not minutes or minutes < 1 or minutes > RPConfig.Jail.maxMinutes or minutes ~= math.floor(minutes) then
    return notify(src, ('Usage: /jail <id> <1-%d minutes>'):format(RPConfig.Jail.maxMinutes), 'inform', 'Police')
  end
  if not close(src, target, 5.0) then return notify(src, 'Stand next to them (5 m).', 'error', 'Police') end
  jail(target, minutes * 60)
  RP.Me(src, ('books %s for %d minute(s)'):format(RP.Name(target), minutes))
end)
idCommand('unjail', 'police', function(src, target) release(target, ('%s released you.'):format(RP.Name(src))) end)
idCommand('treat', 'ambulance', function(src, target) treat(src, target) end)
RegisterCommand('bill', function(src, args)
  local data = src ~= 0 and job(src)
  if not data or not data.onduty or not (data.name == 'ambulance' or data.name == 'mechanic' or data.name == 'taxi') then
    return notify(src, 'Only on-duty EMS, mechanics and taxi drivers send bills.', 'error')
  end
  bill(src, tonumber(args[1]), args[2], table.concat(args, ' ', 3))
end, false)
-- Admins (ACL command.adminunjail) can free anyone.
RegisterCommand('adminunjail', function(src, args)
  local target = tonumber(args[1])
  if target and Jailed[target] then release(target, 'An admin released you.') end
end, true)

-- Places --------------------------------------------------------------------
local GearTaken = {}
local function gear(src, jobName)
  local player = Core.Functions.GetPlayer(src)
  local key = player.PlayerData.citizenid
  local now = GetGameTimer()
  if GearTaken[key] and now - GearTaken[key] < 600000 then return notify(src, 'You already took your gear. Come back later.', 'warning') end
  local got = {}
  for item, count in pairs(RPConfig.JobGear[jobName] or {}) do
    local missing = count - Inventory:GetItemCount(src, item)
    if missing > 0 and Inventory:AddItem(src, item, missing) then got[#got + 1] = ('%dx %s'):format(missing, RPConfig.Items[item].label) end
  end
  GearTaken[key] = now
  notify(src, #got > 0 and ('Took ' .. table.concat(got, ', ')) or 'You already carry your gear.', 'success')
end

local function dutyMenu(src, jobName, extra)
  local data = job(src)
  if data.name ~= jobName then
    return notify(src, ('This is for %s. Ask at City Hall for work.'):format(label(jobName)), 'error')
  end
  local options = {
    { title = data.onduty and 'Go off duty' or 'Go on duty', serverEvent = 'sare_rp_jobs:server:duty', args = jobName },
  }
  for _, option in ipairs(extra or {}) do options[#options + 1] = option end
  RP.Menu(src, label(jobName), options)
end

RegisterNetEvent('sare_rp_jobs:server:duty', function(jobName)
  local src = source
  local data = job(src)
  local point = ({ police = 'police_duty', ambulance = 'hospital_duty', mechanic = 'mechanic_duty', taxi = 'taxi_depot' })[jobName]
  if not data or data.name ~= jobName or not point or not RP.Near(src, point) then return end
  local player = Core.Functions.GetPlayer(src)
  player.Functions.SetJobDuty(not data.onduty)
  notify(src, data.onduty and 'You are off duty.' or 'You are on duty. Press F2 for your job menu.', 'success', label(jobName))
end)

RegisterNetEvent('sare_rp_jobs:server:gear', function(jobName)
  local src = source
  local point = ({ police = 'police_armory', ambulance = 'hospital_duty', mechanic = 'mechanic_duty' })[jobName]
  if point and RP.OnDuty(src, jobName) and RP.Near(src, point) then gear(src, jobName) end
end)

RegisterNetEvent('sare_rp_jobs:server:vehicle', function(jobName)
  local src = source
  local point = ({ police = 'police_garage', ambulance = 'hospital_garage', mechanic = 'mechanic', taxi = 'taxi_depot' })[jobName]
  if not point or not RP.OnDuty(src, jobName) or not RP.Near(src, point) then return end
  local spot = ({ police = RPLocations.police_garage, ambulance = RPLocations.hospital_garage,
    mechanic = RPLocations.dealer_delivery, taxi = RPLocations.taxi_depot })[jobName]
  if exports.sare_rp_vehicles:SpawnJobVehicle(src, jobName, spot) then
    notify(src, 'Your job vehicle is ready (pick again to return it).', 'success')
  end
end)

RegisterNetEvent('sare_rp_jobs:server:hire', function(name)
  local src = source
  if not RP.Near(src, 'city_hall') then return end
  local open = false
  for _, n in ipairs(RPConfig.OpenJobs) do open = open or n == name end
  if not open then return notify(src, 'That job is hired by the boss (an admin).', 'error', 'City Hall') end
  local player = Core.Functions.GetPlayer(src)
  if player.PlayerData.job.name == name then return notify(src, 'You already have that job.', 'inform', 'City Hall') end
  if player.Functions.SetJob(name, 0) then notify(src, ('You work as %s now.'):format(label(name)), 'success', 'City Hall') end
end)

RegisterNetEvent('sare_rp_jobs:server:cityhall', function(what)
  local src = source
  if not RP.Near(src, 'city_hall') then return end
  if what == 'identity' then TriggerEvent('sare_rp_identity:change', src) end
end)

RegisterNetEvent('sare_rp_jobs:server:hospital', function()
  local src = source
  if not RP.Near(src, 'hospital_checkin') then return end
  local medics = RP.OnDutyPlayers({ 'ambulance' })
  if #medics > 0 then return call(src, 'ems', 'is waiting at the hospital reception') end
  local player = Core.Functions.GetPlayer(src)
  local price = RPConfig.Prices.hospitalTreatment
  if not (player.Functions.RemoveMoney('bank', price, 'hospital') or player.Functions.RemoveMoney('cash', price, 'hospital')) then
    return notify(src, ('Treatment costs $%d.'):format(price), 'error', 'Hospital')
  end
  RP.Progress(src, 'The doctor treats you', 8000, function()
    notify(src, 'The doctor patched you up.', 'success', 'Hospital')
  end)
end)

AddEventHandler('sare_rp:interact', function(src, key)
  if key == 'city_hall' then
    local options = { { title = 'Job centre', disabled = true } }
    for _, name in ipairs(RPConfig.OpenJobs) do
      options[#options + 1] = { title = 'Work as ' .. label(name), serverEvent = 'sare_rp_jobs:server:hire', args = name }
    end
    options[#options + 1] = { title = 'Police and EMS', description = 'Hired by an admin: /setjob <id> police 0', disabled = true }
    options[#options + 1] = { title = ('New ID ($%d)'):format(RPConfig.Prices.identityChange), description = 'Change your name and date of birth',
      serverEvent = 'sare_rp_jobs:server:cityhall', args = 'identity' }
    RP.Menu(src, 'City Hall', options)
  elseif key == 'police_duty' then
    if job(src).name ~= 'police' then
      return RP.Menu(src, 'Police front desk', {
        { title = 'Report a crime', description = 'Officers on duty get your report', serverEvent = 'sare_rp_jobs:server:report' },
      })
    end
    dutyMenu(src, 'police')
  elseif key == 'police_armory' then
    gear(src, 'police')
  elseif key == 'police_garage' or key == 'hospital_garage' then
    local name = key == 'police_garage' and 'police' or 'ambulance'
    RP.Menu(src, label(name) .. ' vehicles', {
      { title = 'Take / return a job vehicle', serverEvent = 'sare_rp_jobs:server:vehicle', args = name },
    })
  elseif key == 'hospital_checkin' then
    RP.Menu(src, 'Hospital reception', {
      { title = 'Get treated', description = ('An EMS on duty is called; otherwise the doctor ($%d)'):format(RPConfig.Prices.hospitalTreatment),
        serverEvent = 'sare_rp_jobs:server:hospital' },
    })
  elseif key == 'hospital_duty' then
    dutyMenu(src, 'ambulance', { { title = 'Take EMS gear', serverEvent = 'sare_rp_jobs:server:gear', args = 'ambulance' } })
  elseif key == 'mechanic_duty' then
    dutyMenu(src, 'mechanic', { { title = 'Take repair kits', serverEvent = 'sare_rp_jobs:server:gear', args = 'mechanic' } })
  elseif key == 'mechanic' then
    if RP.OnDuty(src, 'mechanic') then
      RP.Menu(src, 'Workshop', { { title = 'Take / return the tow car', serverEvent = 'sare_rp_jobs:server:vehicle', args = 'mechanic' } })
    else
      RP.Menu(src, 'Workshop', { { title = 'Ask for a mechanic', serverEvent = 'sare_rp_jobs:server:service', args = 'mechanic' } })
    end
  elseif key == 'taxi_depot' then
    if job(src).name ~= 'taxi' then return notify(src, 'Become a taxi driver at City Hall.', 'inform', 'Taxi') end
    dutyMenu(src, 'taxi', { { title = 'Take / return a taxi', serverEvent = 'sare_rp_jobs:server:vehicle', args = 'taxi' } })
  end
end)

RegisterNetEvent('sare_rp_jobs:server:report', function()
  local src = source
  if not RP.Near(src, 'police_duty') then return end
  ask(src, 'Report a crime', { { type = 'input', label = 'What happened?', required = true } }, function(v)
    call(src, 'police', 'at the front desk: ' .. tostring(v[1]))
  end)
end)
RegisterNetEvent('sare_rp_jobs:server:service', function(service)
  local src = source
  if service == 'mechanic' and RP.Near(src, 'mechanic') then call(src, 'mechanic', 'is waiting at the workshop') end
end)

exports('IsJailed', function(src) return Jailed[src] ~= nil end)
