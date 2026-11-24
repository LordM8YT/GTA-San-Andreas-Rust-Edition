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
use std::collections::{BTreeMap, HashMap, HashSet};

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
    /// Players in a vehicle can use it too (a drive-through, a garage
    /// door); otherwise it is on foot only.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub vehicles: bool,
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
        self.contains_within(point, 0.0)
    }
    /// Inside the box grown by `slack` metres on every side.
    pub fn contains_within(&self, point: Vec3, slack: f32) -> bool {
        let local = self.rotation.inverse() * (point - self.base);
        local.x.abs() <= self.size.x * 0.5 + slack
            && local.y.abs() <= self.size.y * 0.5 + slack
            && local.z >= -FEET_TOLERANCE - slack
            && local.z <= self.size.z + slack
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

/// What the game should do after a step of the world logic.
#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// For the player who caused it.
    Message { text: String, seconds: f32 },
    /// Moves the player who caused it.
    Teleport(Pose),
    /// Shared: an object was shown or hidden (hidden objects have no collision).
    Visibility { node: NodeId, visible: bool },
    /// Shared: the hour of day.
    Time(u8),
    /// Shared: the weather.
    Weather(Weather),
}
impl Effect {
    /// Changes the world everyone in it shares, not only the one player.
    pub fn is_shared(&self) -> bool {
        matches!(
            self,
            Self::Visibility { .. } | Self::Time(_) | Self::Weather(_)
        )
    }
}

/// Gameplay moments, for scripts (a server's Lua resources) to react to.
/// Collected per player; see [`PlayerState::take_events`].
#[derive(Clone, Debug, PartialEq)]
pub enum Happened {
    Entered(NodeId),
    Left(NodeId),
    Used(NodeId),
    /// Reached checkpoint `index + 1` of `total` (the start is index 0).
    Checkpoint {
        race: String,
        index: usize,
        total: usize,
    },
    Finished {
        race: String,
        seconds: f32,
    },
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

/// A layout's gameplay, compiled once: zones, places and courses. It holds
/// no state, so one `World` serves every player and every instance: the
/// state lives in [`Shared`] (per world instance) and [`PlayerState`] (per
/// player). A server runs the same code as a client.
#[derive(Clone, Debug)]
pub struct World {
    zones: Vec<Zone>,
    poses: HashMap<NodeId, Pose>,
    spawn: Option<Pose>,
    /// Sorted checkpoint orders of each race.
    races: HashMap<String, Vec<u32>>,
    /// Visible in the layout, before any action.
    base_visible: HashMap<NodeId, bool>,
    /// Objects actions show or hide: the only ones a played world can.
    switchable: HashSet<NodeId>,
    hour: Option<u8>,
    weather: Option<Weather>,
}

/// What everyone in one world instance (a server's routing bucket) shares:
/// objects shown or hidden by actions, the hour and the weather. `revision`
/// counts changes, so a server knows who needs a new [`Snapshot`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shared {
    visibility: HashMap<NodeId, bool>,
    hour: Option<u8>,
    weather: Option<Weather>,
    /// The latest jump of a running clock (number, hour), so players who
    /// get it later still jump once.
    jump: Option<(u32, u8)>,
    /// Jumps made so far, kept through a reset: players compare jump
    /// numbers, so a number must never come round again.
    jumps: u32,
    /// Objects scripts moved, and how far along each move is.
    motions: BTreeMap<NodeId, Motion>,
    revision: u64,
}

/// A move a script made: an object going from one pose to another over
/// `seconds` (0: at once), `elapsed` seconds in. Its zones stay where the
/// object was placed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Motion {
    pub from: Pose,
    pub to: Pose,
    pub seconds: f32,
    pub elapsed: f32,
}
impl Motion {
    /// Longest move, seconds.
    pub const MAX_SECONDS: f32 = 600.0;
    /// Where the object is now: eased in and out along the way, turning the
    /// short way round.
    pub fn now(&self) -> Pose {
        let t = if self.seconds <= 0.0 {
            1.0
        } else {
            (self.elapsed / self.seconds).clamp(0.0, 1.0)
        };
        let t = t * t * (3.0 - 2.0 * t);
        let from = Vec3::from_array(self.from.position);
        let to = Vec3::from_array(self.to.position);
        let turn = (self.to.heading - self.from.heading + 180.0).rem_euclid(360.0) - 180.0;
        Pose {
            position: from.lerp(to, t).to_array(),
            heading: self.from.heading + turn * t,
        }
    }
    pub fn done(&self) -> bool {
        self.elapsed >= self.seconds
    }
}

/// One player's progress: the zones they stand in, the once-triggers they
/// used, their courses and their clock.
#[derive(Clone, Debug)]
pub struct PlayerState {
    inside: HashSet<NodeId>,
    fired: HashSet<NodeId>,
    left: HashSet<NodeId>,
    progress: HashMap<String, Progress>,
    clock: f32,
    /// After a teleport or a spawn: take the zones where the player is
    /// without entering them, so two doors leading to each other do not
    /// bounce the player back.
    settle: bool,
    events: Vec<Happened>,
}
impl Default for PlayerState {
    fn default() -> Self {
        Self {
            inside: HashSet::new(),
            fired: HashSet::new(),
            left: HashSet::new(),
            progress: HashMap::new(),
            clock: 0.0,
            settle: true,
            events: Vec::new(),
        }
    }
}
impl PlayerState {
    pub fn new() -> Self {
        Self::default()
    }
    /// What happened since the last call, oldest first.
    pub fn take_events(&mut self) -> Vec<Happened> {
        std::mem::take(&mut self.events)
    }
    /// The player was moved by something else (a server script, a respawn):
    /// take the zones at the new place quietly.
    pub fn moved(&mut self) {
        self.settle = true;
    }
    /// Time passes for the player without a step (a server waiting for
    /// them to arrive somewhere): course times keep running.
    pub fn wait(&mut self, dt: f32) {
        self.clock += dt.max(0.0);
    }
    /// The player's world instance was reset: "only the first time"
    /// triggers work again, and the zones they stand in are taken quietly.
    /// Course progress and times are kept.
    pub fn world_reset(&mut self) {
        self.fired.clear();
        self.left.clear();
        self.settle = true;
    }
}

