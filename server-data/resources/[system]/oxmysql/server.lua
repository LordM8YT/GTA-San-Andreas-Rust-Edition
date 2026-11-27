-- exports.oxmysql:query(sql, params, cb) and friends, for scripts that call
-- the resource directly instead of loading @oxmysql/lib/MySQL.lua.
for _, method in ipairs({ 'query', 'single', 'scalar', 'insert', 'update', 'prepare', 'rawExecute', 'transaction' }) do
  local fn = MySQL[method]
  exports(method, function(sql, params, cb) return fn(sql, params, cb) end)
  exports(method .. '_async', function(sql, params) return fn.await(sql, params) end)
end
exports('execute', function(sql, params, cb) return MySQL.query(sql, params, cb) end)
exports('isReady', function() return true end)
exports('awaitConnection', function() return true end)
exports('store', function(sql, cb) return MySQL.store(sql, cb) end)

print(('database ready (%s)'):format(GetConvar('sare_db_file', 'db/sare.db')))
