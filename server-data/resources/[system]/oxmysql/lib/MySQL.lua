-- Load with `server_script '@oxmysql/lib/MySQL.lua'`. Gives the `MySQL`
-- table that oxmysql scripts use, backed by SARE's SQLite database:
--
--   MySQL.query('SELECT * FROM players WHERE license = ?', { license }, function(rows) end)
--   local row = MySQL.single.await('SELECT * FROM players WHERE license = ?', { license })
--   local id = MySQL.insert.await('INSERT INTO players (license) VALUES (?)', { license })
--
-- Parameters are a list for `?` or a table for `@name` / `:name`. Queries
-- run right away; callbacks are called on the next tick, as with oxmysql.
-- `.await` raises a Lua error when the query fails; the callback form prints
-- the error and does not call back.
local resource = GetCurrentResourceName()

local function report(err)
  print(('^1[oxmysql] %s: %s^0'):format(resource, tostring(err)))
end

local function later(cb, ...)
  if not cb then return end
  local args = table.pack(...)
  SetTimeout(0, function() cb(table.unpack(args, 1, args.n)) end)
end

-- (sql, params, cb) or (sql, cb)
local function split(params, cb)
  if cb == nil and (type(params) == 'function' or (type(params) == 'table' and rawget(params, '__cfx_functionReference'))) then
    return nil, params
  end
  return params, cb
end

local function method(mode)
  local function await(sql, params)
    return SareDbExecute(mode, sql, (split(params)))
  end
  return setmetatable({ await = await }, {
    __call = function(_, sql, params, cb)
      params, cb = split(params, cb)
      local ok, result = pcall(SareDbExecute, mode, sql, params)
      if not ok then return report(result) end
      later(cb, result)
    end,
  })
end

-- Accepts { 'sql', ... } with shared params, { { query = 'sql', values = {} } },
-- { { 'sql', {} } } or { ['sql'] = params }.
local function queries(list, shared)
  local out = {}
  for key, entry in pairs(list) do
    if type(key) == 'string' then
      out[#out + 1] = { key, entry }
    elseif type(entry) == 'string' then
      out[#out + 1] = { entry, shared }
    else
      out[#out + 1] = {
        entry.query or entry.sql or entry[1],
        entry.values or entry.parameters or entry.params or entry[2] or shared,
      }
    end
  end
  return out
end

local function transaction_await(list, params)
  local ok, err = pcall(SareDbTransaction, queries(list, params))
  if not ok then report(err) end
  return ok
end

MySQL = {
  query = method('query'),
  single = method('single'),
  scalar = method('scalar'),
  insert = method('insert'),
  update = method('update'),
  prepare = method('prepare'),
  rawExecute = method('rawExecute'),
  transaction = setmetatable({ await = transaction_await }, {
    __call = function(_, list, params, cb)
      params, cb = split(params, cb)
      later(cb, transaction_await(list, params))
    end,
  }),
}
MySQL.execute = MySQL.query

-- The database is local, so it is always ready.
function MySQL.ready(cb) later(cb) end
function MySQL.isReady() return true end
function MySQL.awaitConnection() return true end
-- Old mysql-async helper: hands the query back to reuse later.
function MySQL.store(sql, cb)
  later(cb, sql)
  return sql
end

-- mysql-async and ghmattimysql style names used by older scripts.
MySQL.Async = {
  fetchAll = MySQL.query,
  fetchSingle = MySQL.single,
  fetchScalar = MySQL.scalar,
  execute = MySQL.update,
  insert = MySQL.insert,
  transaction = MySQL.transaction,
  store = MySQL.store,
}
MySQL.Sync = {
  fetchAll = MySQL.query.await,
  fetchSingle = MySQL.single.await,
  fetchScalar = MySQL.scalar.await,
  execute = MySQL.update.await,
  insert = MySQL.insert.await,
  transaction = MySQL.transaction.await,
  store = MySQL.store,
}
