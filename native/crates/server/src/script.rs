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
    path::{Path, PathBuf},
    rc::Rc,
    time::Instant,
};

pub type Shared = Rc<RefCell<State>>;

pub struct Resource {
    pub path: PathBuf,
    pub manifest: Manifest,
    pub started: bool,
    pub lua: Option<Lua>,
    /// Names from `provide`, read at refresh and start: `provide 'qb-core'`.
    pub provides: Vec<String>,
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
    /// Function references by `resource:id`, with the number of live proxies
    /// in other resources. Owners keep the functions themselves.
    pub refs: BTreeMap<String, usize>,
    pub next_ref: u64,
    /// References to free at the next tick if nothing holds them by then.
    pub ref_checks: Vec<String>,
    /// Server-side state bags: `global`, `player:<id>`, `entity:<handle>`.
    pub bags: BTreeMap<String, BTreeMap<String, Json>>,
    /// Published client script bundles (resource, SHA-256) in start order,
    /// announced to each joining player.
    pub client_bundles: Vec<(String, String)>,
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
            refs: BTreeMap::new(),
            next_ref: 0,
            ref_checks: Vec::new(),
            bags: BTreeMap::new(),
            client_bundles: Vec::new(),
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

/// Set a state bag value; `null` removes the key. As in FiveM, change
/// handlers run before the value is stored.
pub fn set_bag(shared: &Shared, bag: &str, key: &str, value: Json, replicated: bool) {
    for (resource, lua) in started_states(shared) {
        let result = (|| -> mlua::Result<()> {
            let handler: Function = lua.globals().get("__sare_bag_change")?;
            let value = lua.to_value_with(&value, serialize())?;
            handler.call((bag, key, value, replicated))
        })();
        if let Err(error) = result {
            report_error(shared, &resource, error);
        }
    }
    let mut state = shared.borrow_mut();
    let entries = state.bags.entry(bag.to_string()).or_default();
    if value.is_null() {
        entries.remove(key);
    } else {
        entries.insert(key.to_string(), value);
    }
}

/// Free function references that no other resource claimed: callbacks
/// passed to an export or event that nobody kept.
fn free_unclaimed_refs(shared: &Shared) {
    let unclaimed: Vec<String> = {
        let mut state = shared.borrow_mut();
        let checks = std::mem::take(&mut state.ref_checks);
        let unclaimed: Vec<String> = checks
            .into_iter()
            .filter(|key| state.refs.get(key) == Some(&0))
            .collect();
        for key in &unclaimed {
            state.refs.remove(key);
        }
        unclaimed
    };
    for key in unclaimed {
        let Some((owner, id)) = key.rsplit_once(':') else {
            continue;
        };
        let lua = shared
            .borrow()
            .resources
            .get(owner)
            .and_then(|r| r.lua.clone());
        if let (Some(lua), Ok(id)) = (lua, id.parse::<i64>()) {
            let result = lua
                .globals()
                .get::<Function>("__sare_ref_free")
                .and_then(|free| free.call::<()>(id));
            if let Err(error) = result {
                report_error(shared, owner, error);
            }
        }
    }
}

pub fn tick(shared: &Shared) {
    free_unclaimed_refs(shared);
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
        let provides = provides(&path);
        if let Some(existing) = state.resources.get_mut(&name) {
            existing.path = path;
            if !existing.started {
                existing.provides = provides;
            }
        } else {
            added += 1;
            state.resources.insert(
                name,
                Resource {
                    path,
                    manifest: Manifest::default(),
                    started: false,
                    lua: None,
                    provides,
                },
            );
        }
    }
    Ok(added)
}
fn provides(folder: &std::path::Path) -> Vec<String> {
    manifest::read(folder)
        .map(|m| m.values("provide").map(str::to_string).collect())
        .unwrap_or_default()
}

/// A resource name, or the resource that `provide`s it (a started one first),
/// as FiveM resolves `provide 'qb-core'` for dependencies and exports.
pub fn resolve(shared: &Shared, name: &str) -> Option<String> {
    let state = shared.borrow();
    if state.resources.contains_key(name) {
        return Some(name.to_string());
    }
    let providers = || {
        state
            .resources
            .iter()
            .filter(|(_, r)| r.provides.iter().any(|p| p == name))
    };
    providers()
        .find(|(_, r)| r.started)
        .or_else(|| providers().next())
        .map(|(n, _)| n.clone())
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
        let target = resolve(shared, dependency).unwrap_or_else(|| dependency.to_string());
        start_depth(shared, &target, depth + 1)
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
        resource.provides = manifest.values("provide").map(str::to_string).collect();
        resource.manifest = manifest;
        resource.started = true;
        resource.lua = lua.clone();
    }
    if let Some(lua) = &lua {
        for pattern in scripts {
            let (owner, folder, pattern) = script_source(shared, name, &path, &pattern)?;
            for file in manifest::expand(&folder, &pattern)? {
                let source = std::fs::read_to_string(&file)
                    .with_context(|| format!("Cannot read {}", file.display()))?;
                let chunk_name = format!("@@{owner}/{}", relative(&folder, &file));
                // FiveM's CfxLua syntax (`hash`, +=, ?.) becomes plain Lua;
                // on a translation error Lua reports the original syntax error.
                let source = sa_lua::cfxlua::translate(&source).unwrap_or(source);
                if let Err(error) = lua.load(&source).set_name(chunk_name).exec() {
                    report_error(shared, name, error);
                }
            }
        }
    }
    if client_scripts > 0 {
        if let Err(error) = publish_client(shared, name, &path) {
            out(
                shared,
                format!("{name}: client scripts not sent: {error:#}"),
            );
        }
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
    unpublish_client(shared, name);
    let mut state = shared.borrow_mut();
    let resource = state.resources.get_mut(name).unwrap();
    resource.started = false;
    resource.lua = None;
    state.commands.retain(|_, c| c.resource != name);
    state.exports.retain(|(r, _)| r != name);
    state
        .refs
        .retain(|key, _| key.rsplit_once(':').map(|(owner, _)| owner) != Some(name));
    drop(state);
    out(shared, format!("Stopping resource {name}"));
    Ok(())
}

