-- Points for places other resources add with exports.sare_rp_core:AddPlace.
-- Shared by the server and the players' game.
RPPlaces = {}

local function number(value)
  local n = tonumber(value)
  if n and n == n and math.abs(n) < 100000 then return n + 0.0 end
end

-- vector3(x, y, z), vector4(x, y, z, heading) or { x =, y =, z =, heading = }
-- as a plain point, or nil.
function RPPlaces.point(value)
  if type(value) ~= 'table' then return nil end
  local x, y = number(value.x), number(value.y)
  if not x or not y then return nil end
  return { x = x, y = y, z = number(value.z) or 0.0, heading = number(value.w) or number(value.heading) or 0.0 }
end

-- One point or a list of up to 64 points, always as a list, or nil.
function RPPlaces.points(coords)
  local one = RPPlaces.point(coords)
  if one then return { one } end
  if type(coords) ~= 'table' or #coords == 0 or #coords > 64 then return nil end
  local list = {}
  for i, value in ipairs(coords) do
    list[i] = RPPlaces.point(value)
    if not list[i] then return nil end
  end
  return list
end

-- What the game needs to know about a place (no job rules, no callbacks).
function RPPlaces.public(place)
  return { label = place.label, blip = place.blip, colour = place.colour, radius = place.radius }
end
