-- Routing buckets split the world into instances: players only see, share
-- traffic and ride with players in the same bucket. Bucket 0 is the public
-- world. Script events still reach every player.
local MAX_BUCKET = 9999

local function notify(src, message)
  TriggerClientEvent('chat:addMessage', src, { color = { 120, 200, 255 }, args = { 'Instance', message } })
end

local function SetInstance(src, bucket)
  bucket = math.tointeger(tonumber(bucket)) or 0
  if bucket < 0 or bucket > MAX_BUCKET then return false end
  local previous = GetPlayerRoutingBucket(src)
  if previous == bucket then return true end
  SetPlayerRoutingBucket(src, bucket)
  Player(src).state:set('instance', bucket, false)
  TriggerEvent('sare_instances:changed', src, bucket, previous)
  return true
end

local function GetInstance(src)
  return GetPlayerRoutingBucket(src)
end

exports('SetInstance', SetInstance)
exports('GetInstance', GetInstance)

-- /instance          a private instance of your own
-- /instance <number> join a shared instance, 0 for the public world
RegisterCommand('instance', function(src, args)
  if src == 0 then return end
  local bucket = args[1] and tonumber(args[1]) or (1000 + src)
  if not SetInstance(src, bucket) then
    notify(src, ('Use a number from 0 to %d.'):format(MAX_BUCKET))
    return
  end
  if GetPlayerRoutingBucket(src) == 0 then
    notify(src, 'You are back in the public world.')
  else
    notify(src, ('You are in instance %d. Use /instance 0 to return.'):format(GetPlayerRoutingBucket(src)))
  end
end, false)
