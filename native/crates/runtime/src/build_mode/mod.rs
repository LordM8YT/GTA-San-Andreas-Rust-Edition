//! SARE Build Mode: the in-game world editor.
//!
//! The editor core (projects, undo/redo, catalog, maths) lives in the
//! `sa-editor` crate. This module connects it to the running game:
//! - `gpu`: placed objects as ordinary render batches;
//! - `viewport`: camera, picking, placement, gizmos and grid;
//! - `panels`: the egui interface (toolbar, browser, outliner, inspector, log);
//! - here: the session (open, autosave, playtest and back).
//!
//! Start it with `sa-runtime --build <pack id | folder | new:Name>`.
//! See docs/build-mode.md.
mod gameplay_ui;
pub mod gpu;
mod pacing;
mod panels;
mod viewport;

use crate::State;
use anyhow::{Context, Result};
use glam::Vec3;
use sa_editor::catalog::{AssetStatus, Catalog};
use sa_editor::history::{Command, Document, History};
use sa_editor::project::{Project, Recovery};
use sa_editor::viewport::EditorCamera;
use sa_scene::content_pack::AssetResolver;
use sa_scene::world_layout::{AssetRef, Node, NodeId};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Seconds between autosaves of unsaved work.
const AUTOSAVE_SECONDS: f32 = 15.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Select,
    Move,
    Rotate,
    Scale,
    /// Shape terrain objects with a brush.
    Sculpt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Brush {
    Raise,
    Lower,
    Smooth,
    Flatten,
}
impl Brush {
    pub const ALL: [Brush; 4] = [Self::Raise, Self::Lower, Self::Smooth, Self::Flatten];
    pub fn label(self) -> &'static str {
        match self {
            Self::Raise => "Raise",
            Self::Lower => "Lower",
            Self::Smooth => "Smooth",
            Self::Flatten => "Flatten",
        }
    }
}

