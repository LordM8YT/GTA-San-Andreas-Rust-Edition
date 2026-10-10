//! Server natives with FiveM names. Player IDs are session IDs; a player's
//! ped handle is the same number, as in single-ped FiveM servers.
use crate::script::{self, json_to_pack, out, pack_to_json, Shared};
use anyhow::Result;
use mlua::{Function, Lua, LuaOptions, LuaSerdeExt, StdLib, Table, Value};
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

/// Call `entry(key, args)` in `target`'s Lua state on behalf of `caller`,
/// for exports and function references. Arguments and results are copies.
fn call_into(
    shared: &Shared,
    caller: &str,
    target: &str,
    entry: &str,
    key: &str,
    args: &[Json],
) -> mlua::Result<Vec<Json>> {
    let other = shared
        .borrow()
        .resources
        .get(target)
        .and_then(|r| r.lua.clone())
        .ok_or_else(|| mlua::Error::runtime(format!("Resource {target} is not started")))?;
    shared.borrow_mut().invoking.push(caller.to_string());
    let result = (|| {
        let call: Function = other.globals().get(entry)?;
        let packed: Table = call.call((key, json_to_pack(&other, args)?))?;
        pack_to_json(&other, &packed)
    })();
    shared.borrow_mut().invoking.pop();
    result
}

