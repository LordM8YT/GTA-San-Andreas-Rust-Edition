//! World scripts: the Lua creators put in a Build Mode world, on its objects
//! or for the whole world. The same host runs them in Build Mode's playtest,
//! in worlds played from the launcher and on servers (once per world
//! instance), so a world behaves the same everywhere.
//!
//! The host knows nothing of the game: it is given the world's objects and
//! players, gameplay [`Event`]s and time, and returns [`Request`]s (show an
//! object, message or teleport a player, set the hour or weather) for the
//! game or server to carry out. Each script runs in its own sandboxed Lua
//! state: no files, processes or debug library, a memory limit, and at most
//! [`CALL_LIMIT`] of work per script per frame. A script that goes over is
//! stopped for good (catching the stop with `pcall` does not help), and
//! string patterns too costly for their text are refused, so an endless
//! loop or a runaway match cannot freeze a game or a server. See
//! `docs/world-scripts.md` for the script API.
use mlua::{Function, HookTriggers, Lua, LuaOptions, StdLib, Table, Value, VmState};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

const PRELUDE: &str = include_str!("world.lua");
/// Longest a script may run in one frame, all its calls together.
pub const CALL_LIMIT: Duration = Duration::from_millis(250);
/// Memory per script.
const MEMORY_LIMIT: usize = 16 * 1024 * 1024;
/// Requests one frame may make, all scripts together.
const MAX_REQUESTS: usize = 2_000;
/// Output lines kept between two [`WorldScripts::take_output`] calls.
const MAX_OUTPUT: usize = 500;
/// Longest message text, characters.
const MAX_MESSAGE: usize = 500;
/// Largest `world.emit` payload, bytes of JSON.
const MAX_EMIT: usize = 4096;
/// `world.emit` calls per frame.
const MAX_EMITS: usize = 200;

/// One script to run.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub name: String,
    pub source: String,
    /// The object it is on (`script.object`), or None for a world script.
    pub object: Option<u64>,
}

/// Most attributes and tags an object has (the layout's limits).
const MAX_ATTRIBUTES: usize = 64;
const MAX_TAGS: usize = 32;
/// Longest attribute or tag name and attribute text, bytes.
const MAX_NAME: usize = 64;
const MAX_TEXT: usize = 1024;

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_NAME
        && name.trim() == name
        && !name.chars().any(char::is_control)
}

/// One of an object's own values (see `object:get`).
#[derive(Clone, Debug, PartialEq)]
pub enum Attribute {
    Bool(bool),
    Number(f64),
    Text(String),
    /// x, y, z.
    Vector([f64; 3]),
}

/// An object of the world as scripts see it.
#[derive(Clone, Debug, PartialEq)]
pub struct ObjectInfo {
    pub id: u64,
    pub name: String,
    /// World metres: x east, y north, z up.
    pub position: [f32; 3],
    /// Degrees, 0 faces north.
    pub heading: f32,
    /// Scripts may show, hide and move it (it is drawn and solid on its
    /// own).
    pub changeable: bool,
    /// Scripts may colour and copy it (a model, not terrain, a road or an
    /// object without a model).
    pub paintable: bool,
    /// Shown as the world starts.
    pub visible: bool,
    /// Its attributes, with its groups' (scripts may change them while
    /// the world runs).
    pub attributes: std::collections::BTreeMap<String, Attribute>,
    /// Its tags, with its groups'.
    pub tags: Vec<String>,
}

/// A player in the world.
#[derive(Clone, Debug, PartialEq)]
pub struct PlayerInfo {
    pub id: u32,
    pub name: String,
    /// Feet, world metres; None before they have spawned.
    pub position: Option<[f32; 3]>,
    pub driving: bool,
}

/// What the host is started with.
#[derive(Clone, Debug, Default)]
pub struct Config {
    pub scripts: Vec<Source>,
    pub objects: Vec<ObjectInfo>,
    /// Weather names `world.setWeather` accepts.
    pub weathers: Vec<String>,
    pub hour: Option<u8>,
    pub weather: Option<String>,
    /// The id the first copy a script makes gets (later ones count up).
    pub first_copy: u64,
    /// Most copies the world can have at once (0: scripts cannot copy).
    pub max_copies: usize,
    /// Copies the world has already, made by these scripts before they
    /// last stopped: (copy, the placed object it copies). Their objects
    /// are in `objects`.
    pub copies: Vec<(u64, u64)>,
}

/// Something that happened in the world, for `world.on`.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A player walked or drove into an object's trigger zone.
    Enter {
        player: u32,
        object: u64,
    },
    Leave {
        player: u32,
        object: u64,
    },
    /// A player pressed E at an object's interaction.
    Use {
        player: u32,
        object: u64,
    },
    /// Reached checkpoint `index` (1 is the start) of `total`.
    Checkpoint {
        player: u32,
        race: String,
        index: usize,
        total: usize,
    },
    Finish {
        player: u32,
        race: String,
        seconds: f32,
    },
    Join {
        player: u32,
    },
    Quit {
        player: u32,
    },
}

/// Where a teleport goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Object(u64),
    /// A spot in world metres, with a heading in degrees.
    Point([f32; 3], f32),
}

/// What scripts ask the game to do.
#[derive(Clone, Debug, PartialEq)]
pub enum Request {
    Visible {
        object: u64,
        visible: bool,
    },
    /// For one player, or everyone in the world (None).
    Message {
        player: Option<u32>,
        text: String,
        seconds: f32,
    },
    Teleport {
        player: u32,
        to: Target,
    },
    /// Move an object to `position` (world metres) facing `heading`
    /// (degrees), smoothly over `seconds` (0: at once).
    Move {
        object: u64,
        position: [f32; 3],
        heading: f32,
        seconds: f32,
    },
    /// An object's colour, a multiplier per channel (None: its own again).
    Color {
        object: u64,
        color: Option<[f32; 3]>,
    },
    /// Make copy `id` of object `source` at `position` facing `heading`:
    /// shown, in the source's colour, changeable like it.
    Copy {
        source: u64,
        id: u64,
        position: [f32; 3],
        heading: f32,
    },
    /// Remove a copy for good.
    Destroy {
        object: u64,
    },
    /// A line of on-screen text for one player, or everyone (None); text
    /// None removes it.
    Hud {
        player: Option<u32>,
        key: String,
        text: Option<String>,
    },
    /// Set the hour (a running clock jumps there and keeps running).
    Hour(u8),
    Weather(String),
}

/// A line a script printed, or an error in it.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub script: String,
    pub text: String,
    pub error: bool,
}

/// The world right now, as the game sees it.
#[derive(Clone, Debug, Default)]
pub struct Frame {
    /// Seconds since the last frame.
    pub dt: f32,
    pub players: Vec<PlayerInfo>,
    pub hour: Option<u8>,
    pub weather: Option<String>,
    /// Changeable objects whose visibility differs from the start (a door
    /// a trigger opened).
    pub visible: Vec<(u64, bool)>,
    /// Objects moved from where they were placed: where they are now
    /// (position, heading).
    pub poses: Vec<(u64, [f32; 3], f32)>,
    /// ...and where their move is taking them.
    pub targets: Vec<(u64, [f32; 3], f32)>,
    /// Colours scripts gave objects.
    pub colors: Vec<(u64, [f32; 3])>,
    /// The copies scripts made that the world has now: (copy, the placed
    /// object it copies).
    pub copies: Vec<(u64, u64)>,
}

/// A copy made since the world last heard from the scripts: kept until the
/// world has it (or turned it down).
#[derive(Clone, Copy)]
struct Pending {
    position: [f32; 3],
    heading: f32,
    color: Option<[f32; 3]>,
}

