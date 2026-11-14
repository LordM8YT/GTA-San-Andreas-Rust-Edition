-- The job menu key (F2). The server decides what the menu offers.
RegisterCommand('jobmenu', function()
  TriggerServerEvent('sare_rp_jobs:server:menu')
end)
RegisterKeyMapping('jobmenu', 'Job menu', 'keyboard', RPConfig.JobMenuKey)
