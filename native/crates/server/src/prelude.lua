-- Server additions to common.lua (same chunk: `native` and helpers are in scope).
function TriggerClientEvent(name, target, ...) native.trigger_client(name, tonumber(target) or -1, pack(...)) end
function TriggerLatentClientEvent(name, target, _bps, ...) TriggerClientEvent(name, target, ...) end

-- State bags (server side only; not replicated to clients yet).
-- GlobalState.key, Player(src).state.key and Entity(handle).state:set(k, v, r).
local bag_handlers, next_bag_handler = {}, 0
function AddStateBagChangeHandler(key, bag, handler)
  next_bag_handler = next_bag_handler + 1
  bag_handlers[next_bag_handler] = { key = key, bag = bag, fn = handler }
  return next_bag_handler
end
function RemoveStateBagChangeHandler(cookie) bag_handlers[cookie] = nil end
-- Called before the new value is stored, as in FiveM.
function __sare_bag_change(bag, key, value, replicated)
  for _, h in pairs(bag_handlers) do
    if (h.key == nil or h.key == '' or h.key == key) and (h.bag == nil or h.bag == '' or h.bag == bag) then
      resume(coroutine.create(function() h.fn(bag, key, value, 0, replicated) end))
    end
  end
end
local function state_bag(name)
  local function set(_, key, value, replicated) native.bag_set(name, key, value, replicated == true) end
  return setmetatable({}, {
    __index = function(_, key)
      if key == 'set' then return set end
      return native.bag_get(name, key)
    end,
    __newindex = function(_, key, value) native.bag_set(name, key, value, true) end,
  })
end
GlobalState = state_bag('global')
function Player(id) return { state = state_bag(('player:%s'):format(tonumber(id) or id)) } end
function Entity(handle) return { state = state_bag(('entity:%s'):format(handle)) } end