struct Inner {
    objects: HashMap<u64, ObjectInfo>,
    order: Vec<u64>,
    names: HashMap<String, u64>,
    visible: HashMap<u64, bool>,
    weathers: Vec<String>,
    players: Vec<PlayerInfo>,
    hour: Option<u8>,
    weather: Option<String>,
    requests: Vec<Request>,
    flooded: bool,
    output: Vec<Line>,
    dropped: usize,
    emits: Vec<(String, serde_json::Value)>,
    /// The script running now, for print and errors.
    current: String,
    /// Players as last seen, so `quit` handlers still know who left.
    known: HashMap<u32, PlayerInfo>,
    /// Where moved objects are now.
    poses: HashMap<u64, ([f32; 3], f32)>,
    /// Where their latest move takes them: a move that changes only the
    /// position (or heading) keeps the other half of it.
    targets: HashMap<u64, ([f32; 3], f32)>,
    /// Colours scripts gave objects.
    colors: HashMap<u64, [f32; 3]>,
    /// On-screen line names in use: everyone's (None) and each player's.
    hud: HashMap<Option<u32>, std::collections::HashSet<String>>,
    /// Copies scripts made, and the placed object each copies.
    copies: HashMap<u64, u64>,
    /// Copies made since the requests were last taken.
    pending: HashMap<u64, Pending>,
    next_copy: u64,
    max_copies: usize,
}
impl Inner {
    fn line(&mut self, text: String, error: bool) {
        if self.output.len() >= MAX_OUTPUT {
            self.dropped += 1;
            return;
        }
        self.output.push(Line {
            script: self.current.clone(),
            text,
            error,
        });
    }
    /// Drop copy `id` from what scripts see.
    fn forget(&mut self, id: u64) {
        self.copies.remove(&id);
        self.pending.remove(&id);
        self.objects.remove(&id);
        self.order.retain(|o| *o != id);
        self.visible.remove(&id);
        self.poses.remove(&id);
        self.targets.remove(&id);
        self.colors.remove(&id);
    }
    fn request(&mut self, request: Request) -> Result<(), String> {
        if self.requests.len() >= MAX_REQUESTS {
            self.flooded = true;
            return Err(format!(
                "scripts asked for more than {MAX_REQUESTS} changes in one frame"
            ));
        }
        self.requests.push(request);
        Ok(())
    }
}
type Shared = Rc<RefCell<Inner>>;

struct Script {
    name: String,
    lua: Lua,
    /// False after its body failed to load or it ran too long: it is not
    /// called again.
    running: bool,
}

/// The work limit of the script running now: when it ends, and whether it
/// was passed (the script is then stopped, whatever it catches).
#[derive(Default)]
struct Limit {
    until: Cell<Option<Instant>>,
    passed: Cell<bool>,
}

/// The scripts of one world (or one world instance on a server).
pub struct WorldScripts {
    shared: Shared,
    scripts: Vec<Script>,
    limit: Rc<Limit>,
    time: f64,
}

impl WorldScripts {
    /// Load and start every script: each body runs once now (until its
    /// first `wait`). Problems show in [`Self::take_output`].
    pub fn new(config: Config) -> Self {
        let names = config
            .objects
            .iter()
            .rev()
            .map(|o| (o.name.clone(), o.id))
            .collect();
        let shared = Rc::new(RefCell::new(Inner {
            order: config.objects.iter().map(|o| o.id).collect(),
            visible: config.objects.iter().map(|o| (o.id, o.visible)).collect(),
            objects: config.objects.into_iter().map(|o| (o.id, o)).collect(),
            names,
            weathers: config.weathers,
            players: Vec::new(),
            hour: config.hour,
            weather: config.weather,
            requests: Vec::new(),
            flooded: false,
            output: Vec::new(),
            dropped: 0,
            emits: Vec::new(),
            current: String::new(),
            known: HashMap::new(),
            poses: HashMap::new(),
            targets: HashMap::new(),
            colors: HashMap::new(),
            hud: HashMap::new(),
            copies: config.copies.into_iter().collect(),
            pending: HashMap::new(),
            next_copy: config.first_copy,
            max_copies: config.max_copies,
        }));
        let mut host = Self {
            shared,
            scripts: Vec::new(),
            limit: Rc::new(Limit::default()),
            time: 0.0,
        };
        for source in config.scripts {
            host.shared.borrow_mut().current = source.name.clone();
            match create_state(&host.shared, &source, &host.limit) {
                Ok(lua) => {
                    let mut script = Script {
                        name: source.name.clone(),
                        lua,
                        running: true,
                    };
                    let loaded = script
                        .lua
                        .load(&source.source)
                        .set_name(format!("={}", source.name))
                        .into_function();
                    match loaded {
                        Ok(body) => {
                            host.limit.until.set(Some(Instant::now() + CALL_LIMIT));
                            host.call(&mut script, |lua| {
                                let start: Function = lua.globals().get("__world_start")?;
                                start.call::<()>(body.clone())
                            });
                            host.limit.until.set(None);
                        }
                        Err(error) => {
                            script.running = false;
                            host.shared.borrow_mut().line(clean(&error), true);
                        }
                    }
                    host.scripts.push(script);
                }
                Err(error) => host
                    .shared
                    .borrow_mut()
                    .line(format!("could not start: {error}"), true),
            }
        }
        host
    }

    /// How many scripts were started (some may have failed to load).
    pub fn len(&self) -> usize {
        self.scripts.len()
    }
    pub fn is_empty(&self) -> bool {
        self.scripts.is_empty()
    }

    /// One frame: pass on what happened, wake scripts whose wait is over,
    /// and return what they ask for, oldest first.
    pub fn tick(&mut self, frame: Frame, events: Vec<Event>) -> Vec<Request> {
        self.time += f64::from(frame.dt.clamp(0.0, 1.0));
        {
            let mut inner = self.shared.borrow_mut();
            // Who left is still known to the scripts this frame.
            let mut players = frame.players.clone();
            for event in &events {
                if let Event::Quit { player } = event {
                    if !players.iter().any(|p| p.id == *player) {
                        if let Some(known) = inner.known.get(player) {
                            players.push(known.clone());
                        }
                    }
                }
            }
            inner.known = frame.players.into_iter().map(|p| (p.id, p)).collect();
            inner.players = players;
            inner.hour = frame.hour;
            inner.weather = frame.weather;
            for (id, object) in &inner.objects.clone() {
                inner.visible.insert(*id, object.visible);
            }
            for (id, visible) in frame.visible {
                inner.visible.insert(id, visible);
            }
            inner.poses = frame
                .poses
                .into_iter()
                .map(|(id, position, heading)| (id, (position, heading)))
                .collect();
            inner.colors = frame.colors.into_iter().collect();
            inner.targets = frame
                .targets
                .into_iter()
                .map(|(id, position, heading)| (id, (position, heading)))
                .collect();
            // Copies the world no longer has (a reset, or one it turned
            // down) are gone; ones made since the last frame are on their way.
            let alive: std::collections::HashSet<u64> =
                frame.copies.iter().map(|(copy, _)| *copy).collect();
            // Copies made before (scripts that stopped and started again,
            // or ran on a server before): theirs to change too.
            for (copy, source) in frame.copies {
                if inner.copies.contains_key(&copy) {
                    continue;
                }
                let Some((name, attributes, tags)) = inner
                    .objects
                    .get(&source)
                    .map(|o| (o.name.clone(), o.attributes.clone(), o.tags.clone()))
                else {
                    continue;
                };
                let (position, heading) =
                    inner.poses.get(&copy).copied().unwrap_or(([0.0; 3], 0.0));
                inner.objects.insert(
                    copy,
                    ObjectInfo {
                        id: copy,
                        name,
                        position,
                        heading,
                        changeable: true,
                        paintable: true,
                        visible: true,
                        attributes,
                        tags,
                    },
                );
                inner.order.push(copy);
                inner.copies.insert(copy, source);
            }
            let gone: Vec<u64> = inner
                .copies
                .keys()
                .filter(|id| !alive.contains(id) && !inner.pending.contains_key(id))
                .copied()
                .collect();
            for id in gone {
                inner.forget(id);
            }
            for (id, pending) in inner.pending.clone() {
                if !alive.contains(&id) {
                    let pose = (pending.position, pending.heading);
                    inner.poses.insert(id, pose);
                    inner.targets.insert(id, pose);
                    if let Some(color) = pending.color {
                        inner.colors.insert(id, color);
                    }
                }
            }
            inner.flooded = false;
        }
        let emits = std::mem::take(&mut self.shared.borrow_mut().emits);
        let time = self.time;
        let mut scripts = std::mem::take(&mut self.scripts);
        for script in scripts.iter_mut() {
            // One budget for all of this script's calls this frame.
            self.limit.until.set(Some(Instant::now() + CALL_LIMIT));
            for event in &events {
                self.call(script, |lua| dispatch(lua, event));
            }
            for (name, values) in &emits {
                self.call(script, |lua| {
                    let f: Function = lua.globals().get("__world_event")?;
                    let mut args = vec![
                        Value::String(lua.create_string(name)?),
                        Value::Nil,
                        Value::Nil,
                    ];
                    if let serde_json::Value::Array(values) = values {
                        for value in values {
                            args.push(mlua::LuaSerdeExt::to_value(lua, value)?);
                        }
                    }
                    f.call::<()>(mlua::MultiValue::from_iter(args))
                });
            }
            self.call(script, |lua| {
                let f: Function = lua.globals().get("__world_tick")?;
                f.call::<()>(time)
            });
        }
        self.limit.until.set(None);
        self.scripts = scripts;
        let mut inner = self.shared.borrow_mut();
        if inner.flooded {
            inner.current = String::new();
            inner.line(
                format!("more than {MAX_REQUESTS} changes in one frame: the rest were dropped"),
                true,
            );
        }
        inner.pending.clear();
        std::mem::take(&mut inner.requests)
    }

