//! Resource lifecycle and event dispatch. Each started resource owns one Lua
//! state, as in FiveM; resources talk through events and exports only.
//! Rule: never hold the `State` borrow while calling into Lua.
use crate::{
    acl::Acl,
    manifest::{self, Manifest},
};
use anyhow::{bail, Context, Result};
use mlua::{
    serde::{DeserializeOptions, SerializeOptions},
    Function, Lua, LuaSerdeExt, Table, Value,
};
use serde_json::Value as Json;
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    rc::Rc,
    time::Instant,
};

pub type Shared = Rc<RefCell<State>>;

pub struct Resource {
    pub path: PathBuf,
    pub manifest: Manifest,
    pub started: bool,
    pub lua: Option<Lua>,
}
pub struct Command {
    pub resource: String,
    pub restricted: bool,
}
pub struct State {
    pub session: Option<Rc<sa_net::Session>>,
    pub peers: Vec<sa_net::Peer>,
    pub convars: BTreeMap<String, String>,
    /// Convars set with `sets`, published as server information.
    pub server_info: BTreeSet<String>,
    pub acl: Acl,
    pub commands: BTreeMap<String, Command>,
    pub exports: BTreeSet<(String, String)>,
    pub resources: BTreeMap<String, Resource>,
    pub resources_dir: PathBuf,
    /// Queued `ExecuteCommand` lines with their source (0 = console).
    pub pending: Vec<(u32, String)>,
    pub cancelled: bool,
    pub started: Instant,
    /// Captures console output for an rcon reply.
    pub capture: Option<String>,
    pub invoking: Vec<String>,
    /// Set by `assets_locked` once clients have the boot-time asset snapshot.
    pub assets_locked: bool,
}
impl State {
    pub fn new(resources_dir: PathBuf) -> Self {
        Self {
            session: None,
            peers: Vec::new(),
            convars: BTreeMap::new(),
            server_info: BTreeSet::new(),
            acl: Acl::default(),
            commands: BTreeMap::new(),
            exports: BTreeSet::new(),
            resources: BTreeMap::new(),
            resources_dir,
            pending: Vec::new(),
            cancelled: false,
            started: Instant::now(),
            capture: None,
            invoking: Vec::new(),
            assets_locked: false,
        }
    }
    pub fn convar(&self, name: &str) -> Option<&str> {
        self.convars.get(name).map(String::as_str)
    }
}

pub fn out(shared: &Shared, text: impl AsRef<str>) {
    let text = text.as_ref();
    println!("{text}");
    if let Some(capture) = &mut shared.borrow_mut().capture {
        if capture.len() < 64 * 1024 {
            capture.push_str(text);
            capture.push('\n');
        }
    }
}

pub fn serialize() -> SerializeOptions {
    SerializeOptions::new()
        .serialize_none_to_null(false)
        .serialize_unit_to_null(false)
        .set_array_metatable(false)
}
pub fn deserialize() -> DeserializeOptions {
    DeserializeOptions::new()
        .deny_unsupported_types(false)
        .encode_empty_tables_as_array(false)
}
/// `{ n = count, ... }` -> JSON arguments; `nil` becomes `null`.
pub fn pack_to_json(lua: &Lua, args: &Table) -> mlua::Result<Vec<Json>> {
    let n: Option<usize> = args.get("n")?;
    let n = n.unwrap_or(args.raw_len()).min(256);
    (1..=n)
        .map(|i| lua.from_value_with(args.raw_get::<Value>(i)?, deserialize()))
        .collect()
}
pub fn json_to_pack(lua: &Lua, args: &[Json]) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for (i, value) in args.iter().enumerate() {
        table.raw_set(i + 1, lua.to_value_with(value, serialize())?)?;
    }
    table.raw_set("n", args.len())?;
    Ok(table)
}

fn started_states(shared: &Shared) -> Vec<(String, Lua)> {
    shared
        .borrow()
        .resources
        .iter()
        .filter_map(|(name, r)| r.lua.clone().map(|lua| (name.clone(), lua)))
        .collect()
}
fn report_error(shared: &Shared, resource: &str, error: impl std::fmt::Display) {
    out(shared, format!("SCRIPT ERROR in {resource}: {error}"));
}

/// Fire an event in every started resource; returns whether one cancelled it.
pub fn dispatch(shared: &Shared, name: &str, source: u32, args: &[Json], from_net: bool) -> bool {
    let mut cancelled = false;
    for (resource, lua) in started_states(shared) {
        let result = (|| -> mlua::Result<bool> {
            let handler: Function = lua.globals().get("__sare_event")?;
            let packed = json_to_pack(&lua, args)?;
            let source = if source == 0 {
                Value::String(lua.create_string("")?)
            } else {
                Value::Integer(source as i64)
            };
            handler.call((name, source, packed, from_net))
        })();
        match result {
            Ok(c) => cancelled |= c,
            Err(error) => report_error(shared, &resource, error),
        }
    }
    shared.borrow_mut().cancelled = cancelled;
    cancelled
}

pub fn tick(shared: &Shared) {
    let now = shared.borrow().started.elapsed().as_millis() as i64;
    for (resource, lua) in started_states(shared) {
        let result = lua
            .globals()
            .get::<Function>("__sare_tick")
            .and_then(|tick| tick.call::<()>(now));
        if let Err(error) = result {
            report_error(shared, &resource, error);
        }
    }
}