impl Shared {
    /// Fixed hour of day (None: the clock runs).
    pub fn hour(&self) -> Option<u8> {
        self.hour
    }
    pub fn weather(&self) -> Option<Weather> {
        self.weather
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    /// Back to how `world` starts: every object as placed, the world's own
    /// hour and weather. A new revision, so every player gets it.
    pub fn reset(&mut self, world: &World) {
        let (revision, jumps) = (self.revision + 1, self.jumps);
        *self = Shared {
            revision,
            jumps,
            ..world.shared()
        };
    }
    /// Objects actions hid.
    /// Time passes for moving objects (no new revision: everyone moves them
    /// along by themselves).
    pub fn advance(&mut self, dt: f32) {
        for motion in self.motions.values_mut() {
            if !motion.done() {
                motion.elapsed = (motion.elapsed + dt.max(0.0)).min(motion.seconds);
            }
        }
    }
    /// Start moving `node` from where it is now to `to` over `seconds`.
    /// False when scripts may not move it.
    pub fn place(&mut self, world: &World, node: NodeId, to: Pose, seconds: f32) -> bool {
        if !world.switchable.contains(&node)
            || !to.position.iter().all(|v| v.is_finite())
            || !to.heading.is_finite()
        {
            return false;
        }
        let Some(from) = self.pose(world, node) else {
            return false;
        };
        let seconds = if seconds.is_finite() {
            seconds.clamp(0.0, Motion::MAX_SECONDS)
        } else {
            0.0
        };
        self.motions.insert(
            node,
            Motion {
                from,
                to,
                seconds,
                elapsed: 0.0,
            },
        );
        self.revision += 1;
        true
    }
    /// Where `node` is now: moved by a script, or as placed.
    pub fn pose(&self, world: &World, node: NodeId) -> Option<Pose> {
        match self.motions.get(&node) {
            Some(motion) => Some(motion.now()),
            None => world.poses.get(&node).copied(),
        }
    }
    /// The objects scripts moved, where they are now.
    pub fn moved(&self) -> impl Iterator<Item = (NodeId, Pose)> + '_ {
        self.motions.iter().map(|(id, m)| (*id, m.now()))
    }
    /// Whether some move is still on its way.
    pub fn moving(&self) -> bool {
        self.motions.values().any(|m| !m.done())
    }
    pub fn hidden_ids(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.visibility
            .iter()
            .filter(|(_, visible)| !**visible)
            .map(|(id, _)| *id)
    }
    /// Show or hide an object of `world`. False when there is no such
    /// object or it cannot be shown or hidden (only objects some action
    /// shows or hides can: the rest are part of the merged world).
    /// Nothing changes when it already is that way.
    pub fn set_visible(&mut self, world: &World, node: NodeId, visible: bool) -> bool {
        if !world.switchable.contains(&node) {
            return false;
        }
        if world.visible(self, node) != visible {
            // Only differences from the layout are kept, so snapshots stay small.
            if world.base_visible.get(&node) == Some(&visible) {
                self.visibility.remove(&node);
            } else {
                self.visibility.insert(node, visible);
            }
            self.revision += 1;
        }
        true
    }
    /// Make a running clock jump to `hour` (for everyone in the instance).
    pub fn jump_to(&mut self, hour: u8) {
        self.jumps = self.jumps.wrapping_add(1);
        self.jump = Some((self.jumps, hour.min(23)));
        self.revision += 1;
    }
    /// The latest clock jump: its number and hour.
    pub fn jump(&self) -> Option<(u32, u8)> {
        self.jump
    }
    /// Fix the hour (None lets the clock run).
    pub fn set_hour(&mut self, hour: Option<u8>) {
        let hour = hour.map(|h| h.min(23));
        if self.hour != hour {
            self.hour = hour;
            self.revision += 1;
        }
    }
    pub fn set_weather(&mut self, weather: Option<Weather>) {
        if self.weather != weather {
            self.weather = weather;
            self.revision += 1;
        }
    }
    /// The state as it travels to players.
    pub fn snapshot(&self) -> Snapshot {
        let mut shown = Vec::new();
        let mut hidden = Vec::new();
        for (node, visible) in &self.visibility {
            if *visible {
                shown.push(*node);
            } else {
                hidden.push(*node);
            }
        }
        shown.sort_unstable();
        hidden.sort_unstable();
        Snapshot {
            revision: self.revision,
            shown,
            hidden,
            hour: self.hour,
            weather: self.weather.map(|w| w.as_str().to_owned()),
            jump: self.jump,
            moved: self
                .motions
                .iter()
                .map(|(node, m)| SnapshotMotion {
                    node: *node,
                    from: [
                        m.from.position[0],
                        m.from.position[1],
                        m.from.position[2],
                        m.from.heading,
                    ],
                    to: [
                        m.to.position[0],
                        m.to.position[1],
                        m.to.position[2],
                        m.to.heading,
                    ],
                    seconds: m.seconds,
                    elapsed: m.elapsed,
                })
                .collect(),
        }
    }
    /// The state a server sent; objects `world` does not have are ignored.
    pub fn from_snapshot(world: &World, snapshot: &Snapshot) -> Self {
        let mut visibility = HashMap::new();
        for (list, visible) in [(&snapshot.shown, true), (&snapshot.hidden, false)] {
            for node in list {
                if world.base_visible.contains_key(node) {
                    visibility.insert(*node, visible);
                }
            }
        }
        let pose = |v: [f32; 4]| Pose {
            position: [v[0], v[1], v[2]],
            heading: v[3],
        };
        let motions = snapshot
            .moved
            .iter()
            .filter(|m| {
                world.switchable.contains(&m.node)
                    && m.from.iter().chain(&m.to).all(|v| v.is_finite())
                    && m.seconds.is_finite()
                    && m.elapsed.is_finite()
            })
            .map(|m| {
                let seconds = m.seconds.clamp(0.0, Motion::MAX_SECONDS);
                (
                    m.node,
                    Motion {
                        from: pose(m.from),
                        to: pose(m.to),
                        seconds,
                        elapsed: m.elapsed.clamp(0.0, seconds),
                    },
                )
            })
            .collect();
        Self {
            visibility,
            motions,
            hour: snapshot.hour.map(|h| h.min(23)),
            weather: snapshot.weather.as_deref().and_then(Weather::parse),
            jump: snapshot.jump.map(|(n, h)| (n, h.min(23))),
            jumps: snapshot.jump.map_or(0, |(n, _)| n),
            revision: snapshot.revision,
        }
    }
}

