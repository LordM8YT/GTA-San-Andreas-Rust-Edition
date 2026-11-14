fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'SARE RP identity: a roleplay name, date of birth, ID card and look (character creator, clothes shop) for every character.'
version '1.1.0'

dependencies { 'sarebox', 'sare_rp_core' }
shared_script '@sare_rp_core/shared/config.lua'
server_scripts { 'server.lua', 'appearance.lua' }
