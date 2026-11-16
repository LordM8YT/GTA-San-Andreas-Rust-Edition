fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'SARE RP taxi: the depot, duty, taxis, /call taxi and bills.'
version '1.0.0'

dependencies { 'sarebox', 'sare_rp_core', 'sare_rp_vehicles', 'sare_rp_jobs' }
shared_script 'config.lua'
server_script 'server.lua'