/// The Sculpt tool's brush.
#[derive(Clone, Copy, Debug)]
pub struct Sculpt {
    pub brush: Brush,
    /// Metres.
    pub radius: f32,
    /// 0-1: how fast the brush works.
    pub strength: f32,
}
impl Default for Sculpt {
    fn default() -> Self {
        Self {
            brush: Brush::Raise,
            radius: 6.0,
            strength: 0.5,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Snap {
    /// Snap moves to the grid and rotations/scales to their steps.
    pub enabled: bool,
    pub show_grid: bool,
    /// Grid step in metres.
    pub step: f32,
    /// Rotation step in degrees.
    pub angle: f32,
    /// Scale step.
    pub scale: f32,
    /// Put new objects on the surface under the mouse.
    pub surface: bool,
}
impl Default for Snap {
    fn default() -> Self {
        Self {
            enabled: true,
            show_grid: true,
            step: 1.0,
            angle: 15.0,
            scale: 0.1,
            surface: true,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Info,
    Warning,
    Error,
}
pub struct LogLine {
    pub level: Level,
    pub text: String,
    pub at: Instant,
}

/// An asset waiting to be placed with the next click.
pub struct Placing {
    pub asset: AssetRef,
    pub label: String,
    /// Turn (degrees about up) given with [ and ].
    pub turn: f32,
    /// Dragged out of the Asset Browser: releasing the mouse places it once.
    pub dragged: bool,
}

/// Something only the game state can do, asked for by the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Request {
    Playtest,
    Quit,
}

pub struct BuildMode {
    pub project: Project,
    pub doc: Document,
    pub history: History,
    pub catalog: Catalog,
    pub resolver: AssetResolver,
    pub gpu: gpu::Gpu,
    pub selection: Vec<NodeId>,
    pub tool: Tool,
    /// Gizmo in object (local) axes instead of world axes.
    pub local: bool,
    pub snap: Snap,
    pub sculpt: Sculpt,
    pub camera: EditorCamera,
    pub placing: Option<Placing>,
    pub playtesting: bool,
    pub log: Vec<LogLine>,
    pub recovery: Option<Recovery>,
    pub requests: Vec<Request>,
    /// Another world to open: SARE restarts in Build Mode on it.
    pub switch_to: Option<PathBuf>,
    /// Nodes as they were before an unfinished live edit (gizmo drag,
    /// inspector drag), recorded as one undo step when it ends.
    preview_before: Vec<Node>,
    preview_label: String,
    drag: Option<viewport::Drag>,
    /// The pack's own scene in render space, for surface placement and
    /// playtest collision.
    base_collision: Vec<sa_scene::Batch>,
    base_pick: Vec<[Vec3; 3]>,
    base_bounds: Option<(Vec3, Vec3)>,
    last_autosave: Instant,
    autosaved_revision: u64,
    ui: panels::UiState,
    /// Draw only when something changes while editing (see `pacing`).
    pacing: pacing::Pacing,
}

/// Find or create the project named by `--build`: a pack id, a pack
/// folder, or `new:<Name>` for a new world in the content folder.
pub fn resolve_project(argument: &str) -> Result<PathBuf> {
    let content = sa_scene::content::pack_dirs()
        .into_iter()
        .next()
        .unwrap_or_else(|| PathBuf::from(sa_scene::content_pack::DEFAULT_DIR));
    if let Some(name) = argument.strip_prefix("new:") {
        let name = name.trim();
        let name = if name.is_empty() { "My World" } else { name };
        let id = sa_editor::project::new_id(name, &content);
        let folder = content.join(&id);
        Project::create(&folder, &id, name)?;
        crate::runtime_log::line(format!("Build Mode: created {}", folder.display()));
        return Ok(folder);
    }
    let direct = Path::new(argument);
    if direct.join(sa_scene::content_pack::MANIFEST).is_file() {
        return Ok(direct.to_path_buf());
    }
    for dir in sa_scene::content::pack_dirs() {
        if let Ok(folder) = sa_scene::content_pack::resolve(argument, &dir) {
            return Ok(folder);
        }
    }
    anyhow::bail!(
        "No world {argument:?} to edit. Use a content pack id, a pack folder, or new:<Name>."
    )
}

impl BuildMode {
    pub fn open(folder: &Path) -> Result<Self> {
        let (project, doc) = Project::open(folder)?;
        let recovery = match project.recovery(&doc) {
            Ok(found) => found,
            Err(error) => {
                crate::runtime_log::line(format!("Build Mode autosave ignored: {error:#}"));
                None
            }
        };
        let manifest = sa_scene::content_pack::Manifest::read(folder)?;
        let base = sa_scene::content_pack::base_model(folder, &manifest)
            .context("loading the world's own scene")?;
        let offset = gpu::render_matrix(&Default::default());
        let (_, base_collision) = base.transformed(offset);
        let base_pick: Vec<[Vec3; 3]> = base_collision
            .iter()
            .chain(base.transformed(offset).0.iter())
            .flat_map(|b| b.vertices.as_chunks::<3>().0)
            .map(|t| [0, 1, 2].map(|i| Vec3::from_array(t[i].position)))
            .collect();
        let base_bounds = sa_editor::viewport::bounds(base_pick.iter().flatten().copied());
        let resolver = AssetResolver::new(folder);
        let mut build = Self {
            catalog: Catalog {
                entries: Vec::new(),
            },
            resolver,
            gpu: gpu::Gpu::default(),
            selection: Vec::new(),
            tool: Tool::Move,
            local: false,
            snap: Snap::default(),
            sculpt: Sculpt::default(),
            camera: EditorCamera::default(),
            placing: None,
            playtesting: false,
            log: Vec::new(),
            recovery,
            requests: Vec::new(),
            switch_to: None,
            preview_before: Vec::new(),
            preview_label: String::new(),
            drag: None,
            base_collision,
            base_pick,
            base_bounds,
            last_autosave: Instant::now(),
            autosaved_revision: 0,
            ui: panels::UiState::default(),
            pacing: pacing::Pacing::default(),
            history: History::default(),
            project,
            doc,
        };
        build.refresh_catalog();
        build.reset_camera();
        build.info(format!(
            "Opened {} ({} objects) from {}",
            build.doc.info.name,
            build.doc.layout.nodes.len(),
            build.project.folder.display()
        ));
        if build.recovery.is_some() {
            build
                .warn("Unsaved work from an earlier session was found. Choose Recover or Discard.");
        }
        Ok(build)
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.push(Level::Info, text.into());
    }
    pub fn warn(&mut self, text: impl Into<String>) {
        self.push(Level::Warning, text.into());
    }
    pub fn error(&mut self, text: impl Into<String>) {
        self.push(Level::Error, text.into());
    }
    fn push(&mut self, level: Level, text: String) {
        crate::runtime_log::line(format!("Build Mode: {text}"));
        self.log.push(LogLine {
            level,
            text,
            at: Instant::now(),
        });
        if self.log.len() > 500 {
            self.log.remove(0);
        }
    }

    pub fn refresh_catalog(&mut self) {
        let providers = Catalog::providers_for(
            &self.project.folder,
            &self.project.id,
            &sa_scene::content::pack_dirs(),
        );
        self.catalog = Catalog::scan(&providers);
        self.catalog.add_missing(&self.doc.layout);
        let missing = self
            .catalog
            .entries
            .iter()
            .filter(|e| matches!(e.status, AssetStatus::Missing(_)))
            .count();
        self.info(format!(
            "Asset catalog: {} assets",
            self.catalog.entries.len()
        ));
        if missing > 0 {
            self.warn(format!(
                "{missing} asset(s) used by this world are missing; they show as magenta cubes"
            ));
        }
    }

    /// Run a command and log failures instead of losing them.
    pub fn execute(&mut self, command: Command) {
        self.commit_preview();
        if let Command::Modify { changes, .. } = &command {
            if changes.is_empty() {
                return;
            }
        }
        if let Err(error) = self.history.execute(&mut self.doc, command) {
            self.error(format!("{error:#}"));
        }
        self.selection
            .retain(|id| self.doc.layout.node(*id).is_some());
    }
    pub fn undo(&mut self) {
        self.commit_preview();
        match self.history.undo(&mut self.doc) {
            Ok(Some(label)) => self.info(format!("Undo: {label}")),
            Ok(None) => {}
            Err(error) => self.error(format!("Undo failed: {error:#}")),
        }
        self.selection
            .retain(|id| self.doc.layout.node(*id).is_some());
    }
    pub fn redo(&mut self) {
        self.commit_preview();
        match self.history.redo(&mut self.doc) {
            Ok(Some(label)) => self.info(format!("Redo: {label}")),
            Ok(None) => {}
            Err(error) => self.error(format!("Redo failed: {error:#}")),
        }
        self.selection
            .retain(|id| self.doc.layout.node(*id).is_some());
    }

    /// Change nodes live without history; [`Self::commit_preview`] records
    /// the whole change as one undo step.
    pub fn preview(&mut self, label: &str, ids: &[NodeId], mut edit: impl FnMut(&mut Node)) {
        for id in ids {
            if !self.preview_before.iter().any(|n| n.id == *id) {
                if let Some(node) = self.doc.layout.node(*id) {
                    self.preview_before.push(node.clone());
                }
            }
            if let Some(node) = self.doc.layout.node_mut(*id) {
                edit(node);
            }
        }
        self.preview_label = label.into();
    }
    pub fn commit_preview(&mut self) {
        if self.preview_before.is_empty() {
            return;
        }
        let changes: Vec<(Node, Node)> = std::mem::take(&mut self.preview_before)
            .into_iter()
            .filter_map(|before| {
                let after = self.doc.layout.node(before.id)?.clone();
                (after != before).then_some((before, after))
            })
            .collect();
        if !changes.is_empty() {
            self.history.record(Command::Modify {
                label: std::mem::take(&mut self.preview_label),
                changes,
            });
        }
    }
    pub fn editing_live(&self) -> bool {
        !self.preview_before.is_empty()
    }

    pub fn save(&mut self) {
        self.commit_preview();
        match self.project.save(&self.doc) {
            Ok(()) => {
                self.history.mark_saved();
                self.autosaved_revision = self.history.revision;
                self.info(format!("Saved {}", self.project.folder.display()));
            }
            Err(error) => self.error(format!("Save failed, nothing was overwritten: {error:#}")),
        }
    }
    pub fn save_as(&mut self, name: &str) {
        self.commit_preview();
        let Some(parent) = self.project.folder.parent().map(Path::to_path_buf) else {
            self.error("Save As: this world has no parent folder");
            return;
        };
        let id = sa_editor::project::new_id(name, &parent);
        let mut doc = self.doc.clone();
        doc.info.name = name.trim().to_owned();
        match self.project.save_as(&doc, &parent.join(&id), &id) {
            Ok(copy) => {
                self.project = copy;
                self.doc = doc;
                self.resolver = AssetResolver::new(&self.project.folder);
                self.history.mark_saved();
                self.info(format!(
                    "Saved as {} in {}",
                    name,
                    self.project.folder.display()
                ));
                self.refresh_catalog();
            }
            Err(error) => self.error(format!("Save As failed: {error:#}")),
        }
    }
    /// Autosave unsaved work now and then; never touches the real files.
    pub fn autosave_if_due(&mut self) {
        if self.last_autosave.elapsed().as_secs_f32() < AUTOSAVE_SECONDS {
            return;
        }
        self.last_autosave = Instant::now();
        self.autosave();
    }
    pub fn autosave(&mut self) {
        if !self.history.dirty() || self.history.revision == self.autosaved_revision {
            return;
        }
        match self.project.autosave(&self.doc) {
            Ok(()) => {
                self.autosaved_revision = self.history.revision;
                self.info("Autosaved unsaved changes");
            }
            Err(error) => self.error(format!("Autosave failed: {error:#}")),
        }
    }
    pub fn recover(&mut self, keep: bool) {
        let Some(recovery) = self.recovery.take() else {
            return;
        };
        if keep {
            // Recovered work is unsaved: undo goes back to the saved files.
            let changes: Vec<Command> = vec![Command::Batch {
                label: "Recover unsaved work".into(),
                commands: vec![
                    Command::Remove(self.doc.layout.nodes.iter().cloned().enumerate().collect()),
                    Command::Insert(
                        recovery
                            .document
                            .layout
                            .nodes
                            .iter()
                            .cloned()
                            .enumerate()
                            .collect(),
                    ),
                    Command::Info {
                        before: self.doc.info.clone(),
                        after: recovery.document.info.clone(),
                    },
                    Command::Settings {
                        before: self.doc.layout.settings.clone(),
                        after: recovery.document.layout.settings.clone(),
                    },
                ],
            }];
            for command in changes {
                self.execute(command);
            }
            self.doc.layout.next_id = self
                .doc
                .layout
                .next_id
                .max(recovery.document.layout.next_id);
            self.info("Recovered unsaved work. Save to keep it.");
            self.refresh_catalog();
        } else {
            self.project.discard_autosave();
            self.info("Discarded the autosave; kept the saved world.");
        }
    }

    /// Point the camera at the whole world.
    pub fn reset_camera(&mut self) {
        let speed = self.camera.speed;
        self.camera = EditorCamera {
            speed,
            ..EditorCamera::default()
        };
        let bounds = self.world_bounds();
        if let Some((lo, hi)) = bounds {
            self.camera.frame(lo, hi);
        }
    }
    fn world_bounds(&self) -> Option<(Vec3, Vec3)> {
        let objects = self
            .gpu
            .objects
            .values()
            .flat_map(|o| [o.bounds.0, o.bounds.1]);
        let base = self.base_bounds.into_iter().flat_map(|(a, b)| [a, b]);
        sa_editor::viewport::bounds(objects.chain(base))
    }
    pub fn focus_selection(&mut self) {
        let ids = sa_editor::ops::with_descendants(&self.doc, &self.selection);
        let points = ids
            .iter()
            .filter_map(|id| self.gpu.objects.get(id))
            .flat_map(|o| [o.bounds.0, o.bounds.1]);
        if let Some((lo, hi)) = sa_editor::viewport::bounds(points) {
            self.camera.frame(lo, hi);
        }
    }

    /// Spawn point in render space.
    pub fn spawn_render(&self) -> Vec3 {
        gpu::render_matrix(&Default::default())
            .transform_point3(sa_scene::world_layout::blender_to_gltf(self.doc.info.spawn))
    }

    /// Collision for playtesting: the pack's scene plus every solid object.
    /// Objects gameplay can show or hide are switchable, so an opened door
    /// lets the player through without rebuilding the world.
    fn playtest_collision(&mut self) -> sa_scene::collision::CollisionWorld {
        let switchable = sa_scene::world_logic::switchable(&self.doc.layout);
        let mut switched = Vec::new();
        let mut batches: Vec<sa_scene::Batch> = self
            .base_collision
            .iter()
            .map(|b| sa_scene::Batch {
                key: b.key.clone(),
                vertices: b.vertices.clone(),
                alpha: b.alpha,
                animated: false,
                uv_animation: None,
                map_door: None,
                mlo_entity: None,
            })
            .collect();
        let mut problems = Vec::new();
        for node in &self.doc.layout.nodes {
            if node.asset.is_none() {
                continue;
            }
            if !self.doc.layout.effective_collision(node.id) {
                continue;
            }
            match sa_scene::content_pack::node_model(node, &mut self.resolver) {
                Ok(model) => {
                    let (_, solid) = model.transformed(gpu::render_matrix(&node.transform));
                    if switchable.contains(&node.id) {
                        switched.push((node.id, solid));
                    } else {
                        batches.extend(solid);
                    }
                }
                Err(error) => problems.push(format!("{}: {error:#}", node.name)),
            }
        }
        for problem in problems {
            self.warn(format!("No collision for {problem}"));
        }
        let mut world = sa_scene::collision::CollisionWorld::from_batches(&batches);
        for (id, solid) in switched {
            world.add_switchable(id, &solid);
        }
        world
    }
}

impl State {
    /// Build Mode is open and not playtesting.
    pub(crate) fn build_mode_editing(&self) -> bool {
        self.build.as_ref().is_some_and(|b| !b.playtesting)
    }
    /// Editing, with the light editor graphics on (picked, or automatic on
    /// integrated graphics).
    pub(crate) fn build_mode_light(&self) -> bool {
        self.build_mode_editing()
            && self
                .menu
                .settings
                .editor_performance
                .light(self.integrated_gpu)
    }
    /// The graphics settings this frame is drawn with: the player's own,
    /// made lighter while editing in the light editor mode.
    pub(crate) fn draw_settings(&self) -> std::borrow::Cow<'_, sa_client::settings::Settings> {
        if self.build_mode_light() {
            std::borrow::Cow::Owned(self.menu.settings.light_editor())
        } else {
            std::borrow::Cow::Borrowed(&self.menu.settings)
        }
    }
    /// While editing: a window event arrived. Returns when the next frame
    /// may be drawn (None: not editing, the game paces itself).
    pub(crate) fn build_mode_event(
        &mut self,
        event: &winit::event::WindowEvent,
    ) -> Option<Instant> {
        use winit::event::WindowEvent;
        let fps_limit = self.draw_settings().fps_limit;
        let build = self.build.as_mut().filter(|b| !b.playtesting)?;
        let pacing = &mut build.pacing;
        match event {
            WindowEvent::Focused(focused) => pacing.focused = *focused,
            WindowEvent::Occluded(hidden) => pacing.hidden = *hidden,
            WindowEvent::Resized(size) => pacing.hidden = size.width == 0 || size.height == 0,
            WindowEvent::RedrawRequested => return None,
            _ => {}
        }
        Some(pacing.input(Instant::now(), fps_limit))
    }
    /// While editing: egui finished a pass and wants its next frame after
    /// `delay`.
    pub(crate) fn build_mode_repaint_after(&mut self, delay: std::time::Duration) {
        if let Some(build) = self.build.as_mut() {
            build.pacing.repaint = pacing::repaint_at(Instant::now(), delay);
        }
    }
    /// While editing: a frame was drawn; when the next one is due (None:
    /// not editing).
    pub(crate) fn build_mode_frame_drawn(&mut self) -> Option<Instant> {
        let fps_limit = self.draw_settings().fps_limit;
        let loading = self.loading();
        let build = self.build.as_mut().filter(|b| !b.playtesting)?;
        let busy = loading
            || build.ui.flying
            || build.drag.is_some()
            || build.placing.as_ref().is_some_and(|p| p.dragged);
        Some(build.pacing.frame_drawn(Instant::now(), fps_limit, busy))
    }

    /// Before the game update: the editor camera drives the view.
    pub(crate) fn build_mode_begin_frame(&mut self) {
        let Some(build) = &mut self.build else { return };
        if build.playtesting {
            // The world's gameplay sets the time while playing.
            return;
        }
        if let Some(hour) = build.doc.layout.settings.hour {
            self.daycycle.minutes = f32::from(hour.min(23)) * 60.0;
        }
        build.autosave_if_due();
        build.ui.performance = self.menu.settings.editor_performance;
        build.ui.light = self
            .menu
            .settings
            .editor_performance
            .light(self.integrated_gpu);
        let eye = build.camera.eye();
        self.position = Vec3::new(eye.x, eye.y.max(-90.0), eye.z);
        self.yaw = build.camera.yaw;
        self.pitch = build.camera.pitch;
        self.walking = false;
        self.driving = false;
        self.player = None;
        self.keys.clear();
    }

    /// The editor's interface, inside the frame's egui pass. Returns false
    /// when Build Mode is not drawing (no project, or playtesting).
    pub(crate) fn build_mode_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(mut build) = self.build.take() else {
            return false;
        };
        let drawing = if build.playtesting {
            panels::playtest_overlay(&mut build, ui);
            false
        } else {
            let frame = viewport::Frame {
                view_projection: self.reflections.view_projection,
                eye: self.reflections.eye,
            };
            panels::draw(&mut build, ui, &frame);
            true
        };
        self.build = Some(build);
        drawing
    }

    /// After the egui pass: upload and move objects, then act on requests.
    pub(crate) fn build_mode_end_frame(&mut self) {
        let Some(mut build) = self.build.take() else {
            return;
        };
        // The editor needs the mouse: nothing may keep it locked and hidden
        // while editing (only playtesting captures it).
        if !build.playtesting && self.captured {
            self.capture(false);
        }
        let performance_changed = build.ui.performance != self.menu.settings.editor_performance;
        if performance_changed {
            self.menu.settings.editor_performance = build.ui.performance;
            let light = build.ui.performance.light(self.integrated_gpu);
            build.info(format!(
                "Editor graphics: {} (drawing {})",
                build.ui.performance.name(),
                if light { "light" } else { "full" }
            ));
        }
        let mut problems = Vec::new();
        build
            .gpu
            .sync(self, &mut build.resolver, &build.doc.layout, &mut problems);
        build
            .gpu
            .sync_lights(self, &build.doc.layout, &mut problems);
        let ghost = build.placing.as_ref().map(|p| p.asset.clone());
        match (ghost, build.ui.ghost_transform) {
            (Some(asset), Some(transform)) if !build.playtesting => {
                build
                    .gpu
                    .set_ghost(self, &mut build.resolver, &asset, transform, &mut problems);
            }
            _ => build.gpu.ghost = None,
        }
        for problem in problems {
            if let Some((reference, _)) = problem.split_once(": ") {
                if let Ok(reference) = AssetRef::parse(reference) {
                    if let Some(entry) = build.catalog.find_mut(&reference) {
                        if entry.status == AssetStatus::Ready {
                            entry.status = AssetStatus::Invalid(problem.clone());
                        }
                    }
                }
            }
            build.error(format!("Model failed to load: {problem}"));
        }
        let requests = std::mem::take(&mut build.requests);
        if let Some(folder) = build.switch_to.take() {
            // Each world has its own base scene and collision, loaded at
            // start-up: open the other world in a fresh SARE.
            crate::runtime_log::line(format!("Build Mode: switching to {}", folder.display()));
            self.build_restart = Some(folder);
            self.quit_requested = true;
        }
        self.build = Some(build);
        for request in requests {
            match request {
                Request::Playtest => self.toggle_playtest(),
                Request::Quit => self.quit_requested = true,
            }
        }
        // With Build Mode back in place (draw_settings asks whether it is
        // editing) and any playtest switch made: save a new editor graphics
        // choice, and size the frame for light or full drawing.
        if performance_changed {
            self.apply_settings();
        }
        if self.size.width > 0 && self.size.height > 0 {
            self.postprocess.resize(
                &self.device,
                self.draw_settings()
                    .render_size(self.size.width, self.size.height),
                [self.size.width, self.size.height],
            );
        }
    }

    /// Start playing the edited world, or go back to editing it.
    pub(crate) fn toggle_playtest(&mut self) {
        let Some(playtesting) = self.build.as_ref().map(|b| b.playtesting) else {
            return;
        };
        if playtesting {
            if self.driving {
                self.toggle_car();
            }
            self.walking = false;
            self.player = None;
            self.play_logic = None;
            self.capture(false);
            self.keys.clear();
            if let Some(build) = &mut self.build {
                build.playtesting = false;
                build.info("Back in Build Mode. Your edits are as you left them.");
            }
            return;
        }
        let Some(build) = &mut self.build else { return };
        build.commit_preview();
        build.placing = None;
        build.drag = None;
        // Checkpoint: unsaved work is on disk before the game takes over.
        if build.history.dirty() {
            build.autosaved_revision = 0;
            build.autosave();
        }
        let collision = build.playtest_collision();
        // Gameplay starts fresh from the layout as it is now.
        let play = crate::play_logic::PlayLogic::playtest(&build.doc.layout);
        for problem in sa_scene::world_logic::check(&build.doc.layout) {
            build.warn(format!("Gameplay: {}", problem.text));
        }
        let (feet, yaw) = match play.logic.spawn() {
            Some(pose) => (
                crate::play_logic::blender_to_render(pose.position),
                crate::play_logic::yaw_for(pose.heading),
            ),
            None => (build.spawn_render(), build.camera.yaw),
        };
        let player = sa_scene::collision::Player::spawn(&collision, feet + Vec3::Y * 2.0);
        let standing = collision.standing_at(feet, feet.y + 2.0).is_some();
        build.playtesting = true;
        build.info("Playtest started. Press F6 to return to Build Mode.");
        if !standing {
            build.warn("Nothing solid under the spawn point: the player may fall. Move the spawn or add ground.");
        }
        self.play_logic = Some(play);
        self.collision = Some(collision);
        self.position = player.eye();
        self.player = Some(player);
        self.walking = true;
        self.driving = false;
        self.yaw = yaw;
        self.pitch = 0.0;
        self.keys.clear();
        self.capture(true);
    }

    /// The game is closing: keep unsaved work in the autosave.
    pub(crate) fn build_mode_exiting(&mut self) {
        if let Some(build) = &mut self.build {
            build.commit_preview();
            build.autosaved_revision = 0;
            build.autosave();
        }
    }
}

/// Start SARE again with the same options, editing `folder` instead.
pub fn restart_on(args: &[String], folder: &Path) -> Result<()> {
    let mut next = Vec::new();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            // The world being edited, and entering play (Build Mode opens
            // in the editor anyway).
            "--build" => {
                rest.next();
            }
            "--play" => {}
            _ => next.push(arg.clone()),
        }
    }
    next.push("--build".into());
    next.push(folder.display().to_string());
    let exe = std::env::current_exe().context("finding SARE to restart it")?;
    std::process::Command::new(exe)
        .args(&next)
        .spawn()
        .context("starting SARE on the other world")?;
    Ok(())
}

