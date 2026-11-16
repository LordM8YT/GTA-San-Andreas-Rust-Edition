fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'SARE RP job tools: City Hall job centre, phone dispatch (/911, /call), bills, duty and gear lockers, and the F2 job menu. Each job is its own resource (sare_job_*).'
version '2.0.0'

dependencies { 'sarebox', 'sare_rp_core', 'sare_rp_inventory', 'sare_rp_identity' }
shared_script '@sare_rp_core/shared/config.lua'
server_script 'server.lua'
client_script 'client.lua'
