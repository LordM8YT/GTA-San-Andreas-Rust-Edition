//! Build Mode gameplay: behaviour a creator attaches to layout objects, and
//! the engine that runs it while a world is played.
//!
//! A layout node may carry one gameplay [`Component`] in its `gameplay`
//! field:
//!
//! - `spawn`: players start here, facing the object's +Y (north when not turned).
//! - `trigger`: a box zone; actions run when a player walks in or out.
//! - `interact`: a box zone; a prompt shows inside it and actions run when
//!   the player presses the interact key (E). Doors are this.
//! - `checkpoint`: a box zone, one stop of a timed course.
//!
//! Zones are boxes `size` metres wide (X), deep (Y) and tall (Z) in the
//! object's own axes: centred on the object's position, standing on it,
//! turned with the object and *not* scaled with it, so the numbers in the
//! Inspector are always metres.
//!
//! [`Action`]s are what zones do: show a message, teleport the player to
//! another object, show, hide or toggle an object (an opened door is a
//! hidden one, with no collision), set the time of day or the weather.
//!
//! Everything is plain data with no GPU, UI or San Andreas types, so the
//! editor, the client and later a server can share it. [`Logic`] keeps one
//! player's progress and the world's changed state; it is deterministic and
//! has no clock of its own (callers pass `dt`).
//!
//! Forward compatibility: a component or action kind this SARE does not know
//! (written by a newer SARE or by hand) is kept as [`Kept::Unknown`] and
//! written back unchanged; it simply does nothing here. See docs/build-mode.md.
use crate::world_layout::{Layout, Node, NodeId, Transform};
use glam::{Quat, Vec3};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{HashMap, HashSet};

/// Largest zone side, in metres.
pub const MAX_ZONE: f32 = 500.0;
/// How far below a zone's base the player's feet still count as inside.
const FEET_TOLERANCE: f32 = 0.5;
/// Race name used when a checkpoint names none.
pub const DEFAULT_RACE: &str = "main";

/// A tagged value this SARE may or may not know. Unknown ones are kept
/// exactly as read and written back on save.
#[derive(Clone, Debug, PartialEq)]
pub enum Kept<T> {
    Known(T),
    Unknown(Value),
}
impl<T> Kept<T> {
    pub fn known(&self) -> Option<&T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown(_) => None,
        }
    }
    pub fn known_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Known(value) => Some(value),
            Self::Unknown(_) => None,
        }
    }
}
impl<T> From<T> for Kept<T> {
    fn from(value: T) -> Self {
        Self::Known(value)
    }
}

/// A serde-tagged enum: which field holds the tag and which tags exist.
pub trait Tagged {
    const TAG: &'static str;
    const KINDS: &'static [&'static str];
}

impl<T: Serialize> Serialize for Kept<T> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Known(value) => value.serialize(s),
            Self::Unknown(value) => value.serialize(s),
        }
    }
}
impl<'de, T: Tagged + DeserializeOwned> Deserialize<'de> for Kept<T> {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(d)?;
        let kind = value.get(T::TAG).and_then(Value::as_str).map(str::to_owned);
        match kind.as_deref() {
            // A known kind must be well formed: a typo in its fields is an
            // error, not silently dropped behaviour.
            Some(kind) if T::KINDS.contains(&kind) => serde_json::from_value(value)
                .map(Self::Known)
                .map_err(|error| serde::de::Error::custom(format!("{kind}: {error}"))),
            _ => Ok(Self::Unknown(value)),
        }
    }
}

fn default_zone() -> [f32; 3] {
    [4.0, 4.0, 3.0]
}
fn default_race() -> String {
    DEFAULT_RACE.into()
}
fn first() -> u32 {
    1
}
fn message_seconds() -> f32 {
    4.0
}

/// Behaviour attached to one layout object.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Component {
    Spawn(Spawn),
    Trigger(Trigger),
    Interact(Interact),
    Checkpoint(Checkpoint),
}
impl Tagged for Component {
    const TAG: &'static str = "kind";
    const KINDS: &'static [&'static str] = &["spawn", "trigger", "interact", "checkpoint"];
}
impl Component {
    /// Name shown in the editor.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Spawn(_) => "Spawn point",
            Self::Trigger(_) => "Trigger zone",
            Self::Interact(_) => "Interaction",
            Self::Checkpoint(_) => "Checkpoint",
        }
    }
    /// The zone size, for kinds that have a zone.
    pub fn size(&self) -> Option<[f32; 3]> {
        match self {
            Self::Spawn(_) => None,
            Self::Trigger(t) => Some(t.size),
            Self::Interact(i) => Some(i.size),
            Self::Checkpoint(c) => Some(c.size),
        }
    }
    pub fn size_mut(&mut self) -> Option<&mut [f32; 3]> {
        match self {
            Self::Spawn(_) => None,
            Self::Trigger(t) => Some(&mut t.size),
            Self::Interact(i) => Some(&mut i.size),
            Self::Checkpoint(c) => Some(&mut c.size),
        }
    }
    /// Every action list of this component, for editing and checking.
    pub fn action_lists(&self) -> Vec<&Vec<Kept<Action>>> {
        match self {
            Self::Trigger(t) => vec![&t.on_enter, &t.on_leave],
            Self::Interact(i) => vec![&i.actions],
            Self::Spawn(_) | Self::Checkpoint(_) => Vec::new(),
        }
    }
    pub fn action_lists_mut(&mut self) -> Vec<&mut Vec<Kept<Action>>> {
        match self {
            Self::Trigger(t) => vec![&mut t.on_enter, &mut t.on_leave],
            Self::Interact(i) => vec![&mut i.actions],
            Self::Spawn(_) | Self::Checkpoint(_) => Vec::new(),
        }
    }
}

