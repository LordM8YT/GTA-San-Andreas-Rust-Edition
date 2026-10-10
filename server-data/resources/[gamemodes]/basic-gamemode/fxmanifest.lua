fx_version 'cerulean'
game 'common'

description 'A basic freeroam game type using the spawn logic from spawnmanager.'
version '1.0.0'

resource_type 'gametype' { name = 'Freeroam' }

dependency 'spawnmanager'
server_script 'basic_server.lua'
