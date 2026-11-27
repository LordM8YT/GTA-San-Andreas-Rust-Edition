-- Owned vehicles (the `player_vehicles` table, see migrations/) and
-- server-spawned cars.
-- Cars taken out are server entities: everyone in the same instance sees
-- them. Walk up and press F to drive: the server checks the car is yours
-- (or your job's) through `sare:enteringVehicle`, and everyone sees you
-- drive it. Leave it anywhere; it stays where you parked it.
local Core = exports.sarebox:GetCoreObject()
local RP = exports.sare_rp_core:GetRP()

local Out = {}  -- [server id] = { handle, plate, owned }

-- The player's cars as { plate, model, label, colour }, oldest first.
local function vehicles(src)
  local player = Core.Functions.GetPlayer(src)
  local list = {}
  if not player or not player.PlayerData.license then return list, player end
  local rows = MySQL.query.await('SELECT * FROM player_vehicles WHERE citizenid = ? ORDER BY id', { player.PlayerData.citizenid })
  for _, row in ipairs(rows or {}) do
    local ok, mods = pcall(json.decode, row.mods or '{}')
    list[#list + 1] = { plate = row.plate, model = row.vehicle, label = row.label or row.vehicle,
      colour = ok and type(mods) == 'table' and mods.colour or nil }
  end
  return list, player
end

local function setState(plate, state)
  MySQL.update.await('UPDATE player_vehicles SET state = ? WHERE plate = ?', { state, plate })
end

-- Servers from before the table kept cars in each character's
-- `metadata.vehicles`. Move them into the table (and out of metadata) on
-- start; plates already in the table are kept as they are.
local function importMetadataCars()
  local rows = MySQL.query.await([[SELECT citizenid, json_extract(metadata, '$.vehicles') AS vehicles FROM players
    WHERE json_type(metadata, '$.vehicles') = 'array']])
  local moved = 0
  for _, row in ipairs(rows or {}) do
    local ok, list = pcall(json.decode, row.vehicles)
    local queries = {}
    for _, car in ipairs(ok and type(list) == 'table' and list or {}) do
      if type(car) == 'table' and type(car.plate) == 'string' and type(car.model) == 'string' then
        queries[#queries + 1] = { query = 'INSERT OR IGNORE INTO player_vehicles (citizenid, vehicle, label, plate, mods) VALUES (?, ?, ?, ?, ?)',
          values = { row.citizenid, car.model, car.label, car.plate, json.encode({ colour = car.colour }) } }
        moved = moved + 1
      end
    end
    queries[#queries + 1] = { query = [[UPDATE players SET metadata = json_remove(metadata, '$.vehicles') WHERE citizenid = ?]],
      values = { row.citizenid } }
    if not MySQL.transaction.await(queries) then
      print(('^1[sare_rp_vehicles] Could not move the cars of %s into player_vehicles.^7'):format(row.citizenid))
    end
  end
  -- Loaded characters would save the old list again.
  for _, src in ipairs(Core.Functions.GetPlayers()) do
    local player = Core.Functions.GetPlayer(src)
    if player and player.PlayerData.metadata.vehicles ~= nil then player.Functions.SetMetaData('vehicles', nil) end
  end
  if moved > 0 then print(('[sare_rp_vehicles] Moved %d car(s) from character metadata into player_vehicles.'):format(moved)) end
end
importMetadataCars()
-- Cars that were out when the server stopped are back in their garage.
MySQL.update.await('UPDATE player_vehicles SET state = 1 WHERE state = 0')

local function newPlate()
  local chars = 'ABCDEFGHJKLMNPRSTUVWXYZ0123456789'
  while true do
    local plate = {}
    for i = 1, 7 do
      local n = math.random(#chars)
      plate[i] = chars:sub(n, n)
    end
    plate = table.concat(plate)
    if not MySQL.scalar.await('SELECT 1 FROM player_vehicles WHERE plate = ?', { plate }) then return plate end
  end
end

local function despawn(src)
  local out = Out[src]
  if out and DoesEntityExist(out.handle) then DeleteEntity(out.handle) end
  if out and out.owned then setState(out.plate, 1) end
  Out[src] = nil
end

-- Spawn a car for `src` at a map point; replaces the car they had out.
local function spawn(src, model, point, plate, kind, colour)
  despawn(src)
  -- Entity model names cannot hold spaces; a hash of a catalog name
  -- resolves in every client, like a model file name would.
  if type(model) == 'string' and model:find(' ') then model = GetHashKey(model) end
  local handle = CreateVehicle(model, point.x, point.y, point.z, point.heading or 0.0, true, true)
  if not handle or handle == 0 then return nil end
  SetEntityRoutingBucket(handle, GetPlayerRoutingBucket(src))
  if type(colour) == 'table' and #colour == 3 then
    SetVehicleCustomPrimaryColour(handle, colour[1], colour[2], colour[3])
  end
  local state = Entity(handle).state
  state:set('owner', src, true)
  state:set('plate', plate, true)
  state:set('kind', kind, true)
  Out[src] = { handle = handle, plate = plate, owned = kind == 'owned' }
  if kind == 'owned' then setState(plate, 0) end
  return handle
end

-- Who may drive a car: its owner, or anyone on duty in the job it belongs
-- to. Cars without a kind (other resources' vehicles) are open to all.
AddEventHandler('sare:enteringVehicle', function(handle)
  local src = source
  local state = Entity(handle).state
  local kind = state.kind
  if not kind then return end
  if kind == 'owned' then
    if state.owner ~= src then
      CancelEvent()
      RP.Notify(src, ('Locked. This car [%s] belongs to someone else.'):format(state.plate or '?'), 'error', 'Vehicle')
    end
  elseif not RP.OnDuty(src, kind) then
    CancelEvent()
    RP.Notify(src, 'Only on-duty staff can drive this vehicle.', 'error', 'Vehicle')
  end
end)

local function dealerMenu(src)
  local options = {}
  for index, car in ipairs(RPConfig.Dealer) do
    options[#options + 1] = { title = ('%s  $%d'):format(car.label, car.price), description = 'Paid from your bank account. Pick a colour next.',
      serverEvent = 'sare_rp_vehicles:server:buy', args = index }
  end
  RP.Menu(src, 'Premium Motors', options)
end

local function garageMenu(src)
  local list = vehicles(src)
  local options = {}
  for index, car in ipairs(list) do
    local out = Out[src] and Out[src].plate == car.plate
    options[#options + 1] = { title = ('%s [%s]'):format(car.label, car.plate),
      description = out and 'Out now: pick to put it back' or 'Take it out',
      serverEvent = 'sare_rp_vehicles:server:garage', args = car.plate }
  end
  if #options == 0 then options[1] = { title = 'You own no vehicles. Visit Premium Motors.', disabled = true } end
  RP.Menu(src, 'Public garage', options)
end

AddEventHandler('sare_rp:interact', function(src, key)
  if key == 'dealer' then dealerMenu(src) elseif key == 'garage' then garageMenu(src) end
end)

local function buy(src, car, colour)
  if not RP.Near(src, 'dealer') then return RP.Notify(src, 'Talk to the dealer at Premium Motors.', 'error', 'Dealer') end
  local list, player = vehicles(src)
  if not player then return end
  if #list >= 10 then return RP.Notify(src, 'Your garage is full (10).', 'error', 'Dealer') end
  if not player.PlayerData.license then return RP.Notify(src, 'Buying needs a saved character.', 'error', 'Dealer') end
  if not player.Functions.RemoveMoney('bank', car.price, 'vehicle ' .. car.label) then
    return RP.Notify(src, ('You need $%d in your bank account.'):format(car.price), 'error', 'Dealer')
  end
  local plate = newPlate()
  local rgb = colour and { colour.rgb[1], colour.rgb[2], colour.rgb[3] } or nil
  local ok = pcall(MySQL.insert.await, 'INSERT INTO player_vehicles (citizenid, vehicle, label, plate, mods) VALUES (?, ?, ?, ?, ?)',
    { player.PlayerData.citizenid, car.model, car.label, plate, json.encode({ colour = rgb }) })
  if not ok then
    player.Functions.AddMoney('bank', car.price, 'vehicle refund')
    return RP.Notify(src, 'The dealer could not register the car. Your money is back.', 'error', 'Dealer')
  end
  spawn(src, car.model, RPLocations.dealer_delivery, plate, 'owned', rgb)
  RP.Notify(src, ('You bought the %s%s, plate %s. It waits outside: walk up and press F to drive. Later keep it at the public garage.'):format(
    colour and (colour.label:lower() .. ' ') or '', car.label, plate), 'success', 'Dealer')
  print(('[sare_rp_vehicles] %s bought %s (%s) for $%d'):format(player.PlayerData.citizenid, car.label, plate, car.price))
end

RegisterNetEvent('sare_rp_vehicles:server:buy', function(index)
  local src = source
  local car = RPConfig.Dealer[tonumber(index) or 0]
  if not car or not RP.Near(src, 'dealer') then return RP.Notify(src, 'Talk to the dealer at Premium Motors.', 'error', 'Dealer') end
  local options = {}
  for i, colour in ipairs(RPConfig.CarColours) do options[i] = { value = tostring(i), label = colour.label } end
  RP.Dialog(src, ('%s  $%d'):format(car.label, car.price), {
    { type = 'select', label = 'Colour', default = '1', options = options },
  }, function(values)
    if not values then return RP.Notify(src, 'Maybe next time.', 'inform', 'Dealer') end
    local colour = RPConfig.CarColours[tonumber(values[1]) or 0]
    if not colour then return RP.Notify(src, 'Pick one of the colours.', 'error', 'Dealer') end
    buy(src, car, colour)
  end)
end)

RegisterNetEvent('sare_rp_vehicles:server:garage', function(plate)
  local src = source
  if not RP.Near(src, 'garage') then return RP.Notify(src, 'Go to the public garage.', 'error', 'Garage') end
  for _, car in ipairs((vehicles(src))) do
    if car.plate == plate then
      if Out[src] and Out[src].plate == plate then
        despawn(src)
        return RP.Notify(src, ('%s [%s] is back in the garage.'):format(car.label, plate), 'success', 'Garage')
      end
      if spawn(src, car.model, RPLocations.garage_spawn, plate, 'owned', car.colour) then
        return RP.Notify(src, ('%s [%s] is parked outside.'):format(car.label, plate), 'success', 'Garage')
      end
      return RP.Notify(src, 'The garage could not bring that car.', 'error', 'Garage')
    end
  end
end)

-- Job vehicles: one per on-duty player, from their job's garage point.
-- The player is put at the wheel straight away.
-- A job's work vehicle at `point`, or back into the garage if one is out.
-- Job resources pass their own model (Config.Vehicle).
local function SpawnJobVehicle(src, job, point, model)
  model = type(model) == 'string' and model or (RPConfig.JobVehicle or {})[job]
  if not model or not RP.OnDuty(src, job) then return false end
  if Out[src] and Out[src].plate:sub(1, 3) == 'JOB' then
    despawn(src)
    RP.Notify(src, 'Job vehicle returned.', 'inform')
    return true
  end
  local handle = spawn(src, model, point, ('JOB%04d'):format(src % 10000), job)
  if handle then TaskWarpPedIntoVehicle(src, handle, -1) end
  return handle ~= nil
end
exports('SpawnJobVehicle', SpawnJobVehicle)
exports('Despawn', despawn)

AddEventHandler('playerDropped', function() despawn(source) end)
AddEventHandler('sarebox:server:onJobUpdate', function(src)
  if Out[src] and Out[src].plate:sub(1, 3) == 'JOB' then despawn(src) end
end)
