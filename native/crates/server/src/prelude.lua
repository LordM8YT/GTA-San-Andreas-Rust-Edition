-- FiveM-compatible scheduler, events, vectors and exports for one resource.
-- Rust provides the __sare_* natives; everything here is ordinary Lua.
local native = __sare
__sare = nil

local threads, now = {}, 0
local handlers, net_safe, next_handler = {}, {}, 0
local cancelled = false

local function resume(co, ...)
  local ok, value = coroutine.resume(co, ...)
  if not ok then
    native.error(debug.traceback(co, tostring(value)))
  elseif coroutine.status(co) ~= 'dead' then
    threads[#threads + 1] = { co = co, wake = now + math.max(0, tonumber(value) or 0) }
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
  return result
end

local function pack(...) return { n = select('#', ...), ... } end
function TriggerEvent(name, ...) native.trigger(name, pack(...)) end
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
  if not fn then error(('No such export %s in resource %s'):format(name, GetCurrentResourceName())) end
  return pack(fn(table.unpack(args, 1, args.n or #args)))
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
        return function(first, ...)
          -- Support both exports.res:name(...) and exports.res.name(...)
          local args
          if first == proxy then args = pack(...) else args = pack(first, ...) end
          local result = native.call_export(resource, name, args)
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
