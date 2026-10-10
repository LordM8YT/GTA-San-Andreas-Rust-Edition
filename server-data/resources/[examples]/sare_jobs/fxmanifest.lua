fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'Example resource built on sare_core through the name it provides.'
version '1.0.0'

dependencies {
  'sare_lib',
  'sare-core',
}

shared_script '@sare_lib/init.lua'
server_script 'server.lua'
client_script 'client.lua'
