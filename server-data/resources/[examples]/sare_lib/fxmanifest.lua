fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'Example shared library. Other resources load it with @sare_lib/init.lua, the way ox_lib is used.'
version '1.0.0'

files {
  'init.lua',
  'modules/*.lua',
}

server_script 'server.lua'
