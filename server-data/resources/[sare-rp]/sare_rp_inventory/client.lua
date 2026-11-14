-- The inventory key. The server builds the menu from the saved inventory.
RegisterCommand('inventory', function()
  TriggerServerEvent('sare_rp_inventory:server:open')
end)
RegisterKeyMapping('inventory', 'Inventory', 'keyboard', RPConfig.InventoryKey)
