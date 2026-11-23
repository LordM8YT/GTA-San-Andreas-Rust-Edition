//! `sare_guide`: SARE's own docs, built into the bridge so the assistant
//! reads the version that matches this build, and the list of Lua globals
//! SARE really defines, read from the engine's source at compile time so it
//! never goes stale.
use crate::protocol::Tool;
use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::collections::BTreeSet;

const START: &str = "# SARE for AI assistants

SARE is an open-source game engine and modding platform. Creators make two kinds of things:

## Worlds (Build Mode)
A world is a content pack folder: content.json + layout.json (+ its own .glb models).
- Coordinates: metres, x = east, y = north, z = up. Rotation is degrees [x, y, z]; z turns an object left/right.
- Built-in models stand on their origin (place them at z = 0 on flat ground). Their sizes are in the list_assets labels
  (e.g. \"Road 8 x 32 m\": 8 m wide along x, 32 m long along y; \"Ground plate 32 m\" covers x and y from -16 to 16).
- A new world has a 32 m ground plate at the origin. Add more ground or grass plates before building further out.
- Groups (\"group\" in place_objects) keep the outliner tidy: one group per building, street or area.
- Gameplay assets (category Gameplay) come with their behaviour: \"door\" opens with E, \"marker-checkpoint\"
  numbers itself, \"marker-spawn\" is where players start. Teleport doors and pads need a destination,
  which the creator picks in Build Mode's Inspector; tell them to.
- Workflow: list_worlds / create_world -> list_assets -> place_objects in batches -> get_world to check -> set_spawn.
- The creator sees the result by opening the world in Build Mode (launcher, or `sa-runtime --build <id>`).
  If the world is already open in Build Mode, they must reopen it (File > Open world) to see your changes,
  and saving in Build Mode overwrites them.

## Scripts (Lua resources)
Server resources work like FiveM: resources/[category]/<name>/fxmanifest.lua lists server_script,
client_script and shared_script files; server.cfg starts them with `ensure <name>`.
- Read topic \"lua-api\" for the exact globals SARE has. Do not use FiveM natives that are not listed.
- Client scripts are sandboxed: no files, no os, no NUI/HTML. UI is UI.notify / UI.registerContext /
  UI.inputDialog / UI.progressBar / UI.showTextUI (topic \"client-scripts\").
- Roleplay servers use sare_rp_core and sarebox_core exports; read their files with read_resource_file.
- Each job, shop or place should be its own resource with a Config table (FiveM/QBCore style).
- write_resource_file refuses Lua with syntax errors; fix and write again.

## Rules
- Use only original content. Never copy Rockstar game files or code decompiled from any game.
- Keep files small and readable; creators learn from them.

Other topics: lua-api, resources, client-scripts, server, build-mode, content-packs.";

const TOPICS: [(&str, &str, &str); 6] = [
    (
        "start",
        "How SARE worlds and scripts work, and the workflow (read first)",
        START,
    ),
    (
        "resources",
        "Resource folders, manifests and asset resources",
        include_str!("../../../../docs/creating-resources.md"),
    ),
    (
        "client-scripts",
        "Client Lua sandbox, client API, blips and script UI",
        include_str!("../../../../docs/client-scripts.md"),
    ),
    (
        "server",
        "Running a server, server.cfg and starting resources",
        include_str!("../../../../docs/server-guide.md"),
    ),
    (
        "build-mode",
        "The Build Mode editor and the layout.json format",
        include_str!("../../../../docs/build-mode.md"),
    ),
    (
        "content-packs",
        "Content packs: content.json, worlds, vehicles and characters",
        include_str!("../../../../docs/content-packs.md"),
    ),
];

pub fn tool() -> Tool {
    Tool {
        name: "sare_guide",
        description: "Read SARE's guides. Topics: start (read first), lua-api (every Lua function SARE has, server and client), resources, client-scripts, server, build-mode, content-packs.",
        schema: || {
            let mut names: Vec<&str> = TOPICS.iter().map(|t| t.0).collect();
            names.insert(1, "lua-api");
            json!({
                "type": "object",
                "properties": { "topic": { "type": "string", "enum": names } },
                "required": ["topic"],
            })
        },
        run: |_, args| guide(args["topic"].as_str().unwrap_or("start")),
    }
}

fn guide(topic: &str) -> Result<Value> {
    if topic == "lua-api" {
        return Ok(Value::String(lua_api()));
    }
    match TOPICS.iter().find(|t| t.0 == topic) {
        Some((_, _, text)) => Ok(Value::String((*text).to_owned())),
        None => {
            let known: Vec<String> = TOPICS
                .iter()
                .map(|t| format!("{} ({})", t.0, t.1))
                .collect();
            bail!(
                "unknown topic {topic:?}; topics: lua-api, {}",
                known.join(", ")
            )
        }
    }
}

const SERVER_RS: &str = include_str!("../../server/src/natives.rs");
const SERVER_LUA: &str = include_str!("../../server/src/prelude.lua");
const CLIENT_RS: &str = include_str!("../../lua/src/client.rs");
const CLIENT_LUA: &str = include_str!("../../lua/src/client.lua");
const COMMON_LUA: &str = include_str!("../../lua/src/common.lua");

/// Globals registered from Rust with `func!(g, "Name", ...)`.
fn rust_globals(source: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    let mut rest = source;
    while let Some(at) = rest.find("func!(") {
        rest = &rest[at + 6..];
        let after = rest.trim_start();
        let Some(after) = after.strip_prefix('g') else {
            continue;
        };
        let after = after.trim_start();
        let Some(after) = after.strip_prefix(',') else {
            continue;
        };
        let Some(after) = after.trim_start().strip_prefix('"') else {
            continue;
        };
        if let Some(end) = after.find('"') {
            names.insert(after[..end].to_owned());
        }
    }
    names
}

/// Top-level `function Name(` / `function Table.name(` and `Name = ...`.
fn lua_globals(source: &str) -> BTreeSet<String> {
    let ident = |s: &str| {
        s.chars()
            .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | ':'))
            .collect::<String>()
    };
    source
        .lines()
        .filter_map(|line| {
            if let Some(rest) = line.strip_prefix("function ") {
                return Some(ident(rest));
            }
            let name = ident(line);
            let tail = line[name.len()..].trim_start();
            (!name.is_empty() && tail.starts_with('=') && !tail.starts_with("==")).then_some(name)
        })
        .filter(|n| {
            !n.is_empty()
                && !n.contains("__")
                && !n.contains("_meta")
                && !["load", "print", "local"].contains(&n.as_str())
        })
        .collect()
}

fn lua_api() -> String {
    let common = lua_globals(COMMON_LUA);
    let mut server = rust_globals(SERVER_RS);
    server.extend(lua_globals(SERVER_LUA));
    let mut client = rust_globals(CLIENT_RS);
    client.extend(lua_globals(CLIENT_LUA));
    let list = |set: &BTreeSet<String>| set.iter().cloned().collect::<Vec<_>>().join(", ");
    format!(
        "# Lua globals in this SARE build\n\n\
         Read from the engine source, so this is exactly what exists. Signatures follow FiveM/CfxLua; \
         read a shipped resource (read_resource_file) for real examples.\n\n\
         ## Both sides\n{}\n\nAlso: exports (exports.res:Name(...) and exports('Name', fn)), \
         json.encode/decode, msgpack, vector2/3/4, promise, Citizen.CreateThread/Wait/SetTimeout/Await, \
         source (inside events and commands).\n\n\
         ## Server only (server_script)\n{}\n\n## Client only (client_script, sandboxed)\n{}\n",
        list(&common),
        list(&server),
        list(&client)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_api_lists_real_functions_on_each_side() {
        let api = lua_api();
        let section = |title: &str| {
            let start = api.find(title).unwrap();
            let end = api[start + 3..]
                .find("\n## ")
                .map_or(api.len(), |e| start + 3 + e);
            api[start..end].to_owned()
        };
        let both = section("## Both sides");
        let server = section("## Server only");
        let client = section("## Client only");
        for name in [
            "RegisterNetEvent",
            "TriggerEvent",
            "Citizen.CreateThread",
            "AddEventHandler",
        ] {
            assert!(both.contains(name), "{name}");
        }
        for name in [
            "TriggerClientEvent",
            "GetPlayerName",
            "GetStateBagValue",
            "GlobalState",
        ] {
            assert!(server.contains(name), "server {name}");
        }
        for name in [
            "TriggerServerEvent",
            "GetEntityCoords",
            "AddBlipForCoord",
            "UI.notify",
        ] {
            assert!(client.contains(name), "client {name}");
        }
        assert!(!client.contains("TriggerClientEvent"));
        assert!(!api.contains("__sare"), "internal helpers stay hidden");
        assert!(rust_globals(SERVER_RS).len() > 40);
    }

    #[test]
    fn every_topic_has_text() {
        for (topic, _, _) in TOPICS {
            assert!(
                guide(topic).unwrap().as_str().unwrap().len() > 500,
                "{topic}"
            );
        }
        assert!(guide("lua-api").is_ok());
        assert!(guide("nope").is_err());
    }
}