/// Players start here. With several, the first in the World Outliner wins.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Spawn {
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Actions when a player walks into or out of a box.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Trigger {
    #[serde(default = "default_zone")]
    pub size: [f32; 3],
    /// Run only the first time (per play session).
    #[serde(default)]
    pub once: bool,
    #[serde(default)]
    pub on_enter: Vec<Kept<Action>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on_leave: Vec<Kept<Action>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Actions when a player inside the box presses the interact key.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Interact {
    #[serde(default = "default_zone")]
    pub size: [f32; 3],
    /// Shown while the player is inside, e.g. "Open the door".
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub actions: Vec<Kept<Action>>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One stop of a timed course. Reaching the lowest `order` of a race starts
/// the clock; the others must follow in order; the highest finishes it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Checkpoint {
    #[serde(default = "default_zone")]
    pub size: [f32; 3],
    #[serde(default = "default_race")]
    pub race: String,
    #[serde(default = "first")]
    pub order: u32,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Something a zone does. `node: None` means the object the zone is on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Action {
    Message {
        text: String,
        #[serde(default = "message_seconds")]
        seconds: f32,
    },
    /// Move the player to another object, facing its +Y.
    Teleport {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        target: Option<NodeId>,
    },
    Show {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node: Option<NodeId>,
    },
    Hide {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node: Option<NodeId>,
    },
    /// Show a hidden object or hide a shown one (a door opening and closing).
    Toggle {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        node: Option<NodeId>,
    },
    SetTime {
        hour: u8,
    },
    SetWeather {
        weather: String,
    },
}
impl Tagged for Action {
    const TAG: &'static str = "do";
    const KINDS: &'static [&'static str] = &[
        "message",
        "teleport",
        "show",
        "hide",
        "toggle",
        "set_time",
        "set_weather",
    ];
}
impl Action {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Message { .. } => "Show message",
            Self::Teleport { .. } => "Teleport player",
            Self::Show { .. } => "Show object",
            Self::Hide { .. } => "Hide object",
            Self::Toggle { .. } => "Toggle object",
            Self::SetTime { .. } => "Set time of day",
            Self::SetWeather { .. } => "Set weather",
        }
    }
    /// One of each kind, for the editor's "add action" menu.
    pub fn templates() -> Vec<Action> {
        vec![
            Self::Message {
                text: "Hello!".into(),
                seconds: message_seconds(),
            },
            Self::Teleport { target: None },
            Self::Show { node: None },
            Self::Hide { node: None },
            Self::Toggle { node: None },
            Self::SetTime { hour: 20 },
            Self::SetWeather {
                weather: Weather::Rain.as_str().into(),
            },
        ]
    }
}

/// Weather a world can have. Stored as text (`"rain"`), so a newer SARE's
/// weather in an older SARE is kept and ignored rather than refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weather {
    Clear,
    Cloudy,
    Rain,
    Fog,
}
impl Weather {
    pub const ALL: [Weather; 4] = [Self::Clear, Self::Cloudy, Self::Rain, Self::Fog];
    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|w| w.as_str() == text)
    }
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clear => "clear",
            Self::Cloudy => "cloudy",
            Self::Rain => "rain",
            Self::Fog => "fog",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Clear => "Clear",
            Self::Cloudy => "Cloudy",
            Self::Rain => "Rain",
            Self::Fog => "Fog",
        }
    }
}

/// A place and the way to face there, in Blender axes (metres, degrees
/// about up; 0 faces north, +Y).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position: [f32; 3],
    pub heading: f32,
}
impl Pose {
    pub fn of(transform: &Transform) -> Self {
        Self {
            position: transform.position,
            heading: transform.rotation[2],
        }
    }
}

/// A zone box in Blender axes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoneShape {
    /// Centre of the base.
    pub base: Vec3,
    pub rotation: Quat,
    pub size: Vec3,
}
impl ZoneShape {
    pub fn of(transform: &Transform, size: [f32; 3]) -> Self {
        Self {
            base: Vec3::from_array(transform.position),
            rotation: transform.rotation_quat(),
            size: Vec3::from_array(size).abs(),
        }
    }
    pub fn contains(&self, point: Vec3) -> bool {
        let local = self.rotation.inverse() * (point - self.base);
        local.x.abs() <= self.size.x * 0.5
            && local.y.abs() <= self.size.y * 0.5
            && local.z >= -FEET_TOLERANCE
            && local.z <= self.size.z
    }
    /// The 8 corners in Blender axes; bit 0 = +X, bit 1 = +Y, bit 2 = top.
    pub fn corners(&self) -> [Vec3; 8] {
        std::array::from_fn(|k| {
            let local = Vec3::new(
                if k & 1 == 0 { -0.5 } else { 0.5 } * self.size.x,
                if k & 2 == 0 { -0.5 } else { 0.5 } * self.size.y,
                if k & 4 == 0 { 0.0 } else { self.size.z },
            );
            self.base + self.rotation * local
        })
    }
}

/// The zone of a node, if its component has one.
pub fn zone_of(node: &Node) -> Option<ZoneShape> {
    let size = node.gameplay.as_ref()?.known()?.size()?;
    Some(ZoneShape::of(&node.transform, size))
}

