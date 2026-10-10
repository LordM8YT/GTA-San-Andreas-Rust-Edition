-- FiveM-compatible scheduler, events, vectors and exports for one resource.
-- Rust provides the __sare_* natives; everything here is ordinary Lua.
local native = __sare
__sare = nil

local threads, now = {}, 0
local handlers, net_safe, next_handler = {}, {}, 0
local cancelled = false
local resource_name = GetCurrentResourceName()

-- Function references. A function crossing a resource boundary (export
-- arguments and results, local event arguments) becomes
-- { __cfx_functionReference = 'resource:id' }; the receiver gets a callable
-- proxy. The server frees references that no proxy holds.
local refs, released = {}, {}
local ref_meta = {}
local function encode(value, depth)
  local kind = type(value)
  if kind == 'function' then
    local key, id = native.ref_new()
    refs[id] = value
    return { __cfx_functionReference = key }
  end
  if kind ~= 'table' or depth > 32 then return value end
  local ref = rawget(value, '__cfx_functionReference')
  if ref then return { __cfx_functionReference = ref } end
  local copy
  for key, item in pairs(value) do
    local encoded = encode(item, depth + 1)
    if not rawequal(encoded, item) then
      if not copy then
        copy = {}
        for k, v in pairs(value) do copy[k] = v end
      end
      copy[key] = encoded
    end
  end
  return copy or value
end
-- Decodes in place: values from the server are always fresh copies.
local function decode(value, depth)
  if type(value) ~= 'table' or depth > 32 then return value end
  local ref = rawget(value, '__cfx_functionReference')
  if type(ref) == 'string' then
    local owner, id = ref:match('^(.*):(%d+)$')
    if owner == resource_name then
      return refs[tonumber(id)] or function() error(('function reference %s no longer exists'):format(ref), 2) end
    end
    native.ref_retain(ref)
    return setmetatable({ __cfx_functionReference = ref }, ref_meta)
  end
  for key, item in pairs(value) do value[key] = decode(item, depth + 1) end
  return value
