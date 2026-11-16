-- SARE RP taxi: duty and taxis at the depot, /call taxi and bills.
local Core = exports.sarebox:GetCoreObject()
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
must('the service', Jobs:AddService('taxi', { label = 'Taxi', colour = 5, jobs = { JOB } }))
must('bills', Jobs:AllowBills(JOB))

must('place taxi_depot', exports.sare_rp_core:AddPlace('taxi_depot', {
  label = 'Taxi depot', coords = Where.depot, blip = Config.Blip.label, colour = Config.Blip.colour,
  onUse = function(src)
    if Core.Functions.GetPlayer(src).PlayerData.job.name ~= JOB then
      return RP.Notify(src, 'Become a taxi driver at City Hall.', 'inform', 'Taxi')
    end
    Jobs:DutyMenu(src, JOB, 'taxi_depot', { { title = 'Take / return a taxi', serverEvent = 'sare_job_taxi:server:vehicle' } })
  end,
}))

RegisterNetEvent('sare_job_taxi:server:vehicle', function()
  local src = source
  if not RP.OnDuty(src, JOB) or not RP.Near(src, 'taxi_depot') then return end
  if exports.sare_rp_vehicles:SpawnJobVehicle(src, JOB, spot(Where.depot), Config.Vehicle) then
    RP.Notify(src, 'Your job vehicle is ready (pick again to return it).', 'success', 'Jobs')
  end
end)
