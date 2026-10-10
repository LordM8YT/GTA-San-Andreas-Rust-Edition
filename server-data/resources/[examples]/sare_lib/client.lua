-- sare_lib on players' machines. Starting it here sends its `files` (init.lua
-- and modules/) with it, so `@sare_lib/init.lua` can load modules on the client.
exports('notify', function(title, description, kind)
  UI.notify({ title = title, description = description, type = kind or 'inform' })
end)