/// What the game should do after a step of [`Logic`].
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    Message {
        text: String,
        seconds: f32,
    },
    Teleport(Pose),
    /// An object was shown or hidden (hidden objects have no collision).
    Visibility {
        node: NodeId,
        visible: bool,
    },
    Time(u8),
    Weather(Weather),
}

#[derive(Clone, Debug)]
enum ZoneKind {
    Trigger(Trigger),
    Interact(Interact),
    Checkpoint { race: String, order: u32 },
}
#[derive(Clone, Debug)]
struct Zone {
    node: NodeId,
    shape: ZoneShape,
    kind: ZoneKind,
}

#[derive(Clone, Copy, Debug)]
struct Progress {
    /// Index into the race's sorted orders of the next checkpoint.
    next: usize,
    started: f32,
}

/// A layout's gameplay, ready to run, and one play session's state.
#[derive(Clone, Debug)]
pub struct Logic {
    zones: Vec<Zone>,
    poses: HashMap<NodeId, Pose>,
    spawn: Option<Pose>,
    /// Sorted checkpoint orders of each race.
    races: HashMap<String, Vec<u32>>,
    /// Visible in the layout, before any action.
    base_visible: HashMap<NodeId, bool>,
    // Session state.
    inside: HashSet<NodeId>,
    fired: HashSet<NodeId>,
    left: HashSet<NodeId>,
    progress: HashMap<String, Progress>,
    visibility: HashMap<NodeId, bool>,
    hour: Option<u8>,
    weather: Option<Weather>,
    clock: f32,
    /// After a teleport: take the new position's zones without entering them,
    /// so two doors leading to each other do not bounce the player back.
    settle: bool,
    /// Objects cannot be shown or hidden here (see [`Logic::set_static_objects`]).
    static_objects: bool,
}

impl Logic {
    pub fn new(layout: &Layout) -> Self {
        let mut zones = Vec::new();
        let mut spawn = None;
        let mut races: HashMap<String, Vec<u32>> = HashMap::new();
        for node in &layout.nodes {
            let Some(component) = node.gameplay.as_ref().and_then(Kept::known) else {
                continue;
            };
            let kind = match component {
                Component::Spawn(_) => {
                    spawn.get_or_insert(Pose::of(&node.transform));
                    continue;
                }
                Component::Trigger(t) => ZoneKind::Trigger(t.clone()),
                Component::Interact(i) => ZoneKind::Interact(i.clone()),
                Component::Checkpoint(c) => {
                    races.entry(c.race.clone()).or_default().push(c.order);
                    ZoneKind::Checkpoint {
                        race: c.race.clone(),
                        order: c.order,
                    }
                }
            };
            if let Some(shape) = zone_of(node) {
                zones.push(Zone {
                    node: node.id,
                    shape,
                    kind,
                });
            }
        }
        for orders in races.values_mut() {
            orders.sort_unstable();
            orders.dedup();
        }
        Self {
            zones,
            poses: layout
                .nodes
                .iter()
                .filter(|n| !n.is_group())
                .map(|n| (n.id, Pose::of(&n.transform)))
                .collect(),
            spawn,
            races,
            base_visible: layout
                .effective_all()
                .into_iter()
                .map(|(id, flags)| (id, flags.visible))
                .collect(),
            inside: HashSet::new(),
            fired: HashSet::new(),
            left: HashSet::new(),
            progress: HashMap::new(),
            visibility: HashMap::new(),
            hour: layout.settings.hour,
            weather: layout.settings.weather.as_deref().and_then(Weather::parse),
            clock: 0.0,
            settle: true,
            static_objects: false,
        }
    }

    /// The world draws its objects as one merged scene (a pack played from
    /// the launcher), so they cannot be shown or hidden: those actions do
    /// nothing, and interactions that only show or hide offer no prompt and
    /// leave the interact key to the game.
    pub fn set_static_objects(&mut self) {
        self.static_objects = true;
    }

