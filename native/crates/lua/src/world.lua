-- World scripts: the Lua side of one script in a Build Mode world. Rust
-- provides the __world natives (see world.rs); this file builds the script
-- API on them: world, script, objects, players, vectors, wait and friends.
local native = __world
__world = nil
local stopped = __stopped
__stopped = nil
local raw_resume, raw_pcall, raw_xpcall = coroutine.resume, pcall, xpcall

-- A script stopped for running too long stays stopped: catching the stop
-- only raises it again.
local STOPPED = 'the script ran too long without wait() and was stopped'
local function check(...)
  if stopped() then
    error(STOPPED, 0)
  end
  return ...
end
function pcall(...)
  return check(raw_pcall(...))
end
function xpcall(...)
  return check(raw_xpcall(...))
end
coroutine.resume = function(...)
  return check(raw_resume(...))
end
coroutine.wrap = function(fn)
  local co = coroutine.create(fn)
  return function(...)
    local result = table.pack(check(raw_resume(co, ...)))
    if not result[1] then
      error(result[2], 0)
    end
    return table.unpack(result, 2, result.n)
  end
end

-- Pattern matching runs in C, where the time limit cannot reach: a pattern
-- with many repeats on a long text is refused instead of freezing the game.
local raw = { find = string.find, match = string.match, gmatch = string.gmatch, gsub = string.gsub }
local function repeats(pattern)
  local count, i, n = 0, 1, #pattern
  while i <= n do
    local c = pattern:sub(i, i)
    if c == '%' then
      i = i + 2
    elseif c == '[' then
      i = i + 1
      if pattern:sub(i, i) == '^' then
        i = i + 1
      end
      if pattern:sub(i, i) == ']' then
        i = i + 1
      end
      while i <= n and pattern:sub(i, i) ~= ']' do
        if pattern:sub(i, i) == '%' then
          i = i + 1
        end
        i = i + 1
      end
      i = i + 1
    else
      if c == '*' or c == '+' or c == '-' or c == '?' then
        count = count + 1
      end
      i = i + 1
    end
  end
  return count