impl World {
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
            switchable: switchable(layout),
            hour: layout.settings.hour,
            weather: layout.settings.weather.as_deref().and_then(Weather::parse),
        }
    }
    /// Whether the layout has this object (not a group).
    pub fn has_object(&self, node: NodeId) -> bool {
        self.poses.contains_key(&node)
    }
    /// Carry out what a world script asked for, for `player` (who gets
    /// messages and teleports). Shared changes go to `shared` as actions do.
    pub fn apply_script(
        &self,
        shared: &mut Shared,
        player: &mut PlayerState,
        change: ScriptChange,
    ) -> Vec<Effect> {
        let action = match change {
            ScriptChange::Visible(node, true) => Action::Show { node: Some(node) },
            ScriptChange::Visible(node, false) => Action::Hide { node: Some(node) },
            ScriptChange::Message(text, seconds) => Action::Message { text, seconds },
            ScriptChange::TeleportTo(node) => Action::Teleport { target: Some(node) },
            ScriptChange::TeleportAt(pose) => {
                player.settle = true;
                return vec![Effect::Teleport(pose)];
            }
            ScriptChange::Hour(hour) => Action::SetTime { hour },
            ScriptChange::Weather(weather) => Action::SetWeather { weather },
            ScriptChange::Move(node, pose, seconds) => {
                // Shared: everyone sees it with the next snapshot.
                shared.place(self, node, pose, seconds);
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        self.run(shared, player, 0, &[Kept::Known(action)], &mut out);
        out
    }
    /// A fresh shared state: the world's own hour and weather, nothing
    /// shown or hidden yet.
    pub fn shared(&self) -> Shared {
        Shared {
            hour: self.hour,
            weather: self.weather,
            ..Shared::default()
        }
    }
    /// The first spawn point object, if the world has one.
    pub fn spawn(&self) -> Option<Pose> {
        self.spawn
    }
    pub fn has_gameplay(&self) -> bool {
        !self.zones.is_empty() || self.spawn.is_some()
    }
    /// Shown right now, after actions.
    pub fn visible(&self, shared: &Shared, node: NodeId) -> bool {
        shared
            .visibility
            .get(&node)
            .copied()
            .unwrap_or_else(|| self.base_visible.get(&node).copied().unwrap_or(true))
    }
    /// Objects an action hid: they have no collision until shown again.
    pub fn hidden_by_actions<'a>(&self, shared: &'a Shared) -> impl Iterator<Item = NodeId> + 'a {
        shared
            .visibility
            .iter()
            .filter(|(_, visible)| !**visible)
            .map(|(id, _)| *id)
    }
    /// The interaction the player stands in, and its prompt.
    pub fn prompt(&self, feet: [f32; 3]) -> Option<(NodeId, String)> {
        self.prompt_for(feet, false)
    }
    /// [`Self::prompt`] for a player on foot or (`driving`) in a vehicle,
    /// who only gets interactions that work from one.
    pub fn prompt_for(&self, feet: [f32; 3], driving: bool) -> Option<(NodeId, String)> {
        let (node, interact) = self.interaction_at(feet, 0.0, driving)?;
        Some((
            node,
            if interact.prompt.trim().is_empty() {
                "Use".into()
            } else {
                interact.prompt.clone()
            },
        ))
    }
    /// The nearest interaction whose zone, grown by `slack` metres, holds
    /// `feet` (one that works from a vehicle, when `driving`).
    fn interaction_at(
        &self,
        feet: [f32; 3],
        slack: f32,
        driving: bool,
    ) -> Option<(NodeId, &Interact)> {
        let p = Vec3::from_array(feet);
        self.zones
            .iter()
            .filter_map(|z| match &z.kind {
                ZoneKind::Interact(i)
                    if (!driving || i.vehicles) && z.shape.contains_within(p, slack) =>
                {
                    Some((z.node, i, z.shape.base.distance_squared(p)))
                }
                _ => None,
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(node, interact, _)| (node, interact))
    }

    /// The player pressed the interact key with their feet at `feet`.
    /// None: no interaction here (the key may do something else).
    pub fn interact(
        &self,
        shared: &mut Shared,
        player: &mut PlayerState,
        feet: [f32; 3],
    ) -> Option<Vec<Effect>> {
        self.interact_for(shared, player, feet, false)
    }
    /// [`Self::interact`] for a player on foot or (`driving`) in a vehicle.
    pub fn interact_for(
        &self,
        shared: &mut Shared,
        player: &mut PlayerState,
        feet: [f32; 3],
        driving: bool,
    ) -> Option<Vec<Effect>> {
        let (node, _) = self.interaction_at(feet, 0.0, driving)?;
        self.use_node(shared, player, node, feet, 0.0)
    }
    /// Interaction `node` works only for players on foot.
    pub fn on_foot_only(&self, node: NodeId) -> bool {
        self.zones
            .iter()
            .any(|z| z.node == node && matches!(&z.kind, ZoneKind::Interact(i) if !i.vehicles))
    }

    /// A player asks to use interaction `node` (a server checking a
    /// client's request): None unless that zone, grown by `slack` metres
    /// for network delay, holds the player's `feet`.
    pub fn use_node(
        &self,
        shared: &mut Shared,
        player: &mut PlayerState,
        node: NodeId,
        feet: [f32; 3],
        slack: f32,
    ) -> Option<Vec<Effect>> {
        let zone = self.zones.iter().find(|z| z.node == node)?;
        let ZoneKind::Interact(interact) = &zone.kind else {
            return None;
        };
        if !zone
            .shape
            .contains_within(Vec3::from_array(feet), slack.max(0.0))
        {
            return None;
        }
        let mut out = Vec::new();
        player.events.push(Happened::Used(node));
        self.run(shared, player, node, &interact.actions, &mut out);
        Some(out)
    }

    /// Advance one player by `dt` seconds with their feet at `feet`
    /// (Blender axes).
    pub fn step(
        &self,
        shared: &mut Shared,
        player: &mut PlayerState,
        feet: [f32; 3],
        dt: f32,
    ) -> Vec<Effect> {
        player.clock += dt.max(0.0);
        let p = Vec3::from_array(feet);
        let now: HashSet<NodeId> = self
            .zones
            .iter()
            .filter(|z| z.shape.contains(p))
            .map(|z| z.node)
            .collect();
        let mut out = Vec::new();
        if std::mem::take(&mut player.settle) {
            // Arrival zones are taken quietly, but the zones the player was
            // moved out of still see them leave.
            let left: Vec<&Zone> = self
                .zones
                .iter()
                .filter(|z| player.inside.contains(&z.node) && !now.contains(&z.node))
                .collect();
            player.inside = now;
            for zone in left {
                player.events.push(Happened::Left(zone.node));
                if let ZoneKind::Trigger(trigger) = &zone.kind {
                    self.leave(shared, player, zone.node, trigger, &mut out);
                }
            }
            return out;
        }
        for zone in &self.zones {
            let node = zone.node;
            let entered = now.contains(&node) && !player.inside.contains(&node);
            let left = !now.contains(&node) && player.inside.contains(&node);
            if entered {
                player.events.push(Happened::Entered(node));
            }
            if left {
                player.events.push(Happened::Left(node));
            }
            if !entered && !left {
                continue;
            }
            match &zone.kind {
                ZoneKind::Trigger(trigger) => {
                    // `once`: the enter actions and the leave actions each
                    // run one time per player and play session.
                    if entered && !(trigger.once && player.fired.contains(&node)) {
                        player.fired.insert(node);
                        self.run(shared, player, node, &trigger.on_enter, &mut out);
                    }
                    if left {
                        self.leave(shared, player, node, trigger, &mut out);
                    }
                }
                ZoneKind::Checkpoint { race, order } if entered => {
                    self.checkpoint(player, race, *order, &mut out);
                }
                _ => {}
            }
        }
        // A teleport during this step sets `settle` again: the next step
        // takes the arrival zones quietly.
        player.inside = now;
        out
    }

    fn leave(
        &self,
        shared: &mut Shared,
        player: &mut PlayerState,
        node: NodeId,
        trigger: &Trigger,
        out: &mut Vec<Effect>,
    ) {
        if !(trigger.once && player.left.contains(&node)) {
            player.left.insert(node);
            self.run(shared, player, node, &trigger.on_leave, out);
        }
    }

    fn checkpoint(&self, player: &mut PlayerState, race: &str, order: u32, out: &mut Vec<Effect>) {
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
        let reached = |player: &mut PlayerState, index| {
            player.events.push(Happened::Checkpoint {
                race: race.to_owned(),
                index,
                total,
            })
        };
        if index == 0 {
            reached(player, 0);
            if total == 1 {
                out.push(message(format!("{name}Checkpoint reached")));
                return;
            }
            player.progress.insert(
                race.to_owned(),
                Progress {
                    next: 1,
                    started: player.clock,
                },
            );
            out.push(message(format!("{name}Go! Checkpoint 1/{total}")));
            return;
        }
        let Some(progress) = player.progress.get(race).copied() else {
            return;
        };
        if index != progress.next {
            return;
        }
        reached(player, index);
        if index + 1 == total {
            player.progress.remove(race);
            let seconds = player.clock - progress.started;
            player.events.push(Happened::Finished {
                race: race.to_owned(),
                seconds,
            });
            out.push(Effect::Message {
                text: format!("{name}Finished in {seconds:.1} s"),
                seconds: 6.0,
            });
        } else {
            player.progress.insert(
                race.to_owned(),
                Progress {
                    next: index + 1,
                    ..progress
                },
            );
            out.push(message(format!("{name}Checkpoint {}/{total}", index + 1)));
        }
    }

    fn run(
        &self,
        shared: &mut Shared,
        player: &mut PlayerState,
        owner: NodeId,
        actions: &[Kept<Action>],
        out: &mut Vec<Effect>,
    ) {
        for action in actions.iter().filter_map(Kept::known) {
            match action {
                Action::Message { text, seconds } => out.push(Effect::Message {
                    text: text.clone(),
                    seconds: seconds.clamp(0.5, 60.0),
                }),
                Action::Teleport { target } => {
                    if let Some(pose) = target.and_then(|t| shared.pose(self, t)) {
                        out.push(Effect::Teleport(pose));
                        player.settle = true;
                    }
                }
                Action::Show { node } | Action::Hide { node } | Action::Toggle { node } => {
                    let node = node.unwrap_or(owner);
                    if !self.base_visible.contains_key(&node) {
                        continue;
                    }
                    let visible = match action {
                        Action::Show { .. } => true,
                        Action::Hide { .. } => false,
                        _ => !self.visible(shared, node),
                    };
                    shared.set_visible(self, node, visible);
                    out.push(Effect::Visibility { node, visible });
                }
                Action::SetTime { hour } => {
                    // A fixed clock moves to the new hour; a running clock
                    // jumps there and keeps running.
                    let hour = (*hour).min(23);
                    if shared.hour.is_some() {
                        shared.set_hour(Some(hour));
                    } else {
                        shared.jump_to(hour);
                    }
                    out.push(Effect::Time(hour));
                }
                Action::SetWeather { weather } => {
                    if let Some(weather) = Weather::parse(weather) {
                        shared.set_weather(Some(weather));
                        out.push(Effect::Weather(weather));
                    }
                }
            }
        }
    }
}

