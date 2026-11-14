-- Character identity: first name, last name, date of birth and gender,
-- asked once on the first join and saved with the SARE Box character.
local Core = exports.sarebox:GetCoreObject()
local RP = exports.sare_rp_core:GetRP()

local Asking = {}

local function validName(value)
  if type(value) ~= 'string' then return nil end
  value = value:gsub('^%s+', ''):gsub('%s+$', '')
  if #value < 2 or #value > 16 or not value:match("^%a[%a%-' ]*$") then return nil end
  return value:sub(1, 1):upper() .. value:sub(2)
end

local function validDate(value)
  if type(value) ~= 'string' then return nil end
  local y, m, d = value:match('^(%d%d%d%d)%-(%d%d)%-(%d%d)$')
  y, m, d = tonumber(y), tonumber(m), tonumber(d)
  if not y or y < 1900 or y > 2010 or m < 1 or m > 12 or d < 1 or d > 31 then return nil end
  return ('%04d-%02d-%02d'):format(y, m, d)
end

local function identity(src)
  local player = Core.Functions.GetPlayer(src)
  local id = player and player.PlayerData.metadata.identity
  return type(id) == 'table' and id.firstname and id or nil
end

-- Opens the identity form. `paid` is called after a successful change.
local function ask(src, reason, onDone)
  if Asking[src] then return end
  Asking[src] = true
  local current = identity(src) or {}
  RP.Dialog(src, reason or 'Create your character', {
    { type = 'input', label = 'First name', required = true, default = current.firstname },
    { type = 'input', label = 'Last name', required = true, default = current.lastname },
    { type = 'input', label = 'Date of birth (YYYY-MM-DD)', required = true, default = current.dob or '1995-06-15' },
    { type = 'select', label = 'Gender', default = current.gender or 'x', options = {
      { value = 'f', label = 'Female' }, { value = 'm', label = 'Male' }, { value = 'x', label = 'Other' } } },
  }, function(values)
    Asking[src] = nil
    local player = Core.Functions.GetPlayer(src)
    if not player then return end
    if not values then
      if not identity(src) then RP.Notify(src, 'You can make your character later with /identity.', 'warning', 'Identity') end
      return
    end
    local first, last, dob = validName(values[1]), validName(values[2]), validDate(values[3])
    local gender = ({ f = 'f', m = 'm', x = 'x' })[values[4]] or 'x'
    if not first or not last or not dob then
      RP.Notify(src, 'Use 2-16 letters for names and a date like 1995-06-15 (1900-2010).', 'error', 'Identity')
      return SetTimeout(500, function() ask(src, reason, onDone) end)
    end
    player.Functions.SetMetaData('identity', { firstname = first, lastname = last, dob = dob, gender = gender })
    RP.Notify(src, ('You are %s %s.'):format(first, last), 'success', 'Identity')
    print(('[sare_rp_identity] %s (%d) is now %s %s'):format(GetPlayerName(src), src, first, last))
    if onDone then onDone() end
  end)
end

AddEventHandler('sarebox:server:playerLoaded', function(player)
  local src = player.PlayerData.source
  -- Guests without a saved character are asked too; it lasts the session.
  if not identity(src) then SetTimeout(1000, function() ask(src) end) end
end)

AddEventHandler('onResourceStart', function(resource)
  if resource ~= GetCurrentResourceName() then return end
  for _, src in ipairs(Core.Functions.GetPlayers()) do
    if not identity(src) then ask(src) end
  end
end)

AddEventHandler('playerDropped', function() Asking[source] = nil end)

RegisterCommand('identity', function(src)
  if src == 0 then return end
  if identity(src) then
    return RP.Notify(src, 'Change your name at City Hall.', 'inform', 'Identity')
  end
  ask(src)
end, false)

-- Show your ID card to everyone within 3 m.
RegisterCommand('showid', function(src)
  if src == 0 then return end
  local id = identity(src)
  if not id then return RP.Notify(src, 'You have no ID yet. Use /identity.', 'error', 'Identity') end
  local player = Core.Functions.GetPlayer(src)
  local text = ('%s %s  |  born %s  |  citizen %s'):format(id.firstname, id.lastname, id.dob, player.PlayerData.citizenid)
  for _, other in ipairs(RP.PlayersNear(src, 3.0)) do
    RP.Notify(other, text, 'inform', 'ID card')
  end
  RP.Me(src, 'shows an ID card')
end, false)

-- City Hall: change name for a fee (paid only when the change is saved).
AddEventHandler('sare_rp_identity:change', function(src)
  local player = Core.Functions.GetPlayer(src)
  if not player then return end
  local price = RPConfig.Prices.identityChange
  if player.PlayerData.money.bank < price and player.PlayerData.money.cash < price then
    return RP.Notify(src, ('A new ID costs $%d.'):format(price), 'error', 'City Hall')
  end
  local old = identity(src)
  ask(src, ('New ID ($%d)'):format(price), function()
    local fresh = Core.Functions.GetPlayer(src)
    if not (fresh.Functions.RemoveMoney('bank', price, 'identity change') or fresh.Functions.RemoveMoney('cash', price, 'identity change')) then
      -- The money went elsewhere while the form was open: keep the old ID.
      fresh.Functions.SetMetaData('identity', old)
      RP.Notify(src, 'Payment failed; your ID did not change.', 'error', 'City Hall')
    end
  end)
end)

exports('GetIdentity', identity)