    /// What scripts printed and the errors in them since the last call.
    pub fn take_output(&mut self) -> Vec<Line> {
        let mut inner = self.shared.borrow_mut();
        let dropped = std::mem::take(&mut inner.dropped);
        let mut lines = std::mem::take(&mut inner.output);
        if dropped > 0 {
            lines.push(Line {
                script: String::new(),
                text: format!("{dropped} more lines were not shown"),
                error: false,
            });
        }
        lines
    }

    fn call(&self, script: &mut Script, body: impl FnOnce(&Lua) -> mlua::Result<()>) {
        if !script.running {
            return;
        }
        self.shared.borrow_mut().current = script.name.clone();
        self.limit.passed.set(false);
        let result = body(&script.lua);
        if self.limit.passed.replace(false) {
            // However the script caught it, it stops here.
            script.running = false;
            self.shared.borrow_mut().line(
                format!(
                    "ran for more than {} ms in one frame without wait() and was stopped until the world starts again",
                    CALL_LIMIT.as_millis()
                ),
                true,
            );
            return;
        }
        if let Err(error) = result {
            self.shared.borrow_mut().line(clean(&error), true);
        }
    }
}

fn dispatch(lua: &Lua, event: &Event) -> mlua::Result<()> {
    let f: Function = lua.globals().get("__world_event")?;
    match event {
        Event::Enter { player, object } => f.call(("enter", *object, *player)),
        Event::Leave { player, object } => f.call(("leave", *object, *player)),
        Event::Use { player, object } => f.call(("use", *object, *player)),
        Event::Checkpoint {
            player,
            race,
            index,
            total,
        } => f.call((
            "checkpoint",
            Value::Nil,
            *player,
            race.as_str(),
            *index,
            *total,
        )),
        Event::Finish {
            player,
            race,
            seconds,
        } => f.call(("finish", Value::Nil, *player, race.as_str(), *seconds)),
        Event::Join { player } => f.call(("join", Value::Nil, *player)),
        Event::Quit { player } => f.call(("quit", Value::Nil, *player)),
    }
}

/// An error as a creator wants to read it: `Script name:12: what went
/// wrong`, without mlua's wrapping and Rust tracebacks.
fn clean(error: &mlua::Error) -> String {
    let text = match error {
        mlua::Error::CallbackError { cause, .. } => return clean(cause),
        mlua::Error::SyntaxError { message, .. } => message.clone(),
        mlua::Error::RuntimeError(message) => message.clone(),
        mlua::Error::MemoryError(_) => "the script used too much memory and was stopped".into(),
        other => other.to_string(),
    };
    clean_text(&text)
}

/// [`clean`] for an error that reached Lua as text (from a handler or a
/// script body that failed after it started).
fn clean_text(text: &str) -> String {
    let text = text
        .lines()
        .take_while(|l| !l.trim_start().starts_with("stack traceback"))
        .collect::<Vec<_>>()
        .join(" ");
    let text = text.trim();
    let text = text.strip_prefix("runtime error: ").unwrap_or(text);
    if text == "not enough memory" {
        return "the script used too much memory and was stopped".into();
    }
    text.to_string()
}

/// The error a script that ran too long gets, wherever it is.
const STOPPED: &str = "the script ran too long without wait() and was stopped";

/// `__stopped()`, for the prelude's pcall, xpcall and coroutine wrappers.
fn g_set_stopped(lua: &Lua, stopped: impl Fn() -> bool + 'static) -> mlua::Result<()> {
    lua.globals().set(
        "__stopped",
        lua.create_function(move |_, ()| Ok(stopped()))?,
    )
}

