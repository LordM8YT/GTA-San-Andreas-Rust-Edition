-- Server side of the FiveM chat resource. The SARE client draws the chat
-- box (T to type) and handles chat:addMessage, chat:addSuggestion(s) and chat:clear.

RegisterNetEvent('_chat:messageEntered')
AddEventHandler('_chat:messageEntered', function(author, color, message)
  local src = source
  if type(message) ~= 'string' or message == '' or #message > 256 then
    return
  end
  -- The client sends its name; the server's own record is authoritative.
  local name = GetPlayerName(src) or tostring(author)
  TriggerEvent('chatMessage', src, name, message)
  if not WasEventCanceled() then
    TriggerClientEvent('chat:addMessage', -1, { color = { 255, 255, 255 }, multiline = true, args = { name, message } })
    print(('%s: %s'):format(name, message))
  end
end)

-- /say from the server console, as in FiveM.
RegisterCommand('say', function(src, args)
  local message = table.concat(args, ' ')
  if message == '' then return end
  TriggerClientEvent('chat:addMessage', -1, { color = { 255, 80, 80 }, args = { src == 0 and 'console' or GetPlayerName(src), message } })
end, true)

local function suggestions()
  local list = {}
  for _, command in ipairs(GetRegisteredCommands()) do
    list[#list + 1] = { name = '/' .. command.name, help = '' }
  end
  return list
end

AddEventHandler('playerJoining', function()
  TriggerClientEvent('chat:addSuggestions', source, suggestions())
end)

AddEventHandler('onServerResourceStart', function()
  -- Commands may have changed; refresh everyone's suggestions shortly after.
  SetTimeout(500, function()
    TriggerClientEvent('chat:addSuggestions', -1, suggestions())
  end)
end)
