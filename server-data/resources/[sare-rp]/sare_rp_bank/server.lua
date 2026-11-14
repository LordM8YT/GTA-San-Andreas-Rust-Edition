-- Bank and ATMs. Every action checks again that the player stands at a
-- bank counter or an ATM, and SARE Box refuses invalid amounts and
-- balances below zero.
local Core = exports.sarebox:GetCoreObject()
local RP = exports.sare_rp_core:GetRP()

local function money(n) return ('$%d'):format(math.floor(n or 0)) end

local function atBank(src)
  return RP.Near(src, 'bank') or RP.Near(src, 'atms')
end

local function openMenu(src)
  local player = Core.Functions.GetPlayer(src)
  if not player then return end
  local m = player.PlayerData.money
  local atm = not RP.Near(src, 'bank')
  RP.Menu(src, atm and 'ATM' or 'Bank', {
    { title = 'Bank ' .. money(m.bank), description = 'Cash ' .. money(m.cash) .. '  |  account ' .. player.PlayerData.citizenid, disabled = true },
    { title = 'Deposit', description = 'Cash into your account', serverEvent = 'sare_rp_bank:server:action', args = 'deposit' },
    { title = 'Withdraw', description = 'Cash from your account', serverEvent = 'sare_rp_bank:server:action', args = 'withdraw' },
    { title = 'Transfer', description = 'To another citizen ID or server ID', serverEvent = 'sare_rp_bank:server:action', args = 'transfer' },
  })
end

AddEventHandler('sare_rp:interact', function(src, key)
  if key == 'bank' or key == 'atms' then openMenu(src) end
end)

local function target(value)
  local id = tonumber(value)
  if id and Core.Functions.GetPlayer(id) then return Core.Functions.GetPlayer(id) end
  if type(value) == 'string' then return Core.Functions.GetPlayerByCitizenId(value:upper()) end
end

RegisterNetEvent('sare_rp_bank:server:action', function(action)
  local src = source
  if not atBank(src) then return RP.Notify(src, 'Go to the bank or an ATM.', 'error', 'Bank') end
  if action == 'deposit' or action == 'withdraw' then
    RP.Dialog(src, action == 'deposit' and 'Deposit' or 'Withdraw', {
      { type = 'number', label = 'Amount ($)', required = true },
    }, function(values)
      if not values or not atBank(src) then return end
      local amount = RP.Amount(values[1])
      local player = Core.Functions.GetPlayer(src)
      if not amount or not player then return RP.Notify(src, 'Use a whole number of dollars.', 'error', 'Bank') end
      local from, to = 'cash', 'bank'
      if action == 'withdraw' then from, to = 'bank', 'cash' end
      if not player.Functions.RemoveMoney(from, amount, 'bank ' .. action) then
        return RP.Notify(src, ('Not enough %s.'):format(from), 'error', 'Bank')
      end
      if not player.Functions.AddMoney(to, amount, 'bank ' .. action) then
        player.Functions.AddMoney(from, amount, 'bank refund')
        return RP.Notify(src, 'That is more than an account can hold.', 'error', 'Bank')
      end
      RP.Notify(src, ('%s %s.'):format(action == 'deposit' and 'Deposited' or 'Withdrew', money(amount)), 'success', 'Bank')
    end)
  elseif action == 'transfer' then
    RP.Dialog(src, 'Transfer', {
      { type = 'input', label = 'Citizen ID or server ID', required = true },
      { type = 'number', label = 'Amount ($)', required = true },
    }, function(values)
      if not values or not atBank(src) then return end
      local amount = RP.Amount(values[2])
      local player, other = Core.Functions.GetPlayer(src), target(values[1])
      if not amount or not player then return RP.Notify(src, 'Use a whole number of dollars.', 'error', 'Bank') end
      if not other then return RP.Notify(src, 'No citizen with that ID is in town.', 'error', 'Bank') end
      if other.PlayerData.source == src then return RP.Notify(src, 'You cannot transfer to yourself.', 'error', 'Bank') end
      if not player.Functions.RemoveMoney('bank', amount, 'transfer to ' .. other.PlayerData.citizenid) then
        return RP.Notify(src, 'Not enough money in your account.', 'error', 'Bank')
      end
      if not other.Functions.AddMoney('bank', amount, 'transfer from ' .. player.PlayerData.citizenid) then
        player.Functions.AddMoney('bank', amount, 'transfer refund')
        return RP.Notify(src, 'The transfer failed.', 'error', 'Bank')
      end
      RP.Notify(src, ('Sent %s to %s.'):format(money(amount), RP.Name(other.PlayerData.source)), 'success', 'Bank')
      RP.Notify(other.PlayerData.source, ('%s sent you %s.'):format(RP.Name(src), money(amount)), 'success', 'Bank')
      print(('[sare_rp_bank] %s -> %s: $%d'):format(player.PlayerData.citizenid, other.PlayerData.citizenid, amount))
    end)
  end
end)
