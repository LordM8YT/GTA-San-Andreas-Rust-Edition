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
            let Some(asset) = &node.asset else { continue };
            if flags.get(&node.id).is_some_and(|f| f.collision) {
                match resolver.model(asset) {
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
        Self {
            logic: Logic::new(layout),
            layout: layout.clone(),
            objects,
            message: None,
            prompt: None,
            last_step: None,
        }
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
        // interactions (E) are on foot only.
        let at = if self.driving {
            self.car.as_ref().map(|(car, _)| car.position)
        } else if self.walking {
            self.player.as_ref().map(|p| p.feet)
        } else {
            None
        };
        let Some(at) = at else { return };
        let feet = render_to_blender(at);
        let effects = play.logic.step(feet, dt);
        if !self.driving {
            play.prompt = play.logic.prompt(feet);
        }
        self.apply_play_effects(effects);
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
        if !self.walking || self.driving || self.menu.page.is_some() {
            return false;
        }
        let (Some(play), Some(player)) = (&mut self.play_logic, &self.player) else {
            return false;
        };
        let Some(effects) = play.logic.interact(render_to_blender(player.feet)) else {
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
                Effect::Teleport(_) if self.driving => {
                    // Teleporting moves a player on foot; a car stays put.
                }
                Effect::Teleport(pose) => {
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
