-- Owned cars, in the shape of QBCore's `player_vehicles` table.
-- vehicle: the model name passed to CreateVehicle: a car of the world, or the
--   `model` of an `addon_vehicles` entry in a resource.json (custom glTF cars).
-- mods: JSON, for now { "colour": [r, g, b] }.
-- state: 1 = in its garage, 0 = out in the world.
CREATE TABLE player_vehicles (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  citizenid TEXT NOT NULL REFERENCES players (citizenid) ON DELETE CASCADE,
  vehicle TEXT NOT NULL,
  label TEXT,
  plate TEXT NOT NULL UNIQUE,
  mods TEXT NOT NULL DEFAULT '{}',
  garage TEXT NOT NULL DEFAULT 'public',
  state INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL DEFAULT (unixepoch())
);
CREATE INDEX player_vehicles_citizenid ON player_vehicles (citizenid);
