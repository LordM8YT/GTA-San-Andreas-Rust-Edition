AddEventHandler('onResourceStart', function(resource)
  if resource == GetCurrentResourceName() then
    exports.spawnmanager:setAutoSpawn(true)
  end
end)

AddEventHandler('playerJoining', function()
  local name = GetPlayerName(source)
  TriggerClientEvent('chat:addMessage', -1, { color = { 120, 200, 120 }, args = { 'server', name .. ' joined.' } })
end)

AddEventHandler('playerDropped', function(reason)
  local name = GetPlayerName(source) or ('player ' .. tostring(source))
  TriggerClientEvent('chat:addMessage', -1, { color = { 200, 120, 120 }, args = { 'server', ('%s left (%s).'):format(name, reason) } })
end)

-- /respawn sends a player back to a random spawn point.
RegisterCommand('respawn', function(src)
  if src ~= 0 then exports.spawnmanager:spawnPlayer(src) end
end, false)
