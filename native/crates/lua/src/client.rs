//! Client resources: Lua from the server, run in a sandbox on the player's
//! machine. Each resource gets its own Lua state with only `table`, `string`,
//! `math`, `utf8`, `coroutine` and a read-only `os` subset: no files, no
//! processes, no `debug`, no binary chunks. Memory is capped per resource and
//! a call into Lua that runs too long is aborted.
//!
//! The game feeds a [`View`] each frame and applies the [`Output`]s; scripts
//! reach the game only through the natives defined here.
use crate::{
    bundle::{valid_resource_name, Bundle},
    cfxlua,
    ui::{Owner, Ui},
};
use mlua::{
    serde::{DeserializeOptions, SerializeOptions},
    Function, HookTriggers, Lua, LuaOptions, LuaSerdeExt, StdLib, Table, Value, VmState,
};
use serde_json::{json, Value as Json};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    rc::Rc,
    time::{Duration, Instant},
};

const MEMORY_LIMIT: usize = 64 * 1024 * 1024;
/// Longest a single call into Lua (a tick, an event, a command) may run.
const CALL_LIMIT: Duration = Duration::from_millis(250);
const MAX_OUTPUTS: usize = 256;
const MAX_RESOURCES: usize = 64;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct PlayerView {
    pub id: u32,
    pub name: String,
    /// San Andreas world coordinates.
    pub position: [f32; 3],
    pub heading: f32,
    pub in_vehicle: bool,
}
/// What scripts can read about the game this frame.
#[derive(Clone, Debug, Default)]
pub struct View {
    pub local_id: u32,
    /// Everyone in the session, the local player included.
    pub players: Vec<PlayerView>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Output {
    /// Send to the server as a script event (`payload` is a JSON array).
    ServerEvent {
        name: String,
        payload: String,
    },
    /// Move the local player (SA world coordinates).
    Teleport([f32; 3]),
    Heading(f32),
    /// A line for the F8 console.
    Console(String),
}

struct Resource {
    lua: Lua,
    bundle: Rc<Bundle>,
    sha256: String,
}
#[derive(Default)]
struct Shared {
    view: View,
    now_ms: i64,
    outputs: Vec<Output>,
    ui: Ui,
    resources: BTreeMap<String, Resource>,
    exports: BTreeSet<(String, String)>,
    commands: BTreeMap<String, String>,
    keys: BTreeMap<String, String>,
    refs: BTreeMap<String, usize>,
    next_ref: u64,
    ref_checks: Vec<String>,
    invoking: Vec<String>,
    cancelled: bool,
}
type SharedRef = Rc<RefCell<Shared>>;

fn serialize() -> SerializeOptions {
    SerializeOptions::new()
        .serialize_none_to_null(false)
        .serialize_unit_to_null(false)
        .set_array_metatable(false)
}
fn deserialize() -> DeserializeOptions {
    DeserializeOptions::new()
        .deny_unsupported_types(false)
        .encode_empty_tables_as_array(false)
}
fn pack_to_json(lua: &Lua, args: &Table) -> mlua::Result<Vec<Json>> {
    let n: Option<usize> = args.get("n")?;
    let n = n.unwrap_or(args.raw_len()).min(256);
    (1..=n)
        .map(|i| lua.from_value_with(args.raw_get::<Value>(i)?, deserialize()))
        .collect()
}
fn json_to_pack(lua: &Lua, args: &[Json]) -> mlua::Result<Table> {
    let table = lua.create_table()?;
    for (i, value) in args.iter().enumerate() {
        table.raw_set(i + 1, lua.to_value_with(value, serialize())?)?;
    }
    table.raw_set("n", args.len())?;
    Ok(table)
}
fn push(shared: &SharedRef, output: Output) {
    let mut state = shared.borrow_mut();
    if state.outputs.len() < MAX_OUTPUTS {
        state.outputs.push(output);
    }
}
fn console(shared: &SharedRef, text: String) {
    push(shared, Output::Console(text));
}
fn states(shared: &SharedRef) -> Vec<(String, Lua)> {
    shared
        .borrow()
        .resources
        .iter()
        .map(|(n, r)| (n.clone(), r.lua.clone()))
        .collect()
}
fn player(shared: &SharedRef, id: &Value) -> Option<PlayerView> {
    let state = shared.borrow();
    let id = match id {
        Value::Integer(i) if *i == -1 => state.view.local_id,
        Value::Integer(i) => u32::try_from(*i).ok()?,
        Value::Number(n) => *n as u32,
        Value::String(s) => s.to_str().ok()?.trim().parse().ok()?,
        _ => return None,
    };
    state.view.players.iter().find(|p| p.id == id).cloned()
}

pub struct Host {
    shared: SharedRef,
    /// Announced bundles not loaded yet: SHA-256 -> resource.
    wanted: BTreeMap<String, String>,
    /// Downloaded bundles waiting for a resource they include to start first.
    ready: Vec<(String, Bundle)>,
    requests: Vec<String>,
    deadline: Rc<Cell<Option<Instant>>>,
    started: Instant,
}
impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

impl Host {
    pub fn new() -> Self {
        Self {
            shared: Rc::default(),
            wanted: BTreeMap::new(),
            ready: Vec::new(),
            requests: Vec::new(),
            deadline: Rc::default(),
            started: Instant::now(),
        }
    }
    pub fn resources(&self) -> Vec<String> {
        self.shared.borrow().resources.keys().cloned().collect()
    }
    pub fn ui(&self) -> std::cell::Ref<'_, Ui> {
        std::cell::Ref::map(self.shared.borrow(), |s| &s.ui)
    }
    pub fn ui_mut(&self) -> std::cell::RefMut<'_, Ui> {
        std::cell::RefMut::map(self.shared.borrow_mut(), |s| &mut s.ui)
    }
    pub fn now_ms(&self) -> i64 {
        self.shared.borrow().now_ms
    }
    /// Bundles to fetch from the server (`Session::request_script`).
    pub fn take_requests(&mut self) -> Vec<String> {
        std::mem::take(&mut self.requests)
    }
    pub fn take_outputs(&mut self) -> Vec<Output> {
        std::mem::take(&mut self.shared.borrow_mut().outputs)
    }

    /// Run `f` with the per-call time limit armed.
    fn guarded<T>(&self, f: impl FnOnce() -> T) -> T {
        let outer = self.deadline.get();
        if outer.is_none() {
            self.deadline.set(Some(Instant::now() + CALL_LIMIT));
        }
        let result = f();
        self.deadline.set(outer);
        result
    }

    /// A server event. Returns false when nothing on the client handles it.
    pub fn handle_event(&mut self, name: &str, payload: &str) -> bool {
        let Ok(args) = serde_json::from_str::<Vec<Json>>(payload) else {
            return false;
        };
        let arg = |i: usize| args.get(i).cloned().unwrap_or(Json::Null);
        let now = self.shared.borrow().now_ms;
        match name {
            "__sare:resource" => {
                let (Some(resource), Some(sha)) = (
                    arg(0).as_str().map(str::to_string),
                    arg(1).as_str().map(str::to_string),
                ) else {
                    return true;
                };
                let valid_sha = sha.len() == 64 && sha.bytes().all(|b| b.is_ascii_hexdigit());
                let running = self
                    .shared
                    .borrow()
                    .resources
                    .get(&resource)
                    .is_some_and(|r| r.sha256 == sha);
                if valid_resource_name(&resource) && valid_sha && !running {
                    self.wanted.retain(|_, r| r != &resource);
                    self.wanted.insert(sha.clone(), resource);
                    self.requests.push(sha);
                }
            }
            "__sare:resourceStop" => {
                if let Some(resource) = arg(0).as_str() {
                    self.wanted.retain(|_, r| r != resource);
                    self.ready.retain(|(_, b)| b.resource != resource);
                    self.stop(resource);
                }
            }
            "sare:ui:notify" => self.ui_mut().notify(&arg(0), now),
            "sare:ui:context" => {
                self.ui_mut().context = Ui::parse_context(&arg(0), Owner::Server);
            }
            "sare:ui:hideContext" => self.ui_mut().context = None,
            "sare:ui:text" => {
                self.ui_mut().text = arg(0).as_str().map(|t| crate::ui::text(&json!(t)));
            }
            "sare:ui:dialog" => {
                let token = arg(0).as_i64().unwrap_or(0);
                let dialog = Ui::parse_dialog(&arg(1), &arg(2), Owner::Server, token);
                self.replace_dialog(dialog);
            }
            "sare:ui:progress" => {
                let token = arg(0).as_i64().unwrap_or(0);
                let data = arg(1);
                self.replace_progress(Some(crate::ui::Progress {
                    label: crate::ui::text(&data["label"]),
                    started_ms: now,
                    duration_ms: data["duration"]
                        .as_i64()
                        .unwrap_or(1000)
                        .clamp(100, 120_000),
                    owner: Owner::Server,
                    token,
                }));
            }
            _ => {
                let states = states(&self.shared);
                if states.is_empty() {
                    return false;
                }
                self.guarded(|| {
                    for (resource, lua) in &states {
                        let result = (|| -> mlua::Result<()> {
                            let handler: Function = lua.globals().get("__sare_event")?;
                            handler.call::<Value>((name, "", json_to_pack(lua, &args)?, true))?;
                            Ok(())
                        })();
                        if let Err(error) = result {
                            console(&self.shared, format!("SCRIPT ERROR in {resource}: {error}"));
                        }
                    }
                });
            }
        }
        true
    }

    /// Start a downloaded bundle announced with `__sare:resource`. A bundle
    /// that includes `@other/...` waits until `other` has started, when
    /// `other` is still downloading.
    pub fn load(&mut self, sha256: &str, bytes: &[u8]) -> Result<(), String> {
        let Some(name) = self.wanted.remove(sha256) else {
            return Err("bundle was not announced".into());
        };
        let bundle = Bundle::from_bytes(bytes)?;
        if bundle.resource != name {
            return Err(format!(
                "bundle for {} announced as {name}",
                bundle.resource
            ));
        }
        self.ready.retain(|(_, b)| b.resource != name);
        self.ready.push((sha256.to_string(), bundle));
        self.start_ready()
    }
    fn start_ready(&mut self) -> Result<(), String> {
        let mut result = Ok(());
        loop {
            let waiting = |other: &str, this: &str| {
                other != this
                    && (self.wanted.values().any(|r| r == other)
                        || self.ready.iter().any(|(_, b)| b.resource == other))
            };
            let next = self.ready.iter().position(|(_, bundle)| {
                !bundle.scripts.iter().any(|path| {
                    path.strip_prefix('@')
                        .and_then(|p| p.split_once('/'))
                        .is_some_and(|(other, _)| waiting(other, &bundle.resource))
                })
            });
            // Includes that wait on each other: start in arrival order.
            let index = match next {
                Some(index) => index,
                None if self.ready.is_empty() => return result,
                None if self.wanted.is_empty() => 0,
                None => return result,
            };
            let (sha256, bundle) = self.ready.remove(index);
            if let Err(error) = self.start(&sha256, bundle) {
                result = Err(error);
            }
        }
    }
    fn start(&mut self, sha256: &str, bundle: Bundle) -> Result<(), String> {
        let name = bundle.resource.clone();
        if self.shared.borrow().resources.len() >= MAX_RESOURCES {
            return Err("too many client resources".into());
        }
        self.stop(&name);
        let lua = create_state(&self.shared, &name, &self.deadline).map_err(|e| e.to_string())?;
        let bundle = Rc::new(bundle);
        self.shared.borrow_mut().resources.insert(
            name.clone(),
            Resource {
                lua: lua.clone(),
                bundle: bundle.clone(),
                sha256: sha256.to_string(),
            },
        );
        self.guarded(|| {
            for path in &bundle.scripts {
                let source = cfxlua::translate(&bundle.files[path])
                    .unwrap_or_else(|_| bundle.files[path].clone());
                let chunk = match path.strip_prefix('@') {
                    Some(other) => format!("@@{other}"),
                    None => format!("@@{name}/{path}"),
                };
                if let Err(error) = lua
                    .load(&source)
                    .set_name(chunk)
                    .set_mode(mlua::chunk::ChunkMode::Text)
                    .exec()
                {
                    console(&self.shared, format!("SCRIPT ERROR in {name}: {error}"));
                }
            }
        });
        console(&self.shared, format!("Started client resource {name}"));
        self.dispatch_local(
            &["onClientResourceStart", "onResourceStart"],
            &[json!(name)],
        );
        Ok(())
    }

    pub fn stop(&mut self, name: &str) {
        if !self.shared.borrow().resources.contains_key(name) {
            return;
        }
        self.dispatch_local(&["onClientResourceStop", "onResourceStop"], &[json!(name)]);
        let mut state = self.shared.borrow_mut();
        state.resources.remove(name);
        state.exports.retain(|(r, _)| r != name);
        state.commands.retain(|_, r| r != name);
        let commands: BTreeSet<String> = state.commands.keys().cloned().collect();
        state.keys.retain(|_, c| commands.contains(c));
        state
            .refs
            .retain(|key, _| key.rsplit_once(':').map(|(o, _)| o) != Some(name));
        let owned = Owner::Resource(name.to_string());
        if state.ui.context.as_ref().is_some_and(|c| c.owner == owned) {
            state.ui.context = None;
        }
        if state.ui.dialog.as_ref().is_some_and(|d| d.owner == owned) {
            state.ui.dialog = None;
        }
        if state.ui.progress.as_ref().is_some_and(|p| p.owner == owned) {
            state.ui.progress = None;
        }
    }
    /// Disconnecting: stop every client resource and clear the UI.
    pub fn stop_all(&mut self) {
        for name in self.resources() {
            self.stop(&name);
        }
        self.wanted.clear();
        self.ready.clear();
        self.requests.clear();
        self.shared.borrow_mut().ui = Ui::default();
    }

    fn dispatch_local(&self, names: &[&str], args: &[Json]) {
        let states = states(&self.shared);
        self.guarded(|| {
            for name in names {
                for (resource, lua) in &states {
                    let result = (|| -> mlua::Result<()> {
                        let handler: Function = lua.globals().get("__sare_event")?;
                        handler.call::<Value>((*name, "", json_to_pack(lua, args)?, false))?;
                        Ok(())
                    })();
                    if let Err(error) = result {
                        console(&self.shared, format!("SCRIPT ERROR in {resource}: {error}"));
                    }
                }
            }
        });
    }

    /// One frame: script threads, timers, expiring UI and freed references.
    pub fn tick(&mut self, view: View) {
        let now = self.started.elapsed().as_millis() as i64;
        {
            let mut state = self.shared.borrow_mut();
            state.view = view;
            state.now_ms = now;
            state.ui.expire(now);
        }
        let finished = self
            .shared
            .borrow()
            .ui
            .progress
            .as_ref()
            .filter(|p| now >= p.started_ms + p.duration_ms)
            .map(|p| (p.owner.clone(), p.token));
        if let Some((owner, token)) = finished {
            self.shared.borrow_mut().ui.progress = None;
            self.reply(owner, token, "sare:ui:progressResult", json!(true));
        }
        self.free_unclaimed_refs();
        let states = states(&self.shared);
        self.guarded(|| {
            for (resource, lua) in &states {
                let result = lua
                    .globals()
                    .get::<Function>("__sare_tick")
                    .and_then(|tick| tick.call::<()>(now));
                if let Err(error) = result {
                    console(&self.shared, format!("SCRIPT ERROR in {resource}: {error}"));
                }
            }
        });
    }
    fn free_unclaimed_refs(&self) {
        let unclaimed: Vec<String> = {
            let mut state = self.shared.borrow_mut();
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
            let lua = self
                .shared
                .borrow()
                .resources
                .get(owner)
                .map(|r| r.lua.clone());
            if let (Some(lua), Ok(id)) = (lua, id.parse::<i64>()) {
                let _ = lua
                    .globals()
                    .get::<Function>("__sare_ref_free")
                    .and_then(|free| free.call::<()>(id));
            }
        }
    }

    /// A `/command` typed by the player. True when a client resource has it.
    pub fn command(&mut self, line: &str) -> bool {
        let line = line.trim().trim_start_matches('/');
        let mut words = line.split_whitespace();
        let Some(name) = words.next().map(str::to_ascii_lowercase) else {
            return false;
        };
        let args: Vec<String> = words.map(str::to_string).collect();
        let lua = {
            let state = self.shared.borrow();
            let Some(resource) = state.commands.get(&name) else {
                return false;
            };
            state
                .resources
                .get(resource)
                .map(|r| (resource.clone(), r.lua.clone()))
        };
        let Some((resource, lua)) = lua else {
            return false;
        };
        let local = self.shared.borrow().view.local_id;
        let result = self.guarded(|| {
            lua.globals()
                .get::<Function>("__sare_command")
                .and_then(|f| f.call::<()>((name.as_str(), local, args, line)))
        });
        if let Err(error) = result {
            console(&self.shared, format!("SCRIPT ERROR in {resource}: {error}"));
        }
        true
    }
    /// A key press, by FiveM key name (`F5`, `E`, ...): runs the command a
    /// resource mapped to it with RegisterKeyMapping.
    pub fn key_pressed(&mut self, key: &str) -> bool {
        let command = self
            .shared
            .borrow()
            .keys
            .get(&key.to_ascii_uppercase())
            .cloned();
        command.is_some_and(|command| self.command(&command))
    }

    /// The player picked an option of the open context menu.
    pub fn select_context(&mut self, index: usize) {
        let Some(context) = self.shared.borrow_mut().ui.context.take() else {
            return;
        };
        let Some(option) = context.options.get(index).filter(|o| !o.disabled) else {
            return;
        };
        match &context.owner {
            Owner::Resource(resource) => {
                let lua = self
                    .shared
                    .borrow()
                    .resources
                    .get(resource)
                    .map(|r| r.lua.clone());
                if let Some(lua) = lua {
                    let result = self.guarded(|| {
                        lua.globals()
                            .get::<Function>("__sare_ui_select")
                            .and_then(|f| f.call::<()>((context.id.as_str(), index + 1)))
                    });
                    if let Err(error) = result {
                        console(&self.shared, format!("SCRIPT ERROR in {resource}: {error}"));
                    }
                }
            }
            Owner::Server => {
                if let Some(event) = &option.event {
                    let args = vec![option.args.clone()];
                    let payload = Json::Array(args).to_string();
                    self.handle_event(event, &payload);
                }
                if let Some(event) = &option.server_event {
                    push(
                        &self.shared,
                        Output::ServerEvent {
                            name: event.clone(),
                            payload: json!([option.args]).to_string(),
                        },
                    );
                }
            }
        }
    }
    pub fn close_context(&mut self) {
        self.shared.borrow_mut().ui.context = None;
    }
    /// The open dialog was confirmed (`Some(values)`) or cancelled (`None`).
    pub fn submit_dialog(&mut self, values: Option<Vec<Json>>) {
        let Some(dialog) = self.shared.borrow_mut().ui.dialog.take() else {
            return;
        };
        let value = values.map_or(Json::Null, Json::Array);
        self.reply(dialog.owner, dialog.token, "sare:ui:dialogResult", value);
    }
    /// The player cancelled the running progress bar.
    pub fn cancel_progress(&mut self) {
        let Some(progress) = self.shared.borrow_mut().ui.progress.take() else {
            return;
        };
        self.reply(
            progress.owner,
            progress.token,
            "sare:ui:progressResult",
            json!(false),
        );
    }
    fn replace_dialog(&mut self, dialog: Option<crate::ui::Dialog>) {
        let old = std::mem::replace(&mut self.shared.borrow_mut().ui.dialog, dialog);
        if let Some(old) = old {
            self.reply(old.owner, old.token, "sare:ui:dialogResult", Json::Null);
        }
    }
    fn replace_progress(&mut self, progress: Option<crate::ui::Progress>) {
        let old = std::mem::replace(&mut self.shared.borrow_mut().ui.progress, progress);
        if let Some(old) = old {
            self.reply(old.owner, old.token, "sare:ui:progressResult", json!(false));
        }
    }
    fn reply(&self, owner: Owner, token: i64, server_event: &str, value: Json) {
        reply(
            &self.shared,
            &self.deadline,
            owner,
            token,
            server_event,
            value,
        );
    }
}