/// A layout's gameplay for one player alone: the [`World`] with its own
/// [`Shared`] state and one [`PlayerState`] (Build Mode's playtest, or a
/// pack played without a server).
#[derive(Clone, Debug)]
pub struct Logic {
    world: World,
    shared: Shared,
    player: PlayerState,
}

impl Logic {
    pub fn new(layout: &Layout) -> Self {
        let world = World::new(layout);
        Self {
            shared: world.shared(),
            player: PlayerState::new(),
            world,
        }
    }
    pub fn world(&self) -> &World {
        &self.world
    }
    pub fn shared(&self) -> &Shared {
        &self.shared
    }
    /// Take the shared state a server decided.
    pub fn set_shared(&mut self, shared: Shared) {
        self.shared = shared;
    }
    /// Time passes for objects scripts are moving.
    pub fn advance(&mut self, dt: f32) {
        self.shared.advance(dt);
    }
    /// [`World::apply_script`] for the one player.
    pub fn apply_script(&mut self, change: ScriptChange) -> Vec<Effect> {
        self.world
            .apply_script(&mut self.shared, &mut self.player, change)
    }
    pub fn player_mut(&mut self) -> &mut PlayerState {
        &mut self.player
    }
    /// The first spawn point object, if the world has one.
    pub fn spawn(&self) -> Option<Pose> {
        self.world.spawn()
    }
    pub fn has_gameplay(&self) -> bool {
        self.world.has_gameplay()
    }
    /// Fixed hour of day right now (None: the clock runs).
    pub fn hour(&self) -> Option<u8> {
        self.shared.hour
    }
    pub fn weather(&self) -> Option<Weather> {
        self.shared.weather
    }
    /// Shown right now, after actions.
    pub fn visible(&self, node: NodeId) -> bool {
        self.world.visible(&self.shared, node)
    }
    /// Objects an action hid: they have no collision until shown again.
    pub fn hidden_by_actions(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.world.hidden_by_actions(&self.shared)
    }
    /// The prompt of the interaction the player stands in.
    pub fn prompt(&self, feet: [f32; 3]) -> Option<String> {
        self.prompt_for(feet, false)
    }
    /// The prompt for a player on foot or (`driving`) in a vehicle.
    pub fn prompt_for(&self, feet: [f32; 3], driving: bool) -> Option<String> {
        self.world
            .prompt_for(feet, driving)
            .map(|(_, prompt)| prompt)
    }
    /// The interaction the player stands in (on foot, or one that works
    /// from a vehicle when `driving`).
    pub fn interaction(&self, feet: [f32; 3], driving: bool) -> Option<NodeId> {
        self.world.prompt_for(feet, driving).map(|(node, _)| node)
    }
    /// The player pressed the interact key with their feet at `feet`.
    /// None: no interaction here (the key may do something else).
    pub fn interact(&mut self, feet: [f32; 3]) -> Option<Vec<Effect>> {
        self.interact_for(feet, false)
    }
    /// [`Self::interact`] for a player on foot or (`driving`) in a vehicle.
    pub fn interact_for(&mut self, feet: [f32; 3], driving: bool) -> Option<Vec<Effect>> {
        self.world
            .interact_for(&mut self.shared, &mut self.player, feet, driving)
    }
    /// Advance by `dt` seconds with the player's feet at `feet` (Blender axes).
    pub fn step(&mut self, feet: [f32; 3], dt: f32) -> Vec<Effect> {
        self.world
            .step(&mut self.shared, &mut self.player, feet, dt)
    }
}

