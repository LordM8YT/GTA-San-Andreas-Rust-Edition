-- SARE RP police. Everything is decided on the server: who is on duty,
-- who is close enough, how much a fine is, and where a prisoner may stand.
local Core = exports.sarebox:GetCoreObject()
local RP = exports.sare_rp_core:GetRP()
local Jobs = exports.sare_rp_jobs
local Inventory = exports.sare_rp_inventory
local JOB = Config.Job.name
local Where = Config.Locations

local function notify(src, text, kind) RP.Notify(src, text, kind, 'Police') end
local function must(what, ok, why)
  if not ok then error(('cannot add %s: %s'):format(what, tostring(why))) end
end
-- A vector4 as the point other resources take ({ x, y, z, heading }).
local function spot(v) return { x = v.x, y = v.y, z = v.z, heading = v.w or 0.0 } end
must('the job', exports.sarebox:AddJob(JOB, Config.Job))
must('the 911 service', Jobs:AddService('police', { label = Config.Dispatch.label, colour = Config.Dispatch.colour, jobs = { JOB } }))

local function close(src, target, radius) return target and Jobs:Close(src, target, radius) end
local function minutesOk(minutes)
  return minutes and minutes >= 1 and minutes <= Config.Jail.maxMinutes and minutes == math.floor(minutes)
end

-- Fines -------------------------------------------------------------------------
local function fine(src, target, amount, reason)
  amount = RP.Amount(amount)
  if not RP.OnDuty(src, JOB) then return end
  if not amount or amount > Config.MaxFine then return notify(src, ('Fines are $1 to $%d.'):format(Config.MaxFine), 'error') end
  if not close(src, target, 5.0) then return notify(src, 'Stand next to them (5 m).', 'error') end
  local player = Core.Functions.GetPlayer(target)
  reason = tostring(reason or 'traffic offence'):gsub('[%c]', ' '):sub(1, 80)
  if player.Functions.RemoveMoney('bank', amount, 'fine: ' .. reason) or player.Functions.RemoveMoney('cash', amount, 'fine: ' .. reason) then
    RP.Notify(target, ('You were fined $%d: %s'):format(amount, reason), 'warning', 'Police')
    notify(src, ('%s paid a $%d fine.'):format(RP.Name(target), amount), 'success')
    RP.Me(src, ('writes %s a $%d ticket'):format(RP.Name(target), amount))
    print(('[%s] fine %s -> %s: $%d (%s)'):format(GetCurrentResourceName(), RP.Name(src), RP.Name(target), amount, reason))
  else
    notify(src, ('%s cannot pay $%d.'):format(RP.Name(target), amount), 'error')
  end
end

-- Jail --------------------------------------------------------------------------
local Jailed = {} -- [server id] = { ends = game timer, citizenid }

local function teleport(src, point)
  local ped = GetPlayerPed(src)
  SetEntityCoords(ped, point.x, point.y, point.z)
  SetEntityHeading(ped, point.w or 0.0)
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
  teleport(target, Where.cell)
  saveJail(target)
  RP.Notify(target, ('You are in jail for %d minute(s).'):format(math.ceil(seconds / 60)), 'warning', 'Jail')
end

local function release(target, text)
  if not Jailed[target] then return end
  Jailed[target] = nil
  saveJail(target)
  teleport(target, Where.release)
  RP.Notify(target, text or 'You are free to go.', 'success', 'Jail')
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
      elseif RP.Distance(RP.Coords(src), Where.cell) > Config.Jail.radius then
        teleport(src, Where.cell)
        RP.Notify(src, 'You cannot leave the cell.', 'error', 'Jail')
      elseif tick % 15 == 0 then
        saveJail(src)
      end
    end
  end
end)

