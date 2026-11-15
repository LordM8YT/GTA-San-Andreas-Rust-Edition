-- Player objects and their storage. Characters are saved with resource KVP
-- (kvp/sarebox_core.json next to server.cfg), keyed by the player's license.

local Players = {}
local Core = { Functions = {}, Config = Config }
SareBox = Core

local function kvpKey(citizenid) return ('player:%s'):format(citizenid) end

-- Money amounts are whole dollars. Anything else (fractions, NaN, infinity,
-- strings that are not numbers, huge values) is refused, whoever asks.
local function wholeAmount(value, allowZero)
  local n = tonumber(value)
  if not n or n ~= n or n == math.huge or n == -math.huge then return nil end
  if n ~= math.floor(n) or n > Config.MaxMoney then return nil end
  if n < 0 or (n == 0 and not allowZero) then return nil end
  return math.tointeger(n)
end
local function validAccount(data, account) return type(account) == 'string' and type(data.money[account]) == 'number' end

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

  -- Saves into resource KVP. The server writes KVP to disk within about a
  -- second, atomically (see docs/sarebox.md, "When data is saved").
  function self.Functions.Save()
    if not data.license then return false end
    if DoesPlayerExist(src) then
      local coords = GetEntityCoords(GetPlayerPed(src))
      if coords and (coords.x ~= 0 or coords.y ~= 0) then
        data.position = { x = coords.x, y = coords.y, z = coords.z, heading = GetEntityHeading(GetPlayerPed(src)) }
      end
    end
    local copy = table.clone(data)
    copy.source = nil
    local ok, err = pcall(SetResourceKvp, kvpKey(data.citizenid), json.encode(copy))
    if not ok then
      print(('^1Could not save character %s: %s^7'):format(data.citizenid, err))
      return false
    end
    return true
  end

  function self.Functions.GetMoney(account) return data.money[account] end

  local function changeMoney(account, value, reason, sign)
    local amount = wholeAmount(value, false)
    if not amount or not validAccount(data, account) then return false end
    local new = data.money[account] + sign * amount
    if new < 0 or new > Config.MaxMoney then return false end
    data.money[account] = new
    sync()
    self.Functions.Save()
    TriggerEvent('sarebox:server:moneyChanged', src, account, sign * amount, reason or 'unknown')
    TriggerClientEvent('sarebox:client:moneyChanged', src, account, sign * amount)
    return true
  end
  function self.Functions.AddMoney(account, amount, reason) return changeMoney(account, amount, reason, 1) end
  function self.Functions.RemoveMoney(account, amount, reason) return changeMoney(account, amount, reason, -1) end
  function self.Functions.SetMoney(account, value, reason)
    local amount = wholeAmount(value, true)
    if not amount or not validAccount(data, account) then return false end
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

-- Jobs from other resources, like QBCore's exports['qb-core']:AddJob: a
-- custom job lives in its own resource and adds itself when it starts.
--   exports.sarebox:AddJob('fisher', { label = 'Fisher', defaultDuty = true,
--     grades = { [0] = { name = 'Deckhand', payment = 40 } } })
-- Returns true, or false and the reason. Adding a job again replaces it.
local function cleanJob(name, job)
  if type(name) ~= 'string' or #name > 32 or not name:match('^[%w_%-]+$') then
    return nil, 'job names are 1-32 letters, numbers, _ or -'
  end
  if type(job) ~= 'table' or type(job.label) ~= 'string' or job.label == '' then return nil, 'the job needs a label' end
  if type(job.grades) ~= 'table' then return nil, 'the job needs grades' end
  local grades = {}
  for level, grade in pairs(job.grades) do
    -- Exports and JSON may hand grade numbers over as strings.
    local n = tonumber(level)
    if not n or n < 0 or n > 100 or n ~= math.floor(n) then return nil, ('grade %s is not a whole number'):format(tostring(level)) end
    if type(grade) ~= 'table' or type(grade.name) ~= 'string' then return nil, ('grade %d needs a name'):format(n) end
    local payment = tonumber(grade.payment) or 0
    if payment < 0 or payment > Config.MaxMoney or payment ~= math.floor(payment) then
      return nil, ('grade %d has an invalid payment'):format(n)
    end
    grades[math.tointeger(n)] = { name = grade.name, payment = math.tointeger(payment), isboss = grade.isboss == true }
  end
  if not grades[0] then return nil, 'the job needs grade 0, where new hires start' end
  return { label = job.label, defaultDuty = job.defaultDuty == true, grades = grades }
end
function Core.Functions.AddJob(name, job)
  local clean, err = cleanJob(name, job)
  if not clean then
    print(('^3[sarebox] AddJob(%s) refused: %s^7'):format(tostring(name), err))
    return false, err
  end
  Config.Jobs[name] = clean
  -- Players who logged in before the job existed get it back now.
  for _, player in pairs(Players) do
    local waiting = player.PlayerData.metadata.waitingJob
    if type(waiting) == 'table' and waiting.name == name then
      player.PlayerData.metadata.waitingJob = nil
      if not player.Functions.SetJob(name, waiting.grade) then player.Functions.SetJob(name, 0) end
    end
  end
  TriggerEvent('sarebox:server:jobAdded', name, clean)
  return true
