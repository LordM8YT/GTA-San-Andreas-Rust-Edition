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
pub mod gpu;
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
    pub camera: EditorCamera,
    pub placing: Option<Placing>,
    pub playtesting: bool,
    pub log: Vec<LogLine>,
    pub recovery: Option<Recovery>,
    pub requests: Vec<Request>,
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
            camera: EditorCamera::default(),
            placing: None,
            playtesting: false,
            log: Vec::new(),
            recovery,
            requests: Vec::new(),
            preview_before: Vec::new(),
            preview_label: String::new(),
            drag: None,
            base_collision,
            base_pick,
            base_bounds,
            last_autosave: Instant::now(),
            autosaved_revision: 0,
            ui: panels::UiState::default(),
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
    fn playtest_collision(&mut self) -> sa_scene::collision::CollisionWorld {
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
            let Some(asset) = &node.asset else { continue };
            if !self.doc.layout.effective_collision(node.id) {
                continue;
            }
            match self.resolver.model(asset) {
                Ok(model) => {
                    let (_, solid) = model.transformed(gpu::render_matrix(&node.transform));
                    batches.extend(solid);
                }
                Err(error) => problems.push(format!("{}: {error:#}", node.name)),
            }
        }
        for problem in problems {
            self.warn(format!("No collision for {problem}"));
        }
        sa_scene::collision::CollisionWorld::from_batches(&batches)
    }
}

impl State {
    /// Build Mode is open and not playtesting.
    pub(crate) fn build_mode_editing(&self) -> bool {
        self.build.as_ref().is_some_and(|b| !b.playtesting)
    }
    /// Before the game update: the editor camera drives the view.
    pub(crate) fn build_mode_begin_frame(&mut self) {
        let Some(build) = &mut self.build else { return };
        if let Some(hour) = build.doc.layout.settings.hour {
            self.daycycle.minutes = f32::from(hour.min(23)) * 60.0;
        }
        if build.playtesting {
            return;
        }
        build.autosave_if_due();
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
        let mut problems = Vec::new();
        build
            .gpu
            .sync(self, &mut build.resolver, &build.doc.layout, &mut problems);
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
        self.build = Some(build);
        for request in requests {
            match request {
                Request::Playtest => self.toggle_playtest(),
                Request::Quit => self.quit_requested = true,
            }
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
        let spawn = build.spawn_render() + Vec3::Y * 2.0;
        let player = sa_scene::collision::Player::spawn(&collision, spawn);
        let feet = build.spawn_render();
        let standing = collision.standing_at(feet, feet.y + 2.0).is_some();
        build.playtesting = true;
        build.info("Playtest started. Press F6 to return to Build Mode.");
        if !standing {
            build.warn("Nothing solid under the spawn point: the player may fall. Move the spawn or add ground.");
        }
        let yaw = build.camera.yaw;
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
