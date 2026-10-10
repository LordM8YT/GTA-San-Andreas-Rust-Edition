-- The admin menu. The server sends the player list only to admins and checks
-- every action again.
local function action(name, ...)
  TriggerServerEvent('sarebox_admin:action', name, { ... })
end

local function playerMenu(p)
  UI.registerContext({
    id = 'sarebox_admin_player',
    title = ('%s (%d)'):format(p.name, p.id),
    options = {
      { title = 'Go to', onSelect = function() action('goto', p.id) end },
      { title = 'Bring', onSelect = function() action('bring', p.id) end },
      { title = 'Give money', onSelect = function()
          local values = UI.inputDialog('Give money to ' .. p.name, {
            { type = 'select', label = 'Account', options = { { value = 'cash', label = 'Cash' }, { value = 'bank', label = 'Bank' } }, default = 'cash' },
            { type = 'number', label = 'Amount', required = true },
          })
          if values then action('givemoney', p.id, values[1], values[2]) end
        end },
      { title = 'Set job', onSelect = function()
          local values = UI.inputDialog('Job for ' .. p.name, {
            { type = 'select', label = 'Job', required = true, options = {
              { value = 'unemployed', label = 'Unemployed' }, { value = 'taxi', label = 'Taxi' },
              { value = 'mechanic', label = 'Mechanic' }, { value = 'police', label = 'LSPD' },
            } },
            { type = 'number', label = 'Grade', default = 0 },
          })
          if values then action('setjob', p.id, values[1], values[2] or 0) end
        end },
      { title = 'Kick', onSelect = function()
          local values = UI.inputDialog('Kick ' .. p.name, { { type = 'input', label = 'Reason' } })
          if values then action('kick', p.id, values[1]) end
        end },
      { title = 'Back', menu = 'sarebox_admin' },
    },
  })
  UI.showContext('sarebox_admin_player')
end

RegisterNetEvent('sarebox_admin:menu', function(players)
  local options = {}
  for _, p in ipairs(players) do
    options[#options + 1] = {
      title = ('%d  %s'):format(p.id, p.name),
      description = ('%s, $%d cash, $%d bank'):format(p.job, p.cash, p.bank),
      onSelect = function() playerMenu(p) end,
    }
  end
  options[#options + 1] = { title = 'Announcement', onSelect = function()
    local values = UI.inputDialog('Announcement', { { type = 'input', label = 'Text', required = true } })
    if values then action('announce', values[1]) end
  end }
  UI.registerContext({ id = 'sarebox_admin', title = 'Admin', options = options })
  UI.showContext('sarebox_admin')
end)

RegisterKeyMapping('admin', 'Admin menu', 'keyboard', 'F11')
