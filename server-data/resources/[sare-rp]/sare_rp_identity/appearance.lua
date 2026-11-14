-- Character looks: the character creator on the first join and the clothes
-- shop. The server owns every look and sends it to all players with
-- SetPlayerAppearance; it is saved with the SARE Box character as
-- `metadata.appearance`. Looks need a content pack with characters (SARE
-- RP Town has them); with original San Andreas peds the menus still work
-- but players keep their SA ped.
local Core = exports.sarebox:GetCoreObject()
local RP = exports.sare_rp_core:GetRP()
local L = RPConfig.Looks

-- [src] = { look = table, mode = 'create' | 'shop', before = table }
local Editing = {}

local function identity(src)
  local player = Core.Functions.GetPlayer(src)
  local id = player and player.PlayerData.metadata.identity
  return type(id) == 'table' and id.firstname and id or nil
end

local function copy(look)
  local out = {}
  for k, v in pairs(look) do
    out[k] = type(v) == 'table' and { v[1], v[2], v[3] } or v
  end
  return out
end

local function paletteIndex(palette, rgb)
  for i, entry in ipairs(palette) do
    local c = entry.rgb
    if rgb and c[1] == rgb[1] and c[2] == rgb[2] and c[3] == rgb[3] then return i end
  end
  return 0
end