fn create_state(shared: &Shared, source: &Source, limit: &Rc<Limit>) -> mlua::Result<Lua> {
    let lua = Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8 | StdLib::COROUTINE,
        LuaOptions::new(),
    )?;
    lua.set_memory_limit(MEMORY_LIMIT)?;
    {
        let limit = limit.clone();
        lua.set_global_hook(
            HookTriggers::new().every_nth_instruction(1_000),
            move |_, _| {
                let over = limit.passed.get()
                    || limit
                        .until
                        .get()
                        .is_some_and(|until| Instant::now() > until);
                if over {
                    limit.passed.set(true);
                    Err(mlua::Error::runtime(STOPPED))
                } else {
                    Ok(VmState::Continue)
                }
            },
        )?;
    }
    {
        // pcall, xpcall and coroutines cannot keep a stopped script going.
        let limit = limit.clone();
        g_set_stopped(&lua, move || limit.passed.get())?;
    }
    let g = lua.globals();
    // Text chunks only: binary chunks could crash the game.
    let raw_load: Function = g.get("load")?;
    g.set(
        "load",
        lua.create_function(move |lua, args: mlua::MultiValue| {
            let mut args: Vec<Value> = args.into_iter().collect();
            args.resize(args.len().max(3), Value::Nil);
            args[2] = Value::String(lua.create_string("t")?);
            raw_load.call::<mlua::MultiValue>(mlua::MultiValue::from_iter(args))
        })?,
    )?;
    g.set("dofile", Value::Nil)?;
    g.set("loadfile", Value::Nil)?;
    let os = lua.create_table()?;
    let started = Instant::now();
    os.set(
        "clock",
        lua.create_function(move |_, ()| Ok(started.elapsed().as_secs_f64()))?,
    )?;
    g.set("os", os)?;

    let native: Table = lua.create_table()?;
    macro_rules! func {
        ($key:expr, |$lua:ident, $inner:ident, $args:tt : $ty:ty| $body:expr) => {{
            let shared = shared.clone();
            native.set(
                $key,
                lua.create_function(move |$lua, $args: $ty| {
                    #[allow(unused_mut)]
                    let mut $inner = shared.borrow_mut();
                    $body
                })?,
            )?;
        }};
    }
    let name = source.name.clone();
    native.set(
        "script_name",
        lua.create_function(move |_, ()| Ok(name.clone()))?,
    )?;
    let object = source.object;
    native.set(
        "script_object",
        lua.create_function(move |_, ()| Ok(object))?,
    )?;
    func!("print", |_lua, inner, text: String| {
        inner.line(text, false);
        Ok(())
    });
    func!("error", |_lua, inner, text: String| {
        inner.line(clean_text(&text), true);
        Ok(())
    });
    func!("find", |_lua, inner, name: String| Ok(inner
        .names
        .get(&name)
        .copied()));
    func!("objects", |_lua, inner, (): ()| Ok(inner.order.clone()));
    func!("object_name", |_lua, inner, id: u64| Ok(inner
        .objects
        .get(&id)
        .map(|o| o.name.clone())));
    func!("visible", |_lua, inner, id: u64| Ok(inner
        .visible
        .get(&id)
        .copied()
        .unwrap_or(false)));
    func!("position", |_lua, inner, id: u64| {
        let p = match inner.poses.get(&id) {
            Some((p, _)) => *p,
            None => inner.objects.get(&id).map_or([0.0; 3], |o| o.position),
        };
        Ok((p[0], p[1], p[2]))
    });
    func!("heading", |_lua, inner, id: u64| Ok(
        match inner.poses.get(&id) {
            Some((_, h)) => *h,
            None => inner.objects.get(&id).map_or(0.0, |o| o.heading),
        }
    ));
    func!("color", |_lua, inner, id: u64| Ok(
        match inner.colors.get(&id) {
            Some(c) => (Some(c[0]), Some(c[1]), Some(c[2])),
            None => (None, None, None),
        }
    ));
    func!("set_color", |_lua,
                        inner,
                        (id, r, g, b): (
        u64,
        Option<f64>,
        Option<f64>,
        Option<f64>
    )| {
        // Clamped as f64, so huge values become 2 instead of failing.
        let channel = |v: f64| {
            if v.is_nan() {
                v as f32
            } else {
                v.clamp(0.0, 2.0) as f32
            }
        };
        let color = match (r, g, b) {
            (Some(r), Some(g), Some(b)) => Some((channel(r), channel(g), channel(b))),
            _ => None,
        };
        let Some(object) = inner.objects.get(&id) else {
            return Ok((false, Some("that object is not in the world".to_string())));
        };
        if !object.changeable {
            return Ok((
                    false,
                    Some(format!(
                        "{} cannot be changed by scripts: tick \"Scripts can change it\" in its Properties",
                        object.name
                    )),
                ));
        }
        if !object.paintable {
            return Ok((
                false,
                Some(format!(
                    "{} cannot be coloured: colours work on models, not terrain or roads",
                    object.name
                )),
            ));
        }
        let color = color.map(|(r, g, b)| [r, g, b]);
        if color.is_some_and(|c| !c.iter().all(|v| v.is_finite())) {
            return Ok((
                false,
                Some("a colour is rgb(red, green, blue) from 0 to 1".to_string()),
            ));
        }
        // White is the object's own colour, as the world keeps it.
        let color = color.filter(|c| *c != [1.0; 3]);
        match color {
            Some(c) => inner.colors.insert(id, c),
            None => inner.colors.remove(&id),
        };
        Ok(match inner.request(Request::Color { object: id, color }) {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e)),
        })
    });
    func!("original", |_lua, inner, id: u64| Ok(inner
        .copies
        .get(&id)
        .copied()));
    func!("clone", |_lua,
                    inner,
                    (id, x, y, z, heading): (
        u64,
        f32,
        f32,
        f32,
        f32
    )| {
        let Some(object) = inner.objects.get(&id) else {
            return Ok((None, Some("that object is not in the world".to_string())));
        };
        if !object.changeable {
            return Ok((
                None,
                Some(format!(
                    "{} cannot be copied by scripts: tick \"Scripts can change it\" in its Properties",
                    object.name
                )),
            ));
        }
        if !object.paintable {
            return Ok((
                None,
                Some(format!(
                    "{} cannot be copied: copies work on models, not terrain, roads or objects without a model",
                    object.name
                )),
            ));
        }
        if inner.max_copies == 0 {
            return Ok((None, Some("this world cannot make copies".to_string())));
        }
        if inner.copies.len() >= inner.max_copies {
            return Ok((
                None,
                Some(format!(
                    "a world can have at most {} copies at once: destroy() some first",
                    inner.max_copies
                )),
            ));
        }
        if !(x.is_finite() && y.is_finite() && z.is_finite() && heading.is_finite()) {
            return Ok((
                None,
                Some("a copy needs a position and a heading that are numbers".to_string()),
            ));
        }
        let original = inner.copies.get(&id).copied().unwrap_or(id);
        let copy = inner.next_copy;
        let info = ObjectInfo {
            id: copy,
            name: object.name.clone(),
            position: [x, y, z],
            heading,
            changeable: true,
            paintable: true,
            visible: true,
            attributes: object.attributes.clone(),
            tags: object.tags.clone(),
        };
        if let Err(e) = inner.request(Request::Copy {
            source: id,
            id: copy,
            position: [x, y, z],
            heading,
        }) {
            return Ok((None, Some(e)));
        }
        inner.next_copy += 1;
        let color = inner.colors.get(&id).copied();
        inner.objects.insert(copy, info);
        inner.order.push(copy);
        inner.visible.insert(copy, true);
        inner.poses.insert(copy, ([x, y, z], heading));
        inner.targets.insert(copy, ([x, y, z], heading));
        if let Some(color) = color {
            inner.colors.insert(copy, color);
        }
        inner.copies.insert(copy, original);
        inner.pending.insert(
            copy,
            Pending {
                position: [x, y, z],
                heading,
                color,
            },
        );
        Ok((Some(copy), None))
    });
    func!("destroy", |_lua, inner, id: u64| {
        if !inner.copies.contains_key(&id) {
            let text = match inner.objects.get(&id) {
                Some(object) => format!(
                    "{} was placed in the world: only copies can be destroyed (hide() it instead)",
                    object.name
                ),
                None => "that object is not in the world".to_string(),
            };
            return Ok((false, Some(text)));
        }
        if let Err(e) = inner.request(Request::Destroy { object: id }) {
            return Ok((false, Some(e)));
        }
        inner.forget(id);
        Ok((true, None))
    });
    func!("target", |_lua, inner, id: u64| {
        let (p, h) = match inner.targets.get(&id) {
            Some(target) => *target,
            None => match inner.poses.get(&id) {
                Some(now) => *now,
                None => inner
                    .objects
                    .get(&id)
                    .map_or(([0.0; 3], 0.0), |o| (o.position, o.heading)),
            },
        };
        Ok((p[0], p[1], p[2], h))
    });
    func!("move", |_lua,
                   inner,
                   (id, x, y, z, heading, seconds): (
        u64,
        f32,
        f32,
        f32,
        f32,
        f32
    )| {
        let Some(object) = inner.objects.get(&id) else {
            return Ok((false, Some("that object is not in the world".to_string())));
        };
        if !object.changeable {
            return Ok((
                    false,
                    Some(format!(
                        "{} cannot be changed by scripts: tick \"Scripts can change it\" in its Properties",
                        object.name
                    )),
                ));
        }
        if ![x, y, z, heading, seconds].iter().all(|v| v.is_finite())
            || [x, y, z].iter().any(|v| v.abs() > 100_000.0)
        {
            return Ok((false, Some("moves need real, nearby numbers".to_string())));
        }
        let seconds = seconds.clamp(0.0, 600.0);
        if seconds == 0.0 {
            inner.poses.insert(id, ([x, y, z], heading));
        }
        inner.targets.insert(id, ([x, y, z], heading));
        Ok(
            match inner.request(Request::Move {
                object: id,
                position: [x, y, z],
                heading,
                seconds,
            }) {
                Ok(()) => (true, None),
                Err(e) => (false, Some(e)),
            },
        )
    });
    func!("attribute", |lua, inner, (id, name): (u64, String)| {
        let value = inner
            .objects
            .get(&id)
            .and_then(|o| o.attributes.get(&name))
            .cloned();
        Ok(match value {
            None => (mlua::Value::Nil, None, None),
            Some(Attribute::Bool(b)) => (mlua::Value::Boolean(b), None, None),
            Some(Attribute::Number(n)) => (mlua::Value::Number(n), None, None),
            Some(Attribute::Text(t)) => (mlua::Value::String(lua.create_string(&t)?), None, None),
            Some(Attribute::Vector([x, y, z])) => (mlua::Value::Number(x), Some(y), Some(z)),
        })
    });
    func!("set_attribute", |_lua,
                            inner,
                            (id, name, value, y, z): (
        u64,
        String,
        mlua::Value,
        Option<f64>,
        Option<f64>
    )| {
        if !valid_name(&name) {
            return Ok((false, Some(format!(
                "an attribute name is 1 to {MAX_NAME} characters, without line breaks or spaces at the ends"
            ))));
        }
        let number = |v: &mlua::Value| match v {
            mlua::Value::Number(n) => Some(*n),
            mlua::Value::Integer(n) => Some(*n as f64),
            _ => None,
        };
        let value =
            match (&value, y, z) {
                (mlua::Value::Nil, _, _) => None,
                (mlua::Value::Boolean(b), _, _) => Some(Attribute::Bool(*b)),
                (v, Some(y), Some(z)) if number(v).is_some() => {
                    Some(Attribute::Vector([number(v).unwrap_or_default(), y, z]))
                }
                (v, _, _) if number(v).is_some() => {
                    Some(Attribute::Number(number(v).unwrap_or_default()))
                }
                (mlua::Value::String(t), _, _) => {
                    let t = t.to_str()?.to_string();
                    if t.len() > MAX_TEXT {
                        return Ok((
                            false,
                            Some(format!("an attribute text is at most {MAX_TEXT} bytes")),
                        ));
                    }
                    Some(Attribute::Text(t))
                }
                _ => return Ok((
                    false,
                    Some(
                        "an attribute is true/false, a number, a text or a vector (nil removes it)"
                            .to_string(),
                    ),
                )),
            };
        let finite = match &value {
            Some(Attribute::Number(n)) => n.is_finite(),
            Some(Attribute::Vector(v)) => v.iter().all(|n| n.is_finite()),
            _ => true,
        };
        if !finite {
            return Ok((
                false,
                Some("an attribute number must be finite".to_string()),
            ));
        }
        let Some(object) = inner.objects.get_mut(&id) else {
            return Ok((false, Some("that object is not in the world".to_string())));
        };
        match value {
            None => {
                object.attributes.remove(&name);
            }
            Some(value) => {
                if !object.attributes.contains_key(&name)
                    && object.attributes.len() >= MAX_ATTRIBUTES
                {
                    return Ok((
                        false,
                        Some(format!("an object has at most {MAX_ATTRIBUTES} attributes")),
                    ));
                }
                object.attributes.insert(name, value);
            }
        }
        Ok((true, None))
    });
    func!("tags", |_lua, inner, id: u64| Ok(inner
        .objects
        .get(&id)
        .map(|o| o.tags.clone())
        .unwrap_or_default()));
    func!("has_tag", |_lua, inner, (id, tag): (u64, String)| Ok(inner
        .objects
        .get(&id)
        .is_some_and(|o| o.tags.contains(&tag))));
    func!(
        "set_tag",
        |_lua, inner, (id, tag, on): (u64, String, bool)| {
            if !valid_name(&tag) {
                return Ok((
                    false,
                    Some(format!(
                "a tag is 1 to {MAX_NAME} characters, without line breaks or spaces at the ends"
            )),
                ));
            }
            let Some(object) = inner.objects.get_mut(&id) else {
                return Ok((false, Some("that object is not in the world".to_string())));
            };
            let has = object.tags.contains(&tag);
            if on && !has {
                if object.tags.len() >= MAX_TAGS {
                    return Ok((
                        false,
                        Some(format!("an object has at most {MAX_TAGS} tags")),
                    ));
                }
                object.tags.push(tag);
            } else if !on && has {
                object.tags.retain(|t| *t != tag);
            }
            Ok((true, None))
        }
    );
    func!("tagged", |_lua, inner, tag: String| Ok(inner
        .order
        .iter()
        .filter(|id| inner.objects.get(id).is_some_and(|o| o.tags.contains(&tag)))
        .copied()
        .collect::<Vec<u64>>()));
    func!("set_visible", |_lua, inner, (id, visible): (u64, bool)| {
        let Some(object) = inner.objects.get(&id) else {
            return Ok((false, Some("that object is not in the world".to_string())));
        };
        if !object.changeable {
            return Ok((
                false,
                Some(format!(
                    "{} cannot be changed by scripts: tick \"Scripts can change it\" in its Properties",
                    object.name
                )),
            ));
        }
        if inner.visible.get(&id) == Some(&visible) {
            return Ok((true, None));
        }
        inner.visible.insert(id, visible);
        match inner.request(Request::Visible {
            object: id,
            visible,
        }) {
            Ok(()) => Ok((true, None)),
            Err(e) => Ok((false, Some(e))),
        }
    });
    func!("players", |_lua, inner, (): ()| Ok(inner
        .players
        .iter()
        .map(|p| p.id)
        .collect::<Vec<_>>()));
    func!("player_name", |_lua, inner, id: u32| Ok(inner
        .players
        .iter()
        .find(|p| p.id == id)
        .map(|p| p.name.clone())));
    func!("player_position", |_lua, inner, id: u32| {
        Ok(
            match inner
                .players
                .iter()
                .find(|p| p.id == id)
                .and_then(|p| p.position)
            {
                Some(p) => (Some(p[0]), Some(p[1]), Some(p[2])),
                None => (None, None, None),
            },
        )
    });
    func!("player_driving", |_lua, inner, id: u32| Ok(inner
        .players
        .iter()
        .any(|p| p.id == id && p.driving)));
    func!("message", |_lua,
                      inner,
                      (player, text, seconds): (
        Option<u32>,
        String,
        f32
    )| {
        let text: String = text.chars().take(MAX_MESSAGE).collect();
        let seconds = if seconds.is_finite() {
            seconds.clamp(0.5, 60.0)
        } else {
            4.0
        };
        // A message for someone who left goes nowhere.
        if player.is_some_and(|id| !inner.players.iter().any(|p| p.id == id)) {
            return Ok(());
        }
        let _ = inner.request(Request::Message {
            player,
            text,
            seconds,
        });
        Ok(())
    });
    func!("teleport", |_lua, inner, (player, object): (u32, u64)| {
        if !inner.objects.contains_key(&object) {
            return Ok((false, Some("that object is not in the world".to_string())));
        }
        if !inner.players.iter().any(|p| p.id == player) {
            return Ok((true, None));
        }
        Ok(
            match inner.request(Request::Teleport {
                player,
                to: Target::Object(object),
            }) {
                Ok(()) => (true, None),
                Err(e) => (false, Some(e)),
            },
        )
    });
    func!("teleport_to", |_lua,
                          inner,
                          (player, x, y, z, heading): (
        u32,
        f32,
        f32,
        f32,
        f32
    )| {
        if ![x, y, z, heading].iter().all(|v| v.is_finite())
            || [x, y, z].iter().any(|v| v.abs() > 100_000.0)
        {
            return Ok((
                false,
                Some("teleport needs a vector of real, nearby numbers".to_string()),
            ));
        }
        if !inner.players.iter().any(|p| p.id == player) {
            return Ok((true, None));
        }
        Ok(
            match inner.request(Request::Teleport {
                player,
                to: Target::Point([x, y, z], heading),
            }) {
                Ok(()) => (true, None),
                Err(e) => (false, Some(e)),
            },
        )
    });
    func!("hud", |_lua,
                  inner,
                  (player, key, text): (
        Option<u32>,
        String,
        Option<String>
    )| {
        if key.is_empty() || key.chars().count() > 32 {
            return Ok((
                false,
                Some("hud needs a name of 1 to 32 characters".to_string()),
            ));
        }
        // A player who left has no screen: nothing to do.
        if player.is_some_and(|id| !inner.players.iter().any(|p| p.id == id)) {
            return Ok((true, None));
        }
        let text: Option<String> = text
            .filter(|t| !t.is_empty())
            .map(|t| t.chars().take(120).collect());
        let lines = inner.hud.entry(player).or_default();
        if text.is_none() {
            lines.remove(&key);
        } else if !lines.contains(&key) {
            if lines.len() >= 12 {
                return Ok((
                    false,
                    Some(format!(
                        "on-screen text has at most 12 lines {}; remove one first",
                        if player.is_some() {
                            "per player"
                        } else {
                            "for everyone"
                        }
                    )),
                ));
            }
            lines.insert(key.clone());
        }
        Ok(match inner.request(Request::Hud { player, key, text }) {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e)),
        })
    });
    func!("hour", |_lua, inner, (): ()| Ok(inner.hour));
    func!("weather", |_lua, inner, (): ()| Ok(inner.weather.clone()));
    func!("set_hour", |_lua, inner, hour: Option<f64>| {
        let Some(hour) = hour.filter(|h| h.is_finite() && (0.0..24.0).contains(h)) else {
            return Ok((
                false,
                Some("setHour needs an hour from 0 to 23".to_string()),
            ));
        };
        let hour = hour as u8;
        inner.hour = Some(hour);
        Ok(match inner.request(Request::Hour(hour)) {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e)),
        })
    });
    func!("set_weather", |_lua, inner, weather: String| {
        if !inner.weathers.contains(&weather) {
            return Ok((
                false,
                Some(format!(
                    "setWeather needs one of: {}",
                    inner.weathers.join(", ")
                )),
            ));
        }
        inner.weather = Some(weather.clone());
        Ok(match inner.request(Request::Weather(weather)) {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e)),
        })
    });
    func!("emit", |lua, inner, (name, values): (String, Value)| {
        if inner.emits.len() >= MAX_EMITS {
            return Ok((
                false,
                Some(format!(
                    "more than {MAX_EMITS} world.emit calls in one frame"
                )),
            ));
        }
        let json: serde_json::Value = match mlua::LuaSerdeExt::from_value(lua, values) {
            Ok(json) => json,
            Err(_) => {
                return Ok((
                    false,
                    Some(
                        "world.emit can send numbers, text, true/false and tables of them"
                            .to_string(),
                    ),
                ))
            }
        };
        if serde_json::to_string(&json).map_or(usize::MAX, |s| s.len()) > MAX_EMIT {
            return Ok((
                false,
                Some(format!("world.emit can send at most {MAX_EMIT} bytes")),
            ));
        }
        inner.emits.push((name, json));
        Ok((true, None))
    });
    g.set("__world", native)?;
    lua.load(PRELUDE).set_name("=world").exec()?;
    Ok(lua)
}

