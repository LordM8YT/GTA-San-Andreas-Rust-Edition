fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'SARE RP inventory and shops: items with weight, use, give and drop; the 24/7 and gas station shops.'
version '1.0.0'

dependencies { 'sarebox', 'sare_rp_core' }
shared_script '@sare_rp_core/shared/config.lua'
server_script 'server.lua'
client_script 'client.lua'