/// Run a script command; returns false when no resource registered it.
pub fn run_command(shared: &Shared, name: &str, source: u32, args: &[String], raw: &str) -> bool {
    let lua = {
        let state = shared.borrow();
        let Some(command) = state.commands.get(name) else {
            return false;
        };
        let Some(lua) = state
            .resources
            .get(&command.resource)
            .and_then(|r| r.lua.clone())
        else {
            return false;
        };
        lua
    };
    let result = (|| -> mlua::Result<()> {
        let handler: Function = lua.globals().get("__sare_command")?;
        let source = if source == 0 {
            Value::Integer(0)
        } else {
            Value::Integer(source as i64)
        };
        handler.call((name, source, args.to_vec(), raw))
    })();
    if let Err(error) = result {
        report_error(shared, name, error);
    }
    true
}

pub fn refresh(shared: &Shared) -> Result<usize> {
    let dir = shared.borrow().resources_dir.clone();
    let found = manifest::discover(&dir)?;
    let mut state = shared.borrow_mut();
    let mut added = 0;
    for (name, path) in found {
        if let Some(existing) = state.resources.get_mut(&name) {
            existing.path = path;
        } else {
            added += 1;
            state.resources.insert(
                name,
                Resource {
                    path,
                    manifest: Manifest::default(),
                    started: false,
                    lua: None,
                },
            );
        }
    }
    Ok(added)
}

pub fn start(shared: &Shared, name: &str) -> Result<()> {
    start_depth(shared, name, 0)
}
fn start_depth(shared: &Shared, name: &str, depth: usize) -> Result<()> {
    anyhow::ensure!(depth < 16, "dependency chain too deep at {name}");
    let path = {
        let state = shared.borrow();
        let Some(resource) = state.resources.get(name) else {
            bail!("Couldn't find resource {name}.");
        };
        if resource.started {
            return Ok(());
        }
        resource.path.clone()
    };
    let manifest = manifest::read(&path)?;
    if manifest.native_assets && shared.borrow().assets_locked {
        bail!("{name} has native assets; clients receive assets at server start. Add `ensure {name}` to server.cfg and restart the server.");
    }
    for dependency in manifest.values("dependency") {
        // FiveM allows version constraints such as `/server:5181`; skip them.
        if dependency.starts_with('/') {
            continue;
        }
        start_depth(shared, dependency, depth + 1)
            .with_context(|| format!("{name} depends on {dependency}"))?;
    }
    let scripts: Vec<String> = manifest
        .values("shared_script")
        .chain(manifest.values("server_script"))
        .map(str::to_string)
        .collect();
    let client_scripts = manifest.values("client_script").count();
    let lua = if scripts.is_empty() {
        None
    } else {
        Some(crate::natives::create_state(shared, name)?)
    };
    {
        let mut state = shared.borrow_mut();
        let resource = state.resources.get_mut(name).unwrap();
        resource.manifest = manifest;
        resource.started = true;
        resource.lua = lua.clone();
    }
    if let Some(lua) = &lua {
        for pattern in scripts {
            if pattern.starts_with('@') {
                out(
                    shared,
                    format!("{name}: cross-resource script {pattern} is not supported"),
                );
                continue;
            }
            for file in manifest::expand(&path, &pattern)? {
                let source = std::fs::read_to_string(&file)
                    .with_context(|| format!("Cannot read {}", file.display()))?;
                let chunk_name = format!(
                    "@{name}/{}",
                    file.strip_prefix(&path).unwrap_or(&file).display()
                );
                if let Err(error) = lua.load(&source).set_name(chunk_name).exec() {
                    report_error(shared, name, error);
                }
            }
        }
    }
    if client_scripts > 0 {
        out(shared, format!("{name}: {client_scripts} client script(s) not run; client Lua is not supported yet"));
    }
    out(shared, format!("Started resource {name}"));
    let args = [Json::String(name.into())];
    dispatch(shared, "onResourceStart", 0, &args, false);
    dispatch(shared, "onServerResourceStart", 0, &args, false);
    Ok(())
}

pub fn stop(shared: &Shared, name: &str) -> Result<()> {
    {
        let state = shared.borrow();
        let Some(resource) = state.resources.get(name) else {
            bail!("Couldn't find resource {name}.");
        };
        if !resource.started {
            return Ok(());
        }
        if resource.manifest.native_assets && state.assets_locked {
            bail!("{name} has native assets that clients loaded at join; remove it from server.cfg and restart the server.");
        }
    }
    let args = [Json::String(name.into())];
    dispatch(shared, "onResourceStop", 0, &args, false);
    dispatch(shared, "onServerResourceStop", 0, &args, false);
    let mut state = shared.borrow_mut();
    let resource = state.resources.get_mut(name).unwrap();
    resource.started = false;
    resource.lua = None;
    state.commands.retain(|_, c| c.resource != name);
    state.exports.retain(|(r, _)| r != name);
    drop(state);
    out(shared, format!("Stopping resource {name}"));
    Ok(())
}

/// `ensure [category]`: every resource inside that bracket folder, sorted.
/// Native-only resources (no fxmanifest) join only when their manifest says
/// `"enabled": true`, so disabled demos stay off.
pub fn category_members(shared: &Shared, category: &str) -> Vec<String> {
    let state = shared.borrow();
    state
        .resources
        .iter()
        .filter(|(_, r)| {
            r.path
                .strip_prefix(&state.resources_dir)
                .is_ok_and(|p| p.components().any(|c| c.as_os_str() == category))
        })
        .filter(|(_, r)| {
            if r.path.join("fxmanifest.lua").is_file() || r.path.join("__resource.lua").is_file() {
                return true;
            }
            ["mod.json", "resource.json"].iter().any(|file| {
                std::fs::read(r.path.join(file))
                    .ok()
                    .and_then(|b| serde_json::from_slice::<Json>(&b).ok())
                    .is_some_and(|m| m["enabled"] == Json::Bool(true))
            })
        })
        .map(|(name, _)| name.clone())
        .collect()
}
pub fn is_category(name: &str) -> bool {
    name.len() > 2 && name.starts_with('[') && name.ends_with(']')
}
