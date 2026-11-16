-- Police settings. Positions are San Andreas world coordinates: stand there
-- in game and type /coords, or press Copy position in Build Mode.
Config = {}

-- The job, like QBCore's shared jobs. Police is hired by an admin:
-- /setjob <id> police 0
Config.Job = {
  name = 'police',
  label = 'LSPD',
  defaultDuty = false,
  grades = {
    [0] = { name = 'Cadet', payment = 80 },
    [1] = { name = 'Officer', payment = 120 },
    [2] = { name = 'Chief', payment = 200, isboss = true },
  },
}

-- vector4(x, y, z, heading)
Config.Locations = {
  duty = vector4(2545.00, -1603.00, 0.20, 180.0),    -- front desk: duty, report a crime
  armory = vector4(2528.00, -1582.00, 0.20, 0.0),    -- lockers
  garage = vector4(2530.00, -1640.00, 0.20, 90.0),   -- patrol cars spawn here
  cell = vector4(2565.00, -1578.50, 0.20, 180.0),    -- prisoners stand here
  release = vector4(2545.00, -1616.00, 0.20, 180.0), -- and leave here
}

Config.Blip = { label = 'Police', colour = 3 }

-- Gear from the lockers (item = count), once every ten minutes.
Config.Gear = { radio = 1, handcuffs = 1, bandage = 2 }
-- The patrol car (a model of the content pack, or an SA model name).
Config.Vehicle = 'police'

-- Largest fine in dollars.
Config.MaxFine = 25000
-- Longest jail sentence in minutes, and how far a prisoner may walk.
Config.Jail = { maxMinutes = 60, radius = 6.0 }
-- 911 calls reach these jobs (EMS adds itself from its own resource).
Config.Dispatch = { label = '911', colour = 1 }