pub fn create_state(shared: &Shared, resource: &str) -> Result<Lua> {
    // FiveM resources expect the `debug` library (ox_lib uses debug.getinfo).
    // Server resources are trusted code installed by the server owner.
    let lua = unsafe { Lua::unsafe_new_with(StdLib::ALL_SAFE | StdLib::DEBUG, LuaOptions::new()) };
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
        // `exports['qb-core']` reaches the resource that provides it.
        let target = script::resolve(&shared, &target).unwrap_or(target);
        if !shared
            .borrow()
            .exports
            .contains(&(target.clone(), export.clone()))
        {
            return Err(mlua::Error::runtime(format!(
                "No such export {export} in resource {target}"
            )));
        }
        let args = pack_to_json(lua, &args)?;
        let result = call_into(
            &shared,
            &name,
            &target,
            "__sare_export_call",
            &export,
            &args,
        )?;
        json_to_pack(lua, &result)
    });
    // Function references, see prelude.lua.
    // IDs are unique for the server's lifetime, so a proxy kept across a
    // restart of its owner never reaches a different function.
    func!(native, "ref_new", shared, name, |_lua, (): ()| {
        let mut state = shared.borrow_mut();
        state.next_ref += 1;
        let id = state.next_ref;
        let key = format!("{name}:{id}");
        state.refs.insert(key.clone(), 0);
        state.ref_checks.push(key.clone());
        Ok((key, id))
    });
    func!(native, "ref_retain", shared, name, |_lua, key: String| {
        if let Some(count) = shared.borrow_mut().refs.get_mut(&key) {
            *count += 1;
        }
        Ok(())
    });
    func!(
        native,
        "ref_release",
        shared,
        name,
        |_lua, keys: Vec<String>| {
            let mut state = shared.borrow_mut();
            for key in keys {
                if let Some(count) = state.refs.get_mut(&key) {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        state.ref_checks.push(key);
                    }
                }
            }
            Ok(())
        }
    );
    func!(native, "call_ref", shared, name, |lua,
                                             (key, args): (
        String,
        Table
    )| {
        let Some((owner, id)) = key.rsplit_once(':') else {
            return Err(mlua::Error::runtime(format!(
                "Invalid function reference {key}"
            )));
        };
        if !shared.borrow().refs.contains_key(&key) {
            return Err(mlua::Error::runtime(format!(
                "Function reference {key} no longer exists"
            )));
        }
        let args = pack_to_json(lua, &args)?;
        let result = call_into(&shared, &name, owner, "__sare_ref_call", id, &args)?;
        json_to_pack(lua, &result)
    });
    // State bags, see prelude.lua.
    func!(native, "bag_get", shared, name, |lua,
                                            (bag, key): (
        String,
        String
    )| {
        let value = shared
            .borrow()
            .bags
            .get(&bag)
            .and_then(|b| b.get(&key))
            .cloned();
        match value {
            Some(value) => lua.to_value_with(&value, script::serialize()),
            None => Ok(Value::Nil),
        }
    });
    func!(native, "bag_set", shared, name, |lua,
                                            (
        bag,
        key,
        value,
        replicated,
    ): (
        String,
        String,
        Value,
        bool
    )| {
        let value: Json = lua.from_value_with(value, script::deserialize())?;
        script::set_bag(&shared, &bag, &key, value, replicated);
        Ok(())
    });
    func!(native, "readdir", shared, name, |_lua, path: String| {
        let root = shared.borrow().resources_dir.canonicalize().ok();
        let dir = std::path::Path::new(&path).canonicalize().ok();
        let (Some(root), Some(dir)) = (root, dir) else {
            return Ok(None);
        };
        if !dir.starts_with(&root) {
            return Ok(None);
        }
        let mut names: Vec<String> = fs::read_dir(dir)
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        Ok(Some(names))
    });
    func!(native, "cfxlua", shared, name, |_lua, source: String| {
        Ok(crate::cfxlua::translate(&source).unwrap_or(source))
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

    // msgpack, as FiveM's Lua runtime provides it. Values pass through JSON,
    // so binary strings and integer-keyed maps are not preserved exactly.
    let msgpack = lua.create_table()?;
    let pack = |lua: &Lua, value: Value| -> mlua::Result<mlua::LuaString> {
        let json: Json = lua.from_value_with(value, script::deserialize())?;
        lua.create_string(rmp_serde::to_vec(&json).map_err(mlua::Error::external)?)
    };
    msgpack.set(
        "pack",
        lua.create_function(move |lua, value: Value| pack(lua, value))?,
    )?;
    msgpack.set(
        "pack_args",
        lua.create_function(move |lua, args: mlua::Variadic<Value>| {
            let list = lua.create_sequence_from(args)?;
            pack(lua, Value::Table(list))
        })?,
    )?;
    msgpack.set(
        "unpack",
        lua.create_function(|lua, data: mlua::LuaString| {
            let json: Json =
                rmp_serde::from_slice(&data.as_bytes()).map_err(mlua::Error::external)?;
            lua.to_value_with(&json, script::serialize())
        })?,
    )?;
    msgpack.set(
        "setoption",
        lua.create_function(|_, _: mlua::MultiValue| Ok(()))?,
    )?;
    g.set("msgpack", msgpack)?;

    func!(g, "GetStateBagValue", shared, name, |lua,
                                                (bag, key): (
        String,
        String
    )| {
        let value = shared
            .borrow()
            .bags
            .get(&bag)
            .and_then(|b| b.get(&key))
            .cloned();
        match value {
            Some(value) => lua.to_value_with(&value, script::serialize()),
            None => Ok(Value::Nil),
        }
    });
    func!(g, "SetStateBagValue", shared, name, |_lua,
                                                (
        bag,
        key,
        data,
        _length,
        replicated,
    ): (
        String,
        String,
        mlua::LuaString,
        Option<i64>,
        Option<bool>
    )| {
        let value: Json = rmp_serde::from_slice(&data.as_bytes()).map_err(mlua::Error::external)?;
        script::set_bag(&shared, &bag, &key, value, replicated.unwrap_or(false));
        Ok(())
    });
    func!(g, "StateBagHasKey", shared, name, |_lua,
                                              (bag, key): (
        String,
        String
    )| {
        Ok(shared
            .borrow()
            .bags
            .get(&bag)
            .is_some_and(|b| b.contains_key(&key)))
    });
    func!(g, "GetStateBagKeys", shared, name, |_lua, bag: String| {
        Ok(shared
            .borrow()
            .bags
            .get(&bag)
            .map(|b| b.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default())
    });
    func!(
        g,
        "GetPlayerFromStateBagName",
        shared,
        name,
        |_lua, bag: String| {
            Ok(bag
                .strip_prefix("player:")
                .and_then(|id| id.parse::<u32>().ok())
                .unwrap_or(0))
        }
    );
    func!(
        g,
        "GetEntityFromStateBagName",
        shared,
        name,
        |_lua, bag: String| {
            Ok(bag
                .strip_prefix("entity:")
                .and_then(|id| id.parse::<u32>().ok())
                .unwrap_or(0))
        }
    );

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
    func!(g, "GetConvarBool", shared, name, |_lua,
                                             (key, default): (
        String,
        Option<bool>
    )| {
        Ok(match shared.borrow().convar(&key) {
            Some(v) => matches!(v.to_ascii_lowercase().as_str(), "true" | "1" | "yes" | "on"),
            None => default.unwrap_or(false),
        })
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
            let resource = script::resolve(&shared, &resource).unwrap_or(resource);
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
    func!(
        g,
        "GetNumPlayerIdentifiers",
        shared,
        name,
        |_lua, player: Value| { Ok(usize::from(peer(&shared, &player).is_some())) }
    );
    func!(
        g,
        "GetPlayerIdentifier",
        shared,
        name,
        |_lua, (player, index): (Value, usize)| {
            Ok(peer(&shared, &player)
                .filter(|_| index == 0)
                .map(|p| format!("sare:{}", p.id)))
        }
    );
    // Only the `sare` session type exists: `license`, `discord`, `fivem` and
    // others return nil, so frameworks that require a license reject joins.
    func!(g, "GetPlayerIdentifierByType", shared, name, |_lua,
                                                         (
        player,
        kind,
    ): (
        Value,
        String
    )| {
        Ok(peer(&shared, &player)
            .filter(|_| kind == "sare")
            .map(|p| format!("sare:{}", p.id)))
    });
    func!(
        g,
        "GetNumPlayerTokens",
        shared,
        name,
        |_lua, _player: Value| { Ok(0) }
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