    /// The first spawn point object, if the world has one.
    pub fn spawn(&self) -> Option<Pose> {
        self.spawn
    }
    pub fn has_gameplay(&self) -> bool {
        !self.zones.is_empty() || self.spawn.is_some()
    }
    /// Fixed hour of day right now (None: the clock runs).
    pub fn hour(&self) -> Option<u8> {
        self.hour
    }
    pub fn weather(&self) -> Option<Weather> {
        self.weather
    }
    /// Shown right now, after actions.
    pub fn visible(&self, node: NodeId) -> bool {
        self.visibility
            .get(&node)
            .copied()
            .unwrap_or_else(|| self.base_visible.get(&node).copied().unwrap_or(true))
    }
    /// Objects an action hid: they have no collision until shown again.
    pub fn hidden_by_actions(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.visibility
            .iter()
            .filter(|(_, visible)| !**visible)
            .map(|(id, _)| *id)
    }
    /// Objects an action showed or hid, and how they are now.
    pub fn changed_visibility(&self) -> &HashMap<NodeId, bool> {
        &self.visibility
    }
    /// The interaction the player stands in (nearest one), if any.
    pub fn prompt(&self, feet: [f32; 3]) -> Option<String> {
        let (_, interact) = self.interaction_at(feet)?;
        Some(if interact.prompt.trim().is_empty() {
            "Use".into()
        } else {
            interact.prompt.clone()
        })
    }
    fn interaction_at(&self, feet: [f32; 3]) -> Option<(NodeId, &Interact)> {
        let p = Vec3::from_array(feet);
        self.zones
            .iter()
            .filter_map(|z| match &z.kind {
                ZoneKind::Interact(i) if z.shape.contains(p) && self.usable(i) => {
                    Some((z.node, i, z.shape.base.distance_squared(p)))
                }
                _ => None,
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(node, interact, _)| (node, interact))
    }

    /// Whether pressing E here would do something in this world.
    fn usable(&self, interact: &Interact) -> bool {
        !self.static_objects
            || interact.actions.iter().any(|a| {
                !matches!(
                    a,
                    Kept::Known(Action::Show { .. } | Action::Hide { .. } | Action::Toggle { .. })
                )
            })
    }

    /// The player pressed the interact key with their feet at `feet`.
    /// None: no interaction here (the key may do something else).
    pub fn interact(&mut self, feet: [f32; 3]) -> Option<Vec<Effect>> {
        let (node, interact) = self.interaction_at(feet)?;
        let actions = interact.actions.clone();
        let mut out = Vec::new();
        self.run(node, &actions, &mut out);
        Some(out)
    }

    /// Advance by `dt` seconds with the player's feet at `feet` (Blender axes).
    pub fn step(&mut self, feet: [f32; 3], dt: f32) -> Vec<Effect> {
        self.clock += dt.max(0.0);
        let p = Vec3::from_array(feet);
        let now: HashSet<NodeId> = self
            .zones
            .iter()
            .filter(|z| z.shape.contains(p))
            .map(|z| z.node)
            .collect();
        let mut out = Vec::new();
        if std::mem::take(&mut self.settle) {
            // Arrival zones are taken quietly, but the zones the player was
            // teleported out of still see them leave.
            let left: Vec<usize> = (0..self.zones.len())
                .filter(|i| {
                    let node = self.zones[*i].node;
                    self.inside.contains(&node) && !now.contains(&node)
                })
                .collect();
            self.inside = now;
            for index in left {
                let node = self.zones[index].node;
                if let ZoneKind::Trigger(trigger) = self.zones[index].kind.clone() {
                    self.leave(node, &trigger, &mut out);
                }
            }
            return out;
        }
        for index in 0..self.zones.len() {
            let node = self.zones[index].node;
            let entered = now.contains(&node) && !self.inside.contains(&node);
            let left = !now.contains(&node) && self.inside.contains(&node);
            if !entered && !left {
                continue;
            }
            match self.zones[index].kind.clone() {
                ZoneKind::Trigger(trigger) => {
                    // `once`: the enter actions and the leave actions each
                    // run one time per play session.
                    if entered && !(trigger.once && self.fired.contains(&node)) {
                        self.fired.insert(node);
                        self.run(node, &trigger.on_enter, &mut out);
                    }
                    if left {
                        self.leave(node, &trigger, &mut out);
                    }
                }
                ZoneKind::Checkpoint { race, order } if entered => {
                    self.checkpoint(&race, order, &mut out);
                }
                _ => {}
            }
        }
        // A teleport during this step sets `settle` again: the next step
        // takes the arrival zones quietly.
        self.inside = now;
        out
    }

    fn leave(&mut self, node: NodeId, trigger: &Trigger, out: &mut Vec<Effect>) {
        if !(trigger.once && self.left.contains(&node)) {
            self.left.insert(node);
            self.run(node, &trigger.on_leave, out);
        }
    }

    fn checkpoint(&mut self, race: &str, order: u32, out: &mut Vec<Effect>) {
        let Some(orders) = self.races.get(race) else {
            return;
        };
        let Some(index) = orders.iter().position(|o| *o == order) else {
            return;
        };
        let total = orders.len();
        let name = if race == DEFAULT_RACE {
            String::new()
        } else {
            format!("{race}: ")
        };
        let message = |text: String| Effect::Message { text, seconds: 3.0 };
        if index == 0 {
            if total == 1 {
                out.push(message(format!("{name}Checkpoint reached")));
                return;
            }
            self.progress.insert(
                race.to_owned(),
                Progress {
                    next: 1,
                    started: self.clock,
                },
            );
            out.push(message(format!("{name}Go! Checkpoint 1/{total}")));
            return;
        }
        let Some(progress) = self.progress.get(race).copied() else {
            return;
        };
        if index != progress.next {
            return;
        }
        if index + 1 == total {
            self.progress.remove(race);
            let seconds = self.clock - progress.started;
            out.push(Effect::Message {
                text: format!("{name}Finished in {seconds:.1} s"),
                seconds: 6.0,
            });
        } else {
            self.progress.insert(
                race.to_owned(),
                Progress {
                    next: index + 1,
                    ..progress
                },
            );
            out.push(message(format!("{name}Checkpoint {}/{total}", index + 1)));
        }
    }

    fn run(&mut self, owner: NodeId, actions: &[Kept<Action>], out: &mut Vec<Effect>) {
        for action in actions.iter().filter_map(Kept::known) {
            match action {
                Action::Message { text, seconds } => out.push(Effect::Message {
                    text: text.clone(),
                    seconds: seconds.clamp(0.5, 60.0),
                }),
                Action::Teleport { target } => {
                    if let Some(pose) = target.and_then(|t| self.poses.get(&t)) {
                        out.push(Effect::Teleport(*pose));
                        self.settle = true;
                    }
                }
                Action::Show { node } | Action::Hide { node } | Action::Toggle { node } => {
                    let node = node.unwrap_or(owner);
                    if self.static_objects || !self.base_visible.contains_key(&node) {
                        continue;
                    }
                    let visible = match action {
                        Action::Show { .. } => true,
                        Action::Hide { .. } => false,
                        _ => !self.visible(node),
                    };
                    self.visibility.insert(node, visible);
                    out.push(Effect::Visibility { node, visible });
                }
                Action::SetTime { hour } => {
                    // A fixed clock moves to the new hour; a running clock
                    // jumps there and keeps running.
                    let hour = (*hour).min(23);
                    if self.hour.is_some() {
                        self.hour = Some(hour);
                    }
                    out.push(Effect::Time(hour));
                }
                Action::SetWeather { weather } => {
                    if let Some(weather) = Weather::parse(weather) {
                        self.weather = Some(weather);
                        out.push(Effect::Weather(weather));
                    }
                }
            }
        }
    }
}

/// A problem in a layout's gameplay, shown in the editor.
#[derive(Clone, Debug, PartialEq)]
pub struct Problem {
    pub node: NodeId,
    pub text: String,
}

/// Check every object's gameplay: missing targets, bad sizes, kinds this
/// SARE cannot run.
pub fn check(layout: &Layout) -> Vec<Problem> {
    let mut problems = Vec::new();
    let objects: HashSet<NodeId> = layout
        .nodes
        .iter()
        .filter(|n| !n.is_group())
        .map(|n| n.id)
        .collect();
    let exists = |id: NodeId| objects.contains(&id);
    let mut orders: HashMap<(&str, u32), NodeId> = HashMap::new();
    for node in &layout.nodes {
        let mut say = |text: String| {
            problems.push(Problem {
                node: node.id,
                text: format!("{}: {text}", node.name),
            })
        };
        let Some(gameplay) = &node.gameplay else {
            continue;
        };
        let component = match gameplay {
            Kept::Known(component) => component,
            Kept::Unknown(value) => {
                let kind = value.get("kind").and_then(Value::as_str).unwrap_or("?");
                say(format!(
                    "gameplay kind {kind:?} needs a newer SARE; it is kept but does nothing here"
                ));
                continue;
            }
        };
        if let Some(size) = component.size() {
            if !size
                .iter()
                .all(|v| v.is_finite() && *v >= 0.1 && *v <= MAX_ZONE)
            {
                say(format!(
                    "zone size must be between 0.1 and {MAX_ZONE} m on every side"
                ));
            }
        }
        if let Component::Checkpoint(c) = component {
            if let Some(other) = orders.insert((c.race.as_str(), c.order), node.id) {
                let other = layout.node(other).map_or("another object", |n| &n.name);
                say(format!(
                    "checkpoint {} of race {:?} is also used by {other}",
                    c.order, c.race
                ));
            }
        }
        if let Component::Interact(i) = component {
            if i.actions.is_empty() {
                say("the interaction has no actions".into());
            }
        }
        if let Component::Trigger(t) = component {
            if t.on_enter.is_empty() && t.on_leave.is_empty() {
                say("the trigger has no actions".into());
            }
        }
        for action in component.action_lists().into_iter().flatten() {
            let action = match action {
                Kept::Known(action) => action,
                Kept::Unknown(value) => {
                    let kind = value.get("do").and_then(Value::as_str).unwrap_or("?");
                    say(format!(
                        "action {kind:?} needs a newer SARE; it is kept but does nothing here"
                    ));
                    continue;
                }
            };
            match action {
                Action::Teleport { target: None } => say("Teleport has no destination".into()),
                Action::Teleport { target: Some(t) } if !exists(*t) => {
                    say(format!("Teleport destination {t} no longer exists"))
                }
                Action::Show { node: Some(t) }
                | Action::Hide { node: Some(t) }
                | Action::Toggle { node: Some(t) }
                    if !exists(*t) =>
                {
                    say(format!("{} target {t} no longer exists", action.label()))
                }
                Action::SetTime { hour } if *hour > 23 => say(format!("hour {hour} is not 0-23")),
                Action::SetWeather { weather } if Weather::parse(weather).is_none() => {
                    say(format!("unknown weather {weather:?}"))
                }
                Action::Message { text, .. } if text.trim().is_empty() => {
                    say("the message is empty".into())
                }
                _ => {}
            }
        }
    }
    problems
}

/// The gameplay a built-in asset comes with when placed, so dropping a
/// "Door" from the Asset Browser gives a working door. `next_order` is the
/// next free checkpoint number.
pub fn preset(builtin: &str, next_order: u32) -> Option<Component> {
    let message = |text: &str| -> Kept<Action> {
        Action::Message {
            text: text.into(),
            seconds: message_seconds(),
        }
        .into()
    };
    Some(match builtin {
        "marker-spawn" => Component::Spawn(Spawn::default()),
        "marker-trigger" => Component::Trigger(Trigger {
            size: [4.0, 4.0, 3.0],
            once: false,
            on_enter: vec![message("You entered the zone")],
            on_leave: Vec::new(),
            extra: Map::new(),
        }),
        "marker-teleport" => Component::Trigger(Trigger {
            size: [1.5, 1.5, 2.5],
            once: false,
            on_enter: vec![Action::Teleport { target: None }.into()],
            on_leave: Vec::new(),
            extra: Map::new(),
        }),
        "marker-interact" => Component::Interact(Interact {
            size: [2.0, 2.0, 2.5],
            prompt: "Use".into(),
            actions: vec![message("It works!")],
            extra: Map::new(),
        }),
        "marker-checkpoint" => Component::Checkpoint(Checkpoint {
            size: [6.0, 6.0, 4.0],
            race: default_race(),
            order: next_order.max(1),
            extra: Map::new(),
        }),
        "door" => Component::Interact(Interact {
            size: [2.4, 3.0, 2.6],
            prompt: "Open / close the door".into(),
            actions: vec![Action::Toggle { node: None }.into()],
            extra: Map::new(),
        }),
        "door-teleport" => Component::Interact(Interact {
            size: [2.4, 3.0, 2.6],
            prompt: "Go through the door".into(),
            actions: vec![Action::Teleport { target: None }.into()],
            extra: Map::new(),
        }),
        _ => return None,
    })
}

/// The next free checkpoint number of the default race.
pub fn next_checkpoint(layout: &Layout) -> u32 {
    layout
        .nodes
        .iter()
        .filter_map(|n| match n.gameplay.as_ref()?.known()? {
            Component::Checkpoint(c) if c.race == DEFAULT_RACE => Some(c.order),
            _ => None,
        })
        .max()
        .map_or(1, |o| o + 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_layout::AssetRef;

    fn object(
        layout: &mut Layout,
        name: &str,
        at: [f32; 3],
        gameplay: Option<Component>,
    ) -> NodeId {
        let id = layout.allocate_id();
        let mut node = Node::object(
            id,
            name,
            AssetRef::Builtin("cube".into()),
            Transform::at(at),
        );
        node.gameplay = gameplay.map(Kept::Known);
        layout.nodes.push(node);
        id
    }
    fn trigger(actions: Vec<Action>) -> Component {
        Component::Trigger(Trigger {
            size: [2.0, 2.0, 2.0],
            once: false,
            on_enter: actions.into_iter().map(Kept::Known).collect(),
            on_leave: Vec::new(),
            extra: Map::new(),
        })
    }
    fn messages(effects: &[Effect]) -> Vec<String> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Message { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn gameplay_round_trips_and_keeps_unknown_kinds() {
        let mut layout = Layout::default();
        object(
            &mut layout,
            "Zone",
            [0.0; 3],
            Some(trigger(vec![Action::Message {
                text: "Hi".into(),
                seconds: 2.0,
            }])),
        );
        let mut doc: Value = serde_json::to_value(&layout).unwrap();
        assert_eq!(doc["nodes"][0]["gameplay"]["kind"], "trigger");
        assert_eq!(doc["nodes"][0]["gameplay"]["on_enter"][0]["do"], "message");
        // A newer SARE's component and action, and an extra field.
        doc["nodes"][0]["gameplay"]["on_enter"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({"do": "play_sound", "file": "a.ogg"}));
        doc["nodes"][0]["gameplay"]["colour"] = Value::from("red");
        let id = layout.allocate_id();
        doc["next_id"] = Value::from(layout.next_id);
        doc["nodes"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "id": id, "name": "Vendor", "asset": "builtin:cube",
                "gameplay": {"kind": "shop", "items": [1, 2]}
            }));
        let read = Layout::from_json(&doc.to_string()).unwrap();
        let again: Value = serde_json::from_str(&read.to_json().unwrap()).unwrap();
        assert_eq!(
            again["nodes"][0]["gameplay"]["on_enter"][1]["file"],
            "a.ogg"
        );
        assert_eq!(again["nodes"][0]["gameplay"]["colour"], "red");
        assert_eq!(again["nodes"][1]["gameplay"]["kind"], "shop");
        assert_eq!(again["nodes"][1]["gameplay"]["items"][1], 2);
        let problems = check(&read);
        assert!(problems.iter().any(|p| p.text.contains("play_sound")));
        assert!(problems.iter().any(|p| p.text.contains("shop")));
        // Unknown parts do nothing but do not stop the rest.
        let mut logic = Logic::new(&read);
        logic.step([10.0, 10.0, 0.0], 0.1);
        assert_eq!(messages(&logic.step([0.0; 3], 0.1)), vec!["Hi"]);
    }

    #[test]
    fn broken_known_kinds_are_errors_not_dropped() {
        let text = r#"{"format":"sare-world-layout","version":1,"next_id":2,"nodes":[
            {"id":1,"name":"Z","asset":"builtin:cube",
             "gameplay":{"kind":"trigger","on_enter":[{"do":"set_time","hour":"late"}]}}]}"#;
        let error = Layout::from_json(text).unwrap_err();
        assert!(format!("{error:#}").contains("set_time"), "{error:#}");
    }

    #[test]
    fn triggers_fire_on_enter_and_leave_and_once() {
        let mut layout = Layout::default();
        let zone = object(&mut layout, "Zone", [0.0; 3], None);
        layout.node_mut(zone).unwrap().gameplay = Some(Kept::Known(Component::Trigger(Trigger {
            size: [2.0, 2.0, 2.0],
            once: false,
            on_enter: vec![Action::Message {
                text: "in".into(),
                seconds: 1.0,
            }
            .into()],
            on_leave: vec![Action::Message {
                text: "out".into(),
                seconds: 1.0,
            }
            .into()],
            extra: Map::new(),
        })));
        let mut logic = Logic::new(&layout);
        let outside = [5.0, 0.0, 0.0];
        assert!(logic.step(outside, 0.1).is_empty());
        assert_eq!(messages(&logic.step([0.5, 0.5, 0.0], 0.1)), vec!["in"]);
        assert!(
            logic.step([0.4, 0.5, 1.0], 0.1).is_empty(),
            "staying inside is quiet"
        );
        assert_eq!(messages(&logic.step(outside, 0.1)), vec!["out"]);
        assert_eq!(messages(&logic.step([0.0; 3], 0.1)), vec!["in"]);

        if let Some(Kept::Known(Component::Trigger(t))) =
            &mut layout.node_mut(zone).unwrap().gameplay
        {
            t.once = true;
        }
        let mut logic = Logic::new(&layout);
        logic.step(outside, 0.1);
        assert_eq!(messages(&logic.step([0.0; 3], 0.1)), vec!["in"]);
        assert_eq!(messages(&logic.step(outside, 0.1)), vec!["out"]);
        assert!(logic.step([0.0; 3], 0.1).is_empty(), "once means once");
        assert!(logic.step(outside, 0.1).is_empty());
    }

    #[test]
    fn players_starting_inside_a_zone_do_not_trigger_it() {
        let mut layout = Layout::default();
        object(
            &mut layout,
            "Zone",
            [0.0; 3],
            Some(trigger(vec![Action::SetTime { hour: 3 }])),
        );
        let mut logic = Logic::new(&layout);
        assert!(logic.step([0.0; 3], 0.1).is_empty());
        assert!(logic.step([0.0; 3], 0.1).is_empty());
    }

    #[test]
    fn zones_turn_with_the_object_and_stand_on_it() {
        let mut transform = Transform::at([10.0, 0.0, 5.0]);
        transform.rotation[2] = 90.0;
        transform.scale = [10.0; 3]; // ignored: sizes are metres
        let zone = ZoneShape::of(&transform, [4.0, 1.0, 2.0]);
        // Turned 90 degrees, the 4 m side runs north-south.
        assert!(zone.contains(Vec3::new(10.0, 1.8, 5.0)));
        assert!(!zone.contains(Vec3::new(11.8, 0.0, 5.0)));
        assert!(zone.contains(Vec3::new(10.0, 0.0, 6.9)));
        assert!(!zone.contains(Vec3::new(10.0, 0.0, 7.5)));
        assert!(
            zone.contains(Vec3::new(10.0, 0.0, 4.7)),
            "feet may sink a little"
        );
        assert!(!zone.contains(Vec3::new(10.0, 0.0, 4.0)));
        let corners = zone.corners();
        assert!(
            corners[7].distance(Vec3::new(9.5, 2.0, 7.0)) < 1e-4,
            "{}",
            corners[7]
        );
    }

    #[test]
    fn doors_toggle_and_teleports_do_not_bounce() {
        let mut layout = Layout::default();
        let door = object(&mut layout, "Door", [0.0; 3], preset("door", 1));
        let inside = object(&mut layout, "Inside", [100.0, 0.0, 0.0], None);
        let outside = object(&mut layout, "Outside", [0.0, -3.0, 0.0], None);
        let pad_in = object(&mut layout, "Pad in", [0.0, 3.0, 0.0], None);
        let pad_out = object(&mut layout, "Pad out", [100.0, 0.0, 0.0], None);
        let pad = |target| {
            trigger(vec![Action::Teleport {
                target: Some(target),
            }])
        };
        layout.node_mut(pad_in).unwrap().gameplay = Some(pad(inside).into());
        layout.node_mut(pad_out).unwrap().gameplay = Some(pad(outside).into());
        layout.node_mut(inside).unwrap().transform.rotation[2] = 45.0;
        let mut logic = Logic::new(&layout);
        assert_eq!(
            logic.prompt([0.0; 3]).as_deref(),
            Some("Open / close the door")
        );
        assert_eq!(logic.prompt([20.0, 0.0, 0.0]), None);
        assert_eq!(logic.interact([20.0, 0.0, 0.0]), None);
        assert_eq!(
            logic.interact([0.0; 3]).unwrap(),
            vec![Effect::Visibility {
                node: door,
                visible: false
            }]
        );
        assert!(!logic.visible(door));
        assert_eq!(logic.hidden_by_actions().collect::<Vec<_>>(), vec![door]);
        logic.interact([0.0; 3]);
        assert!(logic.visible(door));

        logic.step([0.0, -10.0, 0.0], 0.1);
        let effects = logic.step([0.0, 3.0, 0.0], 0.1);
        assert_eq!(
            effects,
            vec![Effect::Teleport(Pose {
                position: [100.0, 0.0, 0.0],
                heading: 45.0
            })]
        );
        // Arriving on the way-back pad does not send the player back...
        assert!(logic.step([100.0, 0.0, 0.0], 0.1).is_empty());
        assert!(logic.step([100.2, 0.0, 0.0], 0.1).is_empty());
        // ...until they step off and on again.
        logic.step([110.0, 0.0, 0.0], 0.1);
        assert!(matches!(
            logic.step([100.0, 0.0, 0.0], 0.1)[..],
            [Effect::Teleport(_)]
        ));
    }

    #[test]
    fn teleporting_out_of_a_zone_still_leaves_it() {
        let mut layout = Layout::default();
        let far = object(&mut layout, "Far", [100.0, 0.0, 0.0], None);
        let zone = object(&mut layout, "Zone", [0.0; 3], None);
        layout.node_mut(zone).unwrap().gameplay = Some(Kept::Known(Component::Trigger(Trigger {
            size: [2.0, 2.0, 2.0],
            once: false,
            on_enter: vec![Action::Teleport { target: Some(far) }.into()],
            on_leave: vec![Action::Message {
                text: "left".into(),
                seconds: 1.0,
            }
            .into()],
            extra: Map::new(),
        })));
        let mut logic = Logic::new(&layout);
        logic.step([50.0, 0.0, 0.0], 0.1);
        assert!(matches!(
            logic.step([0.0; 3], 0.1)[..],
            [Effect::Teleport(_)]
        ));
        assert_eq!(messages(&logic.step([100.0, 0.0, 0.0], 0.1)), vec!["left"]);
    }

    #[test]
    fn a_running_clock_jumps_and_keeps_running() {
        let mut layout = Layout::default();
        layout.settings.hour = None;
        object(
            &mut layout,
            "Dusk",
            [0.0; 3],
            Some(trigger(vec![Action::SetTime { hour: 20 }])),
        );
        let mut logic = Logic::new(&layout);
        logic.step([9.0, 9.0, 0.0], 0.1);
        assert_eq!(logic.step([0.0; 3], 0.1), vec![Effect::Time(20)]);
        assert_eq!(logic.hour(), None, "the clock is not frozen");
    }

    #[test]
    fn static_worlds_leave_show_and_hide_to_the_game() {
        let mut layout = Layout::default();
        let door = object(&mut layout, "Door", [0.0; 3], preset("door", 1));
        let sign = object(
            &mut layout,
            "Sign",
            [20.0, 0.0, 0.0],
            preset("marker-interact", 1),
        );
        let _ = sign;
        let mut logic = Logic::new(&layout);
        logic.set_static_objects();
        assert_eq!(
            logic.prompt([0.0; 3]),
            None,
            "a door that cannot open offers nothing"
        );
        assert_eq!(logic.interact([0.0; 3]), None, "E stays with the game");
        assert!(logic.visible(door));
        assert!(
            logic.interact([20.0, 0.0, 0.0]).is_some(),
            "other interactions still work"
        );
    }

    #[test]
    fn checkpoints_time_a_course_in_order() {
        let mut layout = Layout::default();
        for (i, x) in [0.0, 20.0, 40.0].into_iter().enumerate() {
            let order = next_checkpoint(&layout);
            assert_eq!(order, i as u32 + 1);
            object(
                &mut layout,
                "CP",
                [x, 0.0, 0.0],
                preset("marker-checkpoint", order),
            );
        }
        let mut logic = Logic::new(&layout);
        let at = |x: f32| [x, 0.0, 0.0];
        logic.step(at(-50.0), 0.0);
        assert_eq!(
            messages(&logic.step(at(40.0), 1.0)),
            Vec::<String>::new(),
            "not started"
        );
        logic.step(at(-50.0), 0.0);
        assert_eq!(
            messages(&logic.step(at(0.0), 1.0)),
            vec!["Go! Checkpoint 1/3"]
        );
        logic.step(at(-50.0), 2.0);
        assert!(
            messages(&logic.step(at(40.0), 1.0)).is_empty(),
            "skipping is ignored"
        );
        logic.step(at(-50.0), 0.0);
        assert_eq!(messages(&logic.step(at(20.0), 1.0)), vec!["Checkpoint 2/3"]);
        logic.step(at(-50.0), 0.5);
        assert_eq!(
            messages(&logic.step(at(40.0), 0.5)),
            vec!["Finished in 5.0 s"]
        );
    }

    #[test]
    fn spawn_time_weather_and_checks() {
        let mut layout = Layout::default();
        layout.settings.weather = Some("fog".into());
        let mut spawn_at = Transform::at([1.0, 2.0, 3.0]);
        spawn_at.rotation[2] = -90.0;
        let id = layout.allocate_id();
        let mut spawn = Node::object(
            id,
            "Start",
            AssetRef::Builtin("marker-spawn".into()),
            spawn_at,
        );
        spawn.gameplay = preset("marker-spawn", 1).map(Kept::Known);
        layout.nodes.push(spawn);
        object(
            &mut layout,
            "Second start",
            [9.0; 3],
            preset("marker-spawn", 1),
        );
        let night = object(
            &mut layout,
            "Night",
            [50.0, 0.0, 0.0],
            Some(trigger(vec![
                Action::SetTime { hour: 40 },
                Action::SetWeather {
                    weather: "rain".into(),
                },
                Action::SetWeather {
                    weather: "snow".into(),
                },
            ])),
        );
        let mut logic = Logic::new(&layout);
        assert_eq!(
            logic.spawn(),
            Some(Pose {
                position: [1.0, 2.0, 3.0],
                heading: -90.0
            })
        );
        assert_eq!(logic.weather(), Some(Weather::Fog));
        assert_eq!(logic.hour(), Some(12));
        logic.step([0.0; 3], 0.1);
        let effects = logic.step([50.0, 0.0, 0.0], 0.1);
        assert_eq!(
            effects,
            vec![Effect::Time(23), Effect::Weather(Weather::Rain)]
        );
        assert_eq!(
            (logic.hour(), logic.weather()),
            (Some(23), Some(Weather::Rain))
        );

        let problems = check(&layout);
        assert!(problems
            .iter()
            .any(|p| p.node == night && p.text.contains("hour 40")));
        assert!(problems.iter().any(|p| p.text.contains("\"snow\"")));
        let mut broken = Layout::default();
        object(&mut broken, "Pad", [0.0; 3], preset("marker-teleport", 1));
        object(
            &mut broken,
            "Door",
            [0.0; 3],
            Some(trigger(vec![Action::Hide { node: Some(99) }])),
        );
        let problems = check(&broken);
        assert!(problems[0].text.contains("no destination"), "{problems:?}");
        assert!(
            problems[1].text.contains("99 no longer exists"),
            "{problems:?}"
        );
    }
}
