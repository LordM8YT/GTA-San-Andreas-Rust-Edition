fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'SARE RP chat: local talk, /me, /do, /ooc, /looc, /try, /shout, /twt and /ad.'
version '1.0.0'

dependencies { 'chat', 'sarebox', 'sare_rp_core' }
shared_script '@sare_rp_core/shared/config.lua'
server_script 'server.lua'
