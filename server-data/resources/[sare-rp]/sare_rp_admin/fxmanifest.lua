fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'SARE RP admin extras: teleports to places and coordinates, items, vehicle cleanup, and /coords for map editing.'
version '1.0.0'

dependencies { 'sarebox', 'sare_rp_core', 'sare_rp_inventory' }
shared_scripts { '@sare_rp_core/shared/locations.lua', '@sare_rp_core/shared/config.lua' }
server_script 'server.lua'