/// How gameplay travels between a server and its players, over ordinary
/// script events (no protocol change).
pub mod wire {
    use super::{Effect, NodeId, Pose};
    use serde::{Deserialize, Serialize};

    /// Server to players: the shared state of their world instance, as a
    /// [`super::Snapshot`]. Sent when it changes and when a player arrives.
    pub const STATE_EVENT: &str = "__sare:worldState";
    /// Server to one player: what only that player sees or feels.
    pub const EFFECTS_EVENT: &str = "__sare:worldEffects";
    /// Player to server: use the interaction with this node id.
    pub const USE_EVENT: &str = "__sare:worldUse";

    /// A personal effect on the wire.
    #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
    #[serde(tag = "do", rename_all = "snake_case")]
    pub enum Personal {
        Message { text: String, seconds: f32 },
        Teleport { position: [f32; 3], heading: f32 },
    }
    impl Personal {
        /// The personal part of an effect; shared ones travel in snapshots.
        pub fn of(effect: &Effect) -> Option<Self> {
            match effect {
                Effect::Message { text, seconds } => Some(Self::Message {
                    text: text.clone(),
                    seconds: *seconds,
                }),
                Effect::Teleport(pose) => Some(Self::Teleport {
                    position: pose.position,
                    heading: pose.heading,
                }),
                _ => None,
            }
        }
        /// Back to an effect, if the values are sane.
        pub fn effect(&self) -> Option<Effect> {
            match self {
                Self::Message { text, seconds } if seconds.is_finite() => Some(Effect::Message {
                    text: text.chars().take(512).collect(),
                    seconds: seconds.clamp(0.5, 60.0),
                }),
                Self::Teleport { position, heading }
                    if position
                        .iter()
                        .all(|v| v.is_finite() && v.abs() <= 100_000.0)
                        && heading.is_finite() =>
                {
                    Some(Effect::Teleport(Pose {
                        position: *position,
                        heading: *heading,
                    }))
                }
                _ => None,
            }
        }
    }
    /// The payload of a `USE_EVENT`.
    pub fn parse_use(payload: &str) -> Option<NodeId> {
        serde_json::from_str::<(NodeId,)>(payload)
            .ok()
            .map(|(n,)| n)
    }
}

/// The shared state of one world instance on the wire.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    #[serde(rename = "r")]
    pub revision: u64,
    /// Objects actions showed.
    #[serde(rename = "s", default)]
    pub shown: Vec<NodeId>,
    /// Objects actions hid.
    #[serde(rename = "x", default)]
    pub hidden: Vec<NodeId>,
    #[serde(rename = "h", default, skip_serializing_if = "Option::is_none")]
    pub hour: Option<u8>,
    #[serde(rename = "w", default, skip_serializing_if = "Option::is_none")]
    pub weather: Option<String>,
    /// The latest jump of a running clock: (number, hour).
    #[serde(rename = "j", default, skip_serializing_if = "Option::is_none")]
    pub jump: Option<(u32, u8)>,
    /// Objects scripts moved.
    #[serde(rename = "m", default, skip_serializing_if = "Vec::is_empty")]
    pub moved: Vec<SnapshotMotion>,
}

