-- Shared by the server and the players' game.
Config = {}

Config.StartMoney = { cash = 500, bank = 5000 }

-- Paid to the bank account of on-duty players every PaycheckMinutes.
Config.PaycheckMinutes = 10

-- Jobs and grades, like QBCore's shared jobs. Grade 0 is where new hires start.
Config.Jobs = {
  unemployed = { label = 'Unemployed', defaultDuty = true, grades = { [0] = { name = 'Freelancer', payment = 10 } } },
  taxi = {
    label = 'Taxi',
    defaultDuty = true,
    grades = {
      [0] = { name = 'Recruit', payment = 50 },
      [1] = { name = 'Driver', payment = 75 },
      [2] = { name = 'Boss', payment = 120, isboss = true },
    },
  },
  mechanic = {
    label = 'Mechanic',
    defaultDuty = true,
    grades = {
      [0] = { name = 'Apprentice', payment = 60 },
      [1] = { name = 'Mechanic', payment = 90 },
      [2] = { name = 'Boss', payment = 140, isboss = true },
    },
  },
  police = {
    label = 'LSPD',
    defaultDuty = false,
    grades = {
      [0] = { name = 'Cadet', payment = 80 },
      [1] = { name = 'Officer', payment = 120 },
      [2] = { name = 'Chief', payment = 200, isboss = true },
    },
  },
}
