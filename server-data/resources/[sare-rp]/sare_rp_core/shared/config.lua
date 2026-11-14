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
  clothes = { label = 'Clothes shop', blip = 'Clothes', colour = 8 },
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

-- Vehicles. Models are the SARE RP Town content pack's vehicles (see
-- content/sare-rp-town/content.json); with your own San Andreas copy you
-- can also use SA model names (`infernus`).
RPConfig.Dealer = {
  { model = 'compact', label = 'Compact', price = 4500 },
  { model = 'sedan', label = 'Sedan', price = 9000 },
  { model = 'pickup', label = 'Pickup', price = 14000 },
  { model = 'suv', label = 'SUV', price = 16500 },
  { model = 'sports', label = 'Sports car', price = 42000 },
}
-- Paint the dealer offers (sRGB).
RPConfig.CarColours = {
  { label = 'Midnight black', rgb = { 25, 25, 30 } }, { label = 'Arctic white', rgb = { 235, 235, 235 } },
  { label = 'Silver', rgb = { 170, 172, 178 } }, { label = 'Ocean blue', rgb = { 50, 90, 170 } },
  { label = 'Racing red', rgb = { 190, 35, 35 } }, { label = 'Forest green', rgb = { 40, 105, 60 } },
  { label = 'Sunset orange', rgb = { 230, 120, 30 } }, { label = 'Canary yellow', rgb = { 240, 200, 40 } },
  { label = 'Plum', rgb = { 105, 50, 120 } }, { label = 'Sand', rgb = { 200, 180, 140 } },
}
-- The vehicle each job takes from its garage, already in work colours.
RPConfig.JobVehicle = { police = 'police', ambulance = 'ambulance', mechanic = 'towtruck', taxi = 'taxi' }

-- Characters (content packs with characters, like SARE RP Town). Style
-- numbers match the pack's characters.glb; see tools/blender/make_rp_characters.py.
RPConfig.Looks = {
  bodies = { 'Male', 'Female' },
  hair = { 'Buzz cut', 'Short', 'Quiff', 'Long', 'Ponytail', 'Afro', 'Bun' },
  top = { 'T-shirt', 'Long sleeve', 'Hoodie', 'Tank top', 'Jacket', 'Suit' },
  pants = { 'Jeans', 'Shorts', 'Cargo', 'Skirt' },
  shoes = { 'Sneakers', 'Boots', 'Dress shoes' },
  skin = {
    { label = 'Porcelain', rgb = { 245, 215, 190 } }, { label = 'Light', rgb = { 232, 190, 155 } },
    { label = 'Tan', rgb = { 214, 160, 120 } }, { label = 'Olive', rgb = { 190, 140, 95 } },
    { label = 'Brown', rgb = { 150, 100, 65 } }, { label = 'Dark', rgb = { 105, 70, 45 } },
    { label = 'Deep', rgb = { 70, 45, 30 } },
  },
  hairColours = {
    { label = 'Black', rgb = { 22, 20, 18 } }, { label = 'Dark brown', rgb = { 58, 40, 28 } },
    { label = 'Brown', rgb = { 100, 65, 40 } }, { label = 'Auburn', rgb = { 130, 55, 30 } },
    { label = 'Blonde', rgb = { 215, 180, 110 } }, { label = 'Grey', rgb = { 150, 150, 150 } },
    { label = 'Red', rgb = { 170, 50, 30 } }, { label = 'Blue', rgb = { 60, 90, 200 } },
    { label = 'Pink', rgb = { 220, 110, 170 } },
  },
  colours = {
    { label = 'Black', rgb = { 28, 28, 32 } }, { label = 'White', rgb = { 235, 235, 235 } },
    { label = 'Grey', rgb = { 130, 130, 135 } }, { label = 'Navy', rgb = { 35, 50, 90 } },
    { label = 'Blue', rgb = { 60, 110, 190 } }, { label = 'Denim', rgb = { 52, 62, 84 } },
    { label = 'Red', rgb = { 180, 40, 40 } }, { label = 'Green', rgb = { 50, 120, 60 } },
    { label = 'Yellow', rgb = { 230, 190, 50 } }, { label = 'Brown', rgb = { 110, 75, 45 } },
    { label = 'Khaki', rgb = { 170, 155, 110 } }, { label = 'Pink', rgb = { 220, 120, 160 } },
    { label = 'Purple', rgb = { 110, 60, 150 } }, { label = 'Orange', rgb = { 220, 120, 40 } },
  },
}

RPConfig.Prices = {
  identityChange = 250,
  hospitalTreatment = 150,
  advert = 50,
  clothes = 120,
}

-- Chat ranges in metres.
RPConfig.Chat = { say = 15.0, me = 20.0, shout = 35.0 }

-- Longest jail sentence in minutes, and how far a prisoner may walk.
RPConfig.Jail = { maxMinutes = 60, radius = 6.0 }
