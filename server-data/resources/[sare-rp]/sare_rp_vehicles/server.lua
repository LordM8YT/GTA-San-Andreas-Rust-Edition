-- Owned vehicles (saved in `metadata.vehicles`) and server-spawned cars.
-- Cars taken out are server entities: everyone in the same instance sees
-- them. Walk up and press F to drive: the server checks the car is yours
-- (or your job's) through `sare:enteringVehicle`, and everyone sees you
-- drive it. Leave it anywhere; it stays where you parked it.
local Core = exports.sarebox:GetCoreObject()
local RP = exports.sare_rp_core:GetRP()

local Out = {}  -- [server id] = { handle, plate }
local Plates = {}

local function vehicles(src)
  local player = Core.Functions.GetPlayer(src)
  local list = player and player.PlayerData.metadata.vehicles
  return type(list) == 'table' and list or {}, player
end

local function newPlate()
  local chars = 'ABCDEFGHJKLMNPRSTUVWXYZ0123456789'
  while true do
    local plate = {}
    for i = 1, 7 do
      local n = math.random(#chars)
      plate[i] = chars:sub(n, n)
    end
    plate = table.concat(plate)
    if not Plates[plate] and not GetResourceKvpString('plate:' .. plate) then return plate end
  end
end

local function despawn(src)
  local out = Out[src]
  if out and DoesEntityExist(out.handle) then DeleteEntity(out.handle) end
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
  Out[src] = { handle = handle, plate = plate }
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
  Plates[plate] = true
  SetResourceKvp('plate:' .. plate, player.PlayerData.citizenid)
  local rgb = colour and { colour.rgb[1], colour.rgb[2], colour.rgb[3] } or nil
  list[#list + 1] = { plate = plate, model = car.model, label = car.label, colour = rgb }
  player.Functions.SetMetaData('vehicles', list)
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
local function SpawnJobVehicle(src, job, point)
  local model = RPConfig.JobVehicle[job]
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
