-- Map resources declare `resource_type 'map' { gameTypes = { ... } }` and
-- `map 'map.lua'`. Map files are Lua with directives such as:
--   spawnpoint 'fam1' { x = 2495.3, y = -1688.9, z = 13.9, heading = 0.0 }

local gametype, map = nil, nil
local spawnpoints = {}

local function resourceType(resource)
  return GetResourceMetadata(resource, 'resource_type', 0)
end

local function loadMap(resource)
  local points = {}
  local env = setmetatable({}, {
    __index = function(_, directive)
      return function(model)
        return function(data)
          if directive == 'spawnpoint' then
            points[#points + 1] = {
              model = model, x = data.x, y = data.y, z = data.z, heading = data.heading or 0.0,
            }
          end
        end
      end
    end,
  })
  for i = 0, GetNumResourceMetadata(resource, 'map') - 1 do
    local file = GetResourceMetadata(resource, 'map', i)
    local text = LoadResourceFile(resource, file)
    if text then
      local chunk, err = load(text, ('@%s/%s'):format(resource, file), 't', env)
      if chunk then
        local ok, runErr = pcall(chunk)
        if not ok then print(('map %s/%s: %s'):format(resource, file, runErr)) end
      else
        print(('map %s/%s: %s'):format(resource, file, err))
      end
    end
  end
  return points
end

AddEventHandler('onResourceStart', function(resource)
  local kind = resourceType(resource)
  if kind == 'gametype' then
    gametype = resource
    local extra = json.decode(GetResourceMetadata(resource, 'resource_type_extra', 0) or '{}') or {}
    SetGameType(extra.name or resource)
    TriggerEvent('onGameTypeStart', resource)
  elseif kind == 'map' then
    map = resource
    spawnpoints = loadMap(resource)
    SetMapName(resource)
    print(('Map %s: %d spawn points'):format(resource, #spawnpoints))
    TriggerEvent('onMapStart', resource)
  end
end)

AddEventHandler('onResourceStop', function(resource)
  if resource == map then
    map, spawnpoints = nil, {}
    TriggerEvent('onMapStop', resource)
  elseif resource == gametype then
    gametype = nil
    TriggerEvent('onGameTypeStop', resource)
  end
end)

exports('getCurrentGameType', function() return gametype end)
exports('getCurrentMap', function() return map end)
exports('getSpawnPoints', function() return spawnpoints end)
