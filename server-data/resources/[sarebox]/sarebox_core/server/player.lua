-- Player objects and their storage. Characters are saved with resource KVP
-- (kvp/sarebox_core.json next to server.cfg), keyed by the player's license.

local Players = {}
local Core = { Functions = {}, Config = Config }
SareBox = Core

local function kvpKey(citizenid) return ('player:%s'):format(citizenid) end

local function newCitizenId()
  local chars = 'ABCDEFGHJKLMNPQRSTUVWXYZ23456789'
  while true do
    local id = {}
    for i = 1, 8 do
      local n = math.random(#chars)
      id[i] = chars:sub(n, n)
    end
    local citizenid = table.concat(id)
    if not GetResourceKvpString(kvpKey(citizenid)) then return citizenid end
  end
end

local function jobData(name, grade)
  local job = Config.Jobs[name]
  if not job then return nil end
  grade = tonumber(grade) or 0
  local info = job.grades[grade]
  if not info then return nil end
  return {
    name = name,
    label = job.label,
    payment = info.payment,
    onduty = job.defaultDuty == true,
    isboss = info.isboss == true,
    grade = { level = grade, name = info.name },
  }
end

local function defaults(src, license, citizenid)
  return {
    citizenid = citizenid,
    license = license,
    name = GetPlayerName(src),
    money = { cash = Config.StartMoney.cash, bank = Config.StartMoney.bank },
    job = jobData('unemployed', 0),
    metadata = {},
    position = nil,
  }
end

local function createPlayer(src, data)
  local self = { PlayerData = data, Functions = {}, Offline = false }
  self.PlayerData.source = src
  local state = Player(src).state

  local function sync()
    state:set('cash', data.money.cash, true)
    state:set('bank', data.money.bank, true)
    state:set('job', data.job.name, true)
    state:set('citizenid', data.citizenid, true)
    TriggerClientEvent('sarebox:client:setPlayerData', src, data)
  end

  function self.Functions.Save()
    if not data.license then return false end
    local coords = GetEntityCoords(GetPlayerPed(src))
    if coords and (coords.x ~= 0 or coords.y ~= 0) then
      data.position = { x = coords.x, y = coords.y, z = coords.z, heading = GetEntityHeading(GetPlayerPed(src)) }
    end
    local copy = table.clone(data)
    copy.source = nil
    SetResourceKvp(kvpKey(data.citizenid), json.encode(copy))
    return true
  end

  function self.Functions.GetMoney(account) return data.money[account] end

  local function changeMoney(account, amount, reason, sign)
    amount = math.floor(tonumber(amount) or 0)
    if amount <= 0 or data.money[account] == nil then return false end
    local new = data.money[account] + sign * amount
    if new < 0 then return false end
    data.money[account] = new
    sync()
    self.Functions.Save()
    TriggerEvent('sarebox:server:moneyChanged', src, account, sign * amount, reason or 'unknown')
    TriggerClientEvent('sarebox:client:moneyChanged', src, account, sign * amount)
    return true
  end
  function self.Functions.AddMoney(account, amount, reason) return changeMoney(account, amount, reason, 1) end
  function self.Functions.RemoveMoney(account, amount, reason) return changeMoney(account, amount, reason, -1) end
  function self.Functions.SetMoney(account, amount, reason)
    amount = math.floor(tonumber(amount) or -1)
    if amount < 0 or data.money[account] == nil then return false end
    local delta = amount - data.money[account]
    data.money[account] = amount
    sync()
    self.Functions.Save()
    TriggerEvent('sarebox:server:moneyChanged', src, account, delta, reason or 'set')
    return true
  end

  function self.Functions.SetJob(name, grade)
    local job = jobData(name, grade)
    if not job then return false end
    data.job = job
    sync()
    self.Functions.Save()
    TriggerEvent('sarebox:server:onJobUpdate', src, job)
    TriggerClientEvent('sarebox:client:onJobUpdate', src, job)
    return true
  end
  function self.Functions.SetJobDuty(onDuty)
    data.job.onduty = onDuty == true
    sync()
    TriggerClientEvent('sarebox:client:setDuty', src, data.job.onduty)
  end

  function self.Functions.SetMetaData(key, value)
    data.metadata[key] = value
    sync()
    self.Functions.Save()
  end
  function self.Functions.GetMetaData(key) return data.metadata[key] end

  sync()
  return self
end

function Core.Functions.GetPlayer(src) return Players[tonumber(src)] end
function Core.Functions.GetPlayerByCitizenId(citizenid)
  for _, player in pairs(Players) do
    if player.PlayerData.citizenid == citizenid then return player end
  end
end
function Core.Functions.GetPlayers()
  local list = {}
  for src in pairs(Players) do list[#list + 1] = src end
  table.sort(list)
  return list
end
function Core.Functions.GetJob(name) return Config.Jobs[name] end
function Core.Functions.Notify(src, text, kind, title)
  TriggerClientEvent('sare:ui:notify', src, { title = title or 'SARE Box', description = text, type = kind or 'inform' })
end

-- Load the character saved for this license, or create one. Players without
-- a license (an old game build) get a character that is not saved.
function Core.Functions.Login(src)
  local license = GetPlayerIdentifierByType(src, 'license')
  local data
  if license then
    local citizenid = GetResourceKvpString(('license:%s'):format(license))
    local saved = citizenid and GetResourceKvpString(kvpKey(citizenid))
    data = saved and json.decode(saved)
    if data then
      data.name = GetPlayerName(src)
      data.job = jobData(data.job and data.job.name, data.job and data.job.grade and data.job.grade.level)
        or jobData('unemployed', 0)
      data.metadata = data.metadata or {}
    else
      citizenid = newCitizenId()
      SetResourceKvp(('license:%s'):format(license), citizenid)
      data = defaults(src, license, citizenid)
    end
  else
    data = defaults(src, nil, 'GUEST' .. src)
  end
  local player = createPlayer(src, data)
  Players[src] = player
  player.Functions.Save()
  return player
end

function Core.Functions.Logout(src)
  local player = Players[src]
  if not player then return end
  player.Functions.Save()
  Players[src] = nil
end
