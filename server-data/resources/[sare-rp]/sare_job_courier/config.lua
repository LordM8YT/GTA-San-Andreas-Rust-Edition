-- Courier job settings. Everything about the job is in this folder: no
-- core files to edit. Positions are San Andreas world coordinates; stand
-- there in game and type /coords, or select an object in Build Mode and
-- press "Copy position", then paste the vector here.
Config = {}

-- The job itself, the same shape as QBCore's shared jobs. Grade 0 is where
-- new hires start; payment is the paycheck while on duty.
Config.Job = {
  name = 'courier',
  label = 'Courier',
  defaultDuty = false,
  grades = {
    [0] = { name = 'Rookie', payment = 30 },
    [1] = { name = 'Courier', payment = 45 },
    [2] = { name = 'Dispatcher', payment = 80, isboss = true },
  },
}

-- Anyone can take the job at City Hall. Set to false to hire with /setjob.
Config.OpenToAll = true

-- Where things are. vector4(x, y, z, heading)
Config.Locations = {
  duty = vector4(2352.50, -1806.00, 0.20, 0.0),
  pickup = vector4(2375.00, -1815.00, 0.20, 0.0),
  dropoffs = {
    vector4(2379.00, -1650.00, 0.20, 90.0),
    vector4(2379.00, -1720.00, 0.20, 90.0),
    vector4(2379.00, -1590.00, 0.20, 90.0),
  },
}

-- Map blip for the duty point (SA radar colour number).
Config.Blip = { label = 'Courier depot', colour = 5 }

-- Cash for each package delivered, on top of the paycheck.
Config.PayPerDelivery = 75
-- How long loading and handing over a package takes (milliseconds).
Config.LoadTime = 2500
Config.DeliverTime = 2000
