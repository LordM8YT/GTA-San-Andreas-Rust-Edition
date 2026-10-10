local M = {}

-- Rounds to `places` decimals; whole numbers stay integers.
function M.round(value, places)
  if not places or places == 0 then return math.floor(value + 0.5) end
  local scale = 10 ^ places
  return math.floor(value * scale + 0.5) / scale
end

return M