-- A starting look: the body from the ID's gender, some variety by id.
local function defaultLook(src)
  local id = identity(src)
  local female = id and id.gender == 'f'
  local pick = function(list, salt) return list[(src * 7 + salt) % #list + 1].rgb end
  return {
    body = female and 1 or 0,
    skin = pick(L.skin, 2),
    hair = female and 3 or 1,
    hairColor = pick(L.hairColours, 1),
    top = 0, topColor = pick(L.colours, 4),
    pants = 0, pantsColor = { 52, 62, 84 },
    shoes = 0, shoesColor = { 235, 235, 235 },
  }
end

local function saved(src)
  local player = Core.Functions.GetPlayer(src)
  local look = player and player.PlayerData.metadata.appearance
  return type(look) == 'table' and look.skin and look or nil
end

local function label(list, index) return list[(index or 0) + 1] or ('Style ' .. tostring(index)) end
local function colourName(palette, rgb)
  local i = paletteIndex(palette, rgb)
  return i > 0 and palette[i].label or 'Custom'
end

-- Menu rows: a style list or a colour palette per appearance field.
-- `create` rows only change in the creator, not in the shop.
local Rows = {
  { field = 'body', label = 'Body', list = L.bodies, create = true },
  { field = 'skin', label = 'Skin tone', palette = L.skin, create = true },
  { field = 'hair', label = 'Hair', list = L.hair },
  { field = 'hairColor', label = 'Hair colour', palette = L.hairColours },
  { field = 'top', label = 'Top', list = L.top },
  { field = 'topColor', label = 'Top colour', palette = L.colours },
  { field = 'pants', label = 'Pants', list = L.pants },
  { field = 'pantsColor', label = 'Pants colour', palette = L.colours },
  { field = 'shoes', label = 'Shoes', list = L.shoes },
  { field = 'shoesColor', label = 'Shoes colour', palette = L.colours },
}

local function openMenu(src)
  local edit = Editing[src]
  if not edit then return end
  local look = edit.look
  local options = {}
  for _, row in ipairs(Rows) do
    if edit.mode == 'create' or not row.create then
      local value = row.list and label(row.list, look[row.field]) or colourName(row.palette, look[row.field])
      options[#options + 1] = { title = ('%s: %s'):format(row.label, value),
        description = 'Pick to change', serverEvent = 'sare_rp_identity:server:look', args = row.field }
    end
  end
  if edit.mode == 'create' then
    options[#options + 1] = { title = 'Done: this is me', serverEvent = 'sare_rp_identity:server:lookDone', args = 'save' }
    RP.Menu(src, 'Create your look', options)
  else
    options[#options + 1] = { title = ('Buy this outfit  $%d'):format(RPConfig.Prices.clothes),
      serverEvent = 'sare_rp_identity:server:lookDone', args = 'buy' }
    options[#options + 1] = { title = 'Leave without buying', serverEvent = 'sare_rp_identity:server:lookDone', args = 'cancel' }
    RP.Menu(src, 'Clothes shop', options)
  end
end

-- A shop preview ends when the customer walks out (even with the menu
-- closed), so nobody keeps an outfit they did not pay for.
local function watchShop(src, edit)
  if Editing[src] ~= edit then return end
  if not Core.Functions.GetPlayer(src) then Editing[src] = nil return end
  if not RP.Near(src, 'clothes', 6.0) then
    Editing[src] = nil
    SetPlayerAppearance(src, edit.before)
    return RP.Notify(src, 'You left the clothes shop.', 'inform', 'Clothes')
  end
  SetTimeout(2000, function() watchShop(src, edit) end)
end

local function startEditing(src, mode)
  local before = saved(src) or defaultLook(src)
  local edit = { look = copy(before), mode = mode, before = copy(before) }
  Editing[src] = edit
  SetPlayerAppearance(src, edit.look)
  openMenu(src)
  if mode == 'shop' then SetTimeout(2000, function() watchShop(src, edit) end) end
end

RegisterNetEvent('sare_rp_identity:server:look', function(field)
  local src = source
  local edit = Editing[src]
  if not edit then return end
  if edit.mode == 'shop' and not RP.Near(src, 'clothes', 6.0) then
    Editing[src] = nil
    SetPlayerAppearance(src, edit.before)
    return RP.Notify(src, 'You left the clothes shop.', 'inform', 'Clothes')
  end
  for _, row in ipairs(Rows) do
    if row.field == field and (edit.mode == 'create' or not row.create) then
      if row.list then
        edit.look[field] = ((edit.look[field] or 0) + 1) % #row.list
      else
        local i = paletteIndex(row.palette, edit.look[field]) % #row.palette + 1
        local c = row.palette[i].rgb
        edit.look[field] = { c[1], c[2], c[3] }
      end
      SetPlayerAppearance(src, edit.look)
      break
    end
  end
  openMenu(src)
end)

RegisterNetEvent('sare_rp_identity:server:lookDone', function(choice)
  local src = source
  local edit = Editing[src]
  if not edit then return end
  local player = Core.Functions.GetPlayer(src)
  if not player then return end
  Editing[src] = nil
  if choice == 'cancel' then
    SetPlayerAppearance(src, edit.before)
    return RP.Notify(src, 'Come back any time.', 'inform', 'Clothes')
  end
  if edit.mode == 'shop' then
    local price = RPConfig.Prices.clothes
    if not RP.Near(src, 'clothes', 6.0) then
      SetPlayerAppearance(src, edit.before)
      return RP.Notify(src, 'You left the clothes shop.', 'inform', 'Clothes')
    end
    if not (player.Functions.RemoveMoney('cash', price, 'clothes') or player.Functions.RemoveMoney('bank', price, 'clothes')) then
      SetPlayerAppearance(src, edit.before)
      return RP.Notify(src, ('New clothes cost $%d.'):format(price), 'error', 'Clothes')
    end
  end
  player.Functions.SetMetaData('appearance', edit.look)
  SetPlayerAppearance(src, edit.look)
  RP.Notify(src, edit.mode == 'shop' and 'Looking sharp!' or 'Welcome to town.', 'success', 'Look')
end)

-- First join: the creator after the ID form; later joins wear the saved look.
local function loaded(src)
  local look = saved(src)
  if look then
    SetPlayerAppearance(src, look)
    return
  end
  SetPlayerAppearance(src, defaultLook(src))
  local tries = 0
  local function wait()
    tries = tries + 1
    if not Core.Functions.GetPlayer(src) or saved(src) or Editing[src] then return end
    -- Let the ID form finish first (the body follows its gender).
    if not identity(src) and tries < 60 then return SetTimeout(1000, wait) end
    startEditing(src, 'create')
  end
  SetTimeout(1500, wait)
end

AddEventHandler('sarebox:server:playerLoaded', function(player) loaded(player.PlayerData.source) end)
AddEventHandler('onResourceStart', function(resource)
  if resource ~= GetCurrentResourceName() then return end
  for _, src in ipairs(Core.Functions.GetPlayers()) do loaded(src) end
end)
AddEventHandler('playerDropped', function() Editing[source] = nil end)

AddEventHandler('sare_rp:interact', function(src, key)
  if key == 'clothes' then startEditing(src, 'shop') end
end)

-- /look reopens the creator while the character has no saved look yet.
RegisterCommand('look', function(src)
  if src == 0 then return end
  if saved(src) then return RP.Notify(src, 'Visit the clothes shop for a new outfit.', 'inform', 'Look') end
  startEditing(src, 'create')
end, false)

exports('GetAppearance', function(src) return saved(src) end)
