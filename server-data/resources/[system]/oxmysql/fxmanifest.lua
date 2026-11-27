-- SARE's built-in database resource. It offers the oxmysql API (exports and
-- @oxmysql/lib/MySQL.lua) on top of the server's SQLite database, so FiveM
-- scripts written for oxmysql run unchanged. This is SARE's own code, not
-- a copy of oxmysql.
fx_version 'cerulean'
game 'common'
author 'SARE'
description 'Server database (SQLite) with the oxmysql API'
version '1.0.0'

provide 'mysql-async'

server_scripts {
  'lib/MySQL.lua',
  'server.lua',
}
