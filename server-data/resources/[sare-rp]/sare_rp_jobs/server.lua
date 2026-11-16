-- SARE RP job tools, shared by every job resource: the City Hall job
-- centre, phone dispatch (/911, /call), bills (/bill), duty and gear
-- lockers, and the F2 job menu. The jobs themselves (police, EMS,
-- mechanic, taxi, courier) are their own resources with their own
-- config.lua; they plug in here through exports:
--
--   local Jobs = exports.sare_rp_jobs
--   Jobs:AddService('taxi', { label = 'Taxi', colour = 5, jobs = { 'taxi' } })  -- /call taxi
--   Jobs:AllowBills('taxi')                                                     -- F2 "Bill" and /bill
--   Jobs:AddJobAction('taxi', { id = 'meter', title = 'Start the meter', target = true,
--     run = function(src, target) ... end })                                    -- F2 menu entry
--   Jobs:DutyMenu(src, 'taxi', 'taxi_depot', { extra menu options })            -- on/off duty menu
--   Jobs:Gear(src, { radio = 1 })                                               -- locker gear
--
-- Everything added from a resource goes away when that resource stops.
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
local function owner() return GetInvokingResource() or GetCurrentResourceName() end
local function callable(value)
  return type(value) == 'function' or (type(value) == 'table' and rawget(value, '__cfx_functionReference') ~= nil)
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
-- `target` is a player in the same instance within `radius` metres of `src`.
local function close(src, target, radius)
  return target and Core.Functions.GetPlayer(target) and GetPlayerRoutingBucket(src) == GetPlayerRoutingBucket(target)
    and RP.Distance(RP.Coords(src), RP.Coords(target)) <= radius
end
exports('Nearest', nearest)
exports('Close', function(src, target, radius) return close(src, target, radius) == true end)