end
local function guard(name, text, pattern)
  if type(text) ~= 'string' or type(pattern) ~= 'string' then
    return
  end
  local k = repeats(pattern)
  -- Backtracking can take about #text ^ (repeats + 1) steps.
  if k > 0 and (k + 1) * math.log(math.max(#text, 1), 10) > 8 then
    error(string.format('string.%s: a pattern with %d repeats on a text of %d characters could take too long; use a shorter text or a simpler pattern', name, k, #text), 3)
  end
end
string.find = function(text, pattern, init, plain)
  if not plain then
    guard('find', text, pattern)
  end
  return raw.find(text, pattern, init, plain)
end
string.match = function(text, pattern, init)
  guard('match', text, pattern)
  return raw.match(text, pattern, init)
end
string.gmatch = function(text, pattern, init)
  guard('gmatch', text, pattern)
  return raw.gmatch(text, pattern, init)
end
string.gsub = function(text, pattern, replacement, n)
  guard('gsub', text, pattern)
  return raw.gsub(text, pattern, replacement, n)
end

local threads, now = {}, 0
local handlers = {}

local function report(message)
  -- A script stopped for running too long is reported once, by the host.
  if not stopped() then
    native.error(tostring(message))
  end
end

-- Every script body and handler runs as its own coroutine, so it can wait.
local function run(co, ...)
  local ok, message = raw_resume(co, ...)
  if not ok then
    report(message)
  end
end
local function start(fn, ...)
  run(coroutine.create(fn), ...)
end

function wait(seconds)
  local co, main = coroutine.running()
  if main then
    error('wait() only works inside a script or a handler', 2)
  end
  seconds = math.max(tonumber(seconds) or 0, 0)
  threads[#threads + 1] = { co = co, at = now + seconds }
  coroutine.yield()
  return seconds
end

function spawn(fn, ...)
  if type(fn) ~= 'function' then
    error('spawn needs a function', 2)
  end
  start(fn, ...)
end

function delay(seconds, fn)
  if type(fn) ~= 'function' then
    error('delay needs a number of seconds and a function', 2)
  end
  start(function()
    wait(seconds)
    fn()
  end)
end

function every(seconds, fn)
  seconds = tonumber(seconds)
  if not seconds or seconds < 0.05 or type(fn) ~= 'function' then
    error('every needs at least 0.05 seconds and a function', 2)
  end
  local running = true
  start(function()
    while running do
      wait(seconds)
      if running then
        fn()
      end
    end
  end)
  return {
    stop = function()
      running = false
    end,
  }
end

function print(...)
  local parts = {}
  for i = 1, select('#', ...) do
    parts[#parts + 1] = tostring((select(i, ...)))
  end
  native.print(table.concat(parts, '  '))
end

-- Vectors: positions in the world's metres (x east, y north, z up), the
-- numbers the Properties panel shows.
local Vector = {}
Vector.__index = Vector
function vector(x, y, z)
  return setmetatable({ x = tonumber(x) or 0, y = tonumber(y) or 0, z = tonumber(z) or 0 }, Vector)
end
local function is_vector(v)
  return getmetatable(v) == Vector
end
Vector.__add = function(a, b)
  return vector(a.x + b.x, a.y + b.y, a.z + b.z)
end
Vector.__sub = function(a, b)
  return vector(a.x - b.x, a.y - b.y, a.z - b.z)
end
Vector.__mul = function(a, b)
  if type(a) == 'number' then
    a, b = b, a
  end
  return vector(a.x * b, a.y * b, a.z * b)
end
Vector.__div = function(a, b)
  return vector(a.x / b, a.y / b, a.z / b)
end
Vector.__unm = function(a)
  return vector(-a.x, -a.y, -a.z)
end
Vector.__eq = function(a, b)
  return a.x == b.x and a.y == b.y and a.z == b.z
end
Vector.__tostring = function(v)
  return string.format('vector(%.2f, %.2f, %.2f)', v.x, v.y, v.z)
end
function Vector:length()
  return math.sqrt(self.x * self.x + self.y * self.y + self.z * self.z)
end
function Vector:distance(other)
  return (self - other):length()
end

-- Objects of the world, by id. Their fields read the world as it is now.
local Object, objects = {}, {}
local ObjectMeta = {}
local function object(id)
  if id == nil then
    return nil
  end
  local o = objects[id]
  if not o then
    o = setmetatable({ __object = id }, ObjectMeta)
    objects[id] = o
  end
  return o
end
ObjectMeta.__index = function(o, key)
  local id = rawget(o, '__object')
  if key == 'id' then
    return id
  elseif key == 'name' then
    return native.object_name(id)
  elseif key == 'visible' then
    return native.visible(id)
  elseif key == 'position' then
    return vector(native.position(id))
  elseif key == 'heading' then
    return native.heading(id)
  end
  return Object[key]
end
ObjectMeta.__newindex = function(o, key, value)
  if key == 'visible' then
    local ok, message = native.set_visible(rawget(o, '__object'), value and true or false)
    if not ok then
      error(message, 2)
    end
  else
    error('objects have no field ' .. tostring(key) .. ' to set (visible can be set)', 2)
  end
end
ObjectMeta.__tostring = function(o)
  return 'object ' .. o.name
end
function Object:show()
  self.visible = true
end
function Object:hide()
  self.visible = false
end
function Object:toggle()
  self.visible = not self.visible
end
function Object:on(event, fn)
  return world.on(event, self, fn)
end

-- Players in the world, by id.
local Player, players = {}, {}
local PlayerMeta = {}
local function player(id)
  if id == nil then
    return nil
  end
  local p = players[id]
  if not p then
    p = setmetatable({ __player = id }, PlayerMeta)
    players[id] = p
  end
  return p
end
PlayerMeta.__index = function(p, key)
  local id = rawget(p, '__player')
  if key == 'id' then
    return id
  elseif key == 'name' then
    return native.player_name(id)
  elseif key == 'position' then
    local x, y, z = native.player_position(id)
    return x and vector(x, y, z) or nil
  elseif key == 'driving' then
    return native.player_driving(id)
  elseif key == 'here' then
    return native.player_name(id) ~= nil
  end
  return Player[key]
end
PlayerMeta.__newindex = function()
  error('players have no fields to set; use their methods (message, teleport)', 2)
end
PlayerMeta.__tostring = function(p)
  return 'player ' .. tostring(p.name)
end
function Player:message(text, seconds)
  native.message(rawget(self, '__player'), tostring(text), tonumber(seconds) or 4)
end
-- To an object (standing on it, facing its way) or a vector (with a heading).
function Player:teleport(to, heading)
  local id = rawget(self, '__player')
  local ok, message
  if type(to) == 'table' and rawget(to, '__object') then
    ok, message = native.teleport(id, rawget(to, '__object'))
  elseif is_vector(to) then
    ok, message = native.teleport_to(id, to.x, to.y, to.z, tonumber(heading) or 0)
  else
    ok, message = false, 'teleport needs an object or a vector'
  end
  if not ok then
    error(message, 2)
  end
end

world = {}
function world.find(name)
  return object(native.find(tostring(name)))
end
function world.objects()
  local list = {}
  for _, id in ipairs(native.objects()) do
    list[#list + 1] = object(id)
  end
  return list
end
function world.players()
  local list = {}
  for _, id in ipairs(native.players()) do
    list[#list + 1] = player(id)
  end
  return list
end
function world.message(text, seconds)
  native.message(nil, tostring(text), tonumber(seconds) or 4)
end
function world.hour()
  return native.hour()
end
function world.setHour(hour)
  local ok, message = native.set_hour(tonumber(hour))
  if not ok then
    error(message, 2)
  end
end
function world.weather()
  return native.weather()
end
function world.setWeather(weather)
  local ok, message = native.set_weather(tostring(weather))
  if not ok then
    error(message, 2)
  end
end
function world.time()
  return now
end

local builtin = {
  enter = true, leave = true, use = true, checkpoint = true, finish = true, join = true, quit = true,
}
-- world.on(event, fn) or world.on(event, object or name, fn).
function world.on(event, filter, fn)
  if fn == nil then
    filter, fn = nil, filter
  end
  if type(event) ~= 'string' or type(fn) ~= 'function' then
    error('world.on needs an event name and a function', 2)
  end
  if type(filter) == 'string' then
    local found = world.find(filter)
    if not found then
      error('no object is named ' .. filter, 2)
    end
    filter = found
  end
  local only
  if filter ~= nil then
    only = type(filter) == 'table' and rawget(filter, '__object')
    if not only then
      error('world.on can listen to an object, an object name, or everything', 2)
    end
  end
  local list = handlers[event]
  if not list then
    list = {}
    handlers[event] = list
  end
  local handler = { object = only, fn = fn }
  list[#list + 1] = handler
  return {
    disconnect = function()
      for i, h in ipairs(list) do
        if h == handler then
          table.remove(list, i)
          break
        end
      end
    end,
  }
end
-- Tell every script (this one too) about something: world.on(name, fn)
-- gets the values, from the next frame on.
function world.emit(name, ...)
  if type(name) ~= 'string' or builtin[name] then
    error('world.emit needs a name of your own (not ' .. tostring(name) .. ')', 2)
  end
  local ok, message = native.emit(name, { ... })
  if not ok then
    error(message, 2)
  end
end

script = { name = native.script_name(), object = object(native.script_object()) }

-- Called by Rust.
function __world_tick(time)
  now = time
  local due, waiting = {}, {}
  for _, t in ipairs(threads) do
    if t.at <= now then
      due[#due + 1] = t
    else
      waiting[#waiting + 1] = t
    end
  end
  threads = waiting
  for _, t in ipairs(due) do
    run(t.co)
  end
end
function __world_event(name, object_id, player_id, ...)
  local list = handlers[name]
  if not list then
    return
  end
  local args
  if name == 'enter' or name == 'leave' or name == 'use' then
    args = table.pack(player(player_id), object(object_id))
  elseif name == 'checkpoint' or name == 'finish' then
    args = table.pack(player(player_id), ...)
  elseif name == 'join' or name == 'quit' then
    args = table.pack(player(player_id))
  else
    args = table.pack(...)
  end
  for _, h in ipairs({ table.unpack(list) }) do
    if h.object == nil or h.object == object_id then
      start(h.fn, table.unpack(args, 1, args.n))
    end
  end
end
function __world_start(body)
  start(body)
end
