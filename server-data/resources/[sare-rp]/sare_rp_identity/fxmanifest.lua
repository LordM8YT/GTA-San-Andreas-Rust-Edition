fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'SARE RP identity: a roleplay name, date of birth and ID card for every character.'
version '1.0.0'

dependencies { 'sarebox', 'sare_rp_core' }
shared_script '@sare_rp_core/shared/config.lua'
server_script 'server.lua'
