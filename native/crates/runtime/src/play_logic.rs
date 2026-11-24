//! Runs a world's Build Mode gameplay (`sa_scene::world_logic`) while it is
//! played: in Build Mode's playtest and when a content pack with a layout is
//! played from the launcher.
//!
//! The logic itself is plain data in `sa-scene`; this module feeds it the
//! player's feet each frame, applies what it asks for (messages, teleports,
//! showing and hiding objects, time and weather) and draws the message and
//! the "E" prompt.
use crate::build_mode::gpu::{render_matrix, Gpu};
use crate::ui_theme;
use crate::{GpuBatch, State};
use glam::Vec3;
use sa_scene::collision::Player;
use sa_scene::content_pack::AssetResolver;
use sa_scene::world_layout::{blender_to_gltf, gltf_to_blender, Layout, NodeId, Transform};
use sa_scene::world_logic::{self, Effect, Logic, Pose, Weather};
use std::time::{Duration, Instant};

pub struct PlayLogic {
    pub logic: Logic,
    /// The layout this gameplay came from.
    layout: Layout,
    /// A pack played from the launcher: the objects gameplay shows and hides
    /// (None in Build Mode's playtest, which draws every object itself).
    objects: Option<PackObjects>,
    message: Option<(String, Instant)>,
    prompt: Option<String>,
    /// Real time of the last step: frame `dt` is capped for physics, so the
    /// checkpoint clock uses the wall clock instead.
    last_step: Option<Instant>,
    /// A server runs this world's gameplay (it sent the world state): zones
    /// and actions are its decisions, and E asks it to use an object.
    remote: bool,
    /// The last clock jump taken from the server.
    last_jump: Option<u32>,
    /// `--smoke-build` over the network: the frame the server took over.
    smoke_remote_since: Option<usize>,
    /// ...and the frame the gate opened (a screenshot is taken after it).
    smoke_opened: Option<usize>,
    /// The world's scripts, when it has some and plays alone (a server runs
    /// them for worlds played on it).
    scripts: Option<sa_scene::world_scripts::Scripts>,
    /// The scripts were told the player joined.
    joined: bool,
}

/// The pack's merged scene leaves out objects gameplay can show or hide
/// (`world_logic::switchable`); they are drawn one by one here, and their
/// collision is added to the world as switchable.
struct PackObjects {
    gpu: Gpu,
    resolver: AssetResolver,
    /// Only the switchable objects.
    layout: Layout,
    /// Their solid meshes in render space.
    solid: Vec<(NodeId, Vec<sa_scene::Batch>)>,
}

