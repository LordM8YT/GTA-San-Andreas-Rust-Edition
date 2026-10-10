fx_version 'cerulean'
game 'common'
lua54 'yes'

description 'Example framework core: player objects with money and jobs, shared through exports, events and state bags.'
version '1.0.0'

-- Other resources can depend on and call `sare-core`, the way qbx_core
-- provides 'qb-core'.
provide 'sare-core'

dependency 'sare_lib'
shared_script '@sare_lib/init.lua'
server_script 'server.lua'