/// Where a manifest script entry lives: `@other/file.lua` names another
/// resource's file, as `@ox_lib/init.lua` and `@oxmysql/lib/MySQL.lua` expect.
fn script_source(
    shared: &Shared,
    name: &str,
    path: &Path,
    pattern: &str,
) -> Result<(String, PathBuf, String)> {
    match pattern.strip_prefix('@') {
        Some(include) => {
            let Some((other, file)) = include.split_once('/') else {
                bail!("{name}: invalid script include {pattern}");
            };
            let other = resolve(shared, other)
                .with_context(|| format!("{name}: missing resource for {pattern}"))?;
            let folder = shared.borrow().resources[&other].path.clone();
            Ok((other, folder, file.to_string()))
        }
        None => Ok((name.to_string(), path.to_path_buf(), pattern.to_string())),
    }
}
fn relative(folder: &Path, file: &Path) -> String {
    file.strip_prefix(folder)
        .unwrap_or(file)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Text files a client script may read with `LoadResourceFile`.
const CLIENT_TEXT: &[&str] = &["lua", "json", "txt", "cfg", "md", "csv", "xml", "meta"];

/// Send the resource's `shared_script` and `client_script` files (and text
/// `files`) to players: shared first, then client, as FiveM loads them.
fn publish_client(shared: &Shared, name: &str, path: &Path) -> Result<()> {
    let manifest = shared.borrow().resources[name].manifest.clone();
    let mut bundle = sa_lua::bundle::Bundle {
        resource: name.to_string(),
        ..Default::default()
    };
    let scripts: Vec<String> = manifest
        .values("shared_script")
        .chain(manifest.values("client_script"))
        .map(str::to_string)
        .collect();
    for pattern in scripts {
        let (owner, folder, pattern) = script_source(shared, name, path, &pattern)?;
        for file in manifest::expand(&folder, &pattern)? {
            let key = if owner == name {
                relative(&folder, &file)
            } else {
                format!("@{owner}/{}", relative(&folder, &file))
            };
            if !bundle.files.contains_key(&key) {
                let source = std::fs::read_to_string(&file)
                    .with_context(|| format!("Cannot read {}", file.display()))?;
                bundle.files.insert(key.clone(), source);
            }
            if !bundle.scripts.contains(&key) {
                bundle.scripts.push(key);
            }
        }
    }
    for pattern in manifest.values("file") {
        for file in manifest::expand(path, pattern).unwrap_or_default() {
            let text = file
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| CLIENT_TEXT.contains(&e.to_ascii_lowercase().as_str()));
            if text {
                if let Ok(source) = std::fs::read_to_string(&file) {
                    bundle.files.entry(relative(path, &file)).or_insert(source);
                }
            }
        }
    }
    bundle.validate().map_err(anyhow::Error::msg)?;
    let Some(session) = shared.borrow().session.clone() else {
        return Ok(());
    };
    let sha = session.publish_script(name, bundle.to_bytes())?;
    {
        let mut state = shared.borrow_mut();
        state.client_bundles.retain(|(r, _)| r != name);
        state.client_bundles.push((name.to_string(), sha.clone()));
    }
    crate::natives::send_client(
        shared,
        None,
        "__sare:resource",
        &[Json::from(name), Json::from(sha)],
    );
    Ok(())
}
fn unpublish_client(shared: &Shared, name: &str) {
    let (session, published) = {
        let mut state = shared.borrow_mut();
        let before = state.client_bundles.len();
        state.client_bundles.retain(|(r, _)| r != name);
        (state.session.clone(), state.client_bundles.len() != before)
    };
    if let (Some(session), true) = (session, published) {
        session.unpublish_script(name);
        crate::natives::send_client(shared, None, "__sare:resourceStop", &[Json::from(name)]);
    }
}
/// A joining player gets every running resource's client scripts.
pub fn announce_client_bundles(shared: &Shared, player: u32) {
    let bundles = shared.borrow().client_bundles.clone();
    for (name, sha) in bundles {
        crate::natives::send_client(
            shared,
            Some(player),
            "__sare:resource",
            &[Json::from(name), Json::from(sha)],
        );
    }
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