impl PlayLogic {
    /// Build Mode's playtest of `layout`.
    pub fn playtest(layout: &Layout) -> Self {
        Self::with(layout, None)
    }
    /// A content pack played from the launcher, from its `folder`.
    pub fn pack(layout: &Layout, folder: &std::path::Path) -> Self {
        let switchable = world_logic::switchable(layout);
        let flags = layout.effective_all();
        let mut resolver = AssetResolver::new(folder);
        let mut objects = Layout {
            nodes: Vec::new(),
            ..layout.clone()
        };
        let mut solid = Vec::new();
        for node in layout.nodes.iter().filter(|n| switchable.contains(&n.id)) {
            if node.asset.is_none() {
                continue;
            }
            if flags.get(&node.id).is_some_and(|f| f.collision) {
                match sa_scene::content_pack::node_model(node, &mut resolver) {
                    Ok(model) => {
                        let (_, batches) = model.transformed(render_matrix(&node.transform));
                        solid.push((node.id, batches));
                    }
                    Err(error) => crate::runtime_log::line(format!(
                        "World gameplay: no collision for {}: {error:#}",
                        node.name
                    )),
                }
            }
            let mut copy = node.clone();
            copy.parent = None;
            objects.nodes.push(copy);
        }
        Self::with(
            layout,
            Some(PackObjects {
                gpu: Gpu::default(),
                resolver,
                layout: objects,
                solid,
            }),
        )
    }
    fn with(layout: &Layout, objects: Option<PackObjects>) -> Self {
        let logic = Logic::new(layout);
        let scripts = sa_scene::world_scripts::Scripts::start(layout, logic.shared());
        Self {
            logic,
            scripts,
            joined: false,
            layout: layout.clone(),
            objects,
            message: None,
            prompt: None,
            last_step: None,
            remote: false,
            last_jump: None,
            smoke_remote_since: None,
            smoke_opened: None,
        }
    }
    /// The glow of the lights of the pack's switchable objects.
    pub fn glows(&self) -> impl Iterator<Item = &GpuBatch> + '_ {
        self.objects
            .iter()
            .flat_map(|o| o.gpu.lights.iter())
            .filter(|(id, _)| self.logic.visible(**id))
            .flat_map(|(_, light)| &light.object.batches)
    }
    /// The world settings of the layout being played.
    pub fn settings(&self) -> &sa_scene::world_layout::WorldSettings {
        &self.layout.settings
    }
    /// The pack's switchable objects as they are drawn right now.
    pub fn batches(&self) -> impl Iterator<Item = &GpuBatch> + '_ {
        self.objects
            .iter()
            .flat_map(|o| o.gpu.objects.iter())
            .filter(|(id, object)| self.logic.visible(**id) && !object.placeholder)
            .flat_map(|(_, object)| &object.batches)
    }
    /// The gameplay's own hour, overriding the clock.
    pub fn hour(&self) -> Option<u8> {
        self.logic.hour()
    }
    pub fn weather(&self) -> Option<Weather> {
        self.logic.weather()
    }
    /// The interaction prompt shown right now.
    pub fn prompt(&self) -> Option<&str> {
        self.prompt.as_deref()
    }
    /// The message shown right now.
    pub fn message(&self) -> Option<&str> {
        self.message.as_ref().map(|(text, _)| text.as_str())
    }
}

/// Objects the `--smoke-build` runs use: (id, position) of the first
/// interaction that toggles, the first that teleports, and the first
/// behaviour that sets the time. Made for `content/gameplay-sample`.
pub struct SmokeTargets {
    pub gate: (NodeId, [f32; 3]),
    pub door: (NodeId, [f32; 3]),
    pub night: (NodeId, [f32; 3]),
}
pub fn smoke_targets(layout: &Layout) -> SmokeTargets {
    use world_logic::{Action, Component, Kept};
    let find = |wanted: &dyn Fn(&Component) -> bool| {
        layout
            .nodes
            .iter()
            .find(|n| {
                n.gameplay
                    .as_ref()
                    .and_then(Kept::known)
                    .is_some_and(wanted)
            })
            .map(|n| (n.id, n.transform.position))
            .expect("the smoke world lacks a behaviour it needs (use content/gameplay-sample)")
    };
    let runs = |c: &Component, f: fn(&Action) -> bool| {
        c.action_lists()
            .into_iter()
            .flatten()
            .any(|a| a.known().is_some_and(f))
    };
    SmokeTargets {
        gate: find(&|c| {
            matches!(c, Component::Interact(_)) && runs(c, |a| matches!(a, Action::Toggle { .. }))
        }),
        door: find(&|c| {
            matches!(c, Component::Interact(_)) && runs(c, |a| matches!(a, Action::Teleport { .. }))
        }),
        night: find(&|c| runs(c, |a| matches!(a, Action::SetTime { .. }))),
    }
}

