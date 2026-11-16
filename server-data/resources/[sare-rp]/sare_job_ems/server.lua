-- SARE RP EMS: check-in at the hospital, duty and lockers, ambulances and
-- treating people. 911 calls reach EMS as well as the police.
local Core = exports.sarebox:GetCoreObject()
local RP = exports.sare_rp_core:GetRP()
local Jobs = exports.sare_rp_jobs
local Inventory = exports.sare_rp_inventory
local JOB = Config.Job.name
local Where = Config.Locations

local function notify(src, text, kind) RP.Notify(src, text, kind, 'EMS') end
local function must(what, ok, why)
  if not ok then error(('cannot add %s: %s'):format(what, tostring(why))) end
end
local function spot(v) return { x = v.x, y = v.y, z = v.z, heading = v.w or 0.0 } end
must('the job', exports.sarebox:AddJob(JOB, Config.Job))
must('the EMS service', Jobs:AddService('ems', { label = 'EMS', colour = 1, jobs = { JOB } }))
must('EMS to 911', Jobs:AddService('police', { jobs = { JOB } }))
must('bills', Jobs:AllowBills(JOB))

local function treat(src, target)
  if not Jobs:Close(src, target, 3.0) then return notify(src, 'Nobody within 3 m.', 'error') end
  local supply = Inventory:GetItemCount(src, 'medkit') > 0 and 'medkit' or (Inventory:GetItemCount(src, 'bandage') > 0 and 'bandage')
  if not supply then return notify(src, 'You need a medkit or a bandage from the EMS lockers.', 'error') end
  RP.Me(src, ('starts treating %s'):format(RP.Name(target)))
  RP.Progress(src, 'Treating ' .. RP.Name(target), Config.TreatTime, function(done)
    if not done then return notify(src, 'Treatment stopped.', 'warning') end
    if not Jobs:Close(src, target, 4.0) or not Inventory:RemoveItem(src, supply, 1) then return end
    RP.Me(src, ('finishes treating %s'):format(RP.Name(target)))
    notify(target, ('%s treated your injuries.'):format(RP.Name(src)), 'success')
  end)
end
must('action treat', Jobs:AddJobAction(JOB, { id = 'treat', title = 'Treat nearest person', description = 'Uses a medkit or a bandage',
  target = true, run = treat }))
RegisterCommand('treat', function(src, args)
  if src == 0 or not RP.OnDuty(src, JOB) then return RP.Notify(src, ('Only on-duty %s can do that.'):format(Config.Job.label), 'error', 'Jobs') end
  local target = tonumber(args[1])
  if not target or target == src then return RP.Notify(src, 'Usage: /treat <server id>', 'inform', 'Jobs') end
  treat(src, target)
end, false)

local function place(key, data) must('place ' .. key, exports.sare_rp_core:AddPlace(key, data)) end
place('hospital_checkin', {
  label = 'Hospital reception', coords = Where.reception, blip = Config.Blip.label, colour = Config.Blip.colour,
  onUse = function(src)
    RP.Menu(src, 'Hospital reception', {
      { title = 'Get treated', description = ('An EMS on duty is called; otherwise the doctor ($%d)'):format(Config.TreatmentPrice),
        serverEvent = 'sare_job_ems:server:hospital' },
    })
  end,
})
place('hospital_duty', {
  label = 'EMS lockers', coords = Where.duty, job = JOB,
  onUse = function(src)
    Jobs:DutyMenu(src, JOB, 'hospital_duty', { { title = 'Take EMS gear', serverEvent = 'sare_job_ems:server:gear' } })
  end,
})
place('hospital_garage', {
  label = 'Ambulance bay', coords = Where.garage, job = JOB, duty = true,
  onUse = function(src)
    RP.Menu(src, Config.Job.label .. ' vehicles', {
      { title = 'Take / return a job vehicle', serverEvent = 'sare_job_ems:server:vehicle' },
    })
  end,
})

RegisterNetEvent('sare_job_ems:server:gear', function()
  local src = source
  if RP.OnDuty(src, JOB) and RP.Near(src, 'hospital_duty') then Jobs:Gear(src, Config.Gear) end
end)
RegisterNetEvent('sare_job_ems:server:vehicle', function()
  local src = source
  if not RP.OnDuty(src, JOB) or not RP.Near(src, 'hospital_garage') then return end
  if exports.sare_rp_vehicles:SpawnJobVehicle(src, JOB, spot(Where.garage), Config.Vehicle) then
    RP.Notify(src, 'Your job vehicle is ready (pick again to return it).', 'success', 'Jobs')
  end
end)
RegisterNetEvent('sare_job_ems:server:hospital', function()
  local src = source
  if not RP.Near(src, 'hospital_checkin') then return end
  if #RP.OnDutyPlayers({ JOB }) > 0 then return Jobs:Call(src, 'ems', 'is waiting at the hospital reception') end
  local player = Core.Functions.GetPlayer(src)
  local price = Config.TreatmentPrice
  if not (player.Functions.RemoveMoney('bank', price, 'hospital') or player.Functions.RemoveMoney('cash', price, 'hospital')) then
    return RP.Notify(src, ('Treatment costs $%d.'):format(price), 'error', 'Hospital')
  end
  RP.Progress(src, 'The doctor treats you', 8000, function()
    RP.Notify(src, 'The doctor patched you up.', 'success', 'Hospital')
  end)
end)
