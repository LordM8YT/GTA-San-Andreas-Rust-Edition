-- Client side: a job menu drawn by the game's own UI. Open it with F2 or /jobs.
local jobs = {
  { id = 'taxi', title = 'Taxi driver', pay = 120 },
  { id = 'mechanic', title = 'Mechanic', pay = 150 },
}

local function openMenu()
  local options = {}
  for _, job in ipairs(jobs) do
    options[#options + 1] = {
      title = job.title,
      -- lib.math comes from sare_lib/modules/math.lua, loaded on this machine.
      description = ('$%d per shift'):format(lib.math.round(job.pay)),
      onSelect = function() ExecuteCommand('job ' .. job.id) end,
    }
  end
  options[#options + 1] = {
    title = 'Work a shift',
    description = 'Takes a few seconds',
    onSelect = function()
      if UI.progressBar({ label = 'Working...', duration = 3000 }) then
        ExecuteCommand('work')
      else
        exports.sare_lib:notify('Jobs', 'Shift cancelled', 'warning')
      end
    end,
  }
  UI.registerContext({ id = 'sare_jobs', title = 'Jobs', options = options })
  UI.showContext('sare_jobs')
end

RegisterCommand('jobs', openMenu)
RegisterKeyMapping('jobs', 'Open the job menu', 'keyboard', 'F2')
