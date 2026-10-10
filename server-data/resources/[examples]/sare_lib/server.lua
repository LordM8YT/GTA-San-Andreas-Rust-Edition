local callbacks = {}

exports('registerCallback', function(name, fn)
  callbacks[name] = { fn = fn, owner = GetInvokingResource() }
end)

exports('triggerCallback', function(name, ...)
  local callback = callbacks[name]
  if not callback then error(('no callback named %s'):format(name), 2) end
  return callback.fn(...)
end)

-- A stopped resource's functions can no longer be called.
AddEventHandler('onResourceStop', function(resource)
  for name, callback in pairs(callbacks) do
    if callback.owner == resource then callbacks[name] = nil end
  end
end)
