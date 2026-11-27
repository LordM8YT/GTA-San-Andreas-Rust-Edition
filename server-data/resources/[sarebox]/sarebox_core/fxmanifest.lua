fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'SARE Box core: saved characters with money, jobs and metadata, in the shape of QBCore/Qbox.'
version '0.1.0'

-- Other resources depend on and call `sarebox`.
provide 'sarebox'

dependencies {
  'spawnmanager',
  'oxmysql',
}

shared_script 'shared/config.lua'
server_scripts {
  '@oxmysql/lib/MySQL.lua',
  'server/player.lua',
  'server/main.lua',
}
client_script 'client/main.lua'