/// A Blender-axes point of the pack in render space.
pub fn blender_to_render(point: [f32; 3]) -> Vec3 {
    crate::build_mode::gpu::render_matrix(&Transform::default())
        .transform_point3(blender_to_gltf(point))
}
/// A render-space point in the pack's Blender axes.
pub fn render_to_blender(point: Vec3) -> [f32; 3] {
    gltf_to_blender(
        crate::build_mode::gpu::render_matrix(&Transform::default())
            .inverse()
            .transform_point3(point),
    )
}
/// Camera yaw facing a Blender heading (0 = north, degrees counter-clockwise).
/// The view looks along (sin yaw, 0, cos yaw), and north is render -Z.
pub fn yaw_for(heading: f32) -> f32 {
    std::f32::consts::PI + heading.to_radians()
}
/// Where to put the eye for a spawn at `pose`.
pub fn spawn_eye(pose: &Pose) -> Vec3 {
    blender_to_render(pose.position) + Vec3::Y * 2.0
}

/// Read the layout of a content pack folder, for playing it.
pub fn load_pack(folder: &std::path::Path) -> Option<Layout> {
    let manifest = sa_scene::content_pack::Manifest::read(folder).ok()?;
    let file = manifest.world.layout.as_ref()?;
    let path = sa_scene::content_pack::inside(folder, file).ok()?;
    match Layout::read(&path) {
        Ok(layout) => Some(layout),
        Err(error) => {
            crate::runtime_log::line(format!("World gameplay not loaded: {error:#}"));
            None
        }
    }
}

impl State {
    /// Once a frame while playing: move the logic on and apply its effects.
    pub(crate) fn play_logic_update(&mut self) {
        if self.build_mode_editing() {
            return;
        }
        self.sync_pack_objects();
        let paused = self.menu.page.is_some();
        let Some(play) = &mut self.play_logic else {
            return;
        };
        let now = Instant::now();
        let dt = match play.last_step.replace(now) {
            Some(last) if !paused => (now - last).as_secs_f32().min(1.0),
            _ => 0.0,
        };
        if let Some(hour) = play.hour() {
            self.daycycle.minutes = f32::from(hour.min(23)) * 60.0;
        }
        play.prompt = None;
        // Zones and checkpoints follow the player on foot or in a car;
        // interactions (E) too, when they work from vehicles.
        let at = if self.driving {
            // Where the car touches the ground, as the server measures it.
            self.car
                .as_ref()
                .map(|(car, _)| car.position - Vec3::Y * car.clearance)
        } else if self.walking {
            self.player.as_ref().map(|p| p.feet)
        } else {
            None
        };
        let Some(at) = at else { return };
        let feet = render_to_blender(at);
        play.prompt = play.logic.prompt_for(feet, self.driving);
        if play.remote {
            // The server steps the zones; its state and effects arrive as
            // events (`handle_world_event`).
            if self.network_session.is_none() {
                play.remote = false;
            }
            return;
        }
        let mut effects = play.logic.step(feet, dt);
        // What happened goes to the world's scripts, and what they ask for
        // is carried out like an action's effects.
        let happened = play.logic.player_mut().take_events();
        let mut lines = Vec::new();
        if let Some(scripts) = &mut play.scripts {
            use sa_scene::world_scripts::{self as ws, PlayerInfo};
            let mut events: Vec<_> = happened.into_iter().map(|h| ws::event(1, h)).collect();
            if !play.joined {
                play.joined = true;
                events.insert(0, ws::joined(1));
            }
            let name = match self.menu.player_name.trim() {
                "" => "Player".to_string(),
                name => name.to_string(),
            };
            let players = vec![PlayerInfo {
                id: 1,
                name,
                position: feet,
                driving: self.driving,
            }];
            let hour = (self.daycycle.minutes / 60.0).rem_euclid(24.0) as u8;
            let changes = scripts.tick(
                play.logic.world(),
                play.logic.shared(),
                dt,
                Some(hour),
                players,
                events,
            );
            for (_, change) in changes {
                effects.extend(play.logic.apply_script(change));
            }
            lines = scripts.take_output();
        }
        self.apply_play_effects(effects);
        for line in lines {
            let text = sa_scene::world_scripts::describe(&line);
            match &mut self.build {
                Some(build) if line.error => build.error(text),
                Some(build) => build.info(text),
                None => crate::runtime_log::line(text),
            }
        }
    }