end
local function pack(...) return { n = select('#', ...), ... } end
ref_meta.__call = function(self, ...)
  local result = decode(native.call_ref(rawget(self, '__cfx_functionReference'), encode(pack(...), 0)), 0)
  return table.unpack(result, 1, result.n or #result)
end
-- Finalizers only queue: the release reaches the server at the next tick.
ref_meta.__gc = function(self) released[#released + 1] = rawget(self, '__cfx_functionReference') end
function __sare_ref_call(id, args)
  local fn = refs[tonumber(id)]
  if not fn then error(('function reference %s:%s no longer exists'):format(resource_name, id)) end
  args = decode(args, 0)
  return encode(pack(fn(table.unpack(args, 1, args.n or #args))), 0)
end
function __sare_ref_free(id) refs[id] = nil end

-- The safe standard library has no `debug`; errors keep their file:line.
local traceback = debug and debug.traceback or function(_, message) return message end
local function resume(co, ...)
  local ok, value = coroutine.resume(co, ...)
  if not ok then
    native.error(traceback(co, tostring(value)))
  elseif coroutine.status(co) ~= 'dead' then
    threads[#threads + 1] = { co = co, wake = now + math.max(0, tonumber(value) or 0) }
  end
end

-- CfxLua syntax in chunks loaded at runtime (ox_lib loads its modules with
-- LoadResourceFile + load); `a?.b` compiles to __cfx_safe(a, 'b').
local raw_load = load
function load(chunk, ...)
  if type(chunk) == 'string' then chunk = native.cfxlua(chunk) end
  return raw_load(chunk, ...)
end
function __cfx_safe(value, key)
  if value == nil then return nil end
  return value[key]
end

-- CfxLua library additions used by ox_lib and Qbox.
function table.clone(t)
  local copy = {}
  for k, v in pairs(t) do copy[k] = v end
  return setmetatable(copy, getmetatable(t))
end
function table.wipe(t)
  for k in pairs(t) do t[k] = nil end
  return t
end
function table.create() return {} end
-- 'empty', 'array' (keys 1..n), 'hash' (no array keys) or 'mixed'.
function table.type(t)
  if type(t) ~= 'table' then return nil end
  if next(t) == nil then return 'empty' end
  local n, array, other = #t, 0, 0
  for k in pairs(t) do
    if math.type(k) == 'integer' and k >= 1 and k <= n then array = array + 1 else other = other + 1 end
  end
  if other == 0 then return 'array' end
  return array == 0 and 'hash' or 'mixed'
end
-- string.strsplit(delimiter, text[, pieces]) returns the parts.
function string.strsplit(delimiter, text, pieces)
  local parts, at = {}, 1
  while not pieces or #parts < pieces - 1 do
    local from, to = text:find(delimiter, at, true)
    if not from then break end
    parts[#parts + 1] = text:sub(at, from - 1)
    at = to + 1
  end
  parts[#parts + 1] = text:sub(at)
  return table.unpack(parts)
end
function string.strjoin(delimiter, ...) return table.concat({ ... }, delimiter) end
function string.strtrim(text, chars)
  chars = chars and ('[' .. chars:gsub('[%]%^%-]', '%%%0') .. ']') or '%s'
  return (text:gsub('^' .. chars .. '+', ''):gsub(chars .. '+$', ''))
end
if io then
  function io.readdir(path)
    local names = native.readdir(path)
    if not names then return nil end
    local i = 0
    return {
      lines = function() return function() i = i + 1 return names[i] end end,
      close = function() end,
    }
  end
end

Citizen = {}
function Citizen.CreateThread(fn) resume(coroutine.create(fn)) end
function Citizen.CreateThreadNow(fn) resume(coroutine.create(fn)) end
function Citizen.Wait(ms)
  local _, main = coroutine.running()
  if main then error('Wait can only be called inside a thread or event handler', 2) end
  coroutine.yield(ms or 0)
end
function Citizen.SetTimeout(ms, fn)
  Citizen.CreateThread(function() Citizen.Wait(ms) fn() end)
end
function print(...)
  local parts = {}
  for i = 1, select('#', ...) do parts[i] = tostring((select(i, ...))) end
  native.print(table.concat(parts, '	'))
end
Citizen.Trace = print
CreateThread, Wait, SetTimeout = Citizen.CreateThread, Citizen.Wait, Citizen.SetTimeout

function __sare_tick(time)
  now = time
  if #released > 0 then
    local list = released
    released = {}
    native.ref_release(list)
  end
  local ready, waiting = {}, {}
  for _, thread in ipairs(threads) do
    if thread.wake <= now then ready[#ready + 1] = thread else waiting[#waiting + 1] = thread end
  end
  threads = waiting
  for _, thread in ipairs(ready) do resume(thread.co) end
end

function AddEventHandler(name, fn)
  next_handler = next_handler + 1
  local list = handlers[name] or {}
  handlers[name] = list
  list[#list + 1] = { id = next_handler, fn = fn }
  return { key = next_handler, name = name }
end
function RemoveEventHandler(handle)
  local list = handle and handlers[handle.name]
  if not list then return end
  for i, entry in ipairs(list) do
    if entry.id == handle.key then table.remove(list, i) return end
  end
end
function RegisterNetEvent(name, fn)
  net_safe[name] = true
  if fn then return AddEventHandler(name, fn) end
end
RegisterServerEvent = RegisterNetEvent
function CancelEvent() cancelled = true end
function WasEventCanceled() return native.was_cancelled() end

-- Returns true when a handler cancelled the event.
function __sare_event(name, src, args, from_net)
  if from_net and not net_safe[name] then
    if handlers[name] then
      native.print(('event %s was not safe for net, use RegisterNetEvent'):format(name))
    end
    return false
  end
  local list = handlers[name]
  if not list then return false end
  -- Clients cannot send function references.
  if not from_net then args = decode(args, 0) end
  local kick_reason
  if name == 'playerConnecting' and not from_net then
    -- The player is already in the session: a rejection drops them.
    args = pack(args[1], function(reason) kick_reason = tostring(reason) end, {
      defer = function() end,
      update = function() end,
      presentCard = function() end,
      handover = function() end,
      done = function(reason)
        if reason then DropPlayer(src, tostring(reason)) end
      end,
    })
  end
  cancelled = false
  local snapshot = { table.unpack(list) }
  for _, entry in ipairs(snapshot) do
    resume(coroutine.create(function()
      source = src
      entry.fn(table.unpack(args, 1, args.n or #args))
    end))
  end
  local result = cancelled
  cancelled = false
  if result and name == 'playerConnecting' and not from_net then
    DropPlayer(src, kick_reason or 'Connection rejected by the server.')
  end
  return result
end

function TriggerEvent(name, ...) native.trigger(name, encode(pack(...), 0)) end
function TriggerClientEvent(name, target, ...) native.trigger_client(name, tonumber(target) or -1, pack(...)) end
function TriggerLatentClientEvent(name, target, _bps, ...) TriggerClientEvent(name, target, ...) end

local commands = {}
function RegisterCommand(name, fn, restricted)
  commands[name:lower()] = fn
  native.register_command(name, restricted == true)
end
function __sare_command(name, src, args, raw)
  local fn = commands[name]
  if fn then resume(coroutine.create(function() fn(src, args, raw) end)) end
end

-- Exports: exports('name', fn) and exports.resource:name(...)
local own_exports = {}
function __sare_export_call(name, args)
  local fn = own_exports[name]
  if not fn then error(('No such export %s in resource %s'):format(name, resource_name)) end
  args = decode(args, 0)
  return encode(pack(fn(table.unpack(args, 1, args.n or #args))), 0)
end
exports = setmetatable({}, {
  __call = function(_, name, fn)
    own_exports[name] = fn
    native.export(name)
  end,
  __index = function(_, resource)
    local proxy = {}
    return setmetatable(proxy, {
      __index = function(_, name)
        -- As in FiveM the first argument is always dropped: call
        -- exports.res:name(...) (ox_lib calls exports.res.name(nil, ...)).
        return function(_, ...)
          local result = decode(native.call_export(resource, name, encode(pack(...), 0)), 0)
          return table.unpack(result, 1, result.n or #result)
        end
      end,
    })
  end,
})

-- vector2/3/4 with arithmetic and #(a - b) distance, like the FiveM runtime.
local vector_meta = {}
local function make(x, y, z, w)
  return setmetatable({ x = x, y = y, z = z, w = w }, vector_meta)
end
local function parts(v) return v.x or 0, v.y or 0, v.z, v.w end
local function combine(a, b, op)
  if type(a) == 'number' then a, b = b, a end
  if type(b) == 'number' then
    return make(op(a.x, b), op(a.y, b), a.z and op(a.z, b), a.w and op(a.w, b))
  end
  local ax, ay, az, aw = parts(a)
  local bx, by, bz, bw = parts(b)
  return make(op(ax, bx), op(ay, by), az and op(az, bz or 0), aw and op(aw, bw or 0))
end
vector_meta.__add = function(a, b) return combine(a, b, function(x, y) return x + y end) end
vector_meta.__sub = function(a, b) return combine(a, b, function(x, y) return x - y end) end
vector_meta.__mul = function(a, b) return combine(a, b, function(x, y) return x * y end) end
vector_meta.__div = function(a, b) return combine(a, b, function(x, y) return x / y end) end
vector_meta.__unm = function(a) return make(-a.x, -a.y, a.z and -a.z, a.w and -a.w) end
vector_meta.__len = function(a) return math.sqrt(a.x * a.x + a.y * a.y + (a.z or 0) ^ 2 + (a.w or 0) ^ 2) end
vector_meta.__eq = function(a, b) return a.x == b.x and a.y == b.y and a.z == b.z and a.w == b.w end
vector_meta.__tostring = function(a)
  local values = { a.x, a.y, a.z, a.w }
  return ('vector%d(%s)'):format(#values, table.concat(values, ', '))
end
vector_meta.__index = function(v, key)
  if key == 'xy' then return make(v.x, v.y) end
  if key == 'xyz' then return make(v.x, v.y, v.z) end
end
function vector2(x, y) return make(x + 0.0, y + 0.0) end
function vector3(x, y, z) return make(x + 0.0, y + 0.0, z + 0.0) end
function vector4(x, y, z, w) return make(x + 0.0, y + 0.0, z + 0.0, w + 0.0) end
vec2, vec3, vec4 = vector2, vector3, vector4

json = {
  encode = function(value) return native.json_encode(value) end,
  decode = function(text) return native.json_decode(text) end,
}

-- FiveM promise subset used by many resources.
promise = {}
promise.__index = promise
function promise.new() return setmetatable({ done = false }, promise) end
function promise:resolve(value) if not self.done then self.done, self.value = true, value end end
function promise:reject(err) if not self.done then self.done, self.err = true, err end end
function Citizen.Await(p)
  while not p.done do Citizen.Wait(0) end
  if p.err then error(p.err, 2) end
  return p.value
end

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
