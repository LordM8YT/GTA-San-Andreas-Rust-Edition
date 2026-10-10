//! Server natives with FiveM names. Player IDs are session IDs; a player's
//! ped handle is the same number, as in single-ped FiveM servers.
use crate::script::{self, json_to_pack, out, pack_to_json, Shared};
use anyhow::Result;
use mlua::{Function, Lua, LuaSerdeExt, Table, Value};
use serde_json::Value as Json;
use std::fs;

/// Runtime origin, identical to `sa_client::ORIGIN`: pose positions are
/// `[x - ORIGIN.x, z, ORIGIN.y - y]` in San Andreas world coordinates.
const ORIGIN: [f32; 2] = [2500.0, -1670.0];
pub fn world_position(pose: &sa_net::Pose) -> [f32; 3] {
    [
        pose.position[0] + ORIGIN[0],
        ORIGIN[1] - pose.position[2],
        pose.position[1],
    ]
}
pub fn heading(pose: &sa_net::Pose) -> f32 {
    pose.yaw.to_degrees().rem_euclid(360.0)
}

fn player_id(value: &Value) -> Option<u32> {
    match value {
        Value::Integer(i) => u32::try_from(*i).ok(),
        Value::Number(n) => Some(*n as u32),
        Value::String(s) => s.to_str().ok()?.trim().parse().ok(),
        _ => None,
    }
}
fn peer(shared: &Shared, id: &Value) -> Option<sa_net::Peer> {
    let id = player_id(id)?;
    shared.borrow().peers.iter().find(|p| p.id == id).cloned()
}
fn vector3(lua: &Lua, [x, y, z]: [f32; 3]) -> mlua::Result<Value> {
    let make: Function = lua.globals().get("vector3")?;
    make.call((x, y, z))
}
fn send_client(shared: &Shared, target: Option<u32>, name: &str, args: &[Json]) {
    let payload = Json::Array(args.to_vec()).to_string();
    let session = shared.borrow().session.clone();
    if let Some(session) = session {
        if session.trigger(target, name, &payload).is_err() {
            out(
                shared,
                format!(
                    "Event {name} dropped: invalid name or payload over {} bytes",
                    sa_net::MAX_EVENT_PAYLOAD
                ),
            );
        }
    }
}
fn resource_file(shared: &Shared, resource: &str, file: &str) -> Option<std::path::PathBuf> {
    let file = file.replace('\\', "/");
    if file.starts_with('/') || file.split('/').any(|p| p == ".." || p.contains(':')) {
        return None;
    }
    let folder = shared.borrow().resources.get(resource)?.path.clone();
    Some(folder.join(file))
}

