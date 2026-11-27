fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'SARE RP vehicles: buy at Premium Motors, keep them in the public garage, job vehicles.'
version '1.0.0'

-- sarebox first: player_vehicles refers to its players table.
dependencies { 'sarebox', 'sare_rp_core', 'oxmysql' }
shared_scripts { '@sare_rp_core/shared/locations.lua', '@sare_rp_core/shared/config.lua' }
server_scripts { '@oxmysql/lib/MySQL.lua', 'server.lua' }
