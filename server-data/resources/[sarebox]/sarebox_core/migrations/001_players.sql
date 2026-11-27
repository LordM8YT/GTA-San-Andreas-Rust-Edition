-- SARE Box characters, in the shape of QBCore's `players` table. money, job,
-- metadata and position are JSON. license is the player's full identifier
-- (`license:<hex>`); NULL means the character is not linked to anyone.
CREATE TABLE players (
  citizenid TEXT PRIMARY KEY,
  license TEXT UNIQUE,
  name TEXT,
  money TEXT NOT NULL DEFAULT '{}',
  job TEXT,
  metadata TEXT NOT NULL DEFAULT '{}',
  position TEXT,
  last_updated INTEGER NOT NULL DEFAULT (unixepoch())
);
