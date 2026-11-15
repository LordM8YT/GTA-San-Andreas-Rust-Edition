fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'Courier: an example SARE RP job in its own resource. Copy it, or run tools/new-resource.py --type rp-job, to make your own job.'
version '1.0.0'

dependencies {
  'sarebox',
  'sare_rp_core',
}

shared_script 'config.lua'
server_script 'server.lua'