AddEventHandler('sarebox:server:playerLoaded', function(player)
  local src = player.PlayerData.source
  local left = tonumber(player.PlayerData.metadata.jail)
  if left and left > 0 then jail(src, math.min(left, Config.Jail.maxMinutes * 60)) end
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
exports('IsJailed', function(src) return Jailed[src] ~= nil end)

-- Cuffs and search -----------------------------------------------------------------
local function cuff(src, target)
  if not close(src, target, 3.0) then return notify(src, 'Nobody within 3 m.', 'error') end
  local cuffed = not Player(target).state.cuffed
  if cuffed and Inventory:GetItemCount(src, 'handcuffs') < 1 then return notify(src, 'You need handcuffs from the lockers.', 'error') end
  Player(target).state:set('cuffed', cuffed, true)
  RP.Me(src, cuffed and ('puts handcuffs on %s'):format(RP.Name(target)) or ('takes the handcuffs off %s'):format(RP.Name(target)))
  RP.Notify(target, cuffed and 'You are handcuffed. Shops, banks and lockers are closed to you.' or 'Your handcuffs are off.', 'warning', 'Police')
end

local function search(src, target)
  if not close(src, target, 3.0) then return notify(src, 'Nobody within 3 m.', 'error') end
  local inv = Inventory:GetInventory(target) or {}
  local options = { { title = ('%s carries:'):format(RP.Name(target)), disabled = true } }
  for name, count in pairs(inv) do
    local item = RPConfig.Items[name]
    if item then
      options[#options + 1] = item.illegal
        and { title = ('%s x%d (illegal)'):format(item.label, count), description = 'Confiscate',
              serverEvent = 'sare_job_police:server:confiscate', args = { target = target, item = name } }
        or { title = ('%s x%d'):format(item.label, count), disabled = true }
    end
  end
  if #options == 1 then options[2] = { title = 'Nothing', disabled = true } end
  RP.Me(src, ('searches %s'):format(RP.Name(target)))
  RP.Menu(src, 'Search', options)
end
RegisterNetEvent('sare_job_police:server:confiscate', function(args)
  local src = source
  if type(args) ~= 'table' or not RP.OnDuty(src, JOB) then return end
  local target, item = tonumber(args.target), args.item
  if not close(src, target, 3.0) or not RPConfig.Items[item] or not RPConfig.Items[item].illegal then return end
  local count = Inventory:GetItemCount(target, item)
  if count > 0 and Inventory:RemoveItem(target, item, count) then
    RP.Me(src, ('confiscates %dx %s'):format(count, RPConfig.Items[item].label))
    RP.Notify(target, ('The police took your %s.'):format(RPConfig.Items[item].label), 'warning', 'Police')
  end
end)

local function ask(src, heading, rows, fn)
  RP.Dialog(src, heading, rows, function(values) if values then fn(values) end end)
end
local function book(src, target, minutes)
  if not minutesOk(minutes) then return notify(src, ('Use 1 to %d minutes.'):format(Config.Jail.maxMinutes), 'error') end
  if not close(src, target, 5.0) then return notify(src, 'Stand next to them (5 m).', 'error') end
  jail(target, minutes * 60)
  RP.Me(src, ('books %s for %d minute(s)'):format(RP.Name(target), minutes))
  print(('[%s] %s jailed %s for %d min'):format(GetCurrentResourceName(), RP.Name(src), RP.Name(target), minutes))
end

-- F2 job menu ----------------------------------------------------------------------
local function action(id, title, target, run, description)
  must('action ' .. id, Jobs:AddJobAction(JOB, { id = id, title = title, target = target, run = run, description = description }))
end
action('cuff', 'Cuff / uncuff nearest person', true, cuff)
action('search', 'Search nearest person', true, search)
action('fine', 'Fine nearest person', true, function(src, target)
  ask(src, 'Fine ' .. RP.Name(target), {
    { type = 'number', label = 'Amount ($)', required = true },
    { type = 'input', label = 'Reason', required = true },
  }, function(v) fine(src, target, v[1], v[2]) end)
end)
action('jail', 'Jail nearest person', true, function(src, target)
  ask(src, 'Jail ' .. RP.Name(target), {
    { type = 'number', label = ('Minutes (1-%d)'):format(Config.Jail.maxMinutes), required = true },
  }, function(v) book(src, target, tonumber(v[1])) end)
end, 'They are taken to the cell at the station')
action('unjail', 'Release a prisoner', false, function(src)
  ask(src, 'Release a prisoner', { { type = 'number', label = 'Server ID', required = true } }, function(v)
    local id = tonumber(v[1])
    if not Jailed[id] then return notify(src, 'That person is not in jail.', 'error') end
    release(id, ('%s released you.'):format(RP.Name(src)))
    notify(src, 'Released.', 'success')
  end)
end)

-- Chat commands for the same actions, with a server ID.
local function idCommand(name, fn)
  RegisterCommand(name, function(src, args)
    if src == 0 or not RP.OnDuty(src, JOB) then return RP.Notify(src, ('Only on-duty %s can do that.'):format(Config.Job.label), 'error', 'Jobs') end
    local target = tonumber(args[1])
    if not target or target == src then return RP.Notify(src, ('Usage: /%s <server id> ...'):format(name), 'inform', 'Jobs') end
    fn(src, target, args)
  end, false)
end
idCommand('cuff', cuff)
idCommand('search', search)
idCommand('fine', function(src, target, args) fine(src, target, args[2], table.concat(args, ' ', 3)) end)
idCommand('jail', function(src, target, args)
  local minutes = tonumber(args[2])
  if not minutesOk(minutes) then return notify(src, ('Usage: /jail <id> <1-%d minutes>'):format(Config.Jail.maxMinutes), 'inform') end
  book(src, target, minutes)
end)
idCommand('unjail', function(src, target) release(target, ('%s released you.'):format(RP.Name(src))) end)
-- Admins (ACL command.adminunjail) can free anyone.
RegisterCommand('adminunjail', function(src, args)
  local target = tonumber(args[1])
  if target and Jailed[target] then release(target, 'An admin released you.') end
end, true)

-- Places ------------------------------------------------------------------------
local function place(key, data)
  must('place ' .. key, exports.sare_rp_core:AddPlace(key, data))
end
place('police_duty', {
  label = 'Police front desk', coords = Where.duty, blip = Config.Blip.label, colour = Config.Blip.colour,
  onUse = function(src)
    if Core.Functions.GetPlayer(src).PlayerData.job.name ~= JOB then
      return RP.Menu(src, 'Police front desk', {
        { title = 'Report a crime', description = 'Officers on duty get your report', serverEvent = 'sare_job_police:server:report' },
      })
    end
    Jobs:DutyMenu(src, JOB, 'police_duty')
  end,
})
place('police_armory', {
  label = 'Police lockers', coords = Where.armory, job = JOB, duty = true,
  onUse = function(src) Jobs:Gear(src, Config.Gear) end,
})
place('police_garage', {
  label = 'Police vehicles', coords = Where.garage, job = JOB, duty = true,
  onUse = function(src)
    RP.Menu(src, Config.Job.label .. ' vehicles', {
      { title = 'Take / return a job vehicle', serverEvent = 'sare_job_police:server:vehicle' },
    })
  end,
})

RegisterNetEvent('sare_job_police:server:vehicle', function()
  local src = source
  if not RP.OnDuty(src, JOB) or not RP.Near(src, 'police_garage') then return end
  if exports.sare_rp_vehicles:SpawnJobVehicle(src, JOB, spot(Where.garage), Config.Vehicle) then
    RP.Notify(src, 'Your job vehicle is ready (pick again to return it).', 'success', 'Jobs')
  end
end)

RegisterNetEvent('sare_job_police:server:report', function()
  local src = source
  if not RP.Near(src, 'police_duty') then return end
  ask(src, 'Report a crime', { { type = 'input', label = 'What happened?', required = true } }, function(v)
    Jobs:Call(src, 'police', 'at the front desk: ' .. tostring(v[1]))
  end)
end)
