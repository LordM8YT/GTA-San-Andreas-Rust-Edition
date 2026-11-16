-- EMS settings. Positions are San Andreas world coordinates: stand there
-- in game and type /coords, or press Copy position in Build Mode.
Config = {}

-- EMS is hired by an admin: /setjob <id> ambulance 0
Config.Job = {
  name = 'ambulance',
  label = 'EMS',
  defaultDuty = false,
  grades = {
    [0] = { name = 'Trainee', payment = 70 },
    [1] = { name = 'Paramedic', payment = 110 },
    [2] = { name = 'Chief', payment = 180, isboss = true },
  },
}

-- vector4(x, y, z, heading)
Config.Locations = {
  reception = vector4(2437.50, -1606.00, 0.20, 180.0), -- patients check in here
  duty = vector4(2416.00, -1586.00, 0.20, 0.0),        -- duty and lockers
  garage = vector4(2455.00, -1640.00, 0.20, 90.0),     -- ambulances spawn here
}

Config.Blip = { label = 'Hospital', colour = 1 }

Config.Gear = { radio = 1, medkit = 2, bandage = 10 }
Config.Vehicle = 'ambulance'

-- What the doctor charges when no EMS is on duty.
Config.TreatmentPrice = 150
-- Treating someone takes this long (milliseconds) and uses a medkit or a bandage.
Config.TreatTime = 6000
