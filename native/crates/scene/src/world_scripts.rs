//! A layout's world scripts, run by `sa_lua::world` against the layout's
//! [`World`] and [`Shared`] state. Build Mode's playtest, worlds played from
//! the launcher and servers (one per world instance) all use this, so a
//! script behaves the same everywhere.
use crate::world_layout::{Layout, NodeId};
use crate::world_logic::{self, Happened, Pose, ScriptChange, Shared, Weather, World};
use sa_lua::world as host;
pub use sa_lua::world::{Line, PlayerInfo};

/// Who a script's change is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    /// The world everyone shares (an object shown, the hour, the weather).
    World,
    /// Every player in the world (a message to all).
    Everyone,
    Player(u32),
}

/// The running scripts of one world (instance).
pub struct Scripts {
    host: host::WorldScripts,
    /// Objects scripts may show or hide.
    changeable: Vec<NodeId>,
}

impl Scripts {
    /// Start the layout's enabled scripts against `shared` as it is now.
    /// None when the world has none.
    pub fn start(layout: &Layout, shared: &Shared) -> Option<Self> {
        let scripts: Vec<host::Source> = world_logic::script_sources(layout)
            .into_iter()
            .map(|s| host::Source {
                name: s.name,
                source: s.source,
                object: s.object,
            })
            .collect();
        if scripts.is_empty() {
            return None;
        }
        let objects: Vec<host::ObjectInfo> = world_logic::script_objects(layout)
            .into_iter()
            .map(|o| host::ObjectInfo {
                id: o.id,
                name: o.name,
                position: o.position,
                heading: o.heading,
                changeable: o.changeable,
                visible: o.visible,
            })
            .collect();
        let changeable = objects
            .iter()
            .filter(|o| o.changeable)
            .map(|o| o.id)
            .collect();
        let host = host::WorldScripts::new(host::Config {
            scripts,
            objects,
            weathers: Weather::ALL
                .iter()
                .map(|w| w.as_str().to_string())
                .collect(),
            hour: shared.hour(),
            weather: shared.weather().map(|w| w.as_str().to_string()),
        });
        Some(Self { host, changeable })
    }

    /// One frame: what happened to each player since the last one, and the
    /// players there now. `hour` is the hour of day right now, if known (a
    /// server does not run a running clock). Returns the
    /// changes scripts asked for, for [`World::apply_script`].
    pub fn tick(
        &mut self,
        world: &World,
        shared: &Shared,
        dt: f32,
        hour: Option<u8>,
        players: Vec<PlayerInfo>,
        events: Vec<host::Event>,
    ) -> Vec<(Who, ScriptChange)> {
        let frame = host::Frame {
            dt,
            players,
            hour: hour.map(|h| h.min(23)),
            weather: shared.weather().map(|w| w.as_str().to_string()),
            visible: self
                .changeable
                .iter()
                .map(|id| (*id, world.visible(shared, *id)))
                .collect(),
            poses: shared
                .moved()
                .map(|(id, pose)| (id, pose.position, pose.heading))
                .collect(),
            targets: shared
                .motions()
                .map(|(id, m)| (id, m.to.position, m.to.heading))
                .collect(),
        };
        self.host
            .tick(frame, events)
            .into_iter()
            .map(|request| match request {
                host::Request::Visible { object, visible } => {
                    (Who::World, ScriptChange::Visible(object, visible))
                }
                host::Request::Message {
                    player,
                    text,
                    seconds,
                } => (
                    player.map_or(Who::Everyone, Who::Player),
                    ScriptChange::Message(text, seconds),
                ),
                host::Request::Teleport { player, to } => (
                    Who::Player(player),
                    match to {
                        host::Target::Object(node) => ScriptChange::TeleportTo(node),
                        host::Target::Point(position, heading) => {
                            ScriptChange::TeleportAt(Pose { position, heading })
                        }
                    },
                ),
                host::Request::Move {
                    object,
                    position,
                    heading,
                    seconds,
                } => (
                    Who::World,
                    ScriptChange::Move(object, Pose { position, heading }, seconds),
                ),
                host::Request::Hour(hour) => (Who::World, ScriptChange::Hour(hour)),
                host::Request::Weather(weather) => (Who::World, ScriptChange::Weather(weather)),
            })
            .collect()
    }