    /// World gameplay events from a server: its shared state for this
    /// instance, and effects for this player. True when the event was one.
    pub(crate) fn handle_world_event(&mut self, name: &str, payload: &str) -> bool {
        use sa_scene::world_logic::{wire, Shared, Snapshot};
        match name {
            wire::STATE_EVENT => {
                let Ok((snapshot,)) = serde_json::from_str::<(Snapshot,)>(payload) else {
                    return true;
                };
                if let Some(play) = &mut self.play_logic {
                    if !play.remote {
                        crate::runtime_log::line("World gameplay: the server runs it");
                    }
                    play.remote = true;
                    // The server runs the world's scripts too.
                    play.scripts = None;
                    let shared = Shared::from_snapshot(play.logic.world(), &snapshot);
                    // A running clock that an action made jump.
                    if let Some((number, hour)) = shared.jump() {
                        if play.last_jump != Some(number) {
                            play.last_jump = Some(number);
                            self.daycycle.minutes = f32::from(hour) * 60.0;
                        }
                    }
                    play.logic.set_shared(shared);
                    if let Some(collision) = &mut self.collision {
                        collision.set_hidden(play.logic.hidden_by_actions());
                    }
                }
                true
            }
            wire::EFFECTS_EVENT => {
                let effects: Vec<Effect> = serde_json::from_str::<(Vec<wire::Personal>,)>(payload)
                    .map(|(list,)| list.iter().filter_map(wire::Personal::effect).collect())
                    .unwrap_or_default();
                if self.play_logic.as_ref().is_some_and(|p| p.remote) {
                    self.apply_play_effects(effects);
                }
                true
            }
            _ => false,
        }
    }

    /// A played pack's switchable objects: upload them once, and keep their
    /// collision in the world (a newly loaded world brings its own).
    fn sync_pack_objects(&mut self) {
        let Some(mut play) = self.play_logic.take() else {
            return;
        };
        if let Some(objects) = &mut play.objects {
            let mut problems = Vec::new();
            objects
                .gpu
                .sync(self, &mut objects.resolver, &objects.layout, &mut problems);
            objects
                .gpu
                .sync_lights(self, &objects.layout, &mut problems);
            for problem in problems {
                crate::runtime_log::line(format!(
                    "World gameplay: model failed to load: {problem}"
                ));
            }
            if let Some(collision) = &mut self.collision {
                let mut added = false;
                for (id, batches) in &objects.solid {
                    if !collision.has_switchable(*id) {
                        collision.add_switchable(*id, batches);
                        added = true;
                    }
                }
                if added {
                    collision.set_hidden(play.logic.hidden_by_actions());
                }
            }
        }
        self.play_logic = Some(play);
    }