-- Dispatch ----------------------------------------------------------------
-- Service name -> { label, colour, jobs = { { job, owner } } }.
local Services = {}
exports('AddService', function(name, info)
  if type(name) ~= 'string' or not name:match('^[%w_%-]+$') or type(info) ~= 'table' then return false, 'AddService(name, { label, colour, jobs })' end
  local service = Services[name] or { label = name, colour = 1, jobs = {} }
  if type(info.label) == 'string' then service.label = info.label end
  if tonumber(info.colour) then service.colour = tonumber(info.colour) end
  for _, jobName in ipairs(type(info.jobs) == 'table' and info.jobs or {}) do
    local known = false
    for _, entry in ipairs(service.jobs) do known = known or entry.job == jobName end
    if not known then service.jobs[#service.jobs + 1] = { job = jobName, owner = owner() } end
  end
  Services[name] = service
  return true
end)

local LastCall = {}
local function call(src, name, text)
  local info = Services[name]
  if not info then return notify(src, 'Nobody answers that number.', 'error', 'Phone') end
  text = tostring(text or ''):gsub('[%c]', ' '):sub(1, 160)
  if text == '' then text = 'needs help' end
  local now = GetGameTimer()
  if LastCall[src] and now - LastCall[src] < 15000 then return notify(src, 'Wait a moment before calling again.', 'warning', 'Phone') end
  LastCall[src] = now
  local jobs = {}
  for _, entry in ipairs(info.jobs) do jobs[#jobs + 1] = entry.job end
  local units = RP.OnDutyPlayers(jobs)
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
exports('Call', call)
AddEventHandler('playerDropped', function() LastCall[source] = nil end)

RegisterCommand('911', function(src, args)
  if src ~= 0 then call(src, 'police', table.concat(args, ' ')) end
end, false)
RegisterCommand('call', function(src, args)
  if src == 0 then return end
  local name = (args[1] or ''):lower()
  if not Services[name] or name == 'police' then
    local names = {}
    for key in pairs(Services) do
      if key ~= 'police' then names[#names + 1] = key end
    end
    table.sort(names)
    return notify(src, ('Usage: /call <%s> <message>, or /911 <message>'):format(table.concat(names, '|')), 'inform', 'Phone')
  end
  table.remove(args, 1)
  call(src, name, table.concat(args, ' '))
end, false)

-- Bills -----------------------------------------------------------------------
local Billers = {} -- job -> resource that allowed it
exports('AllowBills', function(jobName)
  if type(jobName) ~= 'string' then return false end
  Billers[jobName] = owner()
  return true
end)
local function canBill(data) return data and data.onduty and Billers[data.name] ~= nil end

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

RegisterCommand('bill', function(src, args)
  if src == 0 then return end
  if not canBill(job(src)) then return notify(src, 'Your job does not send bills, or you are off duty.', 'error') end
  bill(src, tonumber(args[1]), args[2], table.concat(args, ' ', 3))
end, false)

-- The F2 job menu ---------------------------------------------------------------
local Actions = {} -- ordered { job, id, title, description, target, run, owner }
exports('AddJobAction', function(jobName, action)
  if type(jobName) ~= 'string' or type(action) ~= 'table' or type(action.id) ~= 'string'
    or type(action.title) ~= 'string' or not callable(action.run) then
    return false, 'AddJobAction(job, { id, title, run = function(src, target) end })'
  end
  for i = #Actions, 1, -1 do
    if Actions[i].job == jobName and Actions[i].id == action.id then table.remove(Actions, i) end
  end
  Actions[#Actions + 1] = { job = jobName, id = action.id, title = action.title, description = action.description,
    target = action.target == true, run = action.run, owner = owner() }
  return true
end)

local function ask(src, heading, rows, fn)
  RP.Dialog(src, heading, rows, function(values) if values then fn(values) end end)
end

RegisterNetEvent('sare_rp_jobs:server:menu', function()
  local src = source
  local data = job(src)
  if not data then return end
  local options = { { title = ('%s - %s%s'):format(data.label, data.grade.name, data.onduty and '' or ' (off duty)'), disabled = true } }
  if data.onduty then
    for _, action in ipairs(Actions) do
      if action.job == data.name then
        options[#options + 1] = { title = action.title, description = action.description,
          serverEvent = 'sare_rp_jobs:server:action', args = { job = action.job, id = action.id } }
      end
    end
    if canBill(data) then
      options[#options + 1] = { title = 'Bill nearest person', serverEvent = 'sare_rp_jobs:server:action', args = { job = data.name, id = 'bill' } }
    end
  end
  if #options == 1 then options[2] = { title = 'Nothing for your job here. Go on duty at your workplace.', disabled = true } end
  RP.Menu(src, 'Job', options)
end)

RegisterNetEvent('sare_rp_jobs:server:action', function(args)
  local src = source
  local data = job(src)
  if type(args) ~= 'table' or not data or not data.onduty or data.name ~= args.job then return end
  if args.id == 'bill' and canBill(data) then
    local target = nearest(src, 3.0)
    if not target then return notify(src, 'Nobody within 3 m.', 'error') end
    return ask(src, 'Bill ' .. RP.Name(target), {
      { type = 'number', label = 'Amount ($)', required = true },
      { type = 'input', label = 'What for', required = true },
    }, function(v) bill(src, target, v[1], v[2]) end)
  end
  for _, action in ipairs(Actions) do
    if action.job == data.name and action.id == args.id then
      local target = nil
      if action.target then
        target = nearest(src, 3.0)
        if not target then return notify(src, 'Nobody within 3 m.', 'error') end
      end
      local ok, err = pcall(action.run, src, target)
      if not ok then print(('^1[sare_rp_jobs] %s action %s failed: %s^7'):format(action.owner, action.id, err)) end
      return
    end
  end
end)

-- Duty and lockers --------------------------------------------------------------
-- The duty menu a job opens at its duty point. Going on or off duty only
-- works while the player still stands at that point.
local DutyAt = {} -- [src] = { job, place }
exports('DutyMenu', function(src, jobName, place, extra)
  local data = job(src)
  if not data then return end
  if data.name ~= jobName then
    return notify(src, ('This is for %s. Ask at City Hall for work.'):format(label(jobName)), 'error')
  end
  DutyAt[src] = { job = jobName, place = place }
  local options = { { title = data.onduty and 'Go off duty' or 'Go on duty', serverEvent = 'sare_rp_jobs:server:duty' } }
  for _, option in ipairs(type(extra) == 'table' and extra or {}) do options[#options + 1] = option end
  RP.Menu(src, label(jobName), options)
end)
RegisterNetEvent('sare_rp_jobs:server:duty', function()
  local src = source
  local at, data = DutyAt[src], job(src)
  if not at or not data or data.name ~= at.job or not RP.Near(src, at.place) then return end
  Core.Functions.GetPlayer(src).Functions.SetJobDuty(not data.onduty)
  notify(src, data.onduty and 'You are off duty.' or 'You are on duty. Press F2 for your job menu.', 'success', label(at.job))
end)

-- Locker gear: tops the player up to `items` ({ item = count }), once
-- every ten minutes.
local GearTaken = {}
exports('Gear', function(src, items)
  local player = Core.Functions.GetPlayer(src)
  if not player or type(items) ~= 'table' then return false end
  local key = player.PlayerData.citizenid
  local now = GetGameTimer()
  if GearTaken[key] and now - GearTaken[key] < 600000 then
    notify(src, 'You already took your gear. Come back later.', 'warning')
    return false
  end
  local got = {}
  for item, count in pairs(items) do
    local missing = (tonumber(count) or 0) - Inventory:GetItemCount(src, item)
    if missing > 0 and RPConfig.Items[item] and Inventory:AddItem(src, item, missing) then
      got[#got + 1] = ('%dx %s'):format(missing, RPConfig.Items[item].label)
    end
  end
  GearTaken[key] = now
  notify(src, #got > 0 and ('Took ' .. table.concat(got, ', ')) or 'You already carry your gear.', 'success')
  return true
end)

AddEventHandler('playerDropped', function() DutyAt[source] = nil end)
AddEventHandler('onResourceStop', function(resource)
  for i = #Actions, 1, -1 do
    if Actions[i].owner == resource then table.remove(Actions, i) end
  end
  for jobName, by in pairs(Billers) do
    if by == resource then Billers[jobName] = nil end
  end
  for name, service in pairs(Services) do
    for i = #service.jobs, 1, -1 do
      if service.jobs[i].owner == resource then table.remove(service.jobs, i) end
    end
    if #service.jobs == 0 then Services[name] = nil end
  end
end)

-- City Hall ---------------------------------------------------------------------
RegisterNetEvent('sare_rp_jobs:server:hire', function(name)
  local src = source
  if not RP.Near(src, 'city_hall') then return end
  local open = false
  for _, n in ipairs(RP.OpenJobs()) do open = open or n == name end
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

AddEventHandler('sare_rp:interact', function(src, key)
  if key ~= 'city_hall' then return end
  local options = { { title = 'Job centre', disabled = true } }
  for _, name in ipairs(RP.OpenJobs()) do
    if Core.Functions.GetJob(name) then
      options[#options + 1] = { title = 'Work as ' .. label(name), serverEvent = 'sare_rp_jobs:server:hire', args = name }
    end
  end
  options[#options + 1] = { title = 'Police and EMS', description = 'Hired by an admin: /setjob <id> police 0', disabled = true }
  options[#options + 1] = { title = ('New ID ($%d)'):format(RPConfig.Prices.identityChange), description = 'Change your name and date of birth',
    serverEvent = 'sare_rp_jobs:server:cityhall', args = 'identity' }
  RP.Menu(src, 'City Hall', options)
end)
