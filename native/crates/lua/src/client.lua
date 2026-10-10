-- Client additions to common.lua (same chunk: `native`, `pack`, `resume` and
-- `encode` are in scope).
function TriggerServerEvent(name, ...) native.trigger_server(name, pack(...)) end
function TriggerLatentServerEvent(name, _bps, ...) TriggerServerEvent(name, ...) end
function RegisterKeyMapping(command, _description, _mapper, key)
  native.key_mapping(tostring(command), tostring(key))
end

-- UI drawn by the game with its own widgets: scripts pass text and choices,
-- never markup or code. UI.inputDialog and UI.progressBar wait for the player,
-- so call them from a thread or event handler.
local contexts, replies, next_token = {}, {}, 0
UI = {}
function UI.notify(data) native.ui_notify(data) end
function UI.registerContext(context)
  if context[1] then
    for _, item in ipairs(context) do UI.registerContext(item) end
    return
  end
  contexts[context.id] = context
end
function UI.showContext(id)
  local context = contexts[id]
  if not context then error(('no context menu %s'):format(id), 2) end
  local options = {}
  for i, option in ipairs(context.options or {}) do
    options[i] = { title = option.title, description = option.description, disabled = option.disabled }
  end
  native.ui_context({ id = id, title = context.title, options = options })
end
function UI.hideContext() native.ui_hide_context() end
local function await_reply(start)
  next_token = next_token + 1
  local token, p = next_token, promise.new()
  replies[token] = p
  start(token)
  return Citizen.Await(p)
end
function UI.inputDialog(heading, rows)
  return await_reply(function(token) native.ui_dialog(heading, rows, token) end)
end
function UI.progressBar(data)
  return await_reply(function(token) native.ui_progress(data, token) end)
end
function UI.showTextUI(text) native.ui_text(tostring(text)) end
function UI.hideTextUI() native.ui_text(nil) end

function __sare_ui_select(id, index)
  local context = contexts[id]
  local option = context and context.options and context.options[index]
  if not option then return end
  resume(coroutine.create(function()
    if option.onSelect then option.onSelect(option.args) end
    if option.event then TriggerEvent(option.event, option.args) end
    if option.serverEvent then TriggerServerEvent(option.serverEvent, option.args) end
    if option.menu then UI.showContext(option.menu) end
  end))
end
function __sare_ui_reply(token, value)
  local p = replies[token]
  replies[token] = nil
  if p then p:resolve(value) end
end