pub fn create_state(shared: &Shared, resource: &str) -> Result<Lua> {
    let lua = Lua::new();
    lua.set_memory_limit(256 * 1024 * 1024)?;
    let g = lua.globals();
    let native = lua.create_table()?;
    let name = resource.to_string();

    macro_rules! func {
        ($table:expr, $key:expr, $shared:ident, $name:ident, |$lua:ident, $args:tt : $ty:ty| $body:block) => {{
            let $shared = $shared.clone();
            let $name = $name.clone();
            #[allow(unused_variables)]
            $table.set(
                $key,
                lua.create_function(move |$lua, $args: $ty| {
                    let _ = (&$shared, &$name);
                    $body
                })?,
            )?;
        }};
    }

    // Internal natives used by prelude.lua.
    func!(native, "print", shared, name, |_lua, text: String| {
        out(&shared, format!("[{name}] {text}"));
        Ok(())
    });
    func!(native, "error", shared, name, |_lua, text: String| {
        out(&shared, format!("SCRIPT ERROR in {name}: {text}"));
        Ok(())
    });
    func!(native, "trigger", shared, name, |lua,
                                            (event, args): (
        String,
        Table
    )| {
        let args = pack_to_json(lua, &args)?;
        script::dispatch(&shared, &event, 0, &args, false);
        Ok(())
    });
    func!(native, "trigger_client", shared, name, |lua,
                                                   (
        event,
        target,
        args,
    ): (
        String,
        i64,
        Table
    )| {
        let args = pack_to_json(lua, &args)?;
        let target = (target >= 0).then_some(target as u32);
        send_client(&shared, target, &event, &args);
        Ok(())
    });
    func!(native, "was_cancelled", shared, name, |_lua, (): ()| {
        Ok(shared.borrow().cancelled)
    });
    func!(native, "register_command", shared, name, |_lua,
                                                     (
        command,
        restricted,
    ): (
        String,
        bool
    )| {
        shared.borrow_mut().commands.insert(
            command.to_ascii_lowercase(),
            script::Command {
                resource: name.clone(),
                restricted,
            },
        );
        Ok(())
    });
    func!(native, "export", shared, name, |_lua, export: String| {
        shared.borrow_mut().exports.insert((name.clone(), export));
        Ok(())
    });
    func!(native, "call_export", shared, name, |lua,
                                                (
        target,
        export,
        args,
    ): (
        String,
        String,
        Table
    )| {
        let args = pack_to_json(lua, &args)?;
        let other = {
            let state = shared.borrow();
            if !state.exports.contains(&(target.clone(), export.clone())) {
                return Err(mlua::Error::runtime(format!(
                    "No such export {export} in resource {target}"
                )));
            }
            state.resources.get(&target).and_then(|r| r.lua.clone())
        };
        let other = other
            .ok_or_else(|| mlua::Error::runtime(format!("Resource {target} is not started")))?;
        shared.borrow_mut().invoking.push(name.clone());
        let result = (|| {
            let call: Function = other.globals().get("__sare_export_call")?;
            let packed: Table = call.call((export.as_str(), json_to_pack(&other, &args)?))?;
            pack_to_json(&other, &packed)
        })();
        shared.borrow_mut().invoking.pop();
        json_to_pack(lua, &result?)
    });
    func!(native, "json_encode", shared, name, |lua, value: Value| {
        let json: Json = lua.from_value_with(value, script::deserialize())?;
        Ok(json.to_string())
    });
    func!(native, "json_decode", shared, name, |lua, text: String| {
        match serde_json::from_str::<Json>(&text) {
            Ok(json) => lua.to_value_with(&json, script::serialize()),
            Err(_) => Ok(Value::Nil),
        }
    });
    g.set("__sare", native)?;

    // Resource and server information.
    func!(g, "GetCurrentResourceName", shared, name, |_lua, (): ()| {
        Ok(name.clone())
    });
    func!(g, "GetInvokingResource", shared, name, |_lua, (): ()| {
        Ok(shared.borrow().invoking.last().cloned())
    });
    func!(g, "GetGameTimer", shared, name, |_lua, (): ()| {
        Ok(shared.borrow().started.elapsed().as_millis() as i64)
    });
    func!(g, "GetGameName", shared, name, |_lua, (): ()| {
        Ok("sare")
    });
    func!(g, "IsDuplicityVersion", shared, name, |_lua, (): ()| {
        Ok(true)
    });
    func!(g, "GetConvar", shared, name, |_lua,
                                         (key, default): (
        String,
        Option<String>
    )| {
        Ok(shared
            .borrow()
            .convar(&key)
            .map(str::to_string)
            .or(default)
            .unwrap_or_default())
    });
    func!(g, "GetConvarInt", shared, name, |_lua,
                                            (key, default): (
        String,
        Option<i64>
    )| {
        Ok(shared
            .borrow()
            .convar(&key)
            .and_then(|v| v.parse().ok())
            .or(default)
            .unwrap_or(0))
    });
    func!(g, "SetConvar", shared, name, |_lua,
                                         (key, value): (
        String,
        String
    )| {
        shared.borrow_mut().convars.insert(key, value);
        Ok(())
    });
    func!(g, "SetConvarServerInfo", shared, name, |_lua,
                                                   (key, value): (
        String,
        String
    )| {
        let mut state = shared.borrow_mut();
        state.server_info.insert(key.clone());
        state.convars.insert(key, value);
        Ok(())
    });
    func!(g, "SetConvarReplicated", shared, name, |_lua,
                                                   (key, value): (
        String,
        String
    )| {
        shared.borrow_mut().convars.insert(key, value);
        Ok(())
    });
    func!(g, "SetGameType", shared, name, |_lua, value: String| {
        shared.borrow_mut().convars.insert("gametype".into(), value);
        Ok(())
    });
    func!(g, "SetMapName", shared, name, |_lua, value: String| {
        shared.borrow_mut().convars.insert("mapname".into(), value);
        Ok(())
    });
    func!(
        g,
        "GetResourceState",
        shared,
        name,
        |_lua, resource: String| {
            Ok(match shared.borrow().resources.get(&resource) {
                Some(r) if r.started => "started",
                Some(_) => "stopped",
                None => "missing",
            })
        }
    );
    func!(g, "GetNumResources", shared, name, |_lua, (): ()| {
        Ok(shared.borrow().resources.len())
    });
    func!(
        g,
        "GetResourceByFindIndex",
        shared,
        name,
        |_lua, index: usize| { Ok(shared.borrow().resources.keys().nth(index).cloned()) }
    );
    func!(
        g,
        "GetResourcePath",
        shared,
        name,
        |_lua, resource: String| {
            Ok(shared
                .borrow()
                .resources
                .get(&resource)
                .map(|r| r.path.display().to_string()))
        }
    );
    func!(g, "GetNumResourceMetadata", shared, name, |_lua,
                                                      (
        resource,
        key,
    ): (
        String,
        String
    )| {
        Ok(shared
            .borrow()
            .resources
            .get(&resource)
            .map_or(0, |r| r.manifest.values(&key).count()))
    });
    func!(g, "GetResourceMetadata", shared, name, |_lua,
                                                   (
        resource,
        key,
        index,
    ): (
        String,
        String,
        Option<usize>
    )| {
        let state = shared.borrow();
        let Some(r) = state.resources.get(&resource) else {
            return Ok(None);
        };
        if let Some(base) = key.strip_suffix("_extra") {
            return Ok(r.manifest.extra.get(base).cloned());
        }
        let value = r
            .manifest
            .values(&key)
            .nth(index.unwrap_or(0))
            .map(str::to_string);
        Ok(value)
    });
    func!(g, "LoadResourceFile", shared, name, |_lua,
                                                (resource, file): (
        String,
        String
    )| {
        let Some(path) = resource_file(&shared, &resource, &file) else {
            return Ok(None);
        };
        Ok(fs::metadata(&path)
            .ok()
            .filter(|m| m.len() <= 16 * 1024 * 1024)
            .and_then(|_| fs::read_to_string(path).ok()))
    });
    func!(g, "SaveResourceFile", shared, name, |_lua,
                                                (
        resource,
        file,
        data,
    ): (
        String,
        String,
        String
    )| {
        let Some(path) = resource_file(&shared, &resource, &file) else {
            return Ok(false);
        };
        if data.len() > 16 * 1024 * 1024 {
            return Ok(false);
        }
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        Ok(fs::write(path, data).is_ok())
    });
    func!(
        g,
        "StartResource",
        shared,
        name,
        |_lua, resource: String| {
            shared
                .borrow_mut()
                .pending
                .push((0, format!("start {resource}")));
            Ok(true)
        }
    );
    func!(g, "StopResource", shared, name, |_lua, resource: String| {
        shared
            .borrow_mut()
            .pending
            .push((0, format!("stop {resource}")));
        Ok(true)
    });
    func!(g, "ExecuteCommand", shared, name, |_lua, line: String| {
        shared.borrow_mut().pending.push((0, line));
        Ok(())
    });
    func!(g, "GetRegisteredCommands", shared, name, |lua, (): ()| {
        let list = lua.create_table()?;
        for (command, c) in &shared.borrow().commands {
            let entry = lua.create_table()?;
            entry.set("name", command.as_str())?;
            entry.set("resource", c.resource.as_str())?;
            entry.set("arity", -1)?;
            list.push(entry)?;
        }
        Ok(list)
    });

    // Access control.
    func!(
        g,
        "IsPlayerAceAllowed",
        shared,
        name,
        |_lua, (player, object): (Value, String)| {
            let Some(id) = player_id(&player) else {
                return Ok(false);
            };
            Ok(shared
                .borrow()
                .acl
                .allowed(&format!("player.{id}"), &object))
        }
    );
    func!(g, "IsPrincipalAceAllowed", shared, name, |_lua,
                                                     (
        principal,
        object,
    ): (
        String,
        String
    )| {
        Ok(shared.borrow().acl.allowed(&principal, &object))
    });
    func!(g, "ExecuteCommandAsPlayer", shared, name, |_lua,
                                                      (
        player,
        line,
    ): (
        Value,
        String
    )| {
        if let Some(id) = player_id(&player) {
            shared.borrow_mut().pending.push((id, line));
        }
        Ok(())
    });

    // Players.
    func!(g, "GetPlayers", shared, name, |_lua, (): ()| {
        Ok(shared
            .borrow()
            .peers
            .iter()
            .map(|p| p.id.to_string())
            .collect::<Vec<_>>())
    });
    func!(g, "GetNumPlayerIndices", shared, name, |_lua, (): ()| {
        Ok(shared.borrow().peers.len())
    });
    func!(
        g,
        "GetPlayerFromIndex",
        shared,
        name,
        |_lua, index: usize| { Ok(shared.borrow().peers.get(index).map(|p| p.id.to_string())) }
    );
    func!(g, "GetPlayerName", shared, name, |_lua, player: Value| {
        Ok(peer(&shared, &player).map(|p| p.name))
    });
    func!(g, "DoesPlayerExist", shared, name, |_lua, player: Value| {
        Ok(peer(&shared, &player).is_some())
    });
    func!(g, "GetPlayerPed", shared, name, |_lua, player: Value| {
        Ok(peer(&shared, &player).map_or(0, |p| p.id))
    });
    func!(
        g,
        "GetPlayerIdentifiers",
        shared,
        name,
        |lua, player: Value| {
            // No account system yet: a session identifier only, never trusted.
            let list = lua.create_table()?;
            if let Some(p) = peer(&shared, &player) {
                list.push(format!("sare:{}", p.id))?;
            }
            Ok(list)
        }
    );
    func!(g, "GetPlayerPing", shared, name, |_lua, _player: Value| {
        Ok(0)
    });
    func!(
        g,
        "GetPlayerEndpoint",
        shared,
        name,
        |_lua, _player: Value| { Ok("") }
    );
    func!(g, "DropPlayer", shared, name, |_lua,
                                          (player, reason): (
        Value,
        Option<String>
    )| {
        let session = shared.borrow().session.clone();
        if let (Some(session), Some(id)) = (session, player_id(&player)) {
            session.drop_player(id, reason.as_deref().unwrap_or("Dropped by the server."));
        }
        Ok(())
    });
    func!(g, "DoesEntityExist", shared, name, |_lua, entity: Value| {
        Ok(peer(&shared, &entity).is_some())
    });
    func!(g, "GetEntityCoords", shared, name, |lua, entity: Value| {
        let position = peer(&shared, &entity).map_or([0.0; 3], |p| world_position(&p.pose));
        vector3(lua, position)
    });
    func!(
        g,
        "GetEntityHeading",
        shared,
        name,
        |_lua, entity: Value| { Ok(peer(&shared, &entity).map_or(0.0, |p| heading(&p.pose))) }
    );
    func!(g, "GetEntitySpeed", shared, name, |_lua, entity: Value| {
        Ok(peer(&shared, &entity).map_or(0.0, |p| p.pose.speed))
    });
    func!(g, "IsPedInAnyVehicle", shared, name, |_lua,
                                                 (ped, _last): (
        Value,
        Option<bool>
    )| {
        Ok(peer(&shared, &ped).is_some_and(|p| p.pose.driving || p.pose.ride.is_some()))
    });
    func!(
        g,
        "GetPlayerRoutingBucket",
        shared,
        name,
        |_lua, _player: Value| { Ok(0) }
    );
    // Movement is client-authoritative: these ask the owning client to move.
    func!(g, "SetEntityCoords", shared, name, |_lua,
                                               (entity, x, y, z): (
        Value,
        f32,
        f32,
        f32
    )| {
        if let Some(id) = player_id(&entity) {
            send_client(
                &shared,
                Some(id),
                "__sare:setCoords",
                &[x.into(), y.into(), z.into(), Json::Null],
            );
        }
        Ok(())
    });
    func!(g, "SetEntityCoordsNoOffset", shared, name, |_lua,
                                                       (
        entity,
        x,
        y,
        z,
    ): (
        Value,
        f32,
        f32,
        f32
    )| {
        if let Some(id) = player_id(&entity) {
            send_client(
                &shared,
                Some(id),
                "__sare:setCoords",
                &[x.into(), y.into(), z.into(), Json::Null],
            );
        }
        Ok(())
    });
    func!(g, "SetEntityHeading", shared, name, |_lua,
                                                (entity, value): (
        Value,
        f32
    )| {
        if let Some(id) = player_id(&entity) {
            send_client(&shared, Some(id), "__sare:setHeading", &[value.into()]);
        }
        Ok(())
    });

    lua.load(include_str!("prelude.lua"))
        .set_name("@citizen:/scripting/lua/scheduler.lua")
        .exec()?;
    Ok(lua)
}