end
function Core.Functions.AddJobs(jobs)
  if type(jobs) ~= 'table' then return false, 'AddJobs takes a table of jobs' end
  local failed = {}
  for name, job in pairs(jobs) do
    local ok, err = Core.Functions.AddJob(name, job)
    if not ok then failed[#failed + 1] = ('%s: %s'):format(tostring(name), err) end
  end
  if #failed > 0 then return false, table.concat(failed, '; ') end
  return true
end
function Core.Functions.Notify(src, text, kind, title)
  TriggerClientEvent('sare:ui:notify', src, { title = title or 'SARE Box', description = text, type = kind or 'inform' })
end

local function licenseKey(license) return ('license:%s'):format(license) end

local function loadSaved(src, citizenid)
  local saved = citizenid and GetResourceKvpString(kvpKey(citizenid))
  local ok, data = pcall(json.decode, saved or 'null')
  if not ok or type(data) ~= 'table' then return nil end
  data.name = GetPlayerName(src)
  data.metadata = type(data.metadata) == 'table' and data.metadata or {}
  local savedJob = type(data.job) == 'table' and data.job or {}
  local savedGrade = type(savedJob.grade) == 'table' and savedJob.grade.level or 0
  data.job = jobData(savedJob.name, savedGrade)
  if data.job then
    data.metadata.waitingJob = nil
  else
    -- A job from a resource that has not started (yet): work as unemployed
    -- and get the job back when its resource calls AddJob.
    if type(savedJob.name) == 'string' and savedJob.name ~= 'unemployed' then
      data.metadata.waitingJob = { name = savedJob.name, grade = tonumber(savedGrade) or 0 }
    end
    data.job = jobData('unemployed', 0)
  end
  data.money = type(data.money) == 'table' and data.money or {}
  for name, start in pairs(Config.StartMoney) do
    if type(data.money[name]) ~= 'number' then data.money[name] = start end
  end
  return data
end

-- Characters from before signed identities are saved under the old,
-- unsigned license. A game reports that old license as its `legacy:`
-- identifier, but anyone who has seen a license (in server.cfg, the console
-- or /id) could report it too, so it is only a claim. `sarebox_migration`:
--   admin (default): an admin approves each claim with /sarebox_migrate.
--   auto: the first signed player to claim an old character gets it.
--   off:  old characters are not offered.
Core.Pending = {}
local function migrationMode()
  local mode = GetConvar('sarebox_migration', 'admin')
  if mode == 'auto' or mode == 'off' then return mode end
  return 'admin'
end
-- Licenses are stored as full identifiers (`license:<hex>`), as before.
-- A game's `legacy:<hex>` is the old `license:<hex>` it had here.
function Core.Functions.LegacyClaim(src)
  local legacy = GetPlayerIdentifierByType(src, 'legacy')
  local old = legacy and ('license:' .. legacy:gsub('^legacy:', ''))
  local citizenid = old and GetResourceKvpString(licenseKey(old))
  if citizenid and GetResourceKvpString(kvpKey(citizenid)) then return old, citizenid end
end
-- Move an old character to the player's signed license. The old license is
-- removed, so the same character cannot be claimed again. With `replace`, a
-- character the player already has here is unlinked (kept in KVP).
function Core.Functions.MigrateCharacter(license, old, citizenid, replace)
  if GetResourceKvpString(licenseKey(old)) ~= citizenid then return false end
  local current = GetResourceKvpString(licenseKey(license))
  if current and not replace then return false end
  SetResourceKvp(licenseKey(license), citizenid)
  DeleteResourceKvp(licenseKey(old))
  print(('[sarebox] Character %s moved from %s to %s%s'):format(citizenid, old, license,
    current and (' (unlinked character %s)'):format(current) or ''))
  return true
end

function Core.Functions.NewCharacter(src, license)
  local citizenid = newCitizenId()
  local data = defaults(src, license, citizenid)
  SetResourceKvp(kvpKey(citizenid), json.encode(data))
  SetResourceKvp(licenseKey(license), citizenid)
  return data
end

-- Load the character saved for this license, or create one. Players without
-- a license (an old game build) get a character that is not saved.
function Core.Functions.Login(src)
  -- Repeated joins or a restart racing a join keep the loaded character.
  if Players[src] then return Players[src] end
  local license = GetPlayerIdentifierByType(src, 'license')
  local data
  if license then
    local citizenid = GetResourceKvpString(licenseKey(license))
    data = loadSaved(src, citizenid)
    if not data and not citizenid then
      local legacy, old = Core.Functions.LegacyClaim(src)
      local mode = migrationMode()
      if legacy and mode == 'auto' and Core.Functions.MigrateCharacter(license, legacy, old) then
        citizenid = old
        data = loadSaved(src, citizenid)
      elseif legacy and mode == 'admin' then
        -- Play as an unsaved guest until an admin decides.
        Core.Pending[src] = { license = license, legacy = legacy, citizenid = old }
        local ok, saved = pcall(json.decode, GetResourceKvpString(kvpKey(old)))
        local oldName = ok and type(saved) == 'table' and saved.name or '?'
        print(('[sarebox] %s (%d) claims the old character %s (last saved as %s). Check that it is really them, then: sarebox_migrate %d (or sarebox_migrate %d reject)')
          :format(GetPlayerName(src), src, old, oldName, src, src))
        data = defaults(src, nil, 'GUEST' .. src)
        data.pendingMigration = old
      end
    end
    if data and not data.pendingMigration then
      data.license = license
    elseif not data then
      data = Core.Functions.NewCharacter(src, license)
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
  Core.Pending[src] = nil
  local player = Players[src]
  if not player then return end
  player.Functions.Save()
  Players[src] = nil
end