    /// What scripts printed, and their errors, since the last call.
    pub fn take_output(&mut self) -> Vec<Line> {
        self.host.take_output()
    }
}

/// A player's gameplay moment as a script event.
pub fn event(player: u32, happened: Happened) -> host::Event {
    match happened {
        Happened::Entered(object) => host::Event::Enter { player, object },
        Happened::Left(object) => host::Event::Leave { player, object },
        Happened::Used(object) => host::Event::Use { player, object },
        Happened::Checkpoint { race, index, total } => host::Event::Checkpoint {
            player,
            race,
            index: index + 1,
            total,
        },
        Happened::Finished { race, seconds } => host::Event::Finish {
            player,
            race,
            seconds,
        },
    }
}
pub fn joined(player: u32) -> host::Event {
    host::Event::Join { player }
}
pub fn left(player: u32) -> host::Event {
    host::Event::Quit { player }
}

/// A script line as the Output panel or a server console shows it.
pub fn describe(line: &Line) -> String {
    match (line.script.is_empty(), line.error) {
        (true, _) => format!("Scripts: {}", line.text),
        (false, true) => format!("Script error in {}: {}", line.script, line.text),
        (false, false) => format!("[{}] {}", line.script, line.text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world_layout::{AssetRef, Node, Script, Transform};
    use crate::world_logic::{Component, Effect, Interact, Kept, Logic};

    #[test]
    fn a_script_opens_a_door_used_with_e_and_closes_it_again() {
        let mut layout = Layout::default();
        let id = layout.allocate_id();
        let mut door = Node::object(
            id,
            "Door",
            AssetRef::Builtin("cube".into()),
            Transform::at([0.0; 3]),
        );
        door.gameplay = Some(Kept::Known(Component::Interact(Interact {
            size: [4.0, 4.0, 4.0],
            prompt: "Knock".into(),
            actions: Vec::new(),
            vehicles: false,
            extra: Default::default(),
        })));
        door.scripts.push(Script::new(
            "Opener",
            "script.object:on('use', function(p) script.object:hide() p:message('Come in') wait(1) script.object:show() end)",
        ));
        layout.nodes.push(door);
        let mut logic = Logic::new(&layout);
        let mut scripts = Scripts::start(&layout, logic.shared()).unwrap();
        let alice = || {
            vec![PlayerInfo {
                id: 1,
                name: "Alice".into(),
                position: Some([0.0; 3]),
                driving: false,
            }]
        };
        // E at the door: the use goes to the script.
        let used = logic.interact([0.0; 3]).unwrap();
        assert!(used.is_empty(), "the door has no actions of its own");
        let events = logic
            .player_mut()
            .take_events()
            .into_iter()
            .map(|h| event(1, h))
            .collect();
        let changes = scripts.tick(
            logic.world(),
            logic.shared(),
            0.1,
            Some(12),
            alice(),
            events,
        );
        assert_eq!(
            changes,
            [
                (Who::World, ScriptChange::Visible(id, false)),
                (Who::Player(1), ScriptChange::Message("Come in".into(), 4.0)),
            ]
        );
        for (_, change) in changes {
            logic.apply_script(change);
        }
        assert!(!logic.visible(id), "the door opened");
        let changes = scripts.tick(
            logic.world(),
            logic.shared(),
            1.0,
            Some(12),
            alice(),
            vec![],
        );
        assert_eq!(changes, [(Who::World, ScriptChange::Visible(id, true))]);
        assert_eq!(
            logic.apply_script(changes[0].1.clone()),
            [Effect::Visibility {
                node: id,
                visible: true
            }]
        );
        assert!(scripts.take_output().is_empty());
    }

    #[test]
    fn worlds_without_scripts_start_none() {
        let layout = Layout::default();
        assert!(Scripts::start(&layout, &Shared::default()).is_none());
    }
}
