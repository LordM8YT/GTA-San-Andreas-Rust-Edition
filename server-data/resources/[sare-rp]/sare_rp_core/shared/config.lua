-- SARE RP settings, shared by the server and the players' game.
-- Change prices, items and jobs here, then `restart sarebox_core` (which
-- restarts every RP resource) or restart the resource you changed.
RPConfig = {}

-- How close (metres) a player must stand to a location to use it.
RPConfig.InteractRadius = 2.5
-- Key for "use what is in front of me" (one of the keys SARE lets scripts bind).
RPConfig.InteractKey = 'H'
RPConfig.InventoryKey = 'K'
RPConfig.JobMenuKey = 'F2'

-- What each location on the map is. `job` limits it to that job, `duty`
-- also to players on duty.
RPConfig.Places = {
  bank = { label = 'Bank', blip = 'Bank', colour = 2 },
  atms = { label = 'ATM' },
  shop_247 = { label = '24/7 shop', blip = '24/7', colour = 47 },
  shop_gas = { label = 'Gas station shop', blip = 'Gas station', colour = 47 },
  city_hall = { label = 'City Hall', blip = 'City Hall', colour = 0 },
  police_duty = { label = 'Police front desk', blip = 'Police', colour = 3 },
  police_armory = { label = 'Police lockers', job = 'police', duty = true },
  police_garage = { label = 'Police vehicles', job = 'police', duty = true },
  hospital_checkin = { label = 'Hospital reception', blip = 'Hospital', colour = 1 },
  hospital_duty = { label = 'EMS lockers', job = 'ambulance' },
  hospital_garage = { label = 'Ambulance bay', job = 'ambulance', duty = true },
  dealer = { label = 'Premium Motors', blip = 'Car dealer', colour = 27 },
  garage = { label = 'Public garage', blip = 'Garage', colour = 27 },
  mechanic = { label = 'Mechanic workshop', blip = 'Mechanic', colour = 5 },
  mechanic_duty = { label = 'Mechanic lockers', job = 'mechanic' },
  taxi_depot = { label = 'Taxi depot', blip = 'Taxi', colour = 5 },
}

-- Items for the inventory and the shops. `weight` in grams.
RPConfig.MaxWeight = 30000
RPConfig.Items = {
  water = { label = 'Water', weight = 500, use = 'drinks some water' },
  coffee = { label = 'Coffee', weight = 300, use = 'sips a hot coffee' },
  sandwich = { label = 'Sandwich', weight = 300, use = 'eats a sandwich' },
  donut = { label = 'Donut', weight = 150, use = 'eats a donut' },
  bandage = { label = 'Bandage', weight = 100, use = 'wraps a bandage around the wound' },
  phone = { label = 'Phone', weight = 200, use = 'phone' },
  radio = { label = 'Radio', weight = 400, use = 'speaks into the radio' },
  repairkit = { label = 'Repair kit', weight = 2500, use = 'opens a repair kit and gets to work' },
  lockpick = { label = 'Lockpick', weight = 100, use = 'fiddles with a lockpick', illegal = true },
  handcuffs = { label = 'Handcuffs', weight = 500 },
  medkit = { label = 'Medkit', weight = 1500, use = 'opens a medkit' },
  fuelcan = { label = 'Fuel can', weight = 4000, use = 'pours fuel into the tank' },
}

-- What each shop sells, and for how much (whole dollars).
RPConfig.Shops = {
  shop_247 = {
    { item = 'water', price = 2 }, { item = 'coffee', price = 3 }, { item = 'sandwich', price = 6 },
    { item = 'donut', price = 2 }, { item = 'bandage', price = 15 }, { item = 'phone', price = 250 },
    { item = 'lockpick', price = 80 },
  },
  shop_gas = {
    { item = 'water', price = 3 }, { item = 'coffee', price = 4 }, { item = 'sandwich', price = 7 },
    { item = 'fuelcan', price = 40 }, { item = 'repairkit', price = 300 },
  },
}

-- Gear a job takes from its lockers, once per visit.
RPConfig.JobGear = {
  police = { radio = 1, handcuffs = 1, bandage = 2 },
  ambulance = { radio = 1, medkit = 2, bandage = 10 },
  mechanic = { repairkit = 2 },
}

-- Jobs anyone can take at City Hall. police and ambulance are hired by an
-- admin: /setjob <id> police 0
RPConfig.OpenJobs = { 'unemployed', 'taxi', 'mechanic' }

-- Vehicles. SARE content packs have one car model so far, SARE Kart; with
-- your own San Andreas copy, players can use SA model names (`infernus`).
RPConfig.Dealer = {
  { model = 'SARE Kart', label = 'SARE Kart', price = 6500 },
  -- { model = 'infernus', label = 'Infernus', price = 95000 },  -- needs the player's own SA copy
}
RPConfig.JobVehicle = { police = 'SARE Kart', ambulance = 'SARE Kart', mechanic = 'SARE Kart', taxi = 'SARE Kart' }

RPConfig.Prices = {
  identityChange = 250,
  hospitalTreatment = 150,
  advert = 50,
}

-- Chat ranges in metres.
RPConfig.Chat = { say = 15.0, me = 20.0, shout = 35.0 }

-- Longest jail sentence in minutes, and how far a prisoner may walk.
RPConfig.Jail = { maxMinutes = 60, radius = 6.0 }