fn reply(
    shared: &SharedRef,
    deadline: &Rc<Cell<Option<Instant>>>,
    owner: Owner,
    token: i64,
    server_event: &str,
    value: Json,
) {
    match owner {
        Owner::Server => push(
            shared,
            Output::ServerEvent {
                name: server_event.into(),
                payload: json!([token, value]).to_string(),
            },
        ),
        Owner::Resource(resource) => {
            let lua = shared
                .borrow()
                .resources
                .get(&resource)
                .map(|r| r.lua.clone());
            if let Some(lua) = lua {
                let outer = deadline.get();
                if outer.is_none() {
                    deadline.set(Some(Instant::now() + CALL_LIMIT));
                }
                let result = (|| -> mlua::Result<()> {
                    let f: Function = lua.globals().get("__sare_ui_reply")?;
                    f.call((token, lua.to_value_with(&value, serialize())?))
                })();
                deadline.set(outer);
                if let Err(error) = result {
                    console(shared, format!("SCRIPT ERROR in {resource}: {error}"));
                }
            }
        }
    }
}

#[allow(unused_braces)] // rustfmt wraps some macro closures in braces
fn create_state(
    shared: &SharedRef,
    resource: &str,
    deadline: &Rc<Cell<Option<Instant>>>,
) -> mlua::Result<Lua> {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8 | StdLib::COROUTINE,
        LuaOptions::new(),
    )?;
    lua.set_memory_limit(MEMORY_LIMIT)?;
    {
        let deadline = deadline.clone();
        lua.set_global_hook(
            HookTriggers::new().every_nth_instruction(10_000),
            move |_, _| match deadline.get() {
                Some(limit) if Instant::now() > limit => {
                    Err(mlua::Error::runtime("script ran too long and was stopped"))
                }
                _ => Ok(VmState::Continue),
            },
        )?;
    }
    let g = lua.globals();
    // Text chunks only: binary chunks could crash the game.
    let raw_load: Function = g.get("load")?;
    g.set(
        "load",
        lua.create_function(move |lua, args: mlua::MultiValue| {
            // Keep the argument count: an explicit nil env differs from none.
            let mut args: Vec<Value> = args.into_iter().collect();
            args.resize(args.len().max(3), Value::Nil);
            args[2] = Value::String(lua.create_string("t")?);
            raw_load.call::<mlua::MultiValue>(mlua::MultiValue::from_iter(args))
        })?,
    )?;
    g.set("dofile", Value::Nil)?;
    g.set("loadfile", Value::Nil)?;
    // A read-only os subset.
    let os = lua.create_table()?;
    let started = Instant::now();
    os.set(
        "clock",
        lua.create_function(move |_, ()| Ok(started.elapsed().as_secs_f64()))?,
    )?;
    os.set(
        "time",
        lua.create_function(|_, _: Value| {
            Ok(std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64))
        })?,
    )?;
    g.set("os", os)?;

    let native = lua.create_table()?;
    let name = resource.to_string();
    macro_rules! func {
        ($table:expr, $key:expr, $shared:ident, $name:ident, $deadline:ident, |$lua:ident, $args:tt : $ty:ty| $body:expr) => {{
            let $shared = $shared.clone();
            let $name = $name.clone();
            let $deadline = $deadline.clone();
            #[allow(unused_variables)]
            $table.set(
                $key,
                lua.create_function(move |$lua, $args: $ty| {
                    let ($shared, $name, $deadline) = (&$shared, &$name, &$deadline);
                    $body
                })?,
            )?;
        }};
    }

    func!(
        native,
        "print",
        shared,
        name,
        deadline,
        |_lua, text: String| {
            console(shared, format!("[{name}] {text}"));
            Ok(())
        }
    );
    func!(
        native,
        "error",
        shared,
        name,
        deadline,
        |_lua, text: String| {
            console(shared, format!("SCRIPT ERROR in {name}: {text}"));
            Ok(())
        }
    );
    func!(native, "trigger", shared, name, deadline, |lua,
                                                      (
        event,
        args,
    ): (
        String,
        Table
    )| {
        let args = pack_to_json(lua, &args)?;
        let mut cancelled = false;
        for (resource, other) in states(shared) {
            let result = (|| -> mlua::Result<bool> {
                let handler: Function = other.globals().get("__sare_event")?;
                handler.call((event.as_str(), "", json_to_pack(&other, &args)?, false))
            })();
            match result {
                Ok(c) => cancelled |= c,
                Err(error) => console(shared, format!("SCRIPT ERROR in {resource}: {error}")),
            }
        }
        shared.borrow_mut().cancelled = cancelled;
        Ok(())
    });
    func!(
        native,
        "trigger_server",
        shared,
        name,
        deadline,
        |lua, (event, args): (String, Table)| {
            let payload = Json::Array(pack_to_json(lua, &args)?).to_string();
            if event.is_empty() || event.len() > 64 || payload.len() > 4096 {
                return Err(mlua::Error::runtime(format!(
                    "TriggerServerEvent {event}: name or arguments too large (4 KiB)"
                )));
            }
            push(
                shared,
                Output::ServerEvent {
                    name: event,
                    payload,
                },
            );
            Ok(())
        }
    );
    func!(
        native,
        "was_cancelled",
        shared,
        name,
        deadline,
        |_lua, (): ()| Ok(shared.borrow().cancelled)
    );
    func!(
        native,
        "register_command",
        shared,
        name,
        deadline,
        |_lua, (command, _restricted): (String, bool)| {
            shared
                .borrow_mut()
                .commands
                .insert(command.to_ascii_lowercase(), name.clone());
            Ok(())
        }
    );
    func!(native, "key_mapping", shared, name, deadline, |_lua,
                                                          (
        command,
        key,
    ): (
        String,
        String
    )| {
        shared
            .borrow_mut()
            .keys
            .insert(key.to_ascii_uppercase(), command.to_ascii_lowercase());
        Ok(())
    });
    func!(
        native,
        "export",
        shared,
        name,
        deadline,
        |_lua, export: String| {
            shared.borrow_mut().exports.insert((name.clone(), export));
            Ok(())
        }
    );
    func!(native, "call_export", shared, name, deadline, |lua,
                                                          (
        target,
        export,
        args,
    ): (
        String,
        String,
        Table
    )| {
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
        let result = call_into(shared, name, &target, "__sare_export_call", &export, &args)?;
        json_to_pack(lua, &result)
    });
    func!(native, "ref_new", shared, name, deadline, |_lua, (): ()| {
        let mut state = shared.borrow_mut();
        state.next_ref += 1;
        let id = state.next_ref;
        let key = format!("{name}:{id}");
        state.refs.insert(key.clone(), 0);
        state.ref_checks.push(key.clone());
        Ok((key, id))
    });
    func!(
        native,
        "ref_retain",
        shared,
        name,
        deadline,
        |_lua, key: String| {
            if let Some(count) = shared.borrow_mut().refs.get_mut(&key) {
                *count += 1;
            }
            Ok(())
        }
    );
    func!(
        native,
        "ref_release",
        shared,
        name,
        deadline,
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
    func!(
        native,
        "call_ref",
        shared,
        name,
        deadline,
        |lua, (key, args): (String, Table)| {
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
            let result = call_into(shared, name, owner, "__sare_ref_call", id, &args)?;
            json_to_pack(lua, &result)
        }
    );
    func!(
        native,
        "json_encode",
        shared,
        name,
        deadline,
        |lua, value: Value| {
            let json: Json = lua.from_value_with(value, deserialize())?;
            Ok(json.to_string())
        }
    );
    func!(
        native,
        "json_decode",
        shared,
        name,
        deadline,
        |lua, text: String| {
            match serde_json::from_str::<Json>(&text) {
                Ok(json) => lua.to_value_with(&json, serialize()),
                Err(_) => Ok(Value::Nil),
            }
        }
    );
    func!(
        native,
        "cfxlua",
        shared,
        name,
        deadline,
        |lua, source: mlua::LuaString| {
            match source.to_str() {
                Ok(text) => match cfxlua::translate(&text) {
                    Ok(translated) => Ok(lua.create_string(translated)?),
                    Err(_) => Ok(source.clone()),
                },
                // Not text: `load` rejects it as a binary chunk.
                Err(_) => Ok(source.clone()),
            }
        }
    );
    func!(
        native,
        "ui_notify",
        shared,
        name,
        deadline,
        |lua, data: Value| {
            let data: Json = lua.from_value_with(data, deserialize())?;
            let mut state = shared.borrow_mut();
            let now = state.now_ms;
            state.ui.notify(&data, now);
            Ok(())
        }
    );
    func!(
        native,
        "ui_context",
        shared,
        name,
        deadline,
        |lua, data: Value| {
            let data: Json = lua.from_value_with(data, deserialize())?;
            shared.borrow_mut().ui.context =
                Ui::parse_context(&data, Owner::Resource(name.clone()));
            Ok(())
        }
    );
    func!(native, "ui_hide_context", shared, name, deadline, |_lua,
                                                              (): (
    )| {
        shared.borrow_mut().ui.context = None;
        Ok(())
    });
    func!(native, "ui_text", shared, name, deadline, |_lua,
                                                      text: Option<
        String,
    >| {
        shared.borrow_mut().ui.text = text.map(|t| crate::ui::text(&json!(t)));
        Ok(())
    });
    func!(native, "ui_dialog", shared, name, deadline, |lua,
                                                        (
        heading,
        rows,
        token,
    ): (
        Value,
        Value,
        i64
    )| {
        let heading: Json = lua.from_value_with(heading, deserialize())?;
        let rows: Json = lua.from_value_with(rows, deserialize())?;
        let dialog = Ui::parse_dialog(&heading, &rows, Owner::Resource(name.clone()), token)
            .ok_or_else(|| mlua::Error::runtime("inputDialog rows must be a list"))?;
        let old = shared.borrow_mut().ui.dialog.replace(dialog);
        if let Some(old) = old {
            reply(
                shared,
                deadline,
                old.owner,
                old.token,
                "sare:ui:dialogResult",
                Json::Null,
            );
        }
        Ok(())
    });
    func!(native, "ui_progress", shared, name, deadline, |lua,
                                                          (
        data,
        token,
    ): (
        Value,
        i64
    )| {
        let data: Json = lua.from_value_with(data, deserialize())?;
        let now = shared.borrow().now_ms;
        let progress = crate::ui::Progress {
            label: crate::ui::text(&data["label"]),
            started_ms: now,
            duration_ms: data["duration"]
                .as_i64()
                .unwrap_or(1000)
                .clamp(100, 120_000),
            owner: Owner::Resource(name.clone()),
            token,
        };
        let old = shared.borrow_mut().ui.progress.replace(progress);
        if let Some(old) = old {
            reply(
                shared,
                deadline,
                old.owner,
                old.token,
                "sare:ui:progressResult",
                json!(false),
            );
        }
        Ok(())
    });
    g.set("__sare", native)?;

    func!(
        g,
        "GetCurrentResourceName",
        shared,
        name,
        deadline,
        |_lua, (): ()| Ok(name.clone())
    );
    func!(g, "GetInvokingResource", shared, name, deadline, |_lua,
                                                             (): (
    )| Ok(
        shared.borrow().invoking.last().cloned()
    ));
    func!(
        g,
        "GetGameTimer",
        shared,
        name,
        deadline,
        |_lua, (): ()| Ok(shared.borrow().now_ms)
    );
    func!(g, "GetGameName", shared, name, deadline, |_lua, (): ()| Ok(
        "sare"
    ));
    func!(
        g,
        "IsDuplicityVersion",
        shared,
        name,
        deadline,
        |_lua, (): ()| Ok(false)
    );
    func!(
        g,
        "GetConvar",
        shared,
        name,
        deadline,
        |_lua, (_key, default): (String, Option<String>)| Ok(default.unwrap_or_default())
    );
    func!(g, "GetConvarInt", shared, name, deadline, |_lua,
                                                      (
        _key,
        default,
    ): (
        String,
        Option<i64>
    )| Ok(
        default.unwrap_or(0)
    ));
    func!(
        g,
        "GetResourceState",
        shared,
        name,
        deadline,
        |_lua, resource: String| {
            Ok(if shared.borrow().resources.contains_key(&resource) {
                "started"
            } else {
                "missing"
            })
        }
    );
    func!(g, "LoadResourceFile", shared, name, deadline, |_lua,
                                                          (
        resource,
        file,
    ): (
        String,
        String
    )| {
        let state = shared.borrow();
        let file = file.trim_start_matches("./").replace('\\', "/");
        Ok(state
            .resources
            .get(&resource)
            .and_then(|r| r.bundle.files.get(&file).cloned()))
    });
    func!(g, "PlayerId", shared, name, deadline, |_lua, (): ()| Ok(
        shared.borrow().view.local_id
    ));
    func!(g, "PlayerPedId", shared, name, deadline, |_lua, (): ()| Ok(
        shared.borrow().view.local_id
    ));
    func!(
        g,
        "GetPlayerServerId",
        shared,
        name,
        deadline,
        |_lua, player_id: Value| { Ok(player(shared, &player_id).map_or(0, |p| p.id)) }
    );
    func!(
        g,
        "GetPlayerFromServerId",
        shared,
        name,
        deadline,
        |_lua, id: Value| { Ok(player(shared, &id).map_or(-1, |p| p.id as i64)) }
    );
    func!(
        g,
        "GetPlayerPed",
        shared,
        name,
        deadline,
        |_lua, player_id: Value| { Ok(player(shared, &player_id).map_or(0, |p| p.id)) }
    );
    func!(
        g,
        "GetPlayerName",
        shared,
        name,
        deadline,
        |_lua, player_id: Value| Ok(player(shared, &player_id).map(|p| p.name))
    );
    func!(
        g,
        "NetworkIsPlayerActive",
        shared,
        name,
        deadline,
        |_lua, player_id: Value| Ok(player(shared, &player_id).is_some())
    );
    func!(
        g,
        "GetActivePlayers",
        shared,
        name,
        deadline,
        |_lua, (): ()| {
            Ok(shared
                .borrow()
                .view
                .players
                .iter()
                .map(|p| p.id)
                .collect::<Vec<_>>())
        }
    );
    func!(
        g,
        "DoesEntityExist",
        shared,
        name,
        deadline,
        |_lua, entity: Value| Ok(player(shared, &entity).is_some())
    );
    func!(
        g,
        "IsPedAPlayer",
        shared,
        name,
        deadline,
        |_lua, entity: Value| Ok(player(shared, &entity).is_some())
    );
    func!(
        g,
        "GetEntityCoords",
        shared,
        name,
        deadline,
        |lua, entity: Value| {
            let [x, y, z] = player(shared, &entity).map_or([0.0; 3], |p| p.position);
            let make: Function = lua.globals().get("vector3")?;
            make.call::<Value>((x, y, z))
        }
    );
    func!(
        g,
        "GetEntityHeading",
        shared,
        name,
        deadline,
        |_lua, entity: Value| Ok(player(shared, &entity).map_or(0.0, |p| p.heading))
    );
    func!(g, "IsPedInAnyVehicle", shared, name, deadline, |_lua,
                                                           (
        ped,
        _last,
    ): (
        Value,
        Option<bool>
    )| {
        Ok(player(shared, &ped).is_some_and(|p| p.in_vehicle))
    });
    let local_only = |shared: &SharedRef, entity: &Value| {
        player(shared, entity).is_some_and(|p| p.id == shared.borrow().view.local_id)
    };
    func!(g, "SetEntityCoords", shared, name, deadline, |_lua,
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
        if local_only(shared, &entity) && [x, y, z].iter().all(|v| v.is_finite()) {
            push(shared, Output::Teleport([x, y, z]));
        }
        Ok(())
    });
    func!(g, "SetEntityHeading", shared, name, deadline, |_lua,
                                                          (
        entity,
        heading,
    ): (
        Value,
        f32
    )| {
        if local_only(shared, &entity) && heading.is_finite() {
            push(shared, Output::Heading(heading));
        }
        Ok(())
    });
    func!(
        g,
        "ExecuteCommand",
        shared,
        name,
        deadline,
        |_lua, line: String| {
            // A client resource's command runs here; anything else goes to the server.
            let line = line.trim().trim_start_matches('/').to_string();
            let mut words = line.split_whitespace();
            let command = words.next().unwrap_or_default().to_ascii_lowercase();
            let args: Vec<String> = words.map(str::to_string).collect();
            let target = {
                let state = shared.borrow();
                state
                    .commands
                    .get(&command)
                    .and_then(|r| state.resources.get(r))
                    .map(|r| (r.lua.clone(), state.view.local_id))
            };
            match target {
                Some((lua, local)) => lua
                    .globals()
                    .get::<Function>("__sare_command")?
                    .call::<()>((command, local, args, line)),
                None => {
                    push(
                        shared,
                        Output::ServerEvent {
                            name: "__cfx_internal:commandFallback".into(),
                            payload: json!([line]).to_string(),
                        },
                    );
                    Ok(())
                }
            }
        }
    );

    let prelude = format!("{}\n{}", crate::COMMON_PRELUDE, include_str!("client.lua"));
    lua.load(&prelude)
        .set_name("@citizen:/scripting/lua/scheduler.lua")
        .exec()?;
    Ok(lua)
}