/// A [`Motion`] in a [`Snapshot`]: poses as x, y, z, heading.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotMotion {
    #[serde(rename = "n")]
    pub node: NodeId,
    #[serde(rename = "f")]
    pub from: [f32; 4],
    #[serde(rename = "t")]
    pub to: [f32; 4],
    #[serde(rename = "s")]
    pub seconds: f32,
    #[serde(rename = "e")]
    pub elapsed: f32,
}

/// Objects an action or a script can show or hide. Played worlds keep them
/// out of the merged scene and draw them, and switch their collision, one
/// by one. Scripts get the objects they are on, the objects their text names
/// in quotes (`world.find("Lamp")`), and the ones ticked *Scripts can change
/// it*.
pub fn switchable(layout: &Layout) -> HashSet<NodeId> {
    let objects: HashSet<NodeId> = layout
        .nodes
        .iter()
        .filter(|n| !n.is_group())
        .map(|n| n.id)
        .collect();
    let named = quoted_names(layout);
    let mut out: HashSet<NodeId> = layout
        .nodes
        .iter()
        .filter(|n| {
            objects.contains(&n.id)
                && (n.scriptable
                    || n.scripts.iter().any(|s| s.enabled)
                    || named.contains(n.name.as_str()))
        })
        .map(|n| n.id)
        .collect();
    for node in &layout.nodes {
        let Some(component) = node.gameplay.as_ref().and_then(Kept::known) else {
            continue;
        };
        for action in component.action_lists().into_iter().flatten() {
            if let Some(
                Action::Show { node: target }
                | Action::Hide { node: target }
                | Action::Toggle { node: target },
            ) = action.known()
            {
                let target = target.unwrap_or(node.id);
                if objects.contains(&target) {
                    out.insert(target);
                }
            }
        }
    }
    out
}

/// Text in quotes in the world's enabled scripts: the object names they may
/// look up.
fn quoted_names(layout: &Layout) -> HashSet<&str> {
    let mut names = HashSet::new();
    for (_, script) in layout.all_scripts() {
        if !script.enabled {
            continue;
        }
        let text = script.source.as_str();
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            let quote = bytes[i];
            if quote == b'"' || quote == b'\'' {
                let start = i + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end] != quote && bytes[end] != b'\n' {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                if end < bytes.len() && bytes[end] == quote {
                    if let Some(name) = text.get(start..end) {
                        names.insert(name);
                    }
                }
                i = end + 1;
            } else {
                i += 1;
            }
        }
    }
    names
}

/// A script of the world, ready to run: enabled scripts only, world scripts
/// first. Object scripts are named `Object/Script` so errors say where they
/// are.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptSource {
    pub name: String,
    pub source: String,
    pub object: Option<NodeId>,
}
pub fn script_sources(layout: &Layout) -> Vec<ScriptSource> {
    layout
        .all_scripts()
        .into_iter()
        .filter(|(_, s)| s.enabled)
        .map(|(object, s)| ScriptSource {
            name: match object.and_then(|id| layout.node(id)) {
                Some(node) => format!("{}/{}", node.name, s.name),
                None => s.name.clone(),
            },
            source: s.source.clone(),
            object,
        })
        .collect()
}

/// An object as world scripts see it.
#[derive(Clone, Debug, PartialEq)]
pub struct ScriptObject {
    pub id: NodeId,
    pub name: String,
    pub position: [f32; 3],
    pub heading: f32,
    /// Scripts may show or hide it ([`switchable`]).
    pub changeable: bool,
    pub visible: bool,
}
/// Every object of the layout (not groups), in layout order.
pub fn script_objects(layout: &Layout) -> Vec<ScriptObject> {
    let changeable = switchable(layout);
    let flags = layout.effective_all();
    layout
        .nodes
        .iter()
        .filter(|n| !n.is_group())
        .map(|n| ScriptObject {
            id: n.id,
            name: n.name.clone(),
            position: n.transform.position,
            heading: n.transform.rotation[2],
            changeable: changeable.contains(&n.id),
            visible: flags.get(&n.id).is_none_or(|f| f.visible),
        })
        .collect()
}

/// What a world script asked for, for [`World::apply_script`]. Which
/// players it is for is the host's business.
#[derive(Clone, Debug, PartialEq)]
pub enum ScriptChange {
    Visible(NodeId, bool),
    Message(String, f32),
    TeleportTo(NodeId),
    TeleportAt(Pose),
    Hour(u8),
    Weather(String),
    /// Move an object to a pose over some seconds (0: at once).
    Move(NodeId, Pose, f32),
}

