fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'SARE RP core: map locations, the interaction key, blips, dialogs and shared helpers for the SARE RP Town example server.'
version '1.0.0'

dependency 'sarebox'

shared_scripts {
  'shared/locations.lua',
  'shared/config.lua',
}
server_script 'server/main.lua'
client_script 'client/main.lua'
