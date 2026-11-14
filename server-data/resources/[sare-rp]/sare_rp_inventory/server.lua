-- Inventory: item counts saved in the SARE Box character's metadata
-- (`metadata.inventory`), limited by weight. Shops sell into it.
local Core = exports.sarebox:GetCoreObject()
local RP = exports.sare_rp_core:GetRP()
local Items = RPConfig.Items

local function inventory(src)
  local player = Core.Functions.GetPlayer(src)
  if not player then return nil end
  local inv = player.PlayerData.metadata.inventory
  return type(inv) == 'table' and inv or {}, player
end

local function weight(inv)
  local total = 0
  for name, count in pairs(inv) do
    if Items[name] then total = total + Items[name].weight * count end
  end
  return total
end

local function wholeCount(value)
  local n = tonumber(value)
  if not n or n ~= n or n < 1 or n > 1000 or n ~= math.floor(n) then return nil end
  return math.tointeger(n)
end

-- Adds `count` of `item`; false when the item is unknown or too heavy.
local function AddItem(src, item, count)
  count = wholeCount(count or 1)
  local inv, player = inventory(src)
  if not inv or not Items[item] or not count then return false end
  if weight(inv) + Items[item].weight * count > RPConfig.MaxWeight then return false end
  inv[item] = (inv[item] or 0) + count
  player.Functions.SetMetaData('inventory', inv)
  return true
end

-- Removes `count` of `item`; false when the player has fewer.
local function RemoveItem(src, item, count)
  count = wholeCount(count or 1)
  local inv, player = inventory(src)
  if not inv or not count or (inv[item] or 0) < count then return false end
  inv[item] = inv[item] - count
  if inv[item] == 0 then inv[item] = nil end
  player.Functions.SetMetaData('inventory', inv)
  return true
end

local function GetItemCount(src, item)
  local inv = inventory(src)
  return inv and inv[item] or 0
end