impl Happened {
    /// The object this happened at, if any.
    pub fn node(&self) -> Option<NodeId> {
        match self {
            Self::Entered(n) | Self::Left(n) | Self::Used(n) => Some(*n),
            _ => None,
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
    // Objects a script may react to: its own, or named in one.
    let named = quoted_names(layout);
    let scripted =
        |node: &Node| node.scripts.iter().any(|s| s.enabled) || named.contains(node.name.as_str());
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
        // Without actions, only a script can react to it.
        if let Component::Interact(i) = component {
            if i.actions.is_empty() && !scripted(node) {
                say("the interaction has no actions and no script of its own or naming it".into());
            }
        }
        if let Component::Trigger(t) = component {
            if t.on_enter.is_empty() && t.on_leave.is_empty() && !scripted(node) {
                say("the trigger has no actions and no script of its own or naming it".into());
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
            vehicles: false,
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
            vehicles: false,
            extra: Map::new(),
        }),
        "door-teleport" => Component::Interact(Interact {
            size: [2.4, 3.0, 2.6],
            prompt: "Go through the door".into(),
            actions: vec![Action::Teleport { target: None }.into()],
            vehicles: false,
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
        // Until the world instance is reset.
        logic.player_mut().world_reset();
        logic.step(outside, 0.1);
        assert_eq!(messages(&logic.step([0.0; 3], 0.1)), vec!["in"]);
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
        // The door is on foot only: nothing for a driver, until it works
        // from vehicles too.
        assert_eq!(logic.prompt_for([0.0; 3], true), None);
        assert_eq!(logic.interact_for([0.0; 3], true), None);
        assert!(logic.world.on_foot_only(door));
        let mut drive_through = layout.clone();
        if let Some(Kept::Known(Component::Interact(i))) =
            &mut drive_through.node_mut(door).unwrap().gameplay
        {
            i.vehicles = true;
        }
        let mut drive = Logic::new(&drive_through);
        assert!(drive.prompt_for([0.0; 3], true).is_some());
        assert!(drive.interact_for([0.0; 3], true).is_some());
        assert!(!drive.visible(door));
        assert!(!drive.world.on_foot_only(door));

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
        // Players of a hosted world get the jump through the snapshot.
        let jump = logic.shared().snapshot().jump;
        assert_eq!(jump, Some((1, 20)));
        // A reset forgets the jump but not its number: the next one is
        // new to players who saw the first.
        let world = World::new(&layout);
        let mut shared = logic.shared().clone();
        let revision = shared.revision();
        shared.reset(&world);
        assert!(shared.revision() > revision);
        assert_eq!(shared.jump(), None);
        shared.jump_to(6);
        assert_eq!(shared.jump(), Some((2, 6)));
    }

    #[test]
    fn moved_objects_glide_share_and_reset() {
        use crate::world_layout::Script;
        let mut layout = Layout::default();
        let lift = object(&mut layout, "Lift", [10.0, 0.0, 0.0], None);
        let fixed = object(&mut layout, "Fixed", [20.0, 0.0, 0.0], None);
        layout
            .node_mut(lift)
            .unwrap()
            .scripts
            .push(Script::new("Up", ""));
        let mut logic = Logic::new(&layout);
        let up = Pose {
            position: [10.0, 0.0, 4.0],
            heading: 90.0,
        };
        logic.apply_script(ScriptChange::Move(lift, up, 2.0));
        logic.apply_script(ScriptChange::Move(fixed, up, 2.0));
        let world = logic.world().clone();
        let at = |logic: &Logic| logic.shared().pose(&world, lift).unwrap();
        assert_eq!(at(&logic).position, [10.0, 0.0, 0.0], "starts where it is");
        logic.advance(1.0);
        let half = at(&logic);
        assert!((half.position[2] - 2.0).abs() < 1e-4 && (half.heading - 45.0).abs() < 1e-3);
        // Only objects scripts may change move.
        assert_eq!(
            logic.shared().pose(&world, fixed).unwrap().position,
            [20.0, 0.0, 0.0]
        );
        // A game that joins halfway gets the move halfway.
        let copy = Shared::from_snapshot(&world, &logic.shared().snapshot());
        assert_eq!(copy.pose(&world, lift), Some(half));
        logic.advance(5.0);
        assert_eq!(at(&logic), up);
        assert!(!logic.shared().moving());
        // Turning goes the short way round.
        logic.apply_script(ScriptChange::Move(
            lift,
            Pose {
                heading: 720.0,
                ..up
            },
            1.0,
        ));
        logic.advance(0.5);
        assert!((at(&logic).heading - 45.0).abs() < 1e-3, "{:?}", at(&logic));
        let mut shared = logic.shared().clone();
        shared.reset(&world);
        assert_eq!(
            shared.pose(&world, lift).unwrap().position,
            [10.0, 0.0, 0.0]
        );
    }

    #[test]
    fn scripts_switch_their_objects_and_the_names_they_quote() {
        use crate::world_layout::Script;
        let mut layout = Layout::default();
        let door = object(&mut layout, "Door", [0.0; 3], None);
        let lamp = object(&mut layout, "Lamp", [5.0, 0.0, 0.0], None);
        let sign = object(&mut layout, "Sign", [9.0, 0.0, 0.0], None);
        object(&mut layout, "Crate", [12.0, 0.0, 0.0], None);
        layout
            .node_mut(door)
            .unwrap()
            .scripts
            .push(Script::new("Timer", "print('hi')"));
        layout.settings.scripts.push(Script::new(
            "Lights",
            "world.find(\"Lamp\"):hide() -- \"Crate
is not closed",
        ));
        layout.node_mut(sign).unwrap().scriptable = true;
        assert_eq!(switchable(&layout), HashSet::from([door, lamp, sign]));
        let sources = script_sources(&layout);
        assert_eq!(
            sources
                .iter()
                .map(|s| (s.name.as_str(), s.object))
                .collect::<Vec<_>>(),
            [("Lights", None), ("Door/Timer", Some(door))]
        );
        let seen = script_objects(&layout);
        assert_eq!(seen.len(), 4);
        assert!(seen[1].changeable && !seen[3].changeable && seen[1].visible);

        // A script hides the lamp and sends the player to the sign.
        let mut logic = Logic::new(&layout);
        let effects = logic.apply_script(ScriptChange::Visible(lamp, false));
        assert_eq!(
            effects,
            [Effect::Visibility {
                node: lamp,
                visible: false
            }]
        );
        assert!(!logic.visible(lamp));
        assert!(matches!(
            logic.apply_script(ScriptChange::TeleportTo(sign))[..],
            [Effect::Teleport(Pose {
                position: [9.0, 0.0, 0.0],
                ..
            })]
        ));
        assert_eq!(
            logic.apply_script(ScriptChange::Hour(30)),
            [Effect::Time(23)]
        );

        // A disabled script neither runs nor names objects.
        layout.settings.scripts[0].enabled = false;
        assert!(!switchable(&layout).contains(&lamp));
        assert_eq!(script_sources(&layout).len(), 1);
    }

    #[test]
    fn switchable_objects_are_the_targets_of_show_and_hide() {
        let mut layout = Layout::default();
        let door = object(&mut layout, "Door", [0.0; 3], preset("door", 1));
        let lamp = object(&mut layout, "Lamp", [5.0, 0.0, 0.0], None);
        object(
            &mut layout,
            "Switch",
            [9.0, 0.0, 0.0],
            Some(trigger(vec![Action::Hide { node: Some(lamp) }])),
        );
        object(&mut layout, "Crate", [12.0, 0.0, 0.0], None);
        assert_eq!(switchable(&layout), HashSet::from([door, lamp]));
    }

    #[test]
    fn players_share_doors_time_and_weather_but_not_progress() {
        let mut layout = Layout::default();
        layout.settings.hour = Some(12);
        let door = object(&mut layout, "Door", [0.0; 3], preset("door", 1));
        let once = object(&mut layout, "Hello", [20.0, 0.0, 0.0], None);
        layout.node_mut(once).unwrap().gameplay = Some(Kept::Known(Component::Trigger(Trigger {
            size: [2.0, 2.0, 2.0],
            once: true,
            on_enter: vec![
                Action::Message {
                    text: "hi".into(),
                    seconds: 1.0,
                }
                .into(),
                Action::SetTime { hour: 21 }.into(),
            ],
            on_leave: Vec::new(),
            extra: Map::new(),
        })));
        let world = World::new(&layout);
        let mut shared = world.shared();
        let (mut anna, mut ben) = (PlayerState::new(), PlayerState::new());
        let away = [50.0, 50.0, 0.0];
        world.step(&mut shared, &mut anna, away, 0.1);
        world.step(&mut shared, &mut ben, away, 0.1);
        // Anna opens the door: it is open for Ben too.
        let before = shared.revision();
        world.interact(&mut shared, &mut anna, [0.0; 3]).unwrap();
        assert!(!world.visible(&shared, door));
        assert!(shared.revision() > before);
        assert_eq!(anna.take_events(), vec![Happened::Used(door)]);
        assert!(ben.take_events().is_empty());
        // "Once" is per player: both get the message, the time is shared.
        let hello = |state: &mut PlayerState, shared: &mut Shared| {
            world.step(shared, state, [20.0, 0.0, 0.0], 0.1)
        };
        assert_eq!(messages(&hello(&mut anna, &mut shared)), vec!["hi"]);
        assert_eq!(shared.hour(), Some(21));
        assert_eq!(messages(&hello(&mut ben, &mut shared)), vec!["hi"]);
        assert_eq!(
            ben.take_events(),
            vec![Happened::Entered(once)],
            "scripts see who entered"
        );
        world.step(&mut shared, &mut anna, away, 0.1);
        assert!(messages(&hello(&mut anna, &mut shared)).is_empty());
    }

    #[test]
    fn snapshots_carry_the_shared_state_to_late_joiners() {
        let mut layout = Layout::default();
        let door = object(&mut layout, "Door", [0.0; 3], preset("door", 1));
        let lamp = object(&mut layout, "Lamp", [3.0, 0.0, 0.0], None);
        layout.node_mut(lamp).unwrap().visible = false;
        let house = object(&mut layout, "House", [9.0, 0.0, 0.0], None);
        object(
            &mut layout,
            "Switch",
            [6.0, 0.0, 0.0],
            Some(trigger(vec![Action::Show { node: Some(lamp) }])),
        );
        let world = World::new(&layout);
        assert!(
            !world.shared().set_visible(&world, house, false),
            "only switchable objects can be hidden"
        );
        let mut shared = world.shared();
        assert!(shared.set_visible(&world, door, false));
        assert!(shared.set_visible(&world, lamp, true));
        assert!(!shared.set_visible(&world, 999, true), "no such object");
        shared.set_weather(Some(Weather::Fog));
        let text = serde_json::to_string(&shared.snapshot()).unwrap();
        assert!(crate::world_logic::tests::fits_one_event(&text));
        let snapshot: Snapshot = serde_json::from_str(&text).unwrap();
        let late = Shared::from_snapshot(&world, &snapshot);
        assert_eq!(late, shared);
        assert!(!world.visible(&late, door) && world.visible(&late, lamp));
        // Back to the layout's state: nothing is left to send.
        shared.set_visible(&world, door, true);
        shared.set_visible(&world, lamp, false);
        assert!(shared.snapshot().shown.is_empty() && shared.snapshot().hidden.is_empty());
        // Unknown objects in a snapshot are ignored.
        let odd = Snapshot {
            hidden: vec![door, 12345],
            ..Snapshot::default()
        };
        assert_eq!(
            Shared::from_snapshot(&world, &odd)
                .hidden_ids()
                .collect::<Vec<_>>(),
            vec![door]
        );
    }

    #[test]
    fn remote_use_is_checked_against_the_players_position() {
        let mut layout = Layout::default();
        let door = object(&mut layout, "Door", [0.0; 3], preset("door", 1));
        let crate_id = object(&mut layout, "Crate", [5.0, 0.0, 0.0], None);
        let world = World::new(&layout);
        let mut shared = world.shared();
        let mut player = PlayerState::new();
        // Far away: refused, nothing changes.
        assert!(world
            .use_node(&mut shared, &mut player, door, [10.0, 0.0, 0.0], 1.5)
            .is_none());
        // Not an interaction: refused.
        assert!(world
            .use_node(&mut shared, &mut player, crate_id, [5.0, 0.0, 0.0], 1.5)
            .is_none());
        assert_eq!(shared.revision(), 0);
        // Just outside the zone but within the network slack: allowed.
        assert!(world
            .use_node(&mut shared, &mut player, door, [2.2, 0.0, 0.0], 1.5)
            .is_some());
        assert!(!world.visible(&shared, door));
    }

    #[test]
    fn personal_effects_round_trip_and_bad_ones_are_dropped() {
        use wire::Personal;
        let effects = [
            Effect::Message {
                text: "hi".into(),
                seconds: 3.0,
            },
            Effect::Teleport(Pose {
                position: [1.0, 2.0, 3.0],
                heading: 90.0,
            }),
        ];
        let wire: Vec<Personal> = effects.iter().filter_map(Personal::of).collect();
        let text = serde_json::to_string(&(&wire,)).unwrap();
        let (back,): (Vec<Personal>,) = serde_json::from_str(&text).unwrap();
        let back: Vec<Effect> = back.iter().filter_map(Personal::effect).collect();
        assert_eq!(back, effects);
        assert_eq!(
            Personal::of(&Effect::Time(3)),
            None,
            "shared effects travel in snapshots"
        );
        let bad = Personal::Teleport {
            position: [f32::NAN, 0.0, 0.0],
            heading: 0.0,
        };
        assert_eq!(bad.effect(), None);
        assert_eq!(wire::parse_use("[7]"), Some(7));
        assert_eq!(wire::parse_use("[\"x\"]"), None);
    }

    /// Within sa-net's script event payload limit (4 KiB).
    pub fn fits_one_event(payload: &str) -> bool {
        payload.len() + 2 <= 4 * 1024
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