/// `--smoke-build`: drive Build Mode through editing, a playtest of the
/// world's gameplay and back, with screenshots when `--capture-dir` is set.
/// Meant for `content/gameplay-sample`.
impl State {
    /// One rendered frame of the smoke run; true when it has passed.
    pub(crate) fn build_mode_smoke(&mut self, frame: usize, captures: Option<&Path>) -> bool {
        let capture = |state: &mut State, name: &str| {
            if let Some(dir) = captures {
                state.capture_next = Some(dir.join(name));
            }
        };
        // A menu shown on a first start would take the E key: keep it shut.
        self.menu.page = None;
        let build = self
            .build
            .as_ref()
            .expect("--smoke-build needs --build <world>");
        let crate::play_logic::SmokeTargets { gate, door, night } =
            crate::play_logic::smoke_targets(&build.doc.layout);
        match frame {
            5 => {
                let build = self.build.as_mut().unwrap();
                build.selection = vec![door.0];
                build.focus_selection();
            }
            15 => capture(self, "build-editor.png"),
            20 => {
                self.toggle_playtest();
                assert!(
                    self.build.as_ref().unwrap().playtesting,
                    "playtest did not start"
                );
            }
            30 => self.smoke_stand_at(gate.1),
            34 => {
                let prompt = self
                    .play_logic
                    .as_ref()
                    .and_then(|p| p.prompt().map(str::to_owned));
                assert!(prompt.is_some(), "no prompt at the gate");
                assert!(self.interact_play_logic(), "E did nothing at the gate");
                assert!(
                    !self.play_logic.as_ref().unwrap().logic.visible(gate.0),
                    "the gate did not open"
                );
            }
            40 => capture(self, "playtest-gate-open.png"),
            45 => self.smoke_stand_at(night.1),
            52 => {
                let play = self.play_logic.as_ref().unwrap();
                assert_eq!(play.hour(), Some(22), "the night zone did not set the time");
                assert!(play.message().is_some(), "no message from the night zone");
                capture(self, "playtest-night-zone.png");
            }
            60 => self.smoke_stand_at(door.1),
            64 => {
                let before = self.player.as_ref().unwrap().feet;
                assert!(self.interact_play_logic(), "E did nothing at the door");
                let after = self.player.as_ref().unwrap().feet;
                assert!(
                    before.distance(after) > 20.0,
                    "the door did not teleport the player"
                );
            }
            72 => capture(self, "playtest-inside.png"),
            80 => {
                let revision = self.build.as_ref().unwrap().history.revision;
                self.toggle_playtest();
                let build = self.build.as_ref().unwrap();
                assert!(!build.playtesting && self.play_logic.is_none());
                assert_eq!(
                    build.history.revision, revision,
                    "playtest changed the document"
                );
            }
            // Terrain: a sculpt stroke rebuilds the mesh; one undo takes it back.
            84 => {
                let build = self.build.as_mut().unwrap();
                let hill = build
                    .doc
                    .layout
                    .nodes
                    .iter()
                    .find(|n| n.terrain.is_some())
                    .map(|n| n.id)
                    .expect("the smoke world has a terrain object");
                build.selection = vec![hill];
                build.focus_selection();
                let before = build.gpu.objects[&hill].bounds.1.y;
                build.ui.smoke_terrain = Some((hill, before));
                build.preview("Sculpt terrain", &[hill], |n| {
                    if let Some(terrain) = &mut n.terrain {
                        sa_scene::terrain::raise(terrain, glam::Vec2::ZERO, 10.0, 5.0);
                    }
                });
                build.commit_preview();
            }
            86 => {
                let build = self.build.as_ref().unwrap();
                let (hill, before) = build.ui.smoke_terrain.unwrap();
                let after = build.gpu.objects[&hill].bounds.1.y;
                assert!(
                    after > before + 4.0,
                    "the sculpted terrain mesh did not rise ({before} -> {after})"
                );
                capture(self, "build-terrain.png");
            }
            88 => {
                let build = self.build.as_mut().unwrap();
                build.undo();
            }
            90 => {
                let build = self.build.as_ref().unwrap();
                let (hill, before) = build.ui.smoke_terrain.unwrap();
                let after = build.gpu.objects[&hill].bounds.1.y;
                let log: Vec<&str> = build
                    .log
                    .iter()
                    .rev()
                    .take(4)
                    .map(|l| l.text.as_str())
                    .collect();
                assert!(
                    (after - before).abs() < 0.01,
                    "undo did not restore the terrain ({before} -> {after}); log {log:?}"
                );
            }
            92 => return true,
            _ => {}
        }
        false
    }

    /// Put the player on the ground at a Blender-axes point.
    pub(crate) fn smoke_stand_at(&mut self, at: [f32; 3]) {
        let eye = crate::play_logic::blender_to_render(at) + Vec3::Y * 2.0;
        if let Some(collision) = &self.collision {
            let player = sa_scene::collision::Player::spawn(collision, eye);
            self.position = player.eye();
            self.player = Some(player);
        }
    }
}