/// Compile `source` without running it, as the editor does while a
/// creator types: None when it is valid Lua, or the error with its line
/// (`name:3: ...`).
pub fn check(name: &str, source: &str) -> Option<String> {
    let lua = Lua::new_with(StdLib::NONE, LuaOptions::new()).ok()?;
    lua.load(source)
        .set_name(format!("={name}"))
        .into_function()
        .err()
        .map(|e| clean(&e))
}

/// The line an error text points at (`name:3: ...` gives 3).
pub fn error_line(name: &str, text: &str) -> Option<usize> {
    let rest = text.strip_prefix(name)?.strip_prefix(':')?;
    rest.split(':').next()?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn objects() -> Vec<ObjectInfo> {
        vec![
            ObjectInfo {
                id: 1,
                name: "Door".into(),
                position: [0.0, 0.0, 0.0],
                heading: 0.0,
                changeable: true,
                paintable: true,
                visible: true,
                attributes: [
                    ("Price".to_string(), Attribute::Number(5.0)),
                    ("Owner".to_string(), Attribute::Text("Ada".into())),
                    ("Spot".to_string(), Attribute::Vector([1.0, 2.0, 3.0])),
                ]
                .into(),
                tags: vec!["Shop".into()],
            },
            ObjectInfo {
                id: 2,
                name: "Pad".into(),
                position: [10.0, 0.0, 0.0],
                heading: 90.0,
                changeable: false,
                paintable: true,
                visible: true,
                attributes: Default::default(),
                tags: Vec::new(),
            },
        ]
    }
    fn host(scripts: &[(&str, &str, Option<u64>)]) -> WorldScripts {
        WorldScripts::new(Config {
            scripts: scripts
                .iter()
                .map(|(name, source, object)| Source {
                    name: name.to_string(),
                    source: source.to_string(),
                    object: *object,
                })
                .collect(),
            objects: objects(),
            weathers: vec!["clear".into(), "rain".into()],
            hour: Some(12),
            weather: None,
            first_copy: 1000,
            max_copies: 3,
            copies: Vec::new(),
        })
    }
    fn alice() -> Vec<PlayerInfo> {
        vec![PlayerInfo {
            id: 7,
            name: "Alice".into(),
            position: Some([1.0, 2.0, 3.0]),
            driving: false,
        }]
    }
    fn host_with(name: &str, source: &str) -> WorldScripts {
        host(&[(name, source, None)])
    }
    fn frame(dt: f32) -> Frame {
        Frame {
            dt,
            players: alice(),
            hour: Some(12),
            ..Frame::default()
        }
    }
    fn texts(host: &mut WorldScripts) -> Vec<String> {
        host.take_output()
            .into_iter()
            .map(|l| format!("{}{}: {}", if l.error { "!" } else { "" }, l.script, l.text))
            .collect()
    }

    #[test]
    fn scripts_copy_objects_and_destroy_the_copies() {
        let mut host = host_with(
            "Spawner",
            r#"
            local door = world.find('Door')
            door.color = rgb(1, 0, 0)
            local a = door:clone(vector(5, 0, 0), 45)
            print(a.id, a.name, a.position, a.heading, a.color, a.original == door, door.original)
            local b = a:clone()
            print(b.original == door, b.position, #world.objects())
            print(pcall(function() world.find('Pad'):clone() end))
            local c = door:clone()
            print(pcall(function() door:clone() end))
            print(pcall(function() door:destroy() end))
            c:destroy()
            print(c.exists, a.exists, #world.objects())
            print(pcall(function() c.visible = false end))
            wait(1)
            print(a.position, a.exists, b.exists)
            "#,
        );
        assert_eq!(
            texts(&mut host),
            [
                "Spawner: 1000  Door  vector(5.00, 0.00, 0.00)  45.0  rgb(1.00, 0.00, 0.00)  true  nil",
                "Spawner: true  vector(5.00, 0.00, 0.00)  4",
                "Spawner: false  Spawner:8: Pad cannot be copied by scripts: tick \"Scripts can change it\" in its Properties",
                "Spawner: false  Spawner:10: a world can have at most 3 copies at once: destroy() some first",
                "Spawner: false  Spawner:11: Door was placed in the world: only copies can be destroyed (hide() it instead)",
                "Spawner: false  true  4",
                "Spawner: false  Spawner:14: that object is not in the world",
            ]
        );
        let requests = host.tick(frame(0.1), vec![]);
        assert_eq!(
            requests[1],
            Request::Copy {
                source: 1,
                id: 1000,
                position: [5.0, 0.0, 0.0],
                heading: 45.0
            }
        );
        assert_eq!(
            requests[2],
            Request::Copy {
                source: 1000,
                id: 1001,
                position: [5.0, 0.0, 0.0],
                heading: 45.0
            }
        );
        assert_eq!(requests.last(), Some(&Request::Destroy { object: 1002 }));
        // The world has only the first copy (the second was turned down).
        let mut next = frame(1.0);
        next.copies = vec![(1000, 1)];
        next.poses = vec![(1000, [5.0, 0.0, 0.0], 45.0)];
        assert!(host.tick(next, vec![]).is_empty());
        assert_eq!(
            texts(&mut host),
            ["Spawner: vector(5.00, 0.00, 0.00)  true  false"]
        );
    }

    #[test]
    fn scripts_read_and_change_attributes_and_tags() {
        let mut host = host_with(
            "Shop",
            r#"
            local door, pad = world.find('Door'), world.find('Pad')
            print(door:get('Price'), door:get('Owner'), door:get('Spot'), door:get('Nope'))
            door:set('Price', door:get('Price') + 1)
            door:set('Open', true)
            door:set('Owner', nil)
            print(door:get('Price'), door:get('Open'), door:get('Owner'))
            print(door:hasTag('Shop'), pad:hasTag('Shop'), #world.tagged('Shop'))
            pad:addTag('Shop')
            print(#world.tagged('Shop'), world.tagged('Shop')[2].name, pad.tags[1])
            door:removeTag('Shop')
            print(#world.tagged('Shop'), #door.tags)
            print(pcall(function() door:set('Bad', {}) end))
            print(pcall(function() door:set('', 1) end))
            local copy = door:clone(vector(1, 1, 1), 0)
            print(copy:get('Price'))
            "#,
        );
        let out = texts(&mut host);
        assert_eq!(out[0], "Shop: 5.0  Ada  vector(1.00, 2.00, 3.00)  nil");
        assert_eq!(out[1], "Shop: 6.0  true  nil");
        assert_eq!(out[2], "Shop: true  false  1");
        assert_eq!(out[3], "Shop: 2  Pad  Shop");
        assert_eq!(out[4], "Shop: 1  0");
        assert!(
            out[5].contains("false") && out[5].contains("true/false, a number"),
            "{}",
            out[5]
        );
        assert!(
            out[6].contains("false") && out[6].contains("1 to 64"),
            "{}",
            out[6]
        );
        assert_eq!(out[7], "Shop: 6.0");
    }

    #[test]
    fn a_door_script_opens_on_use_and_closes_after_a_wait() {
        let mut host = host(&[(
            "Door timer",
            r#"
            local door = script.object
            print('ready', door.name, door.visible)
            door:on('use', function(player)
              door:hide()
              player:message('Open for 2 seconds')
              wait(2)
              door:show()
            end)
            "#,
            Some(1),
        )]);
        assert_eq!(texts(&mut host), ["Door timer: ready  Door  true"]);
        assert!(host.tick(frame(0.1), vec![]).is_empty());
        let used = host.tick(
            frame(0.1),
            vec![Event::Use {
                player: 7,
                object: 1,
            }],
        );
        assert_eq!(
            used,
            [
                Request::Visible {
                    object: 1,
                    visible: false
                },
                Request::Message {
                    player: Some(7),
                    text: "Open for 2 seconds".into(),
                    seconds: 4.0
                },
            ]
        );
        // Another object's use is not the door's.
        assert!(host
            .tick(
                frame(1.0),
                vec![Event::Use {
                    player: 7,
                    object: 2
                }]
            )
            .is_empty());
        let later = host.tick(
            Frame {
                visible: vec![(1, false)],
                ..frame(1.0)
            },
            vec![],
        );
        assert_eq!(
            later,
            [Request::Visible {
                object: 1,
                visible: true
            }]
        );
        assert!(texts(&mut host).is_empty());
    }

    #[test]
    fn errors_name_the_script_and_line_and_others_keep_running() {
        let mut host = host(&[
            ("Broken", "local x = 1\nx.y.z = 2", None),
            ("Typo", "this is not lua", None),
            (
                "Fine",
                "world.on('join', function(p) print('hi ' .. p.name) end)",
                None,
            ),
        ]);
        let out = texts(&mut host);
        assert!(out[0].starts_with("!Broken: Broken:2:"), "{out:?}");
        assert!(out[1].starts_with("!Typo: Typo:1:"), "{out:?}");
        host.tick(frame(0.1), vec![Event::Join { player: 7 }]);
        assert_eq!(texts(&mut host), ["Fine: hi Alice"]);
    }

    #[test]
    fn endless_loops_and_memory_hogs_are_stopped() {
        let started = Instant::now();
        let mut host = host(&[
            ("Loop", "while true do end", None),
            (
                "Hog",
                "local t = {} for i = 1, 1e9 do t[i] = string.rep('x', 1000) end",
                None,
            ),
            ("Later", "every(0.5, function() print('tick') end)", None),
        ]);
        assert!(started.elapsed() < Duration::from_secs(5));
        let out = texts(&mut host);
        assert!(out[0].contains("ran for more than 250 ms"), "{out:?}");
        // A slow machine may reach the time limit before the memory one.
        assert!(
            out[1].contains("too much memory") || out[1].contains("ran for more than 250 ms"),
            "{out:?}"
        );
        host.tick(frame(0.6), vec![]);
        assert_eq!(texts(&mut host), ["Later: tick"]);
    }

    #[test]
    fn the_editor_checks_scripts_without_running_them() {
        assert_eq!(check("Ok", "while true do end"), None);
        let error = check(
            "Door",
            "local x = 
print(x",
        )
        .unwrap();
        assert!(error.starts_with("Door:2:"), "{error}");
        assert_eq!(error_line("Door", &error), Some(2));
        assert_eq!(error_line("Door", "other: no"), None);
    }

    #[test]
    fn a_stopped_script_stays_stopped_whatever_it_catches() {
        let started = Instant::now();
        let mut host = host(&[
            (
                "Catcher",
                "while true do pcall(function() while true do end end) end",
                None,
            ),
            (
                "Wrapper",
                "local f = coroutine.wrap(function() while true do end end) while true do pcall(f) end",
                None,
            ),
            (
                "Matcher",
                "local s = string.rep('a', 100000) print(pcall(s.find, s, string.rep('.-', 8) .. 'b'))",
                None,
            ),
            ("Fine", "every(0.5, function() print('tick') end)", None),
        ]);
        assert!(
            started.elapsed() < Duration::from_secs(3),
            "{:?}",
            started.elapsed()
        );
        let out = texts(&mut host);
        assert!(
            out[0].starts_with("!Catcher: ran for more than 250 ms"),
            "{out:?}"
        );
        assert!(
            out[1].starts_with("!Wrapper: ran for more than 250 ms"),
            "{out:?}"
        );
        assert!(
            out[2].starts_with("Matcher: false  string.find: a pattern with 8 repeats"),
            "{out:?}"
        );
        // Stopped scripts are not called again; the others go on.
        host.tick(frame(0.6), vec![]);
        assert_eq!(texts(&mut host), ["Fine: tick"]);
        // Short texts may use patterns freely.
        let mut host = host_with(
            "Words",
            "for w in ('a b c'):gmatch('%a+') do print(w) end print(('x=1'):match('(%w+)=(%d+)'))",
        );
        assert_eq!(
            texts(&mut host),
            ["Words: a", "Words: b", "Words: c", "Words: x  1"]
        );
    }

    #[test]
    fn a_frame_has_one_budget_per_script_however_many_calls() {
        let started = Instant::now();
        let mut host = host(&[(
            "Echo",
            "world.on('x', function() local t = os.clock() while os.clock() - t < 0.1 do end world.emit('x') end) for i = 1, 20 do world.emit('x') end",
            None,
        )]);
        host.tick(frame(0.1), vec![]);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        let out = texts(&mut host);
        assert!(
            out.iter().any(|l| l.contains("ran for more than 250 ms")),
            "{out:?}"
        );
    }

    #[test]
    fn players_who_left_or_have_not_spawned_still_have_names() {
        let mut host = host_with(
            "Hello",
            "world.on('join', function(p) print('hi ' .. p.name, p.position) end) world.on('quit', function(p) print('bye ' .. p.name) end)",
        );
        let unspawned = vec![PlayerInfo {
            id: 3,
            name: "Bo".into(),
            position: None,
            driving: false,
        }];
        host.tick(
            Frame {
                players: unspawned,
                ..frame(0.1)
            },
            vec![Event::Join { player: 3 }],
        );
        assert_eq!(texts(&mut host), ["Hello: hi Bo  nil"]);
        host.tick(
            Frame {
                players: Vec::new(),
                ..frame(0.1)
            },
            vec![Event::Quit { player: 3 }],
        );
        assert_eq!(texts(&mut host), ["Hello: bye Bo"]);
    }

    #[test]
    fn the_sandbox_has_no_files_processes_or_debug() {
        let mut host = host(&[(
            "Probe",
            "print(io, os.execute, require, debug, dofile, loadfile, package, os.getenv)",
            None,
        )]);
        assert_eq!(
            texts(&mut host),
            ["Probe: nil  nil  nil  nil  nil  nil  nil  nil"]
        );
    }

    #[test]
    fn objects_that_scripts_cannot_change_say_how_to_allow_it() {
        let mut host = host(&[("Pad", "world.find('Pad').visible = false", None)]);
        let out = texts(&mut host);
        assert!(
            out[0].contains(
                "Pad:1: Pad cannot be changed by scripts: tick \"Scripts can change it\""
            ),
            "{out:?}"
        );
    }

    #[test]
    fn world_time_weather_teleports_and_messages() {
        let mut host = host(&[(
            "World",
            r#"
            world.on('enter', 'Pad', function(player, pad)
              player:teleport(world.find('Door'))
              player:teleport(vector(1, 2, 3) + vector(0, 0, 1), 90)
              world.setHour(21)
              world.setWeather('rain')
              world.message('Night falls', 3)
              print(world.hour(), world.weather(), pad.position, pad.heading)
            end)
            local ok, err = pcall(world.setWeather, 'snowstorm')
            print(ok, err)
            "#,
            None,
        )]);
        let out = texts(&mut host);
        assert!(
            out[0].contains("false") && out[0].contains("clear, rain"),
            "{out:?}"
        );
        let requests = host.tick(
            frame(0.1),
            vec![Event::Enter {
                player: 7,
                object: 2,
            }],
        );
        assert_eq!(
            requests,
            [
                Request::Teleport {
                    player: 7,
                    to: Target::Object(1)
                },
                Request::Teleport {
                    player: 7,
                    to: Target::Point([1.0, 2.0, 4.0], 90.0)
                },
                Request::Hour(21),
                Request::Weather("rain".into()),
                Request::Message {
                    player: None,
                    text: "Night falls".into(),
                    seconds: 3.0
                },
            ]
        );
        assert_eq!(
            texts(&mut host),
            ["World: 21  rain  vector(10.00, 0.00, 0.00)  90.0"]
        );
    }

    #[test]
    fn scripts_move_objects_at_once_or_smoothly() {
        let mut host = host(&[(
            "Lift",
            r#"
            local door = world.find('Door')
            door.position = vector(1, 2, 3)
            print(door.position, door.heading)
            door.heading = 90
            door:moveTo(door.position + vector(0, 0, 5), 2)
            door:turnTo(180, 1)
            print(pcall(function() world.find('Pad').position = vector(0, 0, 0) end))
            "#,
            None,
        )]);
        let out = texts(&mut host);
        assert_eq!(out[0], "Lift: vector(1.00, 2.00, 3.00)  0.0");
        assert!(
            out[1].starts_with("Lift: false") && out[1].contains("Pad cannot be changed"),
            "{out:?}"
        );
        let moves = host.tick(frame(0.1), vec![]);
        assert_eq!(
            moves,
            [
                Request::Move {
                    object: 1,
                    position: [1.0, 2.0, 3.0],
                    heading: 0.0,
                    seconds: 0.0
                },
                Request::Move {
                    object: 1,
                    position: [1.0, 2.0, 3.0],
                    heading: 90.0,
                    seconds: 0.0
                },
                Request::Move {
                    object: 1,
                    position: [1.0, 2.0, 8.0],
                    heading: 90.0,
                    seconds: 2.0
                },
                Request::Move {
                    object: 1,
                    position: [1.0, 2.0, 8.0],
                    heading: 180.0,
                    seconds: 1.0
                },
            ]
        );
        // A heading that is not a number is an error, not north.
        let mut host = host_with(
            "Typo",
            "world.find('Door').heading = world.find('Door').headng",
        );
        let out = texts(&mut host);
        assert!(
            out[0].contains("Typo:1: a heading is a number of degrees"),
            "{out:?}"
        );
    }

    #[test]
    fn scripts_colour_objects_they_may_change() {
        let mut host = host_with(
            "Paint",
            r#"
            local door = world.find('Door')
            door.color = rgb(1, 0, 0)
            print(door.color, door.color == rgb(1, 0, 0))
            door.color = rgb(1, 0.85, 0.1)
            print(door.color == rgb(1, 0.85, 0.1))
            print(pcall(rgb, 1, 0.5))
            door.color = nil
            print(door.color)
            print(pcall(function() door.color = 'red' end))
            print(pcall(function() world.find('Pad').color = rgb(0, 1, 0) end))
            "#,
        );
        let out = texts(&mut host);
        assert_eq!(out[0], "Paint: rgb(1.00, 0.00, 0.00)  true");
        assert_eq!(out[1], "Paint: true", "colours compare as written");
        assert!(out[2].contains("rgb needs three numbers"), "{out:?}");
        assert_eq!(out[3], "Paint: nil");
        assert!(
            out[4].contains("a colour is rgb(red, green, blue)"),
            "{out:?}"
        );
        assert!(
            out[5].contains("Pad cannot be changed by scripts"),
            "{out:?}"
        );
        assert_eq!(
            host.tick(frame(0.1), vec![]),
            [
                Request::Color {
                    object: 1,
                    color: Some([1.0, 0.0, 0.0])
                },
                Request::Color {
                    object: 1,
                    color: Some([1.0, 0.85, 0.1])
                },
                Request::Color {
                    object: 1,
                    color: None
                },
            ]
        );
    }

    #[test]
    fn scripts_put_text_on_screen_for_one_or_all() {
        let mut host = host_with(
            "Score",
            "world.on('join', function(p) world.hud('round', 'Round 1') p:hud('score', 0) p:hud('old', nil) end)",
        );
        let requests = host.tick(frame(0.1), vec![Event::Join { player: 7 }]);
        let mut full = host_with("Full", "for i = 1, 13 do world.hud('line' .. i, i) end");
        let out = texts(&mut full);
        assert!(
            out[0].contains("Full:1: on-screen text has at most 12 lines for everyone"),
            "{out:?}"
        );
        let mut unnamed = host_with("Unnamed", "world.hud(nil, 'x')");
        assert!(texts(&mut unnamed)[0].contains("hud needs a name"));
        assert_eq!(
            requests,
            [
                Request::Hud {
                    player: None,
                    key: "round".into(),
                    text: Some("Round 1".into())
                },
                Request::Hud {
                    player: Some(7),
                    key: "score".into(),
                    text: Some("0".into())
                },
                Request::Hud {
                    player: Some(7),
                    key: "old".into(),
                    text: None
                },
            ]
        );
    }

    #[test]
    fn scripts_talk_to_each_other_with_emit() {
        let mut host = host(&[
            (
                "Score",
                r#"
                local points = 0
                world.on('scored', function(who, n)
                  points = points + n
                  print(who .. ' scored, total ' .. points)
                end)
                "#,
                None,
            ),
            (
                "Goal",
                "world.on('finish', function(p, race, s) world.emit('scored', p.name, 3) print(race, s) end)",
                None,
            ),
        ]);
        host.tick(
            frame(0.1),
            vec![Event::Finish {
                player: 7,
                race: "Lap".into(),
                seconds: 12.5,
            }],
        );
        assert_eq!(texts(&mut host), ["Goal: Lap  12.5"]);
        host.tick(frame(0.1), vec![]);
        assert_eq!(texts(&mut host), ["Score: Alice scored, total 3"]);
    }
}
