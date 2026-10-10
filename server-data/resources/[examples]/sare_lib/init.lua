-- Runs inside the resource that lists `shared_script '@sare_lib/init.lua'`.
-- Defines `lib`, loading modules/<name>.lua the first time lib.<name> is used.
if GetResourceState('sare_lib') ~= 'started' then
  error('sare_lib must be started before ' .. GetCurrentResourceName(), 0)
end

local sare_lib = exports.sare_lib

lib = setmetatable({ name = 'sare_lib' }, {
  __index = function(self, module)
    local chunk = LoadResourceFile('sare_lib', ('modules/%s.lua'):format(module))
    if not chunk then return nil end
    local fn, err = load(chunk, ('@@sare_lib/modules/%s.lua'):format(module))
    if not fn then error(err, 2) end
    local value = fn()
    rawset(self, module, value)
    return value
  end,
})

-- Callbacks live in sare_lib, so any resource can call any other's by name.
-- The functions cross resources as function references.
lib.callback = {
  register = function(name, fn) sare_lib:registerCallback(name, fn) end,
  await = function(name, ...) return sare_lib:triggerCallback(name, ...) end,
}
