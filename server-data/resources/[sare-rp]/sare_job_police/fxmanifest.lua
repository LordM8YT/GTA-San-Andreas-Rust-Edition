fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'SARE RP police (LSPD): duty, lockers, patrol cars, cuffs, search, fines, jail and the front desk.'
version '1.0.0'

dependencies { 'sarebox', 'sare_rp_core', 'sare_rp_inventory', 'sare_rp_vehicles', 'sare_rp_jobs' }
shared_scripts { '@sare_rp_core/shared/config.lua', 'config.lua' }
server_script 'server.lua'
