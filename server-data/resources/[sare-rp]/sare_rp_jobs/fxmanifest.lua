fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'SARE RP jobs: City Hall job centre, police (cuffs, fines, jail, search), EMS, mechanic, taxi, duty points, 911 and service calls, bills.'
version '1.0.0'

dependencies { 'sarebox', 'sare_rp_core', 'sare_rp_inventory', 'sare_rp_vehicles', 'sare_rp_identity' }
shared_scripts { '@sare_rp_core/shared/locations.lua', '@sare_rp_core/shared/config.lua' }
server_script 'server.lua'
client_script 'client.lua'
