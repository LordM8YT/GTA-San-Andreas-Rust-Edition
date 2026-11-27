# Server database

Every SARE server has a built-in SQL database for roleplay data that has to
survive a restart: players, money, jobs, inventories, owned cars, houses.
It is one [SQLite](https://sqlite.org) file next to server.cfg, so there is
nothing to install or configure. SARE Box and SARE RP Town keep their
characters here (the `players` table, see
[SARE Box](sarebox.md#when-data-is-saved)), and SARE RP Town its owned cars
(`player_vehicles`).

Scripts use it with the **oxmysql API**, the one most FiveM resources already
use. SARE ships its own resource called `oxmysql` (in `resources/[system]/`)
that offers the same exports and the same `@oxmysql/lib/MySQL.lua` include,
so scripts written for oxmysql work without changes. It is SARE's own code,
written against oxmysql's public API, not a copy of oxmysql.

## Use it in a resource

```lua
-- fxmanifest.lua
fx_version 'cerulean'
game 'common'
dependency 'oxmysql'
server_scripts {
  '@oxmysql/lib/MySQL.lua',
  'server.lua',
}
```

```lua
-- server.lua
local row = MySQL.single.await('SELECT * FROM bank_accounts WHERE owner = ?', { license })
if not row then
  MySQL.insert.await('INSERT INTO bank_accounts (owner, balance) VALUES (?, ?)', { license, 500 })
end

-- Callback form: the callback runs on the next server tick.
MySQL.query('SELECT owner, balance FROM bank_accounts ORDER BY balance DESC LIMIT 10', {}, function(rows)
  for _, r in ipairs(rows) do print(r.owner, r.balance) end
end)

-- All or nothing.
local ok = MySQL.transaction.await({
  { query = 'UPDATE bank_accounts SET balance = balance - ? WHERE owner = ?', values = { 100, from } },
  { query = 'UPDATE bank_accounts SET balance = balance + ? WHERE owner = ?', values = { 100, to } },
})
```

`dependency 'oxmysql'` starts the database resource before yours. Scripts
that call `exports.oxmysql:query(sql, params, cb)` directly work too.

| Call | Returns |
| --- | --- |
| `MySQL.query` (`execute`, `rawExecute`) | rows for a SELECT, otherwise `{ affectedRows, insertId, changedRows }` |
| `MySQL.single` | the first row, or `nil` |
| `MySQL.scalar` | the first column of the first row, or `nil` |
| `MySQL.insert` | the new row's id |
| `MySQL.update` | how many rows changed |
| `MySQL.prepare` | one value, one row or a list of rows; a list of parameter lists runs the query once per list |
| `MySQL.transaction` | `true` when every query worked, `false` (and nothing changed) otherwise |

Every call has an `.await` version (`MySQL.scalar.await(...)`) that returns
the result directly. The old `MySQL.Async.fetchAll`/`MySQL.Sync.fetchAll`
names from mysql-async also work, and `@mysql-async/lib/MySQL.lua` loads the
same file.

- **Parameters:** a list for `?` (`{ license, 500 }`) or a table for named
  ones (`@owner`, `:owner`; the key can be written with or without `@`).
  Always pass values as parameters, never glue player input into the SQL text.
- **Values:** `NULL` columns are left out of a row, so they read as `nil`.
  `true`/`false` are stored as `1`/`0`. A Lua table is stored as JSON text;
  read it back with `json.decode`.
- **Errors:** `.await` raises a Lua error (catch it with `pcall`); the
  callback form prints the error with your resource's name and does not call
  back. The SQL text is in the message.

## Tables: migrations

Put the tables your resource needs in `migrations/` inside the resource, as
`.sql` files:

```
my_bank/
  fxmanifest.lua
  server.lua
  migrations/
    001_accounts.sql
    002_account_frozen.sql
```

```sql
-- migrations/001_accounts.sql
CREATE TABLE bank_accounts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  owner TEXT NOT NULL UNIQUE,
  balance INTEGER NOT NULL DEFAULT 0
);
```

When the resource starts, the server runs the files it has not run before,
in file name order, before your scripts load. Each file runs in a
transaction; if one fails, nothing from that file is kept, the error is
printed and the resource does not start. The server remembers which files
ran in the `sare_migrations` table. Never edit a file that has already run on
a server: add a new one (`003_...sql`) instead.

## SQL dialect

The database is SQLite, so queries use SQLite's SQL. Most everyday MySQL
queries work as they are (`SELECT`, `INSERT`, `UPDATE`, `DELETE`, joins,
backtick-quoted names, `INT(11)`, `VARCHAR(50)`, `LIMIT`). Things to write
differently:

| MySQL | SQLite |
| --- | --- |
| `id INT AUTO_INCREMENT PRIMARY KEY` | `id INTEGER PRIMARY KEY AUTOINCREMENT` |
| `ENGINE=InnoDB DEFAULT CHARSET=utf8mb4` | leave it out |
| `INSERT ... ON DUPLICATE KEY UPDATE x = VALUES(x)` | `INSERT ... ON CONFLICT(key) DO UPDATE SET x = excluded.x` |
| `NOW()` | `CURRENT_TIMESTAMP`, or `unixepoch()` for seconds |
| `JSON_EXTRACT(col, '$.a')` | works the same (`json_extract`) |

A FiveM resource's `.sql` file written for MySQL usually needs these small
changes before it can go in `migrations/`.

## Where the data lives

`db/sare.db` next to server.cfg (with `sare.db-wal` beside it while the
server runs). Back up the `db/` folder with the server stopped, or copy it
with any SQLite tool while it runs (`sqlite3 db/sare.db ".backup backup.db"`).
To open or edit the data, use any SQLite browser, such as
[DB Browser for SQLite](https://sqlitebrowser.org).

To put the file somewhere else, set this in server.cfg:

```
set sare_db_file "db/sare.db"
```

A relative path starts at the folder with server.cfg.

## Not yet

- **MySQL/MariaDB.** SARE only has SQLite for now. A
  `mysql_connection_string` in server.cfg is ignored with a warning. One file
  is enough for a single RP server with hundreds of players; MySQL matters
  when several servers share one database.
- Queries run on the server thread. Keep them small (look rows up by an
  indexed column), and don't run a query every frame.
