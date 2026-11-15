-- Courier: go on duty at the depot, load a package, deliver it to the
-- marked address, get paid. A small, complete example of a custom job:
-- AddJob registers the job with SARE Box, AddPlace puts its points on the
-- map, and each point's onUse decides what happens when a player presses H.
local Core = exports.sarebox:GetCoreObject()
local RP = exports.sare_rp_core:GetRP()
local JOB = Config.Job.name

-- Place keys must be unique on the server, so they start with the job name.
local DUTY, PICKUP, DROPOFF = JOB .. '_duty', JOB .. '_pickup', JOB .. '_dropoff'

local ok, err = exports.sarebox:AddJob(JOB, Config.Job)
if not ok then error(('cannot add the %s job: %s'):format(JOB, err)) end
if Config.OpenToAll then exports.sare_rp_core:AddOpenJob(JOB) end

local function addPlace(key, place)
  local added, why = exports.sare_rp_core:AddPlace(key, place)
  if not added then error(('cannot add the place %s: %s'):format(key, why)) end
end

-- Player -> number of the drop-off their package goes to.
local carrying = {}

local function player(src) return Core.Functions.GetPlayer(src) end
local function onDuty(src) return RP.OnDuty(src, JOB) end

local function stopWork(src)
  carrying[src] = nil
end

addPlace(DUTY, {
  label = Config.Blip.label,
  coords = Config.Locations.duty,
  blip = Config.Blip.label,
  colour = Config.Blip.colour,
  onUse = function(src)
    local p = player(src)
    if p.PlayerData.job.name ~= JOB then
      local how = Config.OpenToAll and 'Take the job at City Hall first.' or 'Ask the boss (an admin) for this job.'
      return RP.Notify(src, how, 'error', Config.Job.label)
    end
    local now = not p.PlayerData.job.onduty
    p.Functions.SetJobDuty(now)
    if not now then stopWork(src) end
    RP.Notify(src, now and 'You are on duty. Load a package at the loading bay.' or 'You are off duty.', 'success', Config.Job.label)
  end,
})

addPlace(PICKUP, {
  label = 'Load a package',
  coords = Config.Locations.pickup,
  job = JOB,
  duty = true,
  onUse = function(src)
    if carrying[src] then return RP.Notify(src, 'Deliver the package you have first.', 'error', Config.Job.label) end
    RP.Progress(src, 'Loading a package', Config.LoadTime, function(done)
      if not done or not onDuty(src) or not RP.Near(src, PICKUP) then return end
      local index = math.random(#Config.Locations.dropoffs)
      local target = Config.Locations.dropoffs[index]
      carrying[src] = index
      TriggerClientEvent('sare_rp:client:dispatchBlip', src, { x = target.x, y = target.y, z = target.z, label = 'Delivery', colour = 5 })
      RP.Notify(src, 'Package loaded. Take it to the address on your map.', 'success', Config.Job.label)
    end)
  end,
})

addPlace(DROPOFF, {
  label = 'Deliver the package',
  coords = Config.Locations.dropoffs,
  job = JOB,
  duty = true,
  onUse = function(src, _, point)
    local index = carrying[src]
    local target = index and Config.Locations.dropoffs[index]
    if not target then return RP.Notify(src, 'You have no package. Load one at the loading bay.', 'error', Config.Job.label) end
    if RP.Distance(point, target) > 1.0 then return RP.Notify(src, 'This package goes to another address.', 'error', Config.Job.label) end
    RP.Progress(src, 'Handing over the package', Config.DeliverTime, function(done)
      if not done or carrying[src] ~= index or not RP.Near(src, DROPOFF) or RP.Distance(RP.Coords(src), target) > 3.0 then return end
      carrying[src] = nil
      if player(src).Functions.AddMoney('cash', Config.PayPerDelivery, 'courier delivery') then
        RP.Notify(src, ('Delivered. You earned $%d.'):format(Config.PayPerDelivery), 'success', Config.Job.label)
      end
    end)
  end,
})

-- Leaving the job or the server drops the package.
AddEventHandler('sarebox:server:onJobUpdate', function(src, job)
  if job.name ~= JOB then stopWork(src) end
end)
AddEventHandler('playerDropped', function() stopWork(source) end)
