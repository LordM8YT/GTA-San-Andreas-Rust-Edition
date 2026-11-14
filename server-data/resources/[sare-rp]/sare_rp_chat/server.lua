-- Roleplay chat. Normal chat is local: only people close by (in the same
-- instance) read it. Out-of-character talk goes through /ooc and /looc.
local Core = exports.sarebox:GetCoreObject()
local RP = exports.sare_rp_core:GetRP()

local Last = {}
local function allowed(src)
  local now = GetGameTimer()
  if Last[src] and now - Last[src] < 700 then
    RP.Notify(src, 'Slow down a little.', 'warning', 'Chat')
    return false
  end
  Last[src] = now
  return true
end
AddEventHandler('playerDropped', function() Last[source] = nil end)

local function clean(text)
  if type(text) ~= 'string' then return nil end
  text = text:gsub('[%c]', ' '):gsub('^%s+', ''):gsub('%s+$', '')
  if text == '' then return nil end
  return text:sub(1, 200)
end

local function sendNear(src, radius, color, author, text)
  for _, id in ipairs(RP.PlayersNear(src, radius)) do
    TriggerClientEvent('chat:addMessage', id, { color = color, multiline = true, args = { author, text } })
  end
end
local function sendAll(color, author, text)
  TriggerClientEvent('chat:addMessage', -1, { color = color, multiline = true, args = { author, text } })
end

-- Typing in chat without a command: the character says it out loud.
AddEventHandler('chatMessage', function(src, _, message)
  CancelEvent()
  local text = clean(message)
  if not text or text:sub(1, 1) == '/' or not allowed(src) then return end
  sendNear(src, RPConfig.Chat.say, { 255, 255, 255 }, RP.Name(src) .. ' says', text)
  print(('[say] %s (%d): %s'):format(RP.Name(src), src, text))
end)

local function command(name, handler)
  RegisterCommand(name, function(src, args, raw)
    if src == 0 then return end
    local text = clean(table.concat(args, ' '))
    if not text then return RP.Notify(src, ('Usage: /%s <text>'):format(name), 'inform', 'Chat') end
    if not allowed(src) then return end
    handler(src, text)
  end, false)
end

command('me', function(src, text) RP.Me(src, text) end)
command('do', function(src, text)
  sendNear(src, RPConfig.Chat.me, { 140, 200, 255 }, '* ' .. text, '(( ' .. RP.Name(src) .. ' ))')
end)
command('shout', function(src, text)
  sendNear(src, RPConfig.Chat.shout, { 255, 220, 120 }, RP.Name(src) .. ' shouts', text:upper())
end)
command('looc', function(src, text)
  sendNear(src, RPConfig.Chat.say, { 170, 170, 170 }, ('[LOOC] %s (%d)'):format(GetPlayerName(src), src), text)
end)
command('ooc', function(src, text)
  sendAll({ 170, 170, 170 }, ('[OOC] %s (%d)'):format(GetPlayerName(src), src), text)
end)
command('try', function(src, text)
  local result = math.random(2) == 1 and 'and succeeds' or 'and fails'
  RP.Me(src, ('tries to %s %s'):format(text, result))
end)
command('twt', function(src, text)
  sendAll({ 90, 180, 255 }, ('Birdy @%s'):format(RP.Name(src):gsub(' ', '_')), text)
end)
command('ad', function(src, text)
  local player = Core.Functions.GetPlayer(src)
  if not player or not player.Functions.RemoveMoney('bank', RPConfig.Prices.advert, 'advert') then
    return RP.Notify(src, ('An advert costs $%d from your bank.'):format(RPConfig.Prices.advert), 'error', 'Adverts')
  end
  sendAll({ 120, 220, 120 }, ('[Advert] %s'):format(RP.Name(src)), text)
end)
