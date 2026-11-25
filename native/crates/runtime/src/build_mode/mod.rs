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
mod arrange;
mod attributes_ui;
mod command;
mod gameplay_ui;
pub mod gpu;
mod pacing;
mod panels;
mod saved_models;
mod script_ui;
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
    /// Paint squares with [`Sculpt::material`].
    Paint,
    /// Cut squares out of the terrain.
    Hole,
}
impl Brush {
    pub const ALL: [Brush; 6] = [
        Self::Raise,
        Self::Lower,
        Self::Smooth,
        Self::Flatten,
        Self::Paint,
        Self::Hole,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Raise => "Raise",
            Self::Lower => "Lower",
            Self::Smooth => "Smooth",
            Self::Flatten => "Flatten",
            Self::Paint => "Paint",
            Self::Hole => "Hole",
        }
    }
    pub fn hint(self) -> &'static str {
        match self {
            Self::Raise => "Lift the ground under the brush",
            Self::Lower => "Sink the ground under the brush",
            Self::Smooth => "Even out bumps and sharp edges",
            Self::Flatten => "Level the ground to where the stroke began",
            Self::Paint => "Paint the ground with a material; painting over a hole fills it",
            Self::Hole => "Cut the ground away (for caves and tunnels); Paint fills it again",
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
    /// What Paint paints: an index in `TERRAIN_MATERIALS`.
    pub material: u8,
}
impl Default for Sculpt {
    fn default() -> Self {
        Self {
            brush: Brush::Raise,
            radius: 6.0,
            strength: 0.5,
            material: 1,
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
    /// `--smoke-build`: its test edits must not replace a real autosave.
    pub autosave_off: bool,
    ui: panels::UiState,
    /// Saved models read from the world's folder (My Models), by path.
    saved_models:
        std::collections::HashMap<String, Result<sa_scene::saved_model::SavedModel, String>>,
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
            autosave_off: false,
            ui: panels::UiState::default(),
            saved_models: std::collections::HashMap::new(),
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
        self.saved_models.clear();
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
        script_ui::flush(self);
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
        script_ui::flush(self);
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
        script_ui::flush(self);
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
        if self.autosave_off
            || !self.history.dirty()
            || self.history.revision == self.autosaved_revision
        {
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
        let world = self.doc.layout.world_transforms();
        for node in &self.doc.layout.nodes {
            if node.asset.is_none() {
                continue;
            }
            if !self.doc.layout.effective_collision(node.id) {
                continue;
            }
            match sa_scene::content_pack::node_model(node, &mut self.resolver) {
                Ok(model) => {
                    let (_, solid) = model.transformed(gpu::render_matrix(&world[&node.id]));
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
        if !build.playtesting {
            // However the playtest ended, objects are drawn where placed,
            // in their own colours.
            build.gpu.moved.clear();
            build.gpu.tints.clear();
            build.gpu.copies.clear();
        }
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
                // Objects scripts moved are back where they were placed.
                build.gpu.moved.clear();
                build.info("Back in Build Mode. Your edits are as you left them.");
            }
            return;
        }
        let Some(build) = &mut self.build else { return };
        script_ui::flush(build);
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
        let play = crate::play_logic::PlayLogic::playtest(&build.doc.layout.resolved());
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

/// An object of the smoke world by name: (id, position).
fn smoke_node(state: &State, name: &str) -> (NodeId, [f32; 3]) {
    state
        .build
        .as_ref()
        .and_then(|b| b.doc.layout.nodes.iter().find(|n| n.name == name))
        .map(|n| (n.id, n.transform.position))
        .unwrap_or_else(|| panic!("the smoke world has no {name} (use content/gameplay-sample)"))
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
        if let Some(build) = &mut self.build {
            build.autosave_off = true;
        }
        let build = self
            .build
            .as_ref()
            .expect("--smoke-build needs --build <world>");
        let crate::play_logic::SmokeTargets { gate, door, night } =
            crate::play_logic::smoke_targets(&build.doc.layout);
        match frame {
            5 => {
                let build = self.build.as_mut().unwrap();
                // An autosave left by someone's own session: keep it (the
                // file stays), but do not let its question cover the shots.
                build.recovery = None;
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
                // Lamps light the night per pixel (the default setting).
                assert!(
                    !self.lit_lamps.is_empty() && !self.point_lights(32).is_empty(),
                    "the sample's lamps do not light the night per pixel"
                );
                capture(self, "playtest-night-zone.png");
            }
            // World scripts: E at the bell, and its script hides the
            // signal crate and tells everyone.
            54 => {
                let bell = smoke_node(self, "Bell");
                self.smoke_stand_at(bell.1);
            }
            56 => assert!(self.interact_play_logic(), "E did nothing at the bell"),
            58 => {
                let crate_id = smoke_node(self, "Signal crate").0;
                let play = self.play_logic.as_ref().unwrap();
                assert!(
                    !play.logic.visible(crate_id),
                    "the bell's script did not hide the signal crate"
                );
                assert!(
                    play.message().is_some_and(|m| m.ends_with("rang the bell")),
                    "the bell's script sent no message: {:?}",
                    play.message()
                );
                assert_eq!(
                    play.logic.shared().hud().get("bell").map(String::as_str),
                    Some("Bell rung 1 time"),
                    "the bell's script put no text on screen"
                );
                let bell = smoke_node(self, "Bell").0;
                assert!(
                    play.logic.shared().colors().contains_key(&bell)
                        && self.build.as_ref().unwrap().gpu.tints.contains_key(&bell),
                    "the bell's script did not colour it"
                );
                // ...and left a copy of the crate 2 m north of it: drawn
                // and solid there.
                let signal = smoke_node(self, "Signal crate");
                let copies: Vec<_> = play.logic.shared().copies().iter().collect();
                assert_eq!(
                    copies.len(),
                    1,
                    "the bell's script made no copy: {copies:?}"
                );
                let (copy, source) = (*copies[0].0, *copies[0].1);
                assert_eq!(source, signal.0, "the copy is not of the signal crate");
                assert!(play.logic.visible(copy), "the copy is hidden");
                assert!(
                    self.build.as_ref().unwrap().gpu.objects.contains_key(&copy),
                    "the copy is not drawn"
                );
                let above =
                    crate::play_logic::blender_to_render([signal.1[0], signal.1[1] + 2.0, 10.0]);
                let collision = self.collision.as_ref().unwrap();
                assert!(
                    collision.has_switchable(copy)
                        && collision
                            .ground_below(above, above.y)
                            .is_some_and(|y| y > 0.1),
                    "the copy is not solid where it stands"
                );
                let build = self.build.as_ref().unwrap();
                let errors: Vec<&str> = build
                    .log
                    .iter()
                    .map(|l| l.text.as_str())
                    .filter(|t| t.starts_with("Script error"))
                    .collect();
                assert!(errors.is_empty(), "script errors: {errors:?}");
                // The lift's script has been moving it since the playtest
                // began: drawn and solid higher up.
                let lift = smoke_node(self, "Lift");
                let risen = play
                    .logic
                    .shared()
                    .moved()
                    .find(|(id, _)| *id == lift.0)
                    .map(|(_, pose)| pose.position[2]);
                assert!(
                    risen.is_some_and(|z| z > 0.0),
                    "the lift's script did not move it: {risen:?}"
                );
                assert!(
                    build.gpu.moved.contains_key(&lift.0),
                    "the moved lift is not drawn where it is"
                );
                let top = crate::play_logic::blender_to_render([lift.1[0], lift.1[1], 10.0]);
                let ground = self
                    .collision
                    .as_ref()
                    .unwrap()
                    .ground_below(top, top.y)
                    .unwrap_or(0.0);
                let base = crate::play_logic::blender_to_render([lift.1[0], lift.1[1], 1.0]).y;
                assert!(
                    ground > base + 0.001,
                    "the moved lift is not solid where it is ({ground} vs {base})"
                );
                capture(self, "playtest-script-bell.png");
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
            // A player standing on the lift rides along when it moves.
            74 => {
                let lift = smoke_node(self, "Lift").0;
                let play = self.play_logic.as_ref().unwrap();
                let pose = play.logic.shared().pose(play.logic.world(), lift).unwrap();
                let [x, y, z] = pose.position;
                self.smoke_stand_at([x, y, z + 1.0]);
            }
            76 => {
                use sa_scene::world_logic::{Pose, ScriptChange};
                let lift = smoke_node(self, "Lift").0;
                let feet = self.player.as_ref().unwrap().feet;
                assert_eq!(
                    self.collision.as_ref().unwrap().standing_on(feet),
                    Some(lift),
                    "the player is not standing on the lift"
                );
                let play = self.play_logic.as_mut().unwrap();
                let pose = play.logic.shared().pose(play.logic.world(), lift).unwrap();
                let [x, y, z] = pose.position;
                play.logic.apply_script(ScriptChange::Move(
                    lift,
                    Pose {
                        position: [x, y, z + 2.0],
                        heading: pose.heading,
                    },
                    0.0,
                ));
                self.sync_moved_objects();
                let risen = self.player.as_ref().unwrap().feet.y - feet.y;
                assert!(
                    (risen - 2.0).abs() < 0.05,
                    "the lift did not carry the player up 2 m ({risen})"
                );
            }
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
            // A road along points: placed, drawn, and rebuilt when a point
            // moves.
            92 => {
                let build = self.build.as_mut().unwrap();
                let at = sa_scene::world_layout::Transform {
                    position: [0.0, -40.0, 0.0],
                    ..Default::default()
                };
                let (command, id) = sa_editor::ops::place(
                    &mut build.doc,
                    AssetRef::Builtin("spline-road".into()),
                    "Road",
                    at,
                    None,
                )
                .expect("placing a road");
                build.execute(command);
                build.selection = vec![id];
                build.focus_selection();
            }
            96 => {
                let build = self.build.as_mut().unwrap();
                let id = build.selection[0];
                let before = build.gpu.objects[&id].bounds;
                let triangles = build.gpu.objects[&id].triangles.len();
                build.ui.smoke_road = Some((id, before.1.y, triangles));
                assert!(
                    (before.1 - before.0).x > 40.0,
                    "the road mesh is not along its points: {before:?}"
                );
                build.preview("Move road point", &[id], |n| {
                    if let Some(road) = &mut n.road {
                        road.points[2][2] = 6.0;
                        // And joined back to its start, as a loop.
                        road.closed = true;
                    }
                });
                build.commit_preview();
            }
            100 => {
                let build = self.build.as_ref().unwrap();
                let (id, before, open) = build.ui.smoke_road.unwrap();
                let after = build.gpu.objects[&id].bounds.1.y;
                assert!(
                    after > before + 5.0,
                    "raising a road point did not rebuild the road ({before} -> {after})"
                );
                let looped = build.gpu.objects[&id].triangles.len();
                assert!(
                    looped > open,
                    "closing the road into a loop did not add its last side ({open} -> {looped})"
                );
                capture(self, "build-road.png");
            }
            // Terrain paint and holes: sand around a hole, fewer triangles.
            102 => {
                let build = self.build.as_mut().unwrap();
                let (hill, _) = build.ui.smoke_terrain.unwrap();
                let before = build.gpu.objects[&hill].triangles.len();
                build.ui.smoke_paint = Some((hill, before));
                build.selection = vec![hill];
                build.focus_selection();
                build.preview("Paint terrain", &[hill], |n| {
                    if let Some(terrain) = &mut n.terrain {
                        sa_scene::terrain::paint(terrain, glam::Vec2::ZERO, 14.0, 3);
                        sa_scene::terrain::paint(
                            terrain,
                            glam::Vec2::ZERO,
                            5.0,
                            sa_scene::world_layout::Terrain::HOLE,
                        );
                    }
                });
                build.commit_preview();
            }
            106 => {
                let build = self.build.as_ref().unwrap();
                let (hill, before) = build.ui.smoke_paint.unwrap();
                let after = build.gpu.objects[&hill].triangles.len();
                assert!(
                    after < before,
                    "the hole did not remove terrain triangles ({before} -> {after})"
                );
                capture(self, "build-terrain-paint.png");
            }
            // The script editor: the bell's script, checked as it is typed.
            108 => {
                let bell = smoke_node(self, "Bell").0;
                let build = self.build.as_mut().unwrap();
                build.selection = vec![bell];
                build.focus_selection();
                script_ui::open(build, script_ui::ScriptAt::Object(bell, 0));
            }
            114 => {
                let build = self.build.as_ref().unwrap();
                assert!(build.ui.script.is_some(), "the script editor did not open");
                capture(self, "build-script-editor.png");
            }
            // A world look: the same scene graded as Noir.
            118 => {
                let build = self.build.as_mut().unwrap();
                build.ui.script = None;
                build.selection.clear();
                build.reset_camera();
                build.doc.layout.settings.look = sa_scene::world_layout::WorldLook::presets()
                    .into_iter()
                    .find(|(name, _)| *name == "Noir")
                    .map(|(_, look)| look);
            }
            122 => {
                assert!(self.world_look().is_some(), "the world look is not used");
                capture(self, "build-look-noir.png");
            }
            124 => self.build.as_mut().unwrap().doc.layout.settings.look = None,
            // Parts: every shape, sized, coloured and given a material.
            126 => {
                use sa_scene::world_layout::{PartMaterial, Transform};
                let build = self.build.as_mut().unwrap();
                let parts = [
                    (
                        "part-block",
                        [8.0, 0.5, 3.0],
                        [0.72, 0.26, 0.18],
                        PartMaterial::Brick,
                        0.0,
                    ),
                    (
                        "part-block",
                        [3.0, 0.2, 2.0],
                        [0.55, 0.75, 0.9],
                        PartMaterial::Glass,
                        0.0,
                    ),
                    (
                        "part-cylinder",
                        [1.0, 1.0, 4.0],
                        [0.5, 0.33, 0.18],
                        PartMaterial::Wood,
                        0.0,
                    ),
                    (
                        "part-ball",
                        [2.0, 2.0, 2.0],
                        [0.96, 0.8, 0.18],
                        PartMaterial::Plastic,
                        0.0,
                    ),
                    (
                        "part-wedge",
                        [4.0, 3.0, 1.5],
                        [0.62, 0.62, 0.6],
                        PartMaterial::Concrete,
                        0.0,
                    ),
                    (
                        "part-corner-wedge",
                        [2.0, 2.0, 2.0],
                        [0.13, 0.3, 0.75],
                        PartMaterial::Metal,
                        0.0,
                    ),
                    (
                        "part-block",
                        [4.0, 4.0, 0.2],
                        [0.95, 0.95, 0.93],
                        PartMaterial::Tiles,
                        0.4,
                    ),
                ];
                let mut ids = Vec::new();
                for (i, (name, size, color, material, transparency)) in
                    parts.into_iter().enumerate()
                {
                    let at = Transform {
                        position: [-14.0 + i as f32 * 5.0, 36.0, 0.0],
                        ..Default::default()
                    };
                    let (command, id) = sa_editor::ops::place(
                        &mut build.doc,
                        AssetRef::Builtin(name.into()),
                        "Part",
                        at,
                        None,
                    )
                    .expect("placing a part");
                    build.execute(command);
                    build.preview("Edit part", &[id], |n| {
                        let part = n.part.as_mut().expect("a placed part");
                        (part.size, part.color, part.material) = (size, color, material.clone());
                        part.transparency = transparency;
                    });
                    build.commit_preview();
                    ids.push(id);
                }
                build.ui.smoke_parts = ids;
            }
            // Framed once they are on the GPU.
            128 => {
                let build = self.build.as_mut().unwrap();
                build.selection = build.ui.smoke_parts.clone();
                build.focus_selection();
                build.selection.truncate(1);
            }
            132 => {
                let build = self.build.as_ref().unwrap();
                let ids = &build.ui.smoke_parts;
                let wall = &build.gpu.objects[&ids[0]];
                let size = wall.bounds.1 - wall.bounds.0;
                assert!(
                    (size.x - 8.0).abs() < 0.05 && (size.y - 3.0).abs() < 0.05,
                    "the brick wall is not drawn at its size: {size}"
                );
                assert!(
                    wall.batches
                        .iter()
                        .any(|b| b.texture_key == "builtin:material/brick"),
                    "the brick wall is not drawn with bricks"
                );
                let glass = &build.gpu.objects[&ids[1]];
                assert!(
                    glass.batches.iter().all(|b| b.alpha),
                    "glass is not see-through"
                );
                capture(self, "build-parts.png");
            }
            // The parts as one model: grouped, then turned a quarter about
            // its pivot; everything in it turns along.
            134 => {
                let build = self.build.as_mut().unwrap();
                let ids = build.ui.smoke_parts.clone();
                let (command, model) =
                    sa_editor::ops::group(&mut build.doc, &ids, "Parts model").expect("grouping");
                build.execute(command);
                let wall = build.gpu.objects[&ids[0]].bounds;
                build.ui.smoke_model = Some((model, wall));
                build.preview("Rotate", &[model], |n| n.transform.rotation[2] = 90.0);
                build.commit_preview();
                build.selection = vec![model];
            }
            138 => {
                let build = self.build.as_mut().unwrap();
                let (model, before) = build.ui.smoke_model.unwrap();
                let ids = build.ui.smoke_parts.clone();
                let after = build.gpu.objects[&ids[0]].bounds;
                let (was, now) = (before.1 - before.0, after.1 - after.0);
                // Render space: x east, z south. The 8 m wall now runs north.
                assert!(
                    (was.x - now.z).abs() < 0.1 && (was.z - now.x).abs() < 0.1,
                    "the wall did not turn with its model: {was} -> {now}"
                );
                assert!(build.doc.layout.node(model).is_some_and(|n| n.model));
                build.focus_selection();
            }
            142 => {
                capture(self, "build-model.png");
                // Played, the parts are where the model puts them.
                let played = self.build.as_ref().unwrap().doc.layout.resolved();
                let wall = self.build.as_ref().unwrap().ui.smoke_parts[0];
                let at = played.node(wall).unwrap().transform;
                assert_eq!(
                    at.rotation[2].round(),
                    90.0,
                    "the played wall is not turned: {at:?}"
                );
            }
            // Saved to My Models, placed again, changed there and saved to
            // every copy.
            144 => {
                let build = self.build.as_mut().unwrap();
                let (model, _) = build.ui.smoke_model.unwrap();
                build.save_to_my_models(model);
                let path = build.doc.layout.node(model).unwrap().saved_model.clone();
                let path = path.expect("the model is linked to its saved file");
                assert!(build.project.folder.join(&path).is_file());
                assert!(
                    build.catalog.find(&AssetRef::Own(path.clone())).is_some(),
                    "the saved model is not in the Toolbox"
                );
                let at = sa_scene::world_layout::Transform {
                    position: [-2.0, 56.0, 0.0],
                    ..Default::default()
                };
                let (command, copy) = build.place_saved(&path, at, None).expect("placing a copy");
                build.execute(command);
                build.ui.smoke_copy = Some(copy);
                build.ui.category = Some(sa_editor::catalog::MY_MODELS.into());
            }
            146 => {
                let build = self.build.as_mut().unwrap();
                let (model, _) = build.ui.smoke_model.unwrap();
                let copy = build.ui.smoke_copy.unwrap();
                let wall = build
                    .doc
                    .layout
                    .descendants(copy)
                    .into_iter()
                    .find(|id| build.doc.layout.node(*id).is_some_and(|n| n.part.is_some()))
                    .expect("a part in the copy");
                build.preview("Edit part", &[wall], |n| {
                    n.part.as_mut().expect("a part").color = [0.8, 0.1, 0.1];
                });
                build.commit_preview();
                build.save_to_all_copies(copy);
                let path = build
                    .doc
                    .layout
                    .node(copy)
                    .unwrap()
                    .saved_model
                    .clone()
                    .unwrap();
                let saved = build.saved_model(&path).expect("the saved model");
                assert!(
                    saved.matches(&build.doc.layout, model),
                    "the first copy was not updated"
                );
                assert!(saved.matches(&build.doc.layout, copy));
                build.selection = vec![model, copy];
                build.focus_selection();
                build.selection = vec![copy];
            }
            150 => {
                capture(self, "build-saved-models.png");
                // The smoke run leaves the sample world as it found it.
                let build = self.build.as_ref().unwrap();
                let copy = build.ui.smoke_copy.unwrap();
                let path = build
                    .doc
                    .layout
                    .node(copy)
                    .unwrap()
                    .saved_model
                    .clone()
                    .unwrap();
                let _ = std::fs::remove_file(build.project.folder.join(&path));
                let _ = std::fs::remove_dir(build.project.folder.join(sa_scene::saved_model::DIR));
            }
            // Attributes and tags on the copy, shown in Properties.
            152 => {
                let build = self.build.as_mut().unwrap();
                let copy = build.ui.smoke_copy.unwrap();
                build.preview("Attributes", &[copy], |n| {
                    use sa_scene::world_layout::Attribute;
                    n.attributes
                        .insert("Price".into(), Attribute::Number(250.0));
                    n.attributes
                        .insert("For sale".into(), Attribute::Bool(true));
                    n.attributes
                        .insert("Owner".into(), Attribute::Text("Nobody".into()));
                    n.tags = vec!["Shop".into(), "Market".into()];
                });
                build.commit_preview();
                let inside = build.doc.layout.descendants(copy)[0];
                let played = sa_scene::world_logic::script_objects(&build.doc.layout.resolved());
                let object = played
                    .iter()
                    .find(|o| o.id == inside)
                    .expect("an object in the copy");
                assert!(
                    object.tags.contains(&"Shop".to_string())
                        && object.attributes.contains_key("Price"),
                    "scripts do not see the model's tags and attributes on what is inside it"
                );
                build.selection = vec![copy];
            }
            156 => capture(self, "build-attributes.png"),
            // Arrange: three blocks lined up on the last one, spaced
            // evenly, then nudged.
            158 => {
                let build = self.build.as_mut().unwrap();
                let mut ids = Vec::new();
                for (x, y, z) in [(30.0, 70.0, 2.0), (34.0, 73.0, 0.0), (45.0, 71.0, 5.0)] {
                    let at = sa_scene::world_layout::Transform::at([x, y, z]);
                    let (command, id) = sa_editor::ops::place(
                        &mut build.doc,
                        AssetRef::Builtin("part-block".into()),
                        "Block",
                        at,
                        None,
                    )
                    .expect("placing a block");
                    build.execute(command);
                    ids.push(id);
                }
                build.selection = ids.clone();
                build.ui.smoke_parts = ids;
            }
            160 => {
                let build = self.build.as_mut().unwrap();
                build.align(2, arrange::Edge::Min);
                build.align(1, arrange::Edge::Center);
            }
            162 => {
                let build = self.build.as_mut().unwrap();
                let ids = build.ui.smoke_parts.clone();
                for id in &ids {
                    let p = build.doc.layout.world_transform(*id).position;
                    assert!(
                        (p[2] - 5.0).abs() < 0.01 && (p[1] - 71.0).abs() < 0.01,
                        "block {id} is not lined up with the last one: {p:?}"
                    );
                }
                build.distribute(0);
                let x: Vec<f32> = ids
                    .iter()
                    .map(|id| build.doc.layout.world_transform(*id).position[0])
                    .collect();
                assert!((x[1] - 37.5).abs() < 0.01, "not spaced evenly: {x:?}");
                build.snap.enabled = true;
                build.snap.step = 1.0;
                build.nudge(glam::Vec3::Z);
                let z = build.doc.layout.world_transform(ids[0]).position[2];
                assert!(
                    (z - 6.0).abs() < 0.01,
                    "nudge up did not move one step: {z}"
                );
                build.undo();
            }
            // The command bar: paint the three blocks and mark them.
            164 => {
                let build = self.build.as_mut().unwrap();
                build.selection = build.ui.smoke_parts.clone();
                build.ui.bottom_open = true;
                build.run_command(
                    "for _, o in ipairs(selection()) do o.color = rgb(0.2, 0.4, 1) o:set('Row', true) end print('painted', #selection())",
                );
                for id in build.ui.smoke_parts.clone() {
                    let node = build.doc.layout.node(id).unwrap();
                    assert_eq!(node.part.as_ref().unwrap().color, [0.2, 0.4, 1.0]);
                    assert!(node.attributes.contains_key("Row"));
                }
                assert!(
                    build.log.iter().any(|l| l.text == "painted  3"),
                    "the command's print is not in Output"
                );
                build.focus_selection();
            }
            168 => capture(self, "build-command.png"),
            170 => return true,
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