    /// `--smoke-build` on a pack played without Build Mode: the gate opens
    /// (not drawn, not solid) and closes, and the front door teleports.
    /// One rendered frame; true when it has passed.
    pub(crate) fn play_logic_smoke(
        &mut self,
        frame: usize,
        captures: Option<&std::path::Path>,
    ) -> bool {
        let play = self
            .play_logic
            .as_ref()
            .expect("--smoke-build needs a content pack with gameplay");
        let SmokeTargets { gate, door, .. } = smoke_targets(&play.layout);
        let drawn = |state: &State| state.play_logic.as_ref().unwrap().batches().count();
        let capture = |state: &mut State, name: &str| {
            if let Some(dir) = captures {
                state.capture_next = Some(dir.join(name));
            }
        };
        // A menu shown on a first start would take the E key: keep it shut.
        self.menu.page = None;
        if self.network_session.is_some() {
            return self.play_logic_smoke_remote(frame, captures);
        }
        match frame {
            1 => self.capture(false),
            10 => self.smoke_stand_at(gate.1),
            14 => {
                assert!(drawn(self) > 0, "the gate is not drawn on its own");
                assert!(self.interact_play_logic(), "E did nothing at the gate");
                let play = self.play_logic.as_ref().unwrap();
                assert!(!play.logic.visible(gate.0), "the gate did not open");
                assert_eq!(drawn(self), 0, "the open gate is still drawn");
                assert!(
                    self.collision.as_ref().unwrap().has_switchable(gate.0),
                    "the gate was never solid"
                );
                assert!(
                    self.smoke_walks_through(gate.1),
                    "the open gate still blocks the way"
                );
            }
            20 => capture(self, "pack-gate-open.png"),
            24 => {
                assert!(self.interact_play_logic(), "E did nothing at the open gate");
                assert!(drawn(self) > 0, "the closed gate is not drawn");
                assert!(
                    !self.smoke_walks_through(gate.1),
                    "the closed gate lets the player through"
                );
                capture(self, "pack-gate-closed.png");
            }
            30 => self.smoke_stand_at(door.1),
            34 => {
                let before = self.player.as_ref().unwrap().feet;
                assert!(self.interact_play_logic(), "E did nothing at the door");
                let after = self.player.as_ref().unwrap().feet;
                assert!(
                    before.distance(after) > 20.0,
                    "the door did not teleport the player"
                );
            }
            40 => capture(self, "pack-inside.png"),
            45 => return true,
            _ => {}
        }
        false
    }

    /// `--smoke-build` joined to a server that runs the world: E at the gate
    /// asks the server, and the gate opens only when its state says so.
    fn play_logic_smoke_remote(
        &mut self,
        frame: usize,
        captures: Option<&std::path::Path>,
    ) -> bool {
        let play = self
            .play_logic
            .as_mut()
            .expect("--smoke-build needs a content pack with gameplay");
        let SmokeTargets { gate, .. } = smoke_targets(&play.layout);
        assert!(frame < 3000, "the server never ran the world's gameplay");
        let since = match (play.remote, play.smoke_remote_since) {
            (false, _) => return false,
            (true, Some(since)) => since,
            (true, None) => {
                println!("Remote smoke: the server runs the world (frame {frame})");
                *play.smoke_remote_since.insert(frame)
            }
        };
        match frame - since {
            5 => {
                self.capture(false);
                self.smoke_stand_at(gate.1);
            }
            // Give the server time to see where we stand.
            60 => assert!(self.interact_play_logic(), "E did not ask the server"),
            61..=1200 => {
                let play = self.play_logic.as_mut().unwrap();
                if let Some(opened) = play.smoke_opened {
                    // The screenshot was taken on the frames in between.
                    return frame >= opened + 3;
                }
                if !play.logic.visible(gate.0) {
                    play.smoke_opened = Some(frame);
                    println!(
                        "Remote smoke: the server opened the gate {} frames after E",
                        frame - since - 60
                    );
                    assert!(
                        self.smoke_walks_through(gate.1),
                        "the gate opened but still blocks"
                    );
                    if let Some(dir) = captures {
                        self.capture_next = Some(dir.join("remote-gate-open.png"));
                    }
                }
            }
            1201.. => panic!("the server did not open the gate"),
            _ => {}
        }
        false
    }

    /// Walk a test player west through a gate standing at `at` (Blender axes):
    /// true when it gets past. The real player is not moved.
    fn smoke_walks_through(&self, at: [f32; 3]) -> bool {
        let collision = self.collision.as_ref().unwrap();
        let start = blender_to_render([at[0] + 1.5, at[1], at[2]]) + Vec3::Y * 2.0;
        let mut walker = Player::spawn(collision, start);
        for _ in 0..90 {
            walker.step(collision, Vec3::NEG_X * 3.0, false, 1.0 / 60.0);
        }
        walker.feet.x < blender_to_render(at).x - 0.5
    }

