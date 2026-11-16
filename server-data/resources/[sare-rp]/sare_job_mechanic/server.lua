-- SARE RP mechanic: duty and repair kits at the lockers, the tow car at the
-- workshop, /call mechanic and bills.
local RP = exports.sare_rp_core:GetRP()
local Jobs = exports.sare_rp_jobs
local JOB = Config.Job.name
local Where = Config.Locations

local function must(what, ok, why)
  if not ok then error(('cannot add %s: %s'):format(what, tostring(why))) end
end
local function spot(v) return { x = v.x, y = v.y, z = v.z, heading = v.w or 0.0 } end
must('the job', exports.sarebox:AddJob(JOB, Config.Job))
if Config.OpenToAll then must('the open job', exports.sare_rp_core:AddOpenJob(JOB)) end
must('the service', Jobs:AddService('mechanic', { label = 'Mechanic', colour = 5, jobs = { JOB } }))
must('bills', Jobs:AllowBills(JOB))

local function place(key, data) must('place ' .. key, exports.sare_rp_core:AddPlace(key, data)) end
place('mechanic', {
  label = 'Mechanic workshop', coords = Where.workshop, blip = Config.Blip.label, colour = Config.Blip.colour,
  onUse = function(src)
    if RP.OnDuty(src, JOB) then
      RP.Menu(src, 'Workshop', { { title = 'Take / return the tow car', serverEvent = 'sare_job_mechanic:server:vehicle' } })
    else
      RP.Menu(src, 'Workshop', { { title = 'Ask for a mechanic', serverEvent = 'sare_job_mechanic:server:service' } })
    end
  end,
})
place('mechanic_duty', {
  label = 'Mechanic lockers', coords = Where.duty, job = JOB,
  onUse = function(src)
    Jobs:DutyMenu(src, JOB, 'mechanic_duty', { { title = 'Take repair kits', serverEvent = 'sare_job_mechanic:server:gear' } })
  end,
})

RegisterNetEvent('sare_job_mechanic:server:gear', function()
  local src = source
  if RP.OnDuty(src, JOB) and RP.Near(src, 'mechanic_duty') then Jobs:Gear(src, Config.Gear) end
end)
RegisterNetEvent('sare_job_mechanic:server:vehicle', function()
  local src = source
  if not RP.OnDuty(src, JOB) or not RP.Near(src, 'mechanic') then return end
  if exports.sare_rp_vehicles:SpawnJobVehicle(src, JOB, spot(Where.vehicle), Config.Vehicle) then
    RP.Notify(src, 'Your job vehicle is ready (pick again to return it).', 'success', 'Jobs')
  end
end)
RegisterNetEvent('sare_job_mechanic:server:service', function()
  local src = source
  if RP.Near(src, 'mechanic') then Jobs:Call(src, 'mechanic', 'is waiting at the workshop') end
end)
