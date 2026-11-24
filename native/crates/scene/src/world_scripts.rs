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
        let mut objects: Vec<host::ObjectInfo> = world_logic::script_objects(layout)
            .into_iter()
            .map(|o| host::ObjectInfo {
                id: o.id,
                name: o.name,
                position: o.position,
                heading: o.heading,
                changeable: o.changeable,
                paintable: o.paintable,
                visible: o.visible,
            })
            .collect();
        let changeable = objects
            .iter()
            .filter(|o| o.changeable)
            .map(|o| o.id)
            .collect();
        // Copies scripts made before they last stopped are theirs again.
        let poses: std::collections::HashMap<_, _> = shared.moved().collect();
        let mut copies = Vec::new();
        for (copy, source) in shared.copies() {
            let (Some(name), Some(pose)) = (
                objects
                    .iter()
                    .find(|o| o.id == *source)
                    .map(|o| o.name.clone()),
                poses.get(copy),
            ) else {
                continue;
            };
            objects.push(host::ObjectInfo {
                id: *copy,
                name,
                position: pose.position,
                heading: pose.heading,
                changeable: true,
                paintable: true,
                visible: true,
            });
            copies.push((*copy, *source));
        }
        let host = host::WorldScripts::new(host::Config {
            scripts,
            objects,
            weathers: Weather::ALL
                .iter()
                .map(|w| w.as_str().to_string())
                .collect(),
            hour: shared.hour(),
            weather: shared.weather().map(|w| w.as_str().to_string()),
            first_copy: shared.next_copy(),
            max_copies: world_logic::MAX_COPIES,
            copies,
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
                .chain(shared.copies().keys())
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
            colors: shared.colors().iter().map(|(k, v)| (*k, *v)).collect(),
            copies: shared.copies().iter().map(|(c, s)| (*c, *s)).collect(),
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
                host::Request::Hud {
                    player: None,
                    key,
                    text,
                } => (Who::World, ScriptChange::WorldHud(key, text)),
                host::Request::Hud {
                    player: Some(player),
                    key,
                    text,
                } => (Who::Player(player), ScriptChange::Hud(key, text)),
                host::Request::Color { object, color } => {
                    (Who::World, ScriptChange::Color(object, color))
                }
                host::Request::Copy {
                    source,
                    id,
                    position,
                    heading,
                } => (
                    Who::World,
                    ScriptChange::Copy {
                        source,
                        id,
                        pose: Pose { position, heading },
                    },
                ),
                host::Request::Destroy { object } => (Who::World, ScriptChange::Destroy(object)),
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
    fn copies_scripts_make_are_shared_and_go_with_a_reset() {
        let mut layout = Layout::default();
        let id = layout.allocate_id();
        let mut crate_ = Node::object(
            id,
            "Crate",
            AssetRef::Builtin("cube".into()),
            Transform::at([0.0; 3]),
        );
        crate_.scripts.push(Script::new(
            "Spawner",
            "local c = script.object:clone(vector(4, 0, 0)) c.color = rgb(0, 1, 0) wait(1) print(c.position, c.exists) c:destroy()",
        ));
        layout.nodes.push(crate_);
        let mut logic = Logic::new(&layout);
        let mut scripts = Scripts::start(&layout, logic.shared()).unwrap();
        let mut tick = |logic: &mut Logic, dt: f32| {
            let changes = scripts.tick(logic.world(), logic.shared(), dt, Some(12), vec![], vec![]);
            for (_, change) in changes {
                logic.apply_script(change);
            }
        };
        tick(&mut logic, 0.1);
        let copy = world_logic::COPY_BASE;
        assert_eq!(logic.shared().copies().get(&copy), Some(&id));
        assert_eq!(logic.shared().colors().get(&copy), Some(&[0.0, 1.0, 0.0]));
        assert!(logic.visible(copy));
        // Players get the copy with the world state.
        let snapshot = logic.shared().snapshot();
        let copied = Shared::from_snapshot(logic.world(), &snapshot);
        assert_eq!(copied.copies(), logic.shared().copies());
        assert_eq!(
            copied.moved().collect::<Vec<_>>(),
            logic.shared().moved().collect::<Vec<_>>()
        );
        tick(&mut logic, 1.0);
        assert!(logic.shared().copies().is_empty(), "destroyed");
        assert!(logic.shared().moved().next().is_none());
        assert_eq!(
            scripts.take_output()[0].text,
            "vector(4.00, 0.00, 0.00)  true"
        );
        // A new copy after a reset never gets an old id.
        let mut shared = logic.shared().clone();
        shared.reset(logic.world());
        assert!(shared.next_copy() > copy);
        assert!(!shared.copy(
            logic.world(),
            id,
            copy,
            Pose {
                position: [0.0; 3],
                heading: 0.0
            }
        ));
    }

    #[test]
    fn scripts_that_start_again_take_over_the_copies_made_before() {
        let mut layout = Layout::default();
        let id = layout.allocate_id();
        let mut crate_ = Node::object(
            id,
            "Crate",
            AssetRef::Builtin("cube".into()),
            Transform::at([0.0; 3]),
        );
        crate_.scripts.push(Script::new(
            "Spawner",
            "local made = 0
            for _, o in ipairs(world.objects()) do
              if o.original then
                print('found', o.name, o.position)
                o:destroy()
                made = made + 1
              end
            end
            if made == 0 then script.object:clone(vector(0, 3, 0)) end",
        ));
        layout.nodes.push(crate_);
        let mut logic = Logic::new(&layout);
        let run = |logic: &mut Logic| {
            let mut scripts = Scripts::start(&layout, logic.shared()).unwrap();
            let changes =
                scripts.tick(logic.world(), logic.shared(), 0.1, Some(12), vec![], vec![]);
            for (_, change) in changes {
                logic.apply_script(change);
            }
            scripts.take_output()
        };
        assert!(run(&mut logic).is_empty());
        assert_eq!(logic.shared().copies().len(), 1);
        // The scripts stop (a server with no players) and start again.
        let output = run(&mut logic);
        assert_eq!(output[0].text, "found  Crate  vector(0.00, 3.00, 0.00)");
        assert!(logic.shared().copies().is_empty());
    }

    #[test]
    fn worlds_without_scripts_start_none() {
        let layout = Layout::default();
        assert!(Scripts::start(&layout, &Shared::default()).is_none());
    }
}
