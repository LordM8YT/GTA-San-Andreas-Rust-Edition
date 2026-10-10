-- A minimal framework core in the shape of QBCore/Qbox: one player object
-- per connected player, with functions other resources call through exports.
local Players = {}
local Core = { Functions = {} }

local function notify(src, message)
  TriggerClientEvent('chat:addMessage', src, { color = { 120, 200, 255 }, args = { 'Core', message } })
end

local function updatePlayerCount()
  local count = 0
  for _ in pairs(Players) do count = count + 1 end
  GlobalState.players = count
end

local function createPlayer(src)
  local self = {
    source = src,
    name = GetPlayerName(src),
    money = { cash = GetConvarInt('sare_core_startCash', 500), bank = 0 },
    job = 'unemployed',
    Functions = {},
  }
  local state = Player(src).state

  function self.Functions.GetMoney(account)
    return self.money[account]
  end

  function self.Functions.AddMoney(account, amount)
    amount = lib.math.round(tonumber(amount) or 0)
    if amount <= 0 or not self.money[account] then return false end
    self.money[account] = self.money[account] + amount
    state:set(account, self.money[account], true)
    TriggerEvent('sare_core:moneyChanged', src, account, self.money[account])
    return true
  end

  function self.Functions.RemoveMoney(account, amount)
    amount = lib.math.round(tonumber(amount) or 0)
    if amount <= 0 or (self.money[account] or 0) < amount then return false end
    self.money[account] = self.money[account] - amount
    state:set(account, self.money[account], true)
    TriggerEvent('sare_core:moneyChanged', src, account, self.money[account])
    return true
  end

  function self.Functions.SetJob(job)
    self.job = job
    state:set('job', job, true)
    return true
  end

  state:set('cash', self.money.cash, true)
  state:set('job', self.job, true)
  return self
end

function Core.Functions.GetPlayer(src)
  return Players[tonumber(src)]
end

function Core.Functions.GetPlayers()
  local list = {}
  for src in pairs(Players) do list[#list + 1] = src end
  return list
end

exports('GetCoreObject', function() return Core end)
exports('GetPlayer', Core.Functions.GetPlayer)

lib.callback.register('sare_core:getMoney', function(src)
  local player = Players[tonumber(src)]
  return player and player.money
end)

-- Reject names listed in `set sare_core_bannedNames "a,b"`.
AddEventHandler('playerConnecting', function(name, setKickReason, deferrals)
  deferrals.defer()
  Wait(0)
  for banned in GetConvar('sare_core_bannedNames', ''):gmatch('[^,%s]+') do
    if banned:lower() == name:lower() then
      deferrals.done(('%s is not allowed on this server.'):format(name))
      return
    end
  end
  deferrals.done()
end)

AddEventHandler('playerJoining', function()
  local src = source
  Players[src] = createPlayer(src)
  updatePlayerCount()
  TriggerEvent('sare_core:playerLoaded', src)
end)

AddEventHandler('playerDropped', function()
  Players[source] = nil
  updatePlayerCount()
end)

RegisterCommand('money', function(src)
  local player = Players[src]
  if player then
    notify(src, ('Cash: $%d, bank: $%d'):format(player.money.cash, player.money.bank))
  end
end, false)
