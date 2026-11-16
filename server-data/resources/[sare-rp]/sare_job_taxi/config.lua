-- Taxi settings. Positions are San Andreas world coordinates: stand there
-- in game and type /coords, or press Copy position in Build Mode.
Config = {}

Config.Job = {
  name = 'taxi',
  label = 'Taxi',
  defaultDuty = true,
  grades = {
    [0] = { name = 'Recruit', payment = 50 },
    [1] = { name = 'Driver', payment = 75 },
    [2] = { name = 'Boss', payment = 120, isboss = true },
  },
}
-- Anyone can take the job at City Hall.
Config.OpenToAll = true

-- vector4(x, y, z, heading)
Config.Locations = {
  depot = vector4(2470.00, -1545.00, 0.20, 180.0), -- duty, and where the taxi appears
}

Config.Blip = { label = 'Taxi', colour = 5 }
Config.Vehicle = 'taxi'
