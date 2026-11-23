//! Runs a world's Build Mode gameplay (`sa_scene::world_logic`) while it is
//! played: in Build Mode's playtest and when a content pack with a layout is
//! played from the launcher.
//!
//! The logic itself is plain data in `sa-scene`; this module feeds it the
//! player's feet each frame, applies what it asks for (messages, teleports,
//! showing and hiding objects, time and weather) and draws the message and
//! the "E" prompt.
use crate::ui_theme;
use crate::State;
use glam::Vec3;
use sa_scene::collision::Player;
use sa_scene::world_layout::{blender_to_gltf, gltf_to_blender, Layout, NodeId, Transform};
use sa_scene::world_logic::{Effect, Logic, Pose, Weather};
use std::collections::HashSet;
use std::time::{Duration, Instant};

pub struct PlayLogic {
    pub logic: Logic,
    /// Layout objects are drawn one by one (Build Mode playtest), so actions
    /// can show and hide them. A pack played from the launcher draws its
    /// layout as one merged scene and cannot yet.
    live_objects: bool,
    message: Option<(String, Instant)>,
    prompt: Option<String>,
    warned_static: bool,
    /// Real time of the last step: frame `dt` is capped for physics, so the
    /// checkpoint clock uses the wall clock instead.
    last_step: Option<Instant>,
}

impl PlayLogic {
    pub fn new(layout: &Layout, live_objects: bool) -> Self {
        let mut logic = Logic::new(layout);
        if !live_objects {
            logic.set_static_objects();
        }
        Self {
            logic,
            live_objects,
            message: None,
            prompt: None,
            warned_static: false,
            last_step: None,
        }
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
        if !visibility_changed {
            return;
        }
        let Some(play) = &mut self.play_logic else {
            return;
        };
        if !play.live_objects {
            if !play.warned_static {
                play.warned_static = true;
                crate::runtime_log::line(
                    "World gameplay: showing and hiding objects works in Build Mode playtest only for now",
                );
            }
            return;
        }
        let hidden: HashSet<NodeId> = play.logic.hidden_by_actions().collect();
        if let Some(build) = &mut self.build {
            self.collision = Some(build.playtest_collision(&hidden));
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