fn call_into(
    shared: &SharedRef,
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
        .map(|r| r.lua.clone())
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

#[cfg(test)]
mod tests {
    use super::*;

    fn sha(n: u8) -> String {
        format!("{n:064x}")
    }
    fn start(host: &mut Host, n: u8, resource: &str, files: &[(&str, &str)]) {
        assert!(host.handle_event("__sare:resource", &json!([resource, sha(n)]).to_string()));
        assert_eq!(host.take_requests(), vec![sha(n)]);
        let bundle = Bundle {
            resource: resource.into(),
            scripts: files
                .iter()
                .filter(|(p, _)| p.ends_with(".lua"))
                .map(|(p, _)| p.to_string())
                .collect(),
            files: files
                .iter()
                .map(|(p, s)| (p.to_string(), s.to_string()))
                .collect(),
        };
        host.load(&sha(n), &bundle.to_bytes()).unwrap();
    }
    fn view() -> View {
        View {
            local_id: 3,
            players: vec![PlayerView {
                id: 3,
                name: "CJ".into(),
                position: [2495.0, -1687.0, 13.5],
                heading: 90.0,
                in_vehicle: false,
            }],
        }
    }
    fn consoles(host: &mut Host) -> Vec<String> {
        host.take_outputs()
            .into_iter()
            .filter_map(|o| match o {
                Output::Console(text) => Some(text),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn sandbox_has_no_files_processes_or_debug() {
        let mut host = Host::new();
        start(
            &mut host,
            1,
            "probe",
            &[(
                "client.lua",
                "print(io, debug, require, dofile, os.execute, os.getenv, package)\n\
                 print(load(string.dump(function() end)))\n\
                 print(type(os.time()), CfxLuaCheck?.x, `prop` ~= 0)",
            )],
        );
        let lines = consoles(&mut host);
        assert_eq!(lines[0], "[probe] nil\tnil\tnil\tnil\tnil\tnil\tnil");
        assert!(lines[1].starts_with("[probe] nil\t"), "{lines:?}");
        assert!(lines[1].contains("binary"), "{lines:?}");
        assert_eq!(lines[2], "[probe] number\tnil\ttrue");
    }

    #[test]
    fn runaway_scripts_are_stopped() {
        let mut host = Host::new();
        start(
            &mut host,
            1,
            "spin",
            &[(
                "client.lua",
                "CreateThread(function() while true do end end)",
            )],
        );
        let lines = consoles(&mut host);
        assert!(
            lines.iter().any(|l| l.contains("ran too long")),
            "{lines:?}"
        );
        // The host keeps working after the abort.
        host.tick(view());
        start(
            &mut host,
            2,
            "ok",
            &[(
                "client.lua",
                "print(GetPlayerName(PlayerId()), GetEntityCoords(PlayerPedId()).x)",
            )],
        );
        assert!(consoles(&mut host).contains(&"[ok] CJ\t2495.0".to_string()));
    }

    #[test]
    fn events_commands_and_exports_cross_resources() {
        let mut host = Host::new();
        start(
            &mut host,
            1,
            "lib",
            &[(
                "client.lua",
                "exports('greet', function(name, cb) return 'hi ' .. tostring(name), cb(2) end)\n\
                 RegisterNetEvent('demo:ping', function(n) TriggerServerEvent('demo:pong', n + 1) end)\n\
                 AddEventHandler('demo:local', function() print('local only') end)",
            )],
        );
        start(
            &mut host,
            2,
            "app",
            &[(
                "client.lua",
                "RegisterCommand('hello', function(_, args)\n\
                   print(exports.lib:greet(args[1], function(x) return x * 10 end))\n\
                   SetEntityCoords(PlayerPedId(), 1.0, 2.0, 3.0)\n\
                   SetEntityCoords(99, 1.0, 2.0, 3.0)\n\
                 end)\n\
                 RegisterKeyMapping('hello', 'Say hello', 'keyboard', 'F5')",
            )],
        );
        host.tick(view());
        host.take_outputs();
        assert!(host.handle_event("demo:ping", "[41]"));
        assert!(host.handle_event("demo:local", "[]"));
        assert_eq!(
            host.take_outputs(),
            vec![
                Output::ServerEvent {
                    name: "demo:pong".into(),
                    payload: "[42]".into()
                },
                Output::Console(
                    "[lib] event demo:local was not safe for net, use RegisterNetEvent".into()
                ),
            ]
        );
        assert!(host.command("/hello CJ"));
        assert!(!host.command("/unknown"));
        assert!(host.key_pressed("f5"));
        let outputs = host.take_outputs();
        assert_eq!(outputs[0], Output::Console("[app] hi CJ\t20".into()));
        assert_eq!(outputs[1], Output::Teleport([1.0, 2.0, 3.0]));
        assert_eq!(outputs.len(), 4, "{outputs:?}");
        host.handle_event("__sare:resourceStop", "[\"app\"]");
        assert!(!host.command("/hello"));
        assert_eq!(host.resources(), vec!["lib".to_string()]);
    }

    #[test]
    fn bundles_must_be_announced_and_match() {
        let mut host = Host::new();
        let bundle = Bundle {
            resource: "x".into(),
            ..Bundle::default()
        };
        assert!(host.load(&sha(1), &bundle.to_bytes()).is_err());
        host.handle_event("__sare:resource", &json!(["y", sha(1)]).to_string());
        assert!(host.load(&sha(1), &bundle.to_bytes()).is_err());
        host.handle_event("__sare:resource", &json!(["../x", sha(2)]).to_string());
        host.handle_event("__sare:resource", &json!(["x", "nothex"]).to_string());
        assert!(host.take_requests().len() == 1);
    }

    #[test]
    fn ui_menus_dialogs_and_progress_reply_to_their_owner() {
        let mut host = Host::new();
        start(
            &mut host,
            1,
            "menu",
            &[(
                "client.lua",
                "UI.registerContext({ id = 'main', title = 'Menu', options = {\n\
                   { title = 'Bank', onSelect = function()\n\
                       local values = UI.inputDialog('Deposit', { { type = 'number', label = 'Amount', required = true } })\n\
                       print('dialog', values and values[1])\n\
                       print('progress', UI.progressBar({ label = 'Counting', duration = 100 }))\n\
                     end },\n\
                   { title = 'Locked', disabled = true },\n\
                 } })\n\
                 RegisterCommand('menu', function() UI.showContext('main') end)",
            )],
        );
        host.command("menu");
        assert_eq!(host.ui().context.as_ref().unwrap().options.len(), 2);
        assert!(host.ui().wants_cursor());
        host.select_context(1);
        assert!(
            host.ui().context.is_none(),
            "disabled options close the menu without running"
        );
        host.command("menu");
        host.select_context(0);
        host.tick(view());
        assert_eq!(host.ui().dialog.as_ref().unwrap().heading, "Deposit");
        host.submit_dialog(Some(vec![json!(250)]));
        host.tick(view());
        assert!(host.ui().progress.is_some());
        std::thread::sleep(Duration::from_millis(120));
        host.tick(view());
        host.tick(view());
        let lines = consoles(&mut host);
        assert!(
            lines.contains(&"[menu] dialog\t250".to_string()),
            "{lines:?}"
        );
        assert!(
            lines.contains(&"[menu] progress\ttrue".to_string()),
            "{lines:?}"
        );

        // Server-owned UI answers the server.
        host.handle_event("sare:ui:dialog", &json!([7, "Name", ["First"]]).to_string());
        host.submit_dialog(None);
        host.handle_event(
            "sare:ui:context",
            &json!([{ "title": "Shop", "options": [{ "title": "Buy", "serverEvent": "shop:buy", "args": { "item": "bread" } }] }]).to_string(),
        );
        host.select_context(0);
        assert_eq!(
            host.take_outputs(),
            vec![
                Output::ServerEvent {
                    name: "sare:ui:dialogResult".into(),
                    payload: "[7,null]".into()
                },
                Output::ServerEvent {
                    name: "shop:buy".into(),
                    payload: r#"[{"item":"bread"}]"#.into()
                },
            ]
        );
    }
}