    /// The interact key: true when the world's gameplay used it.
    pub(crate) fn interact_play_logic(&mut self) -> bool {
        if !(self.walking || self.driving) || self.menu.page.is_some() {
            return false;
        }
        let driving = self.driving;
        let at = if driving {
            self.car
                .as_ref()
                .map(|(car, _)| car.position - Vec3::Y * car.clearance)
        } else {
            self.player.as_ref().map(|p| p.feet)
        };
        let (Some(play), Some(at)) = (&mut self.play_logic, at) else {
            return false;
        };
        let feet = render_to_blender(at);
        if play.remote {
            // The server checks we stand there and decides.
            let (Some(node), Some(session)) =
                (play.logic.interaction(feet, driving), &self.network_session)
            else {
                return false;
            };
            let _ = session.trigger(
                None,
                sa_scene::world_logic::wire::USE_EVENT,
                &format!("[{node}]"),
            );
            return true;
        }
        let Some(effects) = play.logic.interact_for(feet, driving) else {
            return false;
        };
        self.apply_play_effects(effects);
        true
    }

    fn apply_play_effects(&mut self, effects: Vec<Effect>) {
        let mut visibility_changed = false;
        for effect in effects {
            match effect {
                Effect::Message { text, seconds } => {
                    if let Some(play) = &mut self.play_logic {
                        crate::runtime_log::line(format!("World message: {text}"));
                        play.message =
                            Some((text, Instant::now() + Duration::from_secs_f32(seconds)));
                    }
                }
                Effect::Teleport(pose) if self.driving => {
                    // The car goes along, with the player at the wheel and
                    // its damage as it was, when there is room for it.
                    let (Some(world), Some((car, _))) = (&self.collision, &mut self.car) else {
                        continue;
                    };
                    let yaw = yaw_for(pose.heading);
                    if car.teleport(world, spawn_eye(&pose), yaw) {
                        self.car_render_pose = None;
                        self.position = car.position;
                        self.yaw = yaw;
                        if let Some(play) = &mut self.play_logic {
                            play.logic.player_mut().moved();
                        }
                    } else if let Some(play) = &mut self.play_logic {
                        play.message = Some((
                            "No room for the car there".into(),
                            Instant::now() + Duration::from_secs(3),
                        ));
                    }
                }
                Effect::Teleport(pose) => {
                    if let Some(play) = &mut self.play_logic {
                        play.logic.player_mut().moved();
                    }
                    if let Some(collision) = &self.collision {
                        let player = Player::spawn(collision, spawn_eye(&pose));
                        self.position = player.eye();
                        self.player = Some(player);
                    }
                    self.yaw = yaw_for(pose.heading);
                    self.pitch = 0.0;
                }
                Effect::Visibility { .. } => visibility_changed = true,
                // A running clock jumps to the hour; a fixed one is set from
                // the logic every frame.
                Effect::Time(hour) => self.daycycle.minutes = f32::from(hour) * 60.0,
                Effect::Weather(_) => {}
            }
        }
        // Hidden objects (an opened door) let the player through.
        if let (true, Some(play), Some(collision)) =
            (visibility_changed, &self.play_logic, &mut self.collision)
        {
            collision.set_hidden(play.logic.hidden_by_actions());
        }
    }

    /// Is a Build Mode object shown right now while playtesting?
    pub(crate) fn play_logic_visible(&self, node: NodeId) -> Option<bool> {
        self.play_logic.as_ref().map(|p| p.logic.visible(node))
    }

