-- Mechanic settings. Positions are San Andreas world coordinates: stand
-- there in game and type /coords, or press Copy position in Build Mode.
Config = {}

Config.Job = {
  name = 'mechanic',
  label = 'Mechanic',
  defaultDuty = true,
  grades = {
    [0] = { name = 'Apprentice', payment = 60 },
    [1] = { name = 'Mechanic', payment = 90 },
    [2] = { name = 'Boss', payment = 140, isboss = true },
  },
}
-- Anyone can take the job at City Hall.
Config.OpenToAll = true

-- vector4(x, y, z, heading)
Config.Locations = {
  workshop = vector4(2640.00, -1712.00, 0.20, 90.0), -- customers ask for help, mechanics take the tow car
  duty = vector4(2658.00, -1700.00, 0.20, 270.0),    -- duty and lockers
  vehicle = vector4(2642.00, -1648.00, 0.20, 0.0),   -- where the tow car appears
}

Config.Blip = { label = 'Mechanic', colour = 5 }

Config.Gear = { repairkit = 2 }
Config.Vehicle = 'towtruck'