local function sorted(inv)
  local names = {}
  for name in pairs(inv) do
    if Items[name] then names[#names + 1] = name end
  end
  table.sort(names, function(a, b) return Items[a].label < Items[b].label end)
  return names
end

local function openInventory(src)
  local inv = inventory(src)
  if not inv then return end
  local options = {
    { title = ('Carrying %.1f / %.0f kg'):format(weight(inv) / 1000, RPConfig.MaxWeight / 1000), disabled = true },
  }
  for _, name in ipairs(sorted(inv)) do
    options[#options + 1] = { title = ('%s x%d'):format(Items[name].label, inv[name]),
      description = 'Use, give or drop', serverEvent = 'sare_rp_inventory:server:item', args = name }
  end
  if #options == 1 then options[#options + 1] = { title = 'Your pockets are empty', disabled = true } end
  RP.Menu(src, 'Inventory', options)
end
RegisterNetEvent('sare_rp_inventory:server:open', function() openInventory(source) end)
RegisterCommand('inv', function(src) if src ~= 0 then openInventory(src) end end, false)

RegisterNetEvent('sare_rp_inventory:server:item', function(name)
  local src = source
  if type(name) ~= 'string' or not Items[name] or GetItemCount(src, name) < 1 then return end
  local item = Items[name]
  local options = {}
  if item.use then
    options[#options + 1] = { title = 'Use ' .. item.label, serverEvent = 'sare_rp_inventory:server:use', args = name }
  end
  options[#options + 1] = { title = 'Give to the nearest person', serverEvent = 'sare_rp_inventory:server:give', args = name }
  options[#options + 1] = { title = 'Drop', serverEvent = 'sare_rp_inventory:server:drop', args = name }
  RP.Menu(src, item.label, options)
end)

local Uses = {}
-- The phone: Birdy posts and service calls through the chat commands.
function Uses.phone(src)
  RP.Menu(src, 'Phone', {
    { title = 'Call 911', description = 'Police and EMS. Or type /911 <what happened>', serverEvent = 'sare_rp_inventory:server:phone', args = 'police' },
    { title = 'Call a taxi', serverEvent = 'sare_rp_inventory:server:phone', args = 'taxi' },
    { title = 'Call a mechanic', serverEvent = 'sare_rp_inventory:server:phone', args = 'mechanic' },
  })
end
RegisterNetEvent('sare_rp_inventory:server:phone', function(service)
  local src = source
  if GetItemCount(src, 'phone') < 1 then return end
  RP.Dialog(src, 'Phone call', { { type = 'input', label = 'What do you need?', required = true } }, function(values)
    if values and GetItemCount(src, 'phone') > 0 then TriggerEvent('sare_rp_jobs:call', src, service, tostring(values[1])) end
  end)
end)

RegisterNetEvent('sare_rp_inventory:server:use', function(name)
  local src = source
  local item = Items[name]
  if not item or not item.use or GetItemCount(src, name) < 1 then return end
  if Uses[item.use] then return Uses[item.use](src) end
  -- Food, drinks and tools are used up; radios are kept.
  if name ~= 'radio' and not RemoveItem(src, name, 1) then return end
  RP.Me(src, item.use)
  TriggerEvent('sare_rp_inventory:used', src, name)
end)

RegisterNetEvent('sare_rp_inventory:server:give', function(name)
  local src = source
  if not Items[name] or GetItemCount(src, name) < 1 then return end
  local nearest, best
  local here = RP.Coords(src)
  for _, id in ipairs(RP.PlayersNear(src, 3.0)) do
    local d = RP.Distance(here, RP.Coords(id))
    if id ~= src and (not best or d < best) then nearest, best = id, d end
  end
  if not nearest then return RP.Notify(src, 'Nobody is close enough (3 m).', 'error', 'Inventory') end
  RP.Dialog(src, ('Give %s to %s'):format(Items[name].label, RP.Name(nearest)), {
    { type = 'number', label = 'How many', required = true, default = 1 },
  }, function(values)
    local count = values and wholeCount(values[1])
    if not count then return end
    if not Core.Functions.GetPlayer(nearest) or RP.Distance(RP.Coords(src), RP.Coords(nearest)) > 3.0 then
      return RP.Notify(src, 'They walked away.', 'error', 'Inventory')
    end
    if not RemoveItem(src, name, count) then return RP.Notify(src, 'You do not have that many.', 'error', 'Inventory') end
    if not AddItem(nearest, name, count) then
      AddItem(src, name, count)
      return RP.Notify(src, 'They cannot carry that much.', 'error', 'Inventory')
    end
    RP.Me(src, ('hands %s %dx %s'):format(RP.Name(nearest), count, Items[name].label))
    RP.Notify(nearest, ('You got %dx %s.'):format(count, Items[name].label), 'success', 'Inventory')
  end)
end)

RegisterNetEvent('sare_rp_inventory:server:drop', function(name)
  local src = source
  local count = GetItemCount(src, name)
  if count < 1 then return end
  RemoveItem(src, name, count)
  RP.Notify(src, ('Dropped %dx %s.'):format(count, Items[name].label), 'inform', 'Inventory')
end)

-- Shops: the 24/7 and the gas station. Prices are in RPConfig.Shops.
local function openShop(src, key)
  local options = {}
  for index, entry in ipairs(RPConfig.Shops[key] or {}) do
    local item = Items[entry.item]
    if item then
      options[#options + 1] = { title = ('%s  $%d'):format(item.label, entry.price), description = ('%.1f kg'):format(item.weight / 1000),
        serverEvent = 'sare_rp_inventory:server:buy', args = { shop = key, index = index } }
    end
  end
  RP.Menu(src, RPConfig.Places[key].label, options)
end

AddEventHandler('sare_rp:interact', function(src, key)
  if RPConfig.Shops[key] then openShop(src, key) end
end)

RegisterNetEvent('sare_rp_inventory:server:buy', function(args)
  local src = source
  if type(args) ~= 'table' or not RPConfig.Shops[args.shop] then return end
  local entry = RPConfig.Shops[args.shop][tonumber(args.index) or 0]
  if not entry or not RP.Near(src, args.shop) then return RP.Notify(src, 'Go to the shop counter.', 'error', 'Shop') end
  RP.Dialog(src, 'Buy ' .. Items[entry.item].label, {
    { type = 'number', label = ('How many ($%d each)'):format(entry.price), required = true, default = 1 },
    { type = 'select', label = 'Pay with', default = 'cash', options = { { value = 'cash', label = 'Cash' }, { value = 'bank', label = 'Card' } } },
  }, function(values)
    local count = values and wholeCount(values[1])
    if not count then return end
    if not RP.Near(src, args.shop) then return RP.Notify(src, 'You left the shop.', 'error', 'Shop') end
    local account = values[2] == 'bank' and 'bank' or 'cash'
    local player = Core.Functions.GetPlayer(src)
    local inv = inventory(src)
    if weight(inv) + Items[entry.item].weight * count > RPConfig.MaxWeight then
      return RP.Notify(src, 'You cannot carry that much.', 'error', 'Shop')
    end
    if not player.Functions.RemoveMoney(account, entry.price * count, 'shop ' .. entry.item) then
      return RP.Notify(src, ('Not enough %s.'):format(account == 'bank' and 'money on your card' or 'cash'), 'error', 'Shop')
    end
    if not AddItem(src, entry.item, count) then
      player.Functions.AddMoney(account, entry.price * count, 'shop refund')
      return RP.Notify(src, 'You cannot carry that much.', 'error', 'Shop')
    end
    RP.Notify(src, ('Bought %dx %s for $%d.'):format(count, Items[entry.item].label, entry.price * count), 'success', 'Shop')
  end)
end)

exports('AddItem', AddItem)
exports('RemoveItem', RemoveItem)
exports('GetItemCount', GetItemCount)
exports('GetInventory', function(src) return (inventory(src)) end)
exports('Weight', function(src) return weight(inventory(src) or {}) end)