    /// The world's weather right now (Build Mode while editing previews the
    /// world setting).
    /// Lights of Build Mode objects and of a played pack's switchable
    /// objects (the pack's other lights are part of its world batches).
    pub(crate) fn creator_glows(&self) -> impl Iterator<Item = &GpuBatch> + '_ {
        let build = self.build.iter().flat_map(move |build| {
            build
                .gpu
                .lights
                .iter()
                .filter(move |(id, _)| {
                    if build.playtesting {
                        self.play_logic_visible(**id)
                            .unwrap_or_else(|| build.doc.layout.effective_visible(**id))
                    } else {
                        build.doc.layout.effective_visible(**id)
                    }
                })
                .flat_map(|(_, light)| &light.object.batches)
        });
        build.chain(self.play_logic.iter().flat_map(PlayLogic::glows))
    }

    /// The world's own fog, sky colour and brightness, over the time and
    /// weather's lighting.
    pub(crate) fn apply_world_environment(&self, lighting: &mut crate::daycycle::Lighting) {
        let settings = match (&self.build, &self.play_logic) {
            (Some(build), _) => &build.doc.layout.settings,
            (None, Some(play)) => play.settings(),
            (None, None) => return,
        };
        if let Some(fog) = settings.fog {
            lighting.fog *= fog.clamp(0.2, 10.0);
        }
        if let Some(sky) = settings.sky {
            lighting.sky = Vec3::from_array(sky).clamp(Vec3::ZERO, Vec3::ONE);
        }
        if let Some(brightness) = settings.brightness {
            lighting.ambient *= brightness.clamp(0.3, 1.5);
        }
    }

    pub(crate) fn world_weather(&self) -> Option<Weather> {
        if let Some(build) = self.build.as_ref().filter(|b| !b.playtesting) {
            return build
                .doc
                .layout
                .settings
                .weather
                .as_deref()
                .and_then(Weather::parse);
        }
        self.play_logic.as_ref().and_then(PlayLogic::weather)
    }

    /// The message and the interaction prompt, over the game.
    pub(crate) fn play_logic_ui(&mut self, ctx: &egui::Context, scale: f32) {
        if self.menu.page.is_some() || self.build_mode_editing() {
            return;
        }
        let Some(play) = &mut self.play_logic else {
            return;
        };
        if play
            .message
            .as_ref()
            .is_some_and(|(_, until)| Instant::now() >= *until)
        {
            play.message = None;
        }
        let panel = |ui: &mut egui::Ui, text: egui::RichText| {
            // Short lines stay on one line; long messages wrap at a readable width.
            let wrap = if text.text().chars().count() > 60 {
                ui.set_max_width(640.0 * scale);
                egui::TextWrapMode::Wrap
            } else {
                egui::TextWrapMode::Extend
            };
            egui::Frame::new()
                .fill(ui_theme::BACKGROUND.gamma_multiply(0.85))
                .stroke(egui::Stroke::new(1.0, ui_theme::ACCENT.gamma_multiply(0.6)))
                .corner_radius(6.0)
                .inner_margin(egui::Margin::symmetric(16, 10))
                .show(ui, |ui| ui.add(egui::Label::new(text).wrap_mode(wrap)));
        };
        if let Some((text, _)) = &play.message {
            egui::Area::new("world-message".into())
                .anchor(egui::Align2::CENTER_CENTER, [0.0, -120.0 * scale])
                .interactable(false)
                .fade_in(false)
                .show(ctx, |ui| {
                    panel(
                        ui,
                        egui::RichText::new(text)
                            .size(22.0 * scale)
                            .color(ui_theme::TEXT),
                    )
                });
            ctx.request_repaint_after(Duration::from_millis(250));
        }
        if let Some(prompt) = &play.prompt {
            egui::Area::new("world-prompt".into())
                .anchor(egui::Align2::CENTER_BOTTOM, [0.0, -150.0 * scale])
                .interactable(false)
                .fade_in(false)
                .show(ctx, |ui| {
                    panel(
                        ui,
                        egui::RichText::new(format!("E   {prompt}"))
                            .size(17.0 * scale)
                            .color(ui_theme::TEXT),
                    )
                });
        }
    }
}
