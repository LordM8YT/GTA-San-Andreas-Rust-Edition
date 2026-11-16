fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'SARE RP EMS: hospital reception, duty and lockers, ambulances, treating people, 911 calls and bills.'
version '1.0.0'

dependencies { 'sarebox', 'sare_rp_core', 'sare_rp_inventory', 'sare_rp_vehicles', 'sare_rp_jobs' }
shared_scripts { '@sare_rp_core/shared/config.lua', 'config.lua' }
server_script 'server.lua'
