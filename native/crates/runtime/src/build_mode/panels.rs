//! The Build Mode interface, laid out like the building editors creators
//! already know: a menu and a ribbon of tabs (Home, Model, Test, View) on
//! top, the Toolbox (assets) on the left, the Explorer above the Properties
//! on the right, Output and the status bar at the bottom and the viewport in
//! the middle. Every dock can be closed and opened again from the View tab.
use super::{viewport, BuildMode, Level, Placing, Request, Tool};
use crate::ui_theme::{ACCENT, BACKGROUND, MUTED, SUCCESS, SURFACE, SURFACE_HIGH, TEXT};
use egui::{Color32, Key, KeyboardShortcut, Modifiers, RichText, Ui};
use sa_client::settings::EditorPerformance;
use sa_editor::catalog::AssetStatus;
use sa_editor::history::{Command, WorldInfo};
use sa_editor::ops;
use sa_scene::world_layout::{AssetRef, Node, NodeId, Transform, WorldSettings};
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

pub(super) const WARNING: Color32 = Color32::from_rgb(232, 170, 70);
pub(super) const ERROR: Color32 = Color32::from_rgb(232, 96, 86);

/// The ribbon tab shown under the menu.
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum RibbonTab {
    #[default]
    Home,
    Model,
    Test,
    View,
}
/// What the Properties show with nothing selected: the Explorer's
/// Workspace row (the world itself) or its Lighting row (environment).
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum WorldPage {
    #[default]
    Workspace,
    Lighting,
    /// Every script in the world.
    Scripts,
}
#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum BottomTab {
    #[default]
    Console,
    Problems,
    Assets,
    Playtest,
}

#[derive(Default)]
pub struct UiState {
    ribbon: RibbonTab,
    pub world_page: WorldPage,
    /// The script open in the script editor.
    pub script: Option<super::script_ui::Editor>,
    /// Docks closed with their close button (the View tab opens them again).
    hide_toolbox: bool,
    hide_explorer: bool,
    hide_properties: bool,
    /// The Explorer's Workspace row is folded.
    workspace_collapsed: bool,
    /// Objects copied or cut, as they were then (see [`ops::copy`]).
    clipboard: Vec<Node>,
    /// The clipboard was cut: the first paste puts the objects back as they
    /// were, with their own ids.
    clipboard_cut: bool,
    /// Copies pasted from this clipboard so far; each lands one step on.
    pastes: u32,
    /// How heavily the editor draws (the player's setting, written back
    /// when changed here).
    pub performance: EditorPerformance,
    /// The editor draws light right now (for the status line).
    pub light: bool,
    bottom: BottomTab,
    pub bottom_open: bool,
    asset_search: String,
    pub category: Option<String>,
    outliner_search: String,
    collapsed: HashSet<NodeId>,
    renaming: Option<(NodeId, String)>,
    name_edit: Option<(NodeId, String)>,
    save_as: Option<String>,
    import: Option<(String, String)>,
    world_name: Option<String>,
    world_description: Option<String>,
    settings_before: Option<WorldSettings>,
    show_help: bool,
    confirm_quit: bool,
    /// Placement preview transform this frame, set by the viewport.
    pub ghost_transform: Option<Transform>,
    /// The right mouse button is flying the camera (WASD belong to it).
    pub flying: bool,
    /// Group new objects are placed into.
    pub place_into: Option<NodeId>,
    /// New world dialog: the name typed so far.
    new_world: Option<String>,
    /// Open world dialog: the worlds found, by folder.
    open_world: Option<Vec<(std::path::PathBuf, String)>>,
    /// Another world to open once unsaved changes are dealt with.
    confirm_switch: Option<std::path::PathBuf>,
    /// Viewport: draw the bounds of solid objects.
    pub show_collision: bool,
    /// Viewport: leave out gameplay zones and links.
    pub hide_gameplay: bool,
    /// A Sculpt stroke is going on: the height Flatten keeps.
    pub sculpt_stroke: Option<f32>,
    /// Clicks in the viewport add points to this road.
    pub road_draw: Option<NodeId>,
    /// A road point is being dragged: the road and the point.
    pub road_drag: Option<(NodeId, usize)>,
    /// `--smoke-build`: the terrain sculpted and its height before.
    pub smoke_terrain: Option<(NodeId, f32)>,
    /// `--smoke-build`: the road placed and its top before a point rose.
    pub smoke_road: Option<(NodeId, f32, usize)>,
    pub smoke_parts: Vec<NodeId>,
    pub smoke_model: Option<(NodeId, (glam::Vec3, glam::Vec3))>,
    /// `--smoke-build`: the saved model's copy.
    pub smoke_copy: Option<NodeId>,
    /// `--smoke-build`: when the physics playtest started, and the
    /// lifted block's height last frame.
    pub smoke_fall: Option<(std::time::Instant, f32)>,
    /// `--smoke-build`: the frame the physics shot was taken.
    pub smoke_shot: Option<u64>,
    /// A new attribute or tag being typed in the Properties panel.
    pub attributes: super::attributes_ui::Draft,
    /// The command bar under Output.
    pub command: super::command::CommandLine,
    /// `--smoke-build`: the painted terrain and its triangles before a hole.
    pub smoke_paint: Option<(NodeId, usize)>,
    /// Asset Browser pictures; None when one could not be made.
    pub thumbnails: HashMap<AssetRef, Option<egui::TextureHandle>>,
}

fn compact(ui: &mut Ui) {
    let style = ui.style_mut();
    style.spacing.item_spacing = egui::vec2(6.0, 4.0);
    style.spacing.button_padding = egui::vec2(8.0, 3.0);
    style.spacing.interact_size.y = 22.0;
    for (kind, size) in [
        (egui::TextStyle::Body, 13.5),
        (egui::TextStyle::Button, 13.5),
        (egui::TextStyle::Small, 11.5),
        (egui::TextStyle::Heading, 16.0),
    ] {
        style
            .text_styles
            .insert(kind, egui::FontId::proportional(size));
    }
}

/// A category bar in the Properties.
pub(super) fn section(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    let (rect, _) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, SURFACE_HIGH);
    ui.painter().text(
        rect.left_center() + egui::vec2(6.0, 0.0),
        egui::Align2::LEFT_CENTER,
        title,
        egui::FontId::proportional(12.5),
        ACCENT,
    );
    ui.add_space(2.0);
}

/// The whole editor for one frame.
pub fn draw(build: &mut BuildMode, root: &mut Ui, frame: &viewport::Frame) {
    compact(root);
    shortcuts(build, root);
    egui::Panel::top("build-top")
        .frame(
            egui::Frame::new()
                .fill(BACKGROUND)
                .inner_margin(egui::Margin::symmetric(8, 4)),
        )
        .show(root, |ui| {
            compact(ui);
            menu_bar(build, ui);
            ribbon(build, ui);
        });
    egui::Panel::bottom("build-status")
        .exact_size(24.0)
        .frame(
            egui::Frame::new()
                .fill(BACKGROUND)
                .inner_margin(egui::Margin::symmetric(8, 3)),
        )
        .show(root, |ui| {
            compact(ui);
            status_bar(build, ui);
        });
    let mut bottom_open = build.ui.bottom_open;
    egui::Panel::bottom("build-log")
        .resizable(true)
        .default_size(170.0)
        .min_size(90.0)
        .frame(egui::Frame::new().fill(SURFACE).inner_margin(6))
        .show_collapsible(root, &mut bottom_open, |ui| {
            compact(ui);
            bottom(build, ui);
        });
    build.ui.bottom_open = bottom_open;
    if !build.ui.hide_toolbox {
        egui::Panel::left("build-left")
            .resizable(true)
            .default_size(270.0)
            .frame(egui::Frame::new().fill(SURFACE).inner_margin(8))
            .show(root, |ui| {
                compact(ui);
                toolbox(build, ui);
            });
    }
    let explorer_on = !build.ui.hide_explorer;
    let properties_on = !build.ui.hide_properties;
    if explorer_on || properties_on {
        egui::Panel::right("build-right")
            .resizable(true)
            .default_size(310.0)
            .frame(egui::Frame::new().fill(SURFACE).inner_margin(8))
            .show(root, |ui| {
                compact(ui);
                if explorer_on && properties_on {
                    // Explorer above, Properties below; drag the line between.
                    egui::Panel::top("build-explorer")
                        .resizable(true)
                        .default_size(300.0)
                        .size_range(120.0..=1000.0)
                        .frame(
                            egui::Frame::new()
                                .fill(SURFACE)
                                .inner_margin(egui::Margin::symmetric(0, 4)),
                        )
                        .show(ui, |ui| {
                            compact(ui);
                            explorer(build, ui);
                        });
                    properties(build, ui);
                } else if explorer_on {
                    explorer(build, ui);
                } else {
                    properties(build, ui);
                }
            });
    }
    egui::CentralPanel::no_frame().show(root, |ui| {
        viewport::show(build, ui, frame);
        if let Some(placing) = build.placing.as_ref().filter(|p| p.dragged) {
            hint(ui, &format!("Drop {} in the world", placing.label));
        } else if let Some(placing) = &build.placing {
            let text = format!(
                "Placing {}: click to place, keep clicking for more, [ ] turn, right-click or Esc to stop",
                placing.label
            );
            hint(ui, &text);
        }
    });
    dropped_files(build, root);
    dialogs(build, root);
}

/// Models dragged onto the window from the file explorer are imported into
/// this world, and the last one is ready to place.
fn dropped_files(build: &mut BuildMode, root: &mut Ui) {
    let (hovering, dropped) = root.input(|i| {
        (
            !i.raw.hovered_files.is_empty(),
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect::<Vec<_>>(),
        )
    });
    if hovering {
        let rect = root.ctx().content_rect();
        let painter = root.ctx().layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("build drop"),
        ));
        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(150));
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Drop .glb models to add them to this world",
            egui::FontId::proportional(22.0),
            TEXT,
        );
    }
    let mut last = None;
    for path in dropped {
        match build.project.import_model(&path, "imported") {
            Ok(asset) => {
                build.info(format!("Imported {asset}"));
                let label = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                last = Some((asset, label));
            }
            Err(error) => build.error(format!("Could not import {}: {error:#}", path.display())),
        }
    }
    if let Some((asset, label)) = last {
        build.refresh_catalog();
        build.ui.hide_toolbox = false;
        build.selection.clear();
        build.placing = Some(Placing {
            asset,
            label,
            turn: 0.0,
            dragged: false,
        });
    }
}

fn hint(ui: &mut Ui, text: &str) {
    let rect = ui.max_rect();
    egui::Area::new("build-hint".into())
        .fixed_pos(rect.left_top() + egui::vec2(10.0, 10.0))
        .interactable(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(SURFACE.gamma_multiply(0.92))
                .show(ui, |ui| {
                    ui.label(RichText::new(text).color(TEXT).size(13.0));
                });
        });
}

fn shortcuts(build: &mut BuildMode, ui: &mut Ui) {
    let typing = ui.ctx().egui_wants_keyboard_input();
    let ctrl = |key| KeyboardShortcut::new(Modifiers::COMMAND, key);
    let ctrl_shift = |key| KeyboardShortcut::new(Modifiers::COMMAND | Modifiers::SHIFT, key);
    let consume = |shortcut| ui.input_mut(|i| i.consume_shortcut(&shortcut));
    if consume(ctrl(Key::S)) {
        build.save();
    }
    if typing {
        return;
    }
    if consume(ctrl_shift(Key::Z)) || consume(ctrl(Key::Y)) {
        build.redo();
    } else if consume(ctrl(Key::Z)) {
        build.undo();
    }
    if consume(ctrl(Key::D)) {
        duplicate(build);
    }
    if consume(ctrl(Key::N)) {
        build.ui.new_world = Some(String::new());
    }
    if consume(ctrl(Key::O)) {
        build.ui.open_world = Some(worlds(build));
    }
    if consume(ctrl_shift(Key::G)) {
        ungroup(build);
    } else if consume(ctrl(Key::G)) {
        group(build);
    }
    // Ctrl+1..4 pick the tools, as in other building editors.
    for (key, tool) in [
        (Key::Num1, Tool::Select),
        (Key::Num2, Tool::Move),
        (Key::Num3, Tool::Scale),
        (Key::Num4, Tool::Rotate),
    ] {
        if consume(ctrl(key)) {
            build.tool = tool;
        }
    }
    if consume(ctrl(Key::L)) {
        build.local = !build.local;
    }
    // Copy, cut and paste arrive as clipboard events, not key presses.
    // Text selected in a label (an Output line) is copied as text instead.
    let label_text = ui
        .ctx()
        .plugin_opt::<egui::text_selection::LabelSelectionState>()
        .is_some_and(|state| state.lock().has_selection());
    let (copy, cut, paste_now) = ui.input(|i| {
        i.events
            .iter()
            .fold((false, false, false), |(c, x, v), e| match e {
                egui::Event::Copy => (true, x, v),
                egui::Event::Cut => (c, true, v),
                egui::Event::Paste(_) => (c, x, true),
                _ => (c, x, v),
            })
    });
    if copy && !label_text {
        copy_selection(build, ui.ctx());
    }
    if cut && !label_text {
        cut_selection(build, ui.ctx());
    }
    if paste_now {
        paste(build);
    }
    // F5 plays; F6 (handled by the game loop) comes back.
    let play = ui.input(|i| {
        i.modifiers.is_none()
            && i.events.iter().any(|e| {
                matches!(
                    e,
                    egui::Event::Key {
                        key: Key::F5,
                        pressed: true,
                        repeat: false,
                        ..
                    }
                )
            })
    });
    if play {
        build.requests.push(Request::Playtest);
    }
    let pressed = |ui: &mut Ui, key| ui.input(|i| i.key_pressed(key) && i.modifiers.is_none());
    if pressed(ui, Key::Delete) || pressed(ui, Key::Backspace) {
        delete(build);
    }
    if pressed(ui, Key::Escape) {
        if build.ui.road_draw.is_some() {
            build.ui.road_draw = None;
        } else if build.placing.is_some() {
            build.placing = None;
        } else {
            build.selection.clear();
        }
    }
    if build.ui.flying {
        return; // WASD/QE fly the camera.
    }
    for (key, tool) in [
        (Key::Q, Tool::Select),
        (Key::W, Tool::Move),
        (Key::E, Tool::Rotate),
        (Key::R, Tool::Scale),
        (Key::T, Tool::Sculpt),
    ] {
        if pressed(ui, key) {
            build.tool = tool;
        }
    }
    if pressed(ui, Key::F) {
        build.focus_selection();
    }
    if pressed(ui, Key::Home) {
        build.reset_camera();
    }
    if pressed(ui, Key::G) {
        build.snap.enabled = !build.snap.enabled;
    }
    if pressed(ui, Key::L) {
        build.local = !build.local;
    }
    // Arrow keys nudge the selection (not while placing or orbiting).
    if build.placing.is_none() && !build.selection.is_empty() {
        for (key, forward, sign) in [
            (Key::ArrowUp, true, 1.0),
            (Key::ArrowDown, true, -1.0),
            (Key::ArrowRight, false, 1.0),
            (Key::ArrowLeft, false, -1.0),
        ] {
            if pressed(ui, key) {
                let direction = build.arrow_direction(forward, sign);
                build.nudge(direction);
            }
        }
        if pressed(ui, Key::PageUp) {
            build.nudge(glam::Vec3::Z);
        }
        if pressed(ui, Key::PageDown) {
            build.nudge(-glam::Vec3::Z);
        }
    }
    if let Some(placing) = &mut build.placing {
        let step = build.snap.angle.max(1.0);
        if pressed(ui, Key::OpenBracket) {
            placing.turn = (placing.turn - step).rem_euclid(360.0);
        }
        if pressed(ui, Key::CloseBracket) {
            placing.turn = (placing.turn + step).rem_euclid(360.0);
        }
    }
}

fn menu_bar(build: &mut BuildMode, ui: &mut Ui) {
    egui::MenuBar::new().ui(ui, |ui| {
        ui.label(RichText::new("SARE").strong().size(18.0).color(TEXT));
        ui.label(RichText::new("Build Mode").color(ACCENT));
        ui.separator();
        ui.menu_button("File", |ui| {
            if ui
                .add(egui::Button::new("New world…").shortcut_text("Ctrl+N"))
                .clicked()
            {
                build.ui.new_world = Some(String::new());
            }
            if ui
                .add(egui::Button::new("Open world…").shortcut_text("Ctrl+O"))
                .clicked()
            {
                build.ui.open_world = Some(worlds(build));
            }
            ui.separator();
            if ui
                .add(egui::Button::new("Save").shortcut_text("Ctrl+S"))
                .clicked()
            {
                build.save();
            }
            if ui.button("Save As…").clicked() {
                build.ui.save_as = Some(format!("{} copy", build.doc.info.name));
            }
            ui.separator();
            if ui.button("Import model (.glb)…").clicked() {
                // The system's file picker first; the window then asks for
                // the category (and still takes a typed path).
                if let Some(path) = pick_model() {
                    build.ui.import = Some((path, "custom".into()));
                }
            }
            if ui.button("Reload assets").clicked() {
                build.gpu.clear();
                build.resolver.clear();
                build.ui.thumbnails.clear();
                build.refresh_catalog();
            }
            ui.separator();
            if ui
                .add(egui::Button::new("Quit").shortcut_text("Alt+F4"))
                .clicked()
            {
                if build.history.dirty() {
                    build.ui.confirm_quit = true;
                } else {
                    build.requests.push(Request::Quit);
                }
            }
        });
        ui.menu_button("Edit", |ui| {
            let undo = build
                .history
                .undo_label()
                .map_or("Undo".into(), |l| format!("Undo {l}"));
            if ui
                .add_enabled(
                    build.history.can_undo(),
                    egui::Button::new(undo).shortcut_text("Ctrl+Z"),
                )
                .clicked()
            {
                build.undo();
            }
            let redo = build
                .history
                .redo_label()
                .map_or("Redo".into(), |l| format!("Redo {l}"));
            if ui
                .add_enabled(
                    build.history.can_redo(),
                    egui::Button::new(redo).shortcut_text("Ctrl+Y"),
                )
                .clicked()
            {
                build.redo();
            }
            ui.separator();
            let any = !build.selection.is_empty();
            if ui
                .add_enabled(any, egui::Button::new("Copy").shortcut_text("Ctrl+C"))
                .clicked()
            {
                copy_selection(build, ui.ctx());
            }
            if ui
                .add_enabled(any, egui::Button::new("Cut").shortcut_text("Ctrl+X"))
                .clicked()
            {
                cut_selection(build, ui.ctx());
            }
            if ui
                .add_enabled(
                    !build.ui.clipboard.is_empty(),
                    egui::Button::new("Paste").shortcut_text("Ctrl+V"),
                )
                .clicked()
            {
                paste(build);
            }
            if ui
                .add_enabled(any, egui::Button::new("Duplicate").shortcut_text("Ctrl+D"))
                .clicked()
            {
                duplicate(build);
            }
            if ui
                .add_enabled(any, egui::Button::new("Delete").shortcut_text("Del"))
                .clicked()
            {
                delete(build);
            }
            if ui
                .add_enabled(any, egui::Button::new("Group").shortcut_text("Ctrl+G"))
                .clicked()
            {
                group(build);
            }
            if ui
                .add_enabled(
                    any,
                    egui::Button::new("Ungroup").shortcut_text("Ctrl+Shift+G"),
                )
                .clicked()
            {
                ungroup(build);
            }
            if ui.button("Select all").clicked() {
                build.selection = build
                    .doc
                    .layout
                    .nodes
                    .iter()
                    .filter(|n| n.parent.is_none())
                    .map(|n| n.id)
                    .collect();
            }
        });
        ui.menu_button("View", |ui| {
            ui.checkbox(&mut build.snap.show_grid, "Show grid");
            if ui
                .add(egui::Button::new("Focus selection").shortcut_text("F"))
                .clicked()
            {
                build.focus_selection();
            }
            if ui
                .add(egui::Button::new("Reset camera").shortcut_text("Home"))
                .clicked()
            {
                build.reset_camera();
            }
            ui.add(
                egui::Slider::new(&mut build.camera.speed, 1.0..=400.0)
                    .logarithmic(true)
                    .text("Fly speed m/s"),
            );
            ui.separator();
            dock_toggles(build, ui);
        });
        ui.menu_button("Help", |ui| {
            if ui.button("Controls").clicked() {
                build.ui.show_help = true;
            }
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new("Create. Build. Play.")
                    .color(ACCENT)
                    .italics(),
            );
            let dirty = if build.history.dirty() { " •" } else { "" };
            ui.label(RichText::new(format!("{}{dirty}", build.doc.info.name)).color(MUTED));
        });
    });
}

/// Height of a ribbon button: icon above, label below.
const RIBBON_BUTTON: f32 = 50.0;

/// A large ribbon button: icon above its label.
fn big_button(
    ui: &mut Ui,
    icon: &str,
    label: &str,
    selected: bool,
    enabled: bool,
    hover: &str,
) -> egui::Response {
    let font = egui::FontId::proportional(11.5);
    let width = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), TEXT)
        .size()
        .x
        .max(34.0)
        + 14.0;
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, RIBBON_BUTTON), sense);
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if selected {
            painter.rect_filled(rect, 4.0, ACCENT.gamma_multiply(0.22));
            painter.rect_stroke(
                rect,
                4.0,
                egui::Stroke::new(1.0, ACCENT),
                egui::StrokeKind::Inside,
            );
        } else if enabled && response.hovered() {
            painter.rect_filled(rect, 4.0, SURFACE_HIGH);
        }
        let color = if !enabled {
            MUTED.gamma_multiply(0.5)
        } else if selected {
            ACCENT
        } else {
            TEXT
        };
        painter.text(
            rect.center_top() + egui::vec2(0.0, 17.0),
            egui::Align2::CENTER_CENTER,
            icon,
            egui::FontId::proportional(21.0),
            color,
        );
        painter.text(
            rect.center_bottom() - egui::vec2(0.0, 9.0),
            egui::Align2::CENTER_CENTER,
            label,
            font,
            color,
        );
    }
    response.on_hover_text(hover)
}

/// A small ribbon button, stacked three high beside the large ones.
fn small_button(ui: &mut Ui, icon: &str, label: &str, enabled: bool, hover: &str) -> bool {
    ui.add_enabled(
        enabled,
        egui::Button::new(RichText::new(format!("{icon} {label}")).size(12.0))
            .frame(false)
            .min_size(egui::vec2(0.0, 15.0)),
    )
    .on_hover_text(hover)
    .clicked()
}

/// One ribbon group: its buttons with the group name underneath.
fn ribbon_group(ui: &mut Ui, caption: &str, add: impl FnOnce(&mut Ui)) {
    ui.vertical(|ui| {
        let width = ui
            .horizontal(|ui| {
                ui.set_min_height(RIBBON_BUTTON + 4.0);
                add(ui);
            })
            .response
            .rect
            .width();
        ui.allocate_ui_with_layout(
            egui::vec2(width, 14.0),
            egui::Layout::top_down(egui::Align::Center),
            |ui| {
                ui.label(RichText::new(caption).size(11.0).color(MUTED));
            },
        );
    });
    ui.separator();
}

/// The ribbon: a row of tabs, then the chosen tab's tool groups.
fn ribbon(build: &mut BuildMode, ui: &mut Ui) {
    ui.horizontal(|ui| {
        for (tab, label) in [
            (RibbonTab::Home, "HOME"),
            (RibbonTab::Model, "MODEL"),
            (RibbonTab::Test, "TEST"),
            (RibbonTab::View, "VIEW"),
        ] {
            let selected = build.ui.ribbon == tab;
            let text = RichText::new(label).size(12.0).strong();
            let text = if selected {
                text.color(BACKGROUND)
            } else {
                text
            };
            if ui.selectable_label(selected, text).clicked() {
                build.ui.ribbon = tab;
            }
        }
    });
    egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(4)
        .inner_margin(egui::Margin::symmetric(6, 3))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::horizontal()
                .id_salt("build-ribbon")
                .show(ui, |ui| {
                    ui.horizontal(|ui| match build.ui.ribbon {
                        RibbonTab::Home => {
                            clipboard_group(build, ui);
                            tools_group(build, ui);
                            terrain_group(build, ui);
                            insert_group(build, ui);
                            edit_group(build, ui);
                            world_group(build, ui);
                            play_group(build, ui);
                        }
                        RibbonTab::Model => {
                            tools_group(build, ui);
                            snap_group(build, ui);
                            arrange_group(build, ui);
                            terrain_group(build, ui);
                            insert_group(build, ui);
                            effects_group(build, ui);
                            edit_group(build, ui);
                        }
                        RibbonTab::Test => {
                            play_group(build, ui);
                            spawn_group(build, ui);
                            world_group(build, ui);
                        }
                        RibbonTab::View => view_groups(build, ui),
                    });
                });
        });
}

fn clipboard_group(build: &mut BuildMode, ui: &mut Ui) {
    let any = !build.selection.is_empty();
    ribbon_group(ui, "Clipboard", |ui| {
        let pasted = big_button(
            ui,
            "📋",
            "Paste",
            false,
            !build.ui.clipboard.is_empty(),
            "Paste the copied objects (Ctrl+V)",
        );
        if pasted.clicked() {
            paste(build);
        }
        ui.vertical(|ui| {
            if small_button(ui, "🗐", "Copy", any, "Copy the selection (Ctrl+C)") {
                copy_selection(build, ui.ctx());
            }
            if small_button(ui, "✂", "Cut", any, "Cut the selection (Ctrl+X)") {
                cut_selection(build, ui.ctx());
            }
            if small_button(
                ui,
                "⎘",
                "Duplicate",
                any,
                "Duplicate the selection (Ctrl+D)",
            ) {
                duplicate(build);
            }
        });
    });
}

fn tools_group(build: &mut BuildMode, ui: &mut Ui) {
    ribbon_group(ui, "Tools", |ui| {
        for (tool, icon, label, keys) in [
            (Tool::Select, "⬉", "Select", "Ctrl+1 or Q"),
            (Tool::Move, "↔", "Move", "Ctrl+2 or W"),
            (Tool::Scale, "⛶", "Scale", "Ctrl+3 or R"),
            (Tool::Rotate, "⟲", "Rotate", "Ctrl+4 or E"),
        ] {
            let hover = format!("{label} ({keys})");
            if big_button(ui, icon, label, build.tool == tool, true, &hover).clicked() {
                build.tool = tool;
            }
        }
        ui.vertical(|ui| {
            let axes = if build.local {
                "Local axes"
            } else {
                "World axes"
            };
            if small_button(
                ui,
                "🌐",
                axes,
                true,
                "Gizmo axes: the world's or the object's own (Ctrl+L or L)",
            ) {
                build.local = !build.local;
            }
            let snap = if build.snap.enabled {
                "Snap on"
            } else {
                "Snap off"
            };
            if small_button(ui, "#", snap, true, "Snap to the grid (G)") {
                build.snap.enabled = !build.snap.enabled;
            }
        });
    });
}

fn terrain_group(build: &mut BuildMode, ui: &mut Ui) {
    ribbon_group(ui, "Terrain", |ui| {
        let sculpting = build.tool == Tool::Sculpt;
        if big_button(
            ui,
            "🗻",
            "Sculpt",
            sculpting,
            true,
            "Raise, lower, smooth, flatten, paint or cut holes in a terrain object (T). Insert one from the Toolbox: Ground › Terrain.",
        )
        .clicked()
        {
            build.tool = Tool::Sculpt;
        }
        if sculpting {
            ui.vertical(|ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("sculpt-brush")
                        .width(80.0)
                        .selected_text(build.sculpt.brush.label())
                        .show_ui(ui, |ui| {
                            for brush in super::Brush::ALL {
                                ui.selectable_value(&mut build.sculpt.brush, brush, brush.label())
                                    .on_hover_text(brush.hint());
                            }
                        });
                    if build.sculpt.brush == super::Brush::Paint {
                        use sa_scene::world_layout::TERRAIN_MATERIALS;
                        let current = TERRAIN_MATERIALS
                            .get(usize::from(build.sculpt.material))
                            .map_or("Ground", |(name, _)| name);
                        egui::ComboBox::from_id_salt("sculpt-material")
                            .width(70.0)
                            .selected_text(current)
                            .show_ui(ui, |ui| {
                                for (index, (name, _)) in TERRAIN_MATERIALS.iter().enumerate() {
                                    ui.selectable_value(
                                        &mut build.sculpt.material,
                                        index as u8,
                                        *name,
                                    );
                                }
                            })
                            .response
                            .on_hover_text(
                                "What Paint paints (Ground is the terrain's own colour)",
                            );
                    }
                });
                ui.add(
                    egui::Slider::new(&mut build.sculpt.radius, 1.0..=40.0)
                        .text("m")
                        .logarithmic(true),
                )
                .on_hover_text("Brush radius");
                ui.add(egui::Slider::new(&mut build.sculpt.strength, 0.05..=1.0).text("strength"));
            });
        }
    });
}

fn insert_group(build: &mut BuildMode, ui: &mut Ui) {
    ribbon_group(ui, "Insert", |ui| {
        let placing_part = build
            .placing
            .as_ref()
            .is_some_and(|p| matches!(&p.asset, AssetRef::Builtin(b) if b.starts_with("part-")));
        let place_part = |build: &mut BuildMode, name: &str, label: &str| {
            build.selection.clear();
            build.placing = Some(Placing {
                asset: AssetRef::Builtin(name.into()),
                label: label.into(),
                turn: 0.0,
                dragged: false,
            });
        };
        ui.vertical(|ui| {
            if big_button(
                ui,
                "■",
                "Part",
                placing_part,
                true,
                "A block to build with: click in the world to place it, then size, colour and give it a material in Properties",
            )
            .clicked()
            {
                place_part(build, "part-block", "Block");
            }
            ui.menu_button(RichText::new("Shapes...").small(), |ui| {
                for builtin in sa_scene::builtin_assets::ALL
                    .iter()
                    .filter(|b| b.category == sa_scene::builtin_assets::PARTS)
                {
                    if ui.button(builtin.label).clicked() {
                        place_part(build, builtin.name, builtin.label);
                        ui.close();
                    }
                }
            });
        });
        if big_button(
            ui,
            "📦",
            "Toolbox",
            !build.ui.hide_toolbox,
            true,
            "Show or hide the Toolbox with every asset",
        )
        .clicked()
        {
            build.ui.hide_toolbox = !build.ui.hide_toolbox;
        }
        let owner = match build.selection.as_slice() {
            [id] => build
                .doc
                .layout
                .node(*id)
                .filter(|n| !n.is_group())
                .map(|n| (n.id, n.name.clone())),
            _ => None,
        };
        let hover = match &owner {
            Some((_, name)) => format!("Add a Lua script to {name} and open it"),
            None => "Add a script for the whole world and open it (select an object first to put it on that object)".to_string(),
        };
        if big_button(ui, "📜", "Script", false, true, &hover).clicked() {
            super::script_ui::add(
                build,
                owner.map(|(id, _)| id),
                &super::script_ui::EXAMPLES[0],
            );
        }
        if big_button(
            ui,
            "📥",
            "Import",
            false,
            true,
            "Import your own .glb model from this computer",
        )
        .clicked()
        {
            if let Some(path) = pick_model() {
                build.ui.import = Some((path, "custom".into()));
            }
        }
    });
}

fn effects_group(build: &mut BuildMode, ui: &mut Ui) {
    let objects: Vec<NodeId> = build
        .selection
        .iter()
        .copied()
        .filter(|id| {
            build.doc.layout.node(*id).is_some_and(|n| !n.is_group())
                && !build.doc.layout.effective_locked(*id)
        })
        .collect();
    ribbon_group(ui, "Effects", |ui| {
        if big_button(
            ui,
            "💡",
            "Light",
            false,
            !objects.is_empty(),
            "Give the selected objects a lamp: a glow and a pool of light on the ground, from dusk to dawn",
        )
        .clicked()
        {
            use sa_scene::world_layout::PointLight;
            run(
                build,
                ops::modify(&build.doc, "Add light", &objects, |n| {
                    n.light.get_or_insert_with(PointLight::lamp);
                }),
            );
        }
    });
}

fn edit_group(build: &mut BuildMode, ui: &mut Ui) {
    let any = !build.selection.is_empty();
    let all_locked = any
        && build
            .selection
            .iter()
            .all(|id| build.doc.layout.node(*id).is_some_and(|n| n.locked));
    ribbon_group(ui, "Edit", |ui| {
        ui.vertical(|ui| {
            let undo = build
                .history
                .undo_label()
                .map_or("Undo".into(), |l| format!("Undo {l}"));
            if small_button(
                ui,
                "↺",
                "Undo",
                build.history.can_undo(),
                &format!("{undo} (Ctrl+Z)"),
            ) {
                build.undo();
            }
            let redo = build
                .history
                .redo_label()
                .map_or("Redo".into(), |l| format!("Redo {l}"));
            if small_button(
                ui,
                "↻",
                "Redo",
                build.history.can_redo(),
                &format!("{redo} (Ctrl+Y)"),
            ) {
                build.redo();
            }
        });
        if big_button(
            ui,
            "📁",
            "Group",
            false,
            any,
            "Group the selection as a model (Ctrl+G): one thing with its own pivot, picked whole with a click",
        )
        .clicked()
        {
            group(build);
        }
        let groups = build
            .selection
            .iter()
            .any(|id| build.doc.layout.node(*id).is_some_and(Node::is_group));
        if big_button(
            ui,
            "📂",
            "Ungroup",
            false,
            groups,
            "Dissolve the selected groups (Ctrl+Shift+G)",
        )
        .clicked()
        {
            ungroup(build);
        }
        let (icon, label) = if all_locked {
            ("🔓", "Unlock")
        } else {
            ("🔒", "Lock")
        };
        if big_button(
            ui,
            icon,
            label,
            all_locked,
            any,
            "Locked objects cannot be picked in the viewport or moved",
        )
        .clicked()
        {
            let ids = build.selection.clone();
            let label = if all_locked {
                "Unlock objects"
            } else {
                "Lock objects"
            };
            run(
                build,
                ops::modify(&build.doc, label, &ids, |n| n.locked = !all_locked),
            );
        }
    });
}

fn play_group(build: &mut BuildMode, ui: &mut Ui) {
    ribbon_group(ui, "Test", |ui| {
        let play = big_button(
            ui,
            "▶",
            "Play",
            false,
            true,
            "Play this world now (F5). F6 brings you back to Build Mode.",
        );
        // The one green button: easy to find, as in other editors.
        ui.painter().text(
            play.rect.center_top() + egui::vec2(0.0, 17.0),
            egui::Align2::CENTER_CENTER,
            "▶",
            egui::FontId::proportional(21.0),
            SUCCESS,
        );
        if play.clicked() {
            build.requests.push(Request::Playtest);
        }
        if big_button(ui, "💾", "Save", false, true, "Save the world (Ctrl+S)").clicked() {
            build.save();
        }
    });
}

/// The world's own pages in the Properties, also with the Explorer closed.
fn world_group(build: &mut BuildMode, ui: &mut Ui) {
    let page = build.selection.is_empty().then_some(build.ui.world_page);
    ribbon_group(ui, "World", |ui| {
        for (target, icon, label, hover) in [
            (
                WorldPage::Workspace,
                "🌐",
                "Workspace",
                "The world's name, description and spawn point",
            ),
            (
                WorldPage::Lighting,
                "☀",
                "Lighting",
                "Time of day, weather, fog, sky colour and brightness",
            ),
        ] {
            if big_button(ui, icon, label, page == Some(target), true, hover).clicked() {
                build.selection.clear();
                build.ui.world_page = target;
                build.ui.hide_properties = false;
            }
        }
    });
}

/// Align, space evenly; the arrow keys nudge.
fn arrange_group(build: &mut BuildMode, ui: &mut Ui) {
    use super::arrange::{Edge, AXES};
    let two = build.selection.len() >= 2;
    let three = build.selection.len() >= 3;
    ribbon_group(ui, "Arrange", |ui| {
        ui.vertical(|ui| {
            ui.add_enabled_ui(two, |ui| {
                ui.menu_button("Align...", |ui| {
                    ui.label(
                        RichText::new("To the last one selected")
                            .small()
                            .color(MUTED),
                    );
                    let names = [
                        ["West sides", "Middles east-west", "East sides"],
                        ["South sides", "Middles north-south", "North sides"],
                        ["Bottoms", "Middles up-down", "Tops"],
                    ];
                    for (axis, row) in names.iter().enumerate() {
                        ui.separator();
                        for (edge, name) in [Edge::Min, Edge::Center, Edge::Max].into_iter().zip(row) {
                            if ui.button(*name).clicked() {
                                build.align(axis, edge);
                                ui.close();
                            }
                        }
                    }
                })
                .response
                .on_hover_text("Line the selection up with the last object selected");
            });
            ui.add_enabled_ui(three, |ui| {
                ui.menu_button("Space evenly...", |ui| {
                    for (axis, name) in AXES.iter().enumerate() {
                        if ui.button(format!("Along {name}")).clicked() {
                            build.distribute(axis);
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text("Even gaps between three or more objects, the two at the ends stay");
            });
            ui.label(RichText::new("Arrows, PgUp/PgDn: nudge").small().color(MUTED))
                .on_hover_text("Move the selection one grid step (10 cm with snapping off): the arrow keys along the ground, the way the camera looks, Page Up and Page Down up and down");
        });
    });
}

fn snap_group(build: &mut BuildMode, ui: &mut Ui) {
    ribbon_group(ui, "Snap to Grid", |ui| {
        ui.vertical(|ui| {
            ui.checkbox(&mut build.snap.enabled, "Snap")
                .on_hover_text("Moves and turns snap to the steps on the right (G)");
            ui.checkbox(&mut build.snap.surface, "Surface")
                .on_hover_text("Place new objects on the surface under the mouse");
        });
        egui::Grid::new("snap-steps").num_columns(2).show(ui, |ui| {
            ui.label("Move");
            egui::ComboBox::from_id_salt("grid-step")
                .width(70.0)
                .selected_text(format!("{} m", build.snap.step))
                .show_ui(ui, |ui| {
                    for step in [0.1, 0.25, 0.5, 1.0, 2.0, 4.0, 8.0] {
                        ui.selectable_value(&mut build.snap.step, step, format!("{step} m"));
                    }
                });
            ui.end_row();
            ui.label("Rotate");
            egui::ComboBox::from_id_salt("angle-step")
                .width(70.0)
                .selected_text(format!("{}°", build.snap.angle))
                .show_ui(ui, |ui| {
                    for step in [5.0, 15.0, 30.0, 45.0, 90.0] {
                        ui.selectable_value(&mut build.snap.angle, step, format!("{step}°"));
                    }
                });
            ui.end_row();
        });
    });
}

fn spawn_group(build: &mut BuildMode, ui: &mut Ui) {
    ribbon_group(ui, "Spawn", |ui| {
        if big_button(
            ui,
            "🚩",
            "Set spawn",
            false,
            true,
            "Players start where the camera is looking",
        )
        .clicked()
        {
            let info = build.doc.info.clone();
            let focus = sa_scene::world_layout::gltf_to_blender(build.camera.focus);
            let spawn = [focus[0], focus[1], focus[2].max(0.0)];
            build.execute(Command::Info {
                before: info.clone(),
                after: WorldInfo { spawn, ..info },
            });
        }
        let [x, y, z] = build.doc.info.spawn;
        ui.label(RichText::new(format!("Spawn\n{x:.1}, {y:.1}, {z:.1}")).color(MUTED));
    });
}

/// Explorer, Properties, Toolbox and Output on or off.
fn dock_toggles(build: &mut BuildMode, ui: &mut Ui) {
    let mut explorer = !build.ui.hide_explorer;
    let mut properties = !build.ui.hide_properties;
    let mut toolbox = !build.ui.hide_toolbox;
    ui.checkbox(&mut explorer, "Explorer");
    ui.checkbox(&mut properties, "Properties");
    ui.checkbox(&mut toolbox, "Toolbox");
    ui.checkbox(&mut build.ui.bottom_open, "Output");
    build.ui.hide_explorer = !explorer;
    build.ui.hide_properties = !properties;
    build.ui.hide_toolbox = !toolbox;
}

fn view_groups(build: &mut BuildMode, ui: &mut Ui) {
    ribbon_group(ui, "Show", |ui| {
        for (icon, label, hover) in [
            ("☰", "Explorer", "Every object in the world, in groups"),
            ("⚙", "Properties", "Edit what is selected"),
            ("📦", "Toolbox", "Every asset you can place"),
            ("🖥", "Output", "Messages, errors and warnings"),
        ] {
            let flag = match label {
                "Explorer" => &mut build.ui.hide_explorer,
                "Properties" => &mut build.ui.hide_properties,
                "Toolbox" => &mut build.ui.hide_toolbox,
                _ => {
                    let open = build.ui.bottom_open;
                    if big_button(ui, icon, label, open, true, hover).clicked() {
                        build.ui.bottom_open = !open;
                    }
                    continue;
                }
            };
            if big_button(ui, icon, label, !*flag, true, hover).clicked() {
                *flag = !*flag;
            }
        }
    });
    ribbon_group(ui, "Viewport", |ui| {
        if big_button(ui, "#", "Grid", build.snap.show_grid, true, "Show the grid").clicked() {
            build.snap.show_grid = !build.snap.show_grid;
        }
        if big_button(
            ui,
            "🎯",
            "Zones",
            !build.ui.hide_gameplay,
            true,
            "Show trigger zones, interactions, checkpoints and where teleports lead",
        )
        .clicked()
        {
            build.ui.hide_gameplay = !build.ui.hide_gameplay;
        }
        if big_button(
            ui,
            "🔲",
            "Collision",
            build.ui.show_collision,
            true,
            "Outline solid objects in cyan and objects without collision in red",
        )
        .clicked()
        {
            build.ui.show_collision = !build.ui.show_collision;
        }
    });
    ribbon_group(ui, "Camera", |ui| {
        if big_button(
            ui,
            "🔍",
            "Focus",
            false,
            !build.selection.is_empty(),
            "Look at the selection (F)",
        )
        .clicked()
        {
            build.focus_selection();
        }
        if big_button(ui, "↺", "Reset", false, true, "Reset the camera (Home)").clicked() {
            build.reset_camera();
        }
        ui.vertical(|ui| {
            ui.label(RichText::new("Fly speed").color(MUTED));
            ui.add(
                egui::Slider::new(&mut build.camera.speed, 1.0..=400.0)
                    .logarithmic(true)
                    .suffix(" m/s"),
            );
        });
    });
    ribbon_group(ui, "Performance", |ui| {
        ui.vertical(|ui| {
            ui.label(RichText::new("Editor graphics").color(MUTED));
            egui::ComboBox::from_id_salt("build-performance")
                .selected_text(build.ui.performance.name())
                .show_ui(ui, |ui| {
                    for mode in EditorPerformance::ALL {
                        let hover = match mode {
                            EditorPerformance::Auto => {
                                "Light on integrated graphics, Full on a graphics card"
                            }
                            EditorPerformance::Full => "Draw with your own graphics settings",
                            EditorPerformance::Light => {
                                "For slower computers: no shadows, reflections or bloom, \
                                 two thirds resolution and at most 30 FPS"
                            }
                        };
                        ui.selectable_value(&mut build.ui.performance, mode, mode.name())
                            .on_hover_text(hover);
                    }
                })
                .response
                .on_hover_text("How heavily the editor draws the world. Playtesting uses your own graphics settings.");
            let (now, hover) = if build.ui.light {
                ("Drawing light", "Lighter drawing that slower computers can keep up with")
            } else {
                ("Drawing full", "Drawing with your own graphics settings")
            };
            ui.label(RichText::new(now).color(MUTED).size(11.0))
                .on_hover_text(hover);
        });
    });
    ribbon_group(ui, "Help", |ui| {
        if big_button(
            ui,
            "?",
            "Controls",
            false,
            true,
            "Every mouse and keyboard control",
        )
        .clicked()
        {
            build.ui.show_help = true;
        }
    });
}

/// A .glb file chosen with the system's file picker, as a path string; None
/// when cancelled.
fn pick_model() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Import a model")
        .add_filter("glTF binary model", &["glb"])
        .pick_file()
        .map(|path| path.display().to_string())
}

/// Put the selection on the editor's clipboard. The number of objects
/// copied (groups count once), or None when nothing was.
fn copy_selection(build: &mut BuildMode, ctx: &egui::Context) -> Option<usize> {
    if build.selection.is_empty() {
        return None;
    }
    match ops::copy(&build.doc, &build.selection) {
        Ok(nodes) => {
            let copied: HashSet<NodeId> = nodes.iter().map(|n| n.id).collect();
            let roots = nodes
                .iter()
                .filter(|n| !n.parent.is_some_and(|p| copied.contains(&p)))
                .count();
            build.ui.clipboard = nodes;
            build.ui.clipboard_cut = false;
            build.ui.pastes = 0;
            // Text on the system clipboard is what lets Ctrl+V reach the editor.
            ctx.copy_text(format!("SARE Build Mode: {roots} objects"));
            build.info(format!("Copied {roots} objects"));
            Some(roots)
        }
        Err(error) => {
            build.error(format!("{error:#}"));
            None
        }
    }
}

fn cut_selection(build: &mut BuildMode, ctx: &egui::Context) {
    if let Some(count) = copy_selection(build, ctx) {
        build.ui.clipboard_cut = true;
        delete(build);
        build.info(format!("Cut {count} objects; paste puts them back"));
    }
}

fn paste(build: &mut BuildMode) {
    if build.ui.clipboard.is_empty() {
        return;
    }
    let copied = build.ui.clipboard.clone();
    // Cut objects come back as themselves, so what links to them still does.
    if build.ui.clipboard_cut {
        build.ui.clipboard_cut = false;
        if let Some((command, roots)) = ops::paste_cut(&build.doc, &copied) {
            build.execute(command);
            build.selection = roots;
            return;
        }
    }
    // Each further paste lands one more step along, not on the last one.
    build.ui.pastes += 1;
    let offset = [build.snap.step.max(1.0) * build.ui.pastes as f32, 0.0, 0.0];
    match ops::paste(&mut build.doc, &copied, offset) {
        Ok((command, roots)) => {
            build.execute(command);
            build.selection = roots;
        }
        Err(error) => build.error(format!("{error:#}")),
    }
}

fn status_bar(build: &mut BuildMode, ui: &mut Ui) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("SARE {}", env!("CARGO_PKG_VERSION"))).color(MUTED));
        ui.separator();
        let objects = build
            .doc
            .layout
            .nodes
            .iter()
            .filter(|n| !n.is_group())
            .count();
        ui.label(format!("{objects} objects"));
        ui.separator();
        ui.label(format!("{} selected", build.selection.len()));
        if let Some(node) = build
            .selection
            .last()
            .and_then(|id| build.doc.layout.node(*id))
        {
            let [x, y, z] = node.transform.position;
            ui.label(RichText::new(format!("Pos ({x:.2}, {y:.2}, {z:.2})")).color(MUTED));
        }
        ui.separator();
        ui.label(
            RichText::new(format!(
                "Grid {} m{}",
                build.snap.step,
                if build.snap.enabled {
                    ""
                } else {
                    " (snap off)"
                }
            ))
            .color(MUTED),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button(if build.ui.bottom_open {
                    "Hide Output"
                } else {
                    "Output"
                })
                .clicked()
            {
                build.ui.bottom_open = !build.ui.bottom_open;
            }
            let (text, color) = if build.history.dirty() {
                ("● Unsaved changes (autosave on)", WARNING)
            } else {
                ("● Saved", SUCCESS)
            };
            ui.label(RichText::new(text).color(color));
            if let Some(last) = build.log.last() {
                let color = match last.level {
                    Level::Info => MUTED,
                    Level::Warning => WARNING,
                    Level::Error => ERROR,
                };
                if last.at.elapsed().as_secs() < 8 {
                    ui.label(RichText::new(&last.text).color(color));
                }
            }
        });
    });
}

fn bottom(build: &mut BuildMode, ui: &mut Ui) {
    ui.horizontal(|ui| {
        let gameplay_problems = sa_scene::world_logic::check(&build.doc.layout).len();
        let problems =
            build.log.iter().filter(|l| l.level != Level::Info).count() + gameplay_problems;
        for (tab, label) in [
            (BottomTab::Console, "Output".to_string()),
            (
                BottomTab::Problems,
                format!("Errors and warnings ({problems})"),
            ),
            (BottomTab::Assets, "Asset status".to_string()),
            (BottomTab::Playtest, "Playtest".to_string()),
        ] {
            if ui.selectable_label(build.ui.bottom == tab, label).clicked() {
                build.ui.bottom = tab;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("Clear").clicked() {
                build.log.clear();
            }
        });
    });
    ui.separator();
    if build.ui.bottom == BottomTab::Console {
        super::command::ui(build, ui);
    }
    egui::ScrollArea::vertical().stick_to_bottom(true).auto_shrink(false).show(ui, |ui| {
        match build.ui.bottom {
            BottomTab::Console | BottomTab::Problems => {
                let only_problems = build.ui.bottom == BottomTab::Problems;
                if only_problems {
                    // Live checks of the world's gameplay; click one to select it.
                    for problem in sa_scene::world_logic::check(&build.doc.layout) {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new("[GAMEPLAY]").monospace().color(WARNING));
                            if ui.link(RichText::new(&problem.text).monospace()).clicked() {
                                build.selection = vec![problem.node];
                            }
                        });
                    }
                }
                for line in build.log.iter().filter(|l| !only_problems || l.level != Level::Info) {
                    let (tag, color) = match line.level {
                        Level::Info => ("[INFO]", SUCCESS),
                        Level::Warning => ("[WARN]", WARNING),
                        Level::Error => ("[ERROR]", ERROR),
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(tag).monospace().color(color));
                        ui.label(RichText::new(&line.text).monospace());
                    });
                }
            }
            BottomTab::Assets => {
                let total = build.catalog.entries.len();
                let bad: Vec<_> = build.catalog.entries.iter().filter(|e| e.status != AssetStatus::Ready).collect();
                ui.label(format!("{total} assets in the catalog, {} with problems.", bad.len()));
                for entry in bad {
                    let text = match &entry.status {
                        AssetStatus::Missing(why) => format!("Missing {}: {why}", entry.reference),
                        AssetStatus::Invalid(why) => format!("Invalid: {why}"),
                        AssetStatus::Ready => continue,
                    };
                    ui.label(RichText::new(text).color(ERROR));
                }
            }
            BottomTab::Playtest => {
                ui.label("Playtest starts the edited world in normal SARE gameplay without saving over your files. Unsaved work is autosaved first.");
                ui.label("Press F6 (or the Back to Build Mode button) to return. The editor keeps every change, selection and undo step.");
                let spawn = build.doc.info.spawn;
                ui.label(format!("Spawn point: {:.1}, {:.1}, {:.1}", spawn[0], spawn[1], spawn[2]));
            }
        }
    });
}

/// A dock's title bar with its close button.
fn dock_header(ui: &mut Ui, title: &str, detail: &str, hide: &mut bool) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong().size(14.0));
        if !detail.is_empty() {
            ui.label(RichText::new(detail).color(MUTED));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new(RichText::new("×").size(16.0)).frame(false))
                .on_hover_text(format!("Close {title}; the View tab opens it again"))
                .clicked()
            {
                *hide = true;
            }
        });
    });
    ui.separator();
}

fn toolbox(build: &mut BuildMode, ui: &mut Ui) {
    let mut hide = false;
    dock_header(ui, "📦 Toolbox", "", &mut hide);
    build.ui.hide_toolbox = hide;
    assets(build, ui);
}

fn assets(build: &mut BuildMode, ui: &mut Ui) {
    ui.add(egui::TextEdit::singleline(&mut build.ui.asset_search).hint_text("🔍 Search assets…"));
    ui.horizontal_wrapped(|ui| {
        if ui
            .selectable_label(build.ui.category.is_none(), "All")
            .clicked()
        {
            build.ui.category = None;
        }
        for category in build.catalog.categories() {
            let selected = build.ui.category.as_deref() == Some(category.as_str());
            if ui.selectable_label(selected, &category).clicked() {
                build.ui.category = Some(category);
            }
        }
    });
    ui.separator();
    let mut chosen: Option<(AssetRef, String, bool)> = None;
    // Thumbnails are drawn on the CPU when a tile first shows, a few per
    // frame, so a big pack does not stall the editor.
    let budget = Instant::now() + Duration::from_millis(12);
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            let entries: Vec<_> = build
                .catalog
                .search(&build.ui.asset_search, build.ui.category.as_deref())
                .cloned()
                .collect();
            if entries.is_empty() {
                ui.label(RichText::new("No assets match.").color(MUTED));
            }
            let gap = ui.spacing().item_spacing.x;
            let columns = ((ui.available_width() + gap) / (TILE_MIN + gap))
                .floor()
                .max(1.0);
            let tile = egui::vec2(((ui.available_width() + gap) / columns - gap).floor(), 0.0);
            let tile = egui::vec2(tile.x, tile.x + 30.0);
            // Entries come sorted by source; one heading per source.
            for group in entries.chunk_by(|a, b| a.source == b.source) {
                ui.label(RichText::new(&group[0].source).small().color(MUTED));
                ui.horizontal_wrapped(|ui| {
                    for entry in group {
                        let placing = build
                            .placing
                            .as_ref()
                            .is_some_and(|p| p.asset == entry.reference);
                        let (rect, response) =
                            ui.allocate_exact_size(tile, egui::Sense::click_and_drag());
                        if ui.is_rect_visible(rect) {
                            let image = thumbnail(build, ui.ctx(), entry, budget);
                            asset_tile(ui, rect, &response, entry, image, placing);
                        }
                        let response = response.on_hover_ui(|ui| asset_tooltip(ui, entry));
                        if entry.status == AssetStatus::Ready {
                            let drag = response.drag_started_by(egui::PointerButton::Primary);
                            if response.clicked() || drag {
                                chosen = Some((entry.reference.clone(), entry.label.clone(), drag));
                            }
                        }
                        if response.dragged() || response.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                        }
                    }
                });
                ui.add_space(4.0);
            }
        });
    if let Some((asset, label, dragged)) = chosen {
        let label = label
            .split_whitespace()
            .take(3)
            .collect::<Vec<_>>()
            .join(" ");
        build.placing = Some(Placing {
            asset,
            label,
            turn: 0.0,
            dragged,
        });
        build.selection.clear();
    }
}

/// Narrowest an Asset Browser tile gets before a column is dropped.
const TILE_MIN: f32 = 76.0;
/// Thumbnail pixels per side (drawn at twice the size of a narrow tile).
const THUMBNAIL_SIZE: usize = 128;

/// The asset's picture: made once and kept. None while it is not made yet
/// (or could not be made: the tile then shows an icon).
fn thumbnail(
    build: &mut BuildMode,
    ctx: &egui::Context,
    entry: &sa_editor::catalog::AssetEntry,
    budget: Instant,
) -> Option<egui::TextureHandle> {
    if let Some(made) = build.ui.thumbnails.get(&entry.reference) {
        return made.clone();
    }
    if entry.status != AssetStatus::Ready {
        return None;
    }
    if Instant::now() > budget {
        ctx.request_repaint();
        return None;
    }
    let made = build.resolver.model(&entry.reference).ok().map(|model| {
        let picture = sa_editor::thumbnail::render(&model, THUMBNAIL_SIZE);
        let image =
            egui::ColorImage::from_rgba_unmultiplied([picture.size, picture.size], &picture.rgba);
        ctx.load_texture(
            format!("build thumbnail {}", entry.reference),
            image,
            egui::TextureOptions::LINEAR,
        )
    });
    build
        .ui
        .thumbnails
        .insert(entry.reference.clone(), made.clone());
    made
}

fn asset_tile(
    ui: &Ui,
    rect: egui::Rect,
    response: &egui::Response,
    entry: &sa_editor::catalog::AssetEntry,
    image: Option<egui::TextureHandle>,
    placing: bool,
) {
    let painter = ui.painter();
    let fill = if placing {
        ACCENT
    } else if response.hovered() {
        SURFACE_HIGH.gamma_multiply(1.4)
    } else {
        SURFACE_HIGH
    };
    painter.rect_filled(rect, 6.0, fill);
    let picture =
        egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), rect.width())).shrink(6.0);
    match (&image, &entry.status) {
        (Some(image), _) => {
            painter.image(
                image.id(),
                picture,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        (None, status) => {
            let (icon, color) = match status {
                AssetStatus::Ready => ("■", MUTED),
                AssetStatus::Missing(_) => ("⚠", ERROR),
                AssetStatus::Invalid(_) => ("⚠", WARNING),
            };
            painter.text(
                picture.center(),
                egui::Align2::CENTER_CENTER,
                icon,
                egui::FontId::proportional(24.0),
                color,
            );
        }
    }
    let text_color = if placing { BACKGROUND } else { TEXT };
    let label = ui.fonts_mut(|fonts| {
        fonts.layout(
            entry.label.clone(),
            egui::FontId::proportional(11.0),
            text_color,
            rect.width() - 6.0,
        )
    });
    let top = egui::pos2(rect.center().x, picture.bottom() + 3.0);
    painter.with_clip_rect(rect).galley(
        top - egui::vec2(label.size().x / 2.0, 0.0),
        label,
        text_color,
    );
}

fn asset_tooltip(ui: &mut Ui, entry: &sa_editor::catalog::AssetEntry) {
    ui.label(RichText::new(&entry.label).strong());
    ui.label(format!("{} · {}", entry.category, entry.source));
    ui.label(
        RichText::new(entry.reference.to_string())
            .monospace()
            .small(),
    );
    match &entry.status {
        AssetStatus::Ready => {
            ui.label("Click, then click in the world. Or drag it into the world.")
        }
        AssetStatus::Missing(why) | AssetStatus::Invalid(why) => {
            ui.label(RichText::new(why).color(ERROR))
        }
    };
}

fn explorer(build: &mut BuildMode, ui: &mut Ui) {
    let mut hide = false;
    dock_header(ui, "☰ Explorer", "", &mut hide);
    build.ui.hide_explorer = hide;
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut build.ui.outliner_search)
                .hint_text("🔍 Name or #tag…")
                .desired_width(150.0),
        );
        if ui
            .small_button("+ Group")
            .on_hover_text("New empty group")
            .clicked()
        {
            let id = build.doc.layout.allocate_id();
            let node = Node::group(id, ops::unique_name(&build.doc, "Group"));
            let at = build.doc.layout.nodes.len();
            build.execute(Command::Insert(vec![(at, node)]));
            build.selection = vec![id];
        }
    });
    ui.separator();
    let filter = build.ui.outliner_search.trim().to_lowercase();
    let mut actions: Vec<OutlinerAction> = Vec::new();
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            if filter.is_empty() {
                let page = build.selection.is_empty().then_some(build.ui.world_page);
                let open = !build.ui.workspace_collapsed;
                ui.horizontal(|ui| {
                    if ui.small_button(if open { "⏷" } else { "⏵" }).clicked() {
                        build.ui.workspace_collapsed = open;
                    }
                    let row = ui
                        .selectable_label(
                            page == Some(WorldPage::Workspace),
                            format!("🌐 Workspace  ({})", build.doc.info.name),
                        )
                        .on_hover_text(
                            "The world itself: name, description and spawn point. Drop objects here to move them to the top level.",
                        );
                    if row.clicked() {
                        build.selection.clear();
                        build.ui.world_page = WorldPage::Workspace;
                    }
                    if let Some(dragged) = row.dnd_release_payload::<Vec<NodeId>>() {
                        actions.push(OutlinerAction::Drop((*dragged).clone(), ops::Drop::End));
                    }
                });
                if build.doc.layout.nodes.is_empty() && open {
                    ui.label(
                        RichText::new(
                            "Nothing placed yet. Insert a Part from the Home tab or pick something in the Toolbox.",
                        )
                        .color(MUTED),
                    );
                }
            }
            if filter.is_empty() && !build.ui.workspace_collapsed {
                let roots: Vec<NodeId> = build
                    .doc
                    .layout
                    .nodes
                    .iter()
                    .filter(|n| n.parent.is_none())
                    .map(|n| n.id)
                    .collect();
                for id in roots {
                    row(build, ui, id, 1, &mut actions);
                }
                // Below the list: drop here to move to the top level, last.
                let (rect, response) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 28.0),
                    egui::Sense::hover(),
                );
                if response.dnd_hover_payload::<Vec<NodeId>>().is_some() {
                    ui.painter().text(
                        rect.left_center() + egui::vec2(6.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        "Drop here: top level, last",
                        egui::FontId::proportional(12.0),
                        ACCENT,
                    );
                }
                if let Some(dragged) = response.dnd_release_payload::<Vec<NodeId>>() {
                    actions.push(OutlinerAction::Drop((*dragged).clone(), ops::Drop::End));
                }
            } else if !filter.is_empty() {
                let matches: Vec<NodeId> = build
                    .doc
                    .layout
                    .nodes
                    .iter()
                    // By name, or by tag: "#lamp" finds everything tagged Lamp.
                    .filter(|n| match filter.strip_prefix('#') {
                        Some(tag) => n.tags.iter().any(|t| t.to_lowercase() == tag),
                        None => {
                            n.name.to_lowercase().contains(&filter)
                                || n.tags.iter().any(|t| t.to_lowercase() == filter)
                        }
                    })
                    .map(|n| n.id)
                    .collect();
                for id in matches {
                    row_single(build, ui, id, 0, &mut actions);
                }
            }
            if filter.is_empty() {
                let page = build.selection.is_empty().then_some(build.ui.world_page);
                ui.horizontal(|ui| {
                    ui.add_space(22.0);
                    if ui
                        .selectable_label(page == Some(WorldPage::Lighting), "☀ Lighting")
                        .on_hover_text("Time of day, weather, fog, sky colour and brightness")
                        .clicked()
                    {
                        build.selection.clear();
                        build.ui.world_page = WorldPage::Lighting;
                    }
                });
                let count = build.doc.layout.all_scripts().len();
                ui.horizontal(|ui| {
                    ui.add_space(22.0);
                    if ui
                        .selectable_label(
                            page == Some(WorldPage::Scripts),
                            format!("📜 Scripts  ({count})"),
                        )
                        .on_hover_text("Every script in the world, and scripts for the whole world")
                        .clicked()
                    {
                        build.selection.clear();
                        build.ui.world_page = WorldPage::Scripts;
                    }
                });
            }
        });
    for action in actions {
        apply_outliner(build, action);
    }
}

enum OutlinerAction {
    Select(NodeId, bool),
    Toggle(NodeId, &'static str),
    Rename(NodeId, String),
    Delete(NodeId),
    Duplicate,
    Group,
    Ungroup(NodeId),
    MoveTo(NodeId, Option<NodeId>),
    /// Dragged objects dropped in the outliner.
    Drop(Vec<NodeId>, ops::Drop),
    Focus(NodeId),
    PlaceInto(Option<NodeId>),
    /// Save to My Models, or a copy's changes to all copies.
    SaveModel(NodeId),
}

fn row(
    build: &mut BuildMode,
    ui: &mut Ui,
    id: NodeId,
    depth: usize,
    actions: &mut Vec<OutlinerAction>,
) {
    row_single(build, ui, id, depth, actions);
    if let Some(node) = build
        .doc
        .layout
        .node(id)
        .filter(|n| !n.scripts.is_empty())
        .cloned()
    {
        super::script_ui::explorer_rows(build, ui, &node, depth + 1);
    }
    let is_group = build.doc.layout.node(id).is_some_and(Node::is_group);
    if is_group && !build.ui.collapsed.contains(&id) {
        let children: Vec<NodeId> = build
            .doc
            .layout
            .nodes
            .iter()
            .filter(|n| n.parent == Some(id))
            .map(|n| n.id)
            .collect();
        for child in children {
            if depth < 32 {
                row(build, ui, child, depth + 1, actions);
            }
        }
    }
}

fn row_single(
    build: &mut BuildMode,
    ui: &mut Ui,
    id: NodeId,
    depth: usize,
    actions: &mut Vec<OutlinerAction>,
) {
    let Some(node) = build.doc.layout.node(id).cloned() else {
        return;
    };
    let selected = build.selection.contains(&id);
    let missing = node.asset.as_ref().is_some_and(|a| {
        build
            .catalog
            .find(a)
            .is_some_and(|e| e.status != AssetStatus::Ready)
            || build.gpu.is_placeholder(a)
    });
    let groups: Vec<(NodeId, String)> = build
        .doc
        .layout
        .nodes
        .iter()
        .filter(|n| n.is_group() && n.id != id)
        .map(|n| (n.id, n.name.clone()))
        .collect();
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 14.0);
        if node.is_group() {
            let open = !build.ui.collapsed.contains(&id);
            if ui.small_button(if open { "⏷" } else { "⏵" }).clicked() {
                if open {
                    build.ui.collapsed.insert(id);
                } else {
                    build.ui.collapsed.remove(&id);
                }
            }
        } else {
            ui.add_space(18.0);
        }
        let eye = if node.visible { "👁" } else { "–" };
        if ui
            .small_button(eye)
            .on_hover_text("Show or hide in the world")
            .clicked()
        {
            actions.push(OutlinerAction::Toggle(id, "visible"));
        }
        let lock = if node.locked { "🔒" } else { "🔓" };
        if ui
            .small_button(lock)
            .on_hover_text("Lock: cannot be picked or moved")
            .clicked()
        {
            actions.push(OutlinerAction::Toggle(id, "locked"));
        }
        if let Some((renaming, text)) = &mut build.ui.renaming {
            if *renaming == id {
                let edit = ui.add(egui::TextEdit::singleline(text).desired_width(140.0));
                edit.request_focus();
                if edit.lost_focus() {
                    if !ui.input(|i| i.key_pressed(Key::Escape)) {
                        actions.push(OutlinerAction::Rename(id, text.clone()));
                    }
                    build.ui.renaming = None;
                }
                return;
            }
        }
        let icon = if node.model {
            "📦"
        } else if node.is_group() {
            "📁"
        } else if missing {
            "⚠"
        } else {
            "■"
        };
        let mut text = RichText::new(format!("{icon} {}", node.name));
        if !node.visible {
            text = text.color(MUTED).italics();
        } else if missing {
            text = text.color(ERROR);
        }
        let response = ui
            .selectable_label(selected, text)
            .interact(egui::Sense::drag())
            .on_hover_text("Drag to reorder, or onto a group to move it there");
        // Drag and drop: dragging a selected row takes the whole selection.
        if response.dragged() {
            let ids = if selected {
                build.selection.clone()
            } else {
                vec![id]
            };
            response.dnd_set_drag_payload(ids);
        }
        let drop_at = |pointer: egui::Pos2| {
            let rect = response.rect;
            let along = ((pointer.y - rect.top()) / rect.height().max(1.0)).clamp(0.0, 1.0);
            match (node.is_group(), along) {
                (true, a) if (0.25..=0.75).contains(&a) => ops::Drop::Into(id),
                (_, a) if a < 0.5 => ops::Drop::Before(id),
                _ => ops::Drop::After(id),
            }
        };
        if let (Some(_), Some(pointer)) = (
            response.dnd_hover_payload::<Vec<NodeId>>(),
            ui.input(|i| i.pointer.hover_pos()),
        ) {
            let rect = response.rect;
            let stroke = egui::Stroke::new(2.0, ACCENT);
            match drop_at(pointer) {
                ops::Drop::Into(_) => {
                    ui.painter()
                        .rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Outside);
                }
                ops::Drop::Before(_) => {
                    ui.painter().hline(rect.x_range(), rect.top(), stroke);
                }
                _ => {
                    ui.painter().hline(rect.x_range(), rect.bottom(), stroke);
                }
            }
        }
        if let (Some(dragged), Some(pointer)) = (
            response.dnd_release_payload::<Vec<NodeId>>(),
            ui.input(|i| i.pointer.interact_pos()),
        ) {
            actions.push(OutlinerAction::Drop((*dragged).clone(), drop_at(pointer)));
        }
        if response.clicked() {
            actions.push(OutlinerAction::Select(
                id,
                ui.input(|i| i.modifiers.command),
            ));
        }
        if response.double_clicked() {
            build.ui.renaming = Some((id, node.name.clone()));
        }
        response.context_menu(|ui| {
            if ui.button("Rename").clicked() {
                build.ui.renaming = Some((id, node.name.clone()));
                ui.close();
            }
            if ui.button("Focus").clicked() {
                actions.push(OutlinerAction::Focus(id));
                ui.close();
            }
            if ui.button("Duplicate").clicked() {
                actions.push(OutlinerAction::Select(id, false));
                actions.push(OutlinerAction::Duplicate);
                ui.close();
            }
            if ui.button("Group selection").clicked() {
                if !selected {
                    actions.push(OutlinerAction::Select(id, false));
                }
                actions.push(OutlinerAction::Group);
                ui.close();
            }
            if node.is_group() {
                if ui.button("Ungroup").clicked() {
                    actions.push(OutlinerAction::Ungroup(id));
                    ui.close();
                }
                if ui.button("Place new objects here").clicked() {
                    actions.push(OutlinerAction::PlaceInto(Some(id)));
                    ui.close();
                }
                let save = if node.saved_model.is_some() {
                    "Save to all copies"
                } else {
                    "Save to My Models"
                };
                if ui.button(save).clicked() {
                    actions.push(OutlinerAction::SaveModel(id));
                    ui.close();
                }
            }
            ui.menu_button("Move to group", |ui| {
                if ui.button("(top level)").clicked() {
                    actions.push(OutlinerAction::MoveTo(id, None));
                    ui.close();
                }
                for (group, name) in &groups {
                    if ui.button(name).clicked() {
                        actions.push(OutlinerAction::MoveTo(id, Some(*group)));
                        ui.close();
                    }
                }
            });
            ui.separator();
            if ui.button("Delete").clicked() {
                actions.push(OutlinerAction::Delete(id));
                ui.close();
            }
        });
    });
}

fn apply_outliner(build: &mut BuildMode, action: OutlinerAction) {
    match action {
        OutlinerAction::Drop(ids, to) => {
            // Let go on itself (a slip of the mouse): nothing to do.
            if let ops::Drop::Before(t) | ops::Drop::After(t) | ops::Drop::Into(t) = to {
                if ops::with_descendants(&build.doc, &ids).contains(&t) {
                    return;
                }
            }
            match ops::move_nodes(&build.doc, &ids, to) {
                Ok(Some(command)) => build.execute(command),
                Ok(None) => {}
                Err(error) => build.warn(format!("{error:#}")),
            }
        }
        OutlinerAction::Select(id, add) => {
            if add {
                if let Some(i) = build.selection.iter().position(|s| *s == id) {
                    build.selection.remove(i);
                } else {
                    build.selection.push(id);
                }
            } else {
                build.selection = vec![id];
            }
        }
        OutlinerAction::Toggle(id, field) => {
            let result = ops::modify(
                &build.doc,
                &format!("Toggle {field}"),
                &[id],
                |n| match field {
                    "visible" => n.visible = !n.visible,
                    _ => n.locked = !n.locked,
                },
            );
            run(build, result);
        }
        OutlinerAction::Rename(id, name) => run(build, ops::rename(&build.doc, id, &name)),
        OutlinerAction::Delete(id) => {
            if !build.selection.contains(&id) {
                build.selection = vec![id];
            }
            delete(build);
        }
        OutlinerAction::Duplicate => duplicate(build),
        OutlinerAction::Group => group(build),
        OutlinerAction::Ungroup(id) => {
            build.selection = vec![id];
            ungroup(build);
        }
        OutlinerAction::MoveTo(id, parent) => {
            let ids = if build.selection.contains(&id) {
                build.selection.clone()
            } else {
                vec![id]
            };
            run(build, ops::reparent(&build.doc, &ids, parent));
        }
        OutlinerAction::Focus(id) => {
            build.selection = vec![id];
            build.focus_selection();
        }
        OutlinerAction::SaveModel(id) => {
            if build
                .doc
                .layout
                .node(id)
                .is_some_and(|n| n.saved_model.is_some())
            {
                build.save_to_all_copies(id);
            } else {
                build.save_to_my_models(id);
            }
        }
        OutlinerAction::PlaceInto(group) => {
            build.ui.place_into = group;
            if let Some(name) = group
                .and_then(|g| build.doc.layout.node(g))
                .map(|n| n.name.clone())
            {
                build.info(format!("New objects go into {name}"));
            }
        }
    }
}

pub(super) fn run(build: &mut BuildMode, command: anyhow::Result<Command>) {
    match command {
        Ok(command) => build.execute(command),
        Err(error) => build.error(format!("{error:#}")),
    }
}

fn duplicate(build: &mut BuildMode) {
    if build.selection.is_empty() {
        return;
    }
    let offset = [build.snap.step.max(1.0), 0.0, 0.0];
    let ids = build.selection.clone();
    match ops::duplicate(&mut build.doc, &ids, offset) {
        Ok((command, roots)) => {
            build.execute(command);
            build.selection = roots;
        }
        Err(error) => build.error(format!("{error:#}")),
    }
}
fn delete(build: &mut BuildMode) {
    if build.selection.is_empty() {
        return;
    }
    let ids = std::mem::take(&mut build.selection);
    run(build, ops::delete(&build.doc, &ids));
}
fn group(build: &mut BuildMode) {
    if build.selection.is_empty() {
        return;
    }
    let ids = build.selection.clone();
    match ops::group(&mut build.doc, &ids, "Group") {
        Ok((command, id)) => {
            build.execute(command);
            build.selection = vec![id];
            build.ui.hide_explorer = false;
        }
        Err(error) => build.error(format!("{error:#}")),
    }
}
fn ungroup(build: &mut BuildMode) {
    let groups: Vec<NodeId> = build
        .selection
        .iter()
        .copied()
        .filter(|id| build.doc.layout.node(*id).is_some_and(Node::is_group))
        .collect();
    for id in groups {
        run(build, ops::ungroup(&build.doc, id));
    }
}

fn properties(build: &mut BuildMode, ui: &mut Ui) {
    let detail = match build.selection.as_slice() {
        [] => match build.ui.world_page {
            WorldPage::Workspace => "Workspace".to_owned(),
            WorldPage::Lighting => "Lighting".to_owned(),
            WorldPage::Scripts => "Scripts".to_owned(),
        },
        [id] => build
            .doc
            .layout
            .node(*id)
            .map(|n| {
                let kind = if n.is_group() { "Group" } else { "Object" };
                format!("{kind} \"{}\"", n.name)
            })
            .unwrap_or_default(),
        many => format!("{} objects", many.len()),
    };
    let mut hide = false;
    dock_header(ui, "⚙ Properties", &format!("– {detail}"), &mut hide);
    build.ui.hide_properties = hide;
    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| inspector(build, ui));
}

fn inspector(build: &mut BuildMode, ui: &mut Ui) {
    match build.selection.len() {
        0 => match build.ui.world_page {
            WorldPage::Workspace => world_settings(build, ui),
            WorldPage::Lighting => lighting(build, ui),
            WorldPage::Scripts => super::script_ui::world_page(build, ui),
        },
        1 => object(build, ui, build.selection[0]),
        n => {
            ui.label(format!("{n} objects selected"));
            ui.label(
                RichText::new("Drag the gizmo to move, turn or scale them together.").color(MUTED),
            );
            ui.horizontal(|ui| {
                if ui.button("Group").clicked() {
                    group(build);
                }
                if ui.button("Duplicate").clicked() {
                    duplicate(build);
                }
                if ui.button("Delete").clicked() {
                    delete(build);
                }
            });
            batch_edit(build, ui);
        }
    }
    // A finished drag or typed value becomes one undo step.
    let busy = ui.ctx().egui_is_using_pointer() || ui.ctx().memory(|m| m.focused().is_some());
    if !busy && build.drag_free() && build.editing_live() {
        build.commit_preview();
    }
}

/// A colour button that changes `color` only when the person picks one:
/// egui's picker hands back a slightly different colour each frame
/// (sRGB conversion), which would otherwise look like an edit.
fn color_button(ui: &mut Ui, color: &mut [f32; 3]) -> bool {
    let mut shown = *color;
    let changed = ui.color_edit_button_rgb(&mut shown).changed();
    if changed {
        *color = shown.map(|c| c.clamp(0.0, 1.0));
    }
    changed
}

/// A group's pivot: where it is and how it is turned. Moving or turning
/// it moves everything inside; Centre pivot moves only the pivot.
fn pivot_section(build: &mut BuildMode, ui: &mut Ui, id: NodeId, node: &Node, locked: bool) {
    section(ui, "Pivot");
    ui.add_enabled_ui(!locked, |ui| {
        let mut model = node.model;
        if ui
            .checkbox(&mut model, "Model")
            .on_hover_text("One thing (a car, a house): a click in the world selects the whole group, Alt+click an object inside. Off: a folder, a click picks the object.")
            .changed()
        {
            run(
                build,
                ops::modify(&build.doc, if model { "Make model" } else { "Make folder" }, &[id], |n| {
                    n.model = model
                }),
            );
        }
        if node.transform.is_unmoved() {
            ui.label(
                RichText::new("No pivot yet: it turns and moves about the middle of its contents. Centre pivot (or the Move or Rotate tool) gives it one.")
                    .small()
                    .color(MUTED),
            );
        }
        let mut t = node.transform;
        let mut changed = axis_row(ui, "Position", &mut t.position, 0.05, " m");
        changed |= axis_row(ui, "Rotation", &mut t.rotation, 0.5, "°");
        if changed && t.is_valid() {
            build.preview("Move model", &[id], |n| n.transform = t);
        }
        ui.horizontal(|ui| {
            if ui
                .small_button("Centre pivot")
                .on_hover_text("Puts the pivot in the middle of the contents, at their base, without moving them")
                .clicked()
            {
                run(build, ops::center_pivot(&build.doc, id));
            }
            if !node.transform.is_unmoved()
                && ui
                    .small_button("Reset rotation")
                    .on_hover_text("Turns the model back straight, with everything in it")
                    .clicked()
            {
                run(
                    build,
                    ops::modify(&build.doc, "Reset rotation", &[id], |n| {
                        n.transform.rotation = [0.0; 3]
                    }),
                );
            }
        });
    });
}

/// A model saved to My Models, or a copy of one: save, keep the copies
/// the same, or go back to the saved version.
fn saved_model_section(build: &mut BuildMode, ui: &mut Ui, id: NodeId, node: &Node, locked: bool) {
    section(ui, "My Models");
    let Some(path) = node.saved_model.clone() else {
        ui.add_enabled_ui(!locked, |ui| {
            if ui
                .button("💾 Save to My Models")
                .on_hover_text("Saves this model to the Toolbox (My Models) to place it again. Copies stay linked: change one and save, and all of them change.")
                .clicked()
            {
                build.save_to_my_models(id);
            }
        });
        return;
    };
    let copies = ops::copies_of(&build.doc, &path).len();
    let saved = build.saved_model(&path);
    match &saved {
        Ok(saved) => {
            ui.label(format!("Copy of {} · {copies} in this world", saved.name()))
                .on_hover_text(&path);
        }
        Err(error) => {
            ui.label(
                RichText::new(format!("The saved model is missing or broken: {error}"))
                    .small()
                    .color(WARNING),
            );
        }
    }
    let changed = saved
        .as_ref()
        .is_ok_and(|s| !s.matches(&build.doc.layout, id));
    if changed {
        ui.label(
            RichText::new("Changed here: save to update every copy, or revert")
                .small()
                .color(WARNING),
        );
    }
    ui.add_enabled_ui(!locked, |ui| {
        ui.horizontal_wrapped(|ui| {
            if (changed || saved.is_err())
                && ui
                    .small_button("Save to all copies")
                    .on_hover_text("Writes this copy to My Models and gives every other copy the same contents; each keeps its own place")
                    .clicked()
            {
                build.save_to_all_copies(id);
            }
            if changed
                && ui
                    .small_button("Revert")
                    .on_hover_text("Throws away the changes made to this copy")
                    .clicked()
            {
                build.revert_copy(id);
            }
            if ui
                .small_button("Unlink")
                .on_hover_text("Makes this an ordinary model: saving other copies no longer changes it")
                .clicked()
            {
                build.unlink_copy(id);
            }
        });
    });
}

/// Quick colours for parts, one click each (the colour button picks any).
const SWATCHES: [(&str, [f32; 3]); 16] = [
    ("White", [0.95, 0.95, 0.93]),
    ("Light grey", [0.64, 0.64, 0.66]),
    ("Dark grey", [0.3, 0.31, 0.33]),
    ("Black", [0.07, 0.07, 0.08]),
    ("Red", [0.78, 0.12, 0.1]),
    ("Orange", [0.93, 0.45, 0.1]),
    ("Yellow", [0.96, 0.8, 0.18]),
    ("Lime", [0.55, 0.82, 0.2]),
    ("Green", [0.16, 0.5, 0.22]),
    ("Teal", [0.1, 0.55, 0.55]),
    ("Sky", [0.4, 0.68, 0.92]),
    ("Blue", [0.13, 0.3, 0.75]),
    ("Purple", [0.45, 0.22, 0.7]),
    ("Pink", [0.92, 0.5, 0.7]),
    ("Brown", [0.45, 0.29, 0.16]),
    ("Sand", [0.84, 0.75, 0.55]),
];

fn swatch(ui: &mut Ui, name: &str, color: [f32; 3], current: bool) -> bool {
    let [r, g, b] = color.map(|c| (c.clamp(0.0, 1.0) * 255.0) as u8);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::click());
    ui.painter()
        .rect_filled(rect, 3.0, Color32::from_rgb(r, g, b));
    if current || response.hovered() {
        ui.painter().rect_stroke(
            rect,
            3.0,
            egui::Stroke::new(1.5, Color32::WHITE),
            egui::StrokeKind::Outside,
        );
    }
    response.on_hover_text(name).clicked()
}

/// What a click in the part editor changes.
enum PartEdit {
    Shape(sa_scene::world_layout::PartShape),
    Color([f32; 3]),
    Material(sa_scene::world_layout::PartMaterial),
}

/// Shape, colour and material pickers for one part (or the first of
/// several); returns the change picked.
fn part_pickers(ui: &mut Ui, part: Option<&sa_scene::world_layout::Part>) -> Option<PartEdit> {
    use sa_scene::world_layout::{PartMaterial, PartShape};
    let mut edit = None;
    let caption = |ui: &mut Ui, text: &str| {
        ui.add_sized(
            [58.0, 20.0],
            egui::Label::new(RichText::new(text).color(MUTED)),
        );
    };
    ui.horizontal_wrapped(|ui| {
        caption(ui, "Shape");
        for shape in PartShape::ALL {
            let on = part.is_some_and(|p| p.shape == shape);
            if ui.selectable_label(on, shape.label()).clicked() && !on {
                edit = Some(PartEdit::Shape(shape));
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        caption(ui, "Colour");
        if let Some(part) = part {
            let mut color = part.color;
            if color_button(ui, &mut color) {
                edit = Some(PartEdit::Color(color));
            }
        }
        ui.spacing_mut().item_spacing.x = 3.0;
        for (name, color) in SWATCHES {
            if swatch(ui, name, color, part.is_some_and(|p| p.color == color)) {
                edit = Some(PartEdit::Color(color));
            }
        }
    });
    ui.horizontal_wrapped(|ui| {
        caption(ui, "Material");
        for material in PartMaterial::ALL {
            let on = part.is_some_and(|p| p.material == material);
            if ui.selectable_label(on, material.label()).clicked() && !on {
                edit = Some(PartEdit::Material(material));
            }
        }
    });
    edit
}

fn apply_part_edit(part: &mut sa_scene::world_layout::Part, edit: &PartEdit) {
    match edit {
        PartEdit::Shape(shape) => part.shape = shape.clone(),
        PartEdit::Color(color) => part.color = color.map(|c| c.clamp(0.0, 1.0)),
        PartEdit::Material(material) => part.material = material.clone(),
    }
}

/// Apply a pick to parts `ids`: a colour drag is one undo step when it
/// ends, like every other Properties drag; other picks are single clicks.
fn edit_parts(build: &mut BuildMode, ids: &[NodeId], edit: PartEdit) {
    let change = |n: &mut Node| {
        if let Some(part) = &mut n.part {
            apply_part_edit(part, &edit);
        }
    };
    if matches!(edit, PartEdit::Color(_)) {
        build.preview("Part colour", ids, change);
    } else {
        run(build, ops::modify(&build.doc, "Edit part", ids, change));
    }
}

/// A part: its shape, colour, material and transparency (its size is in
/// Transform).
fn part_section(build: &mut BuildMode, ui: &mut Ui, id: NodeId, node: &Node) {
    let Some(part) = &node.part else {
        return;
    };
    section(ui, "Part");
    let locked = build.doc.layout.effective_locked(id);
    let mut transparency = part.transparency;
    let mut edit = None;
    ui.add_enabled_ui(!locked, |ui| {
        edit = part_pickers(ui, Some(part));
        ui.add(
            egui::Slider::new(&mut transparency, 0.0..=1.0)
                .fixed_decimals(2)
                .text("transparency"),
        )
        .on_hover_text("0 solid, 1 invisible. Still solid to walk on unless Collision is off.");
        let mut anchored = node.anchored;
        if ui
            .checkbox(&mut anchored, "Anchored")
            .on_hover_text("Stays where you put it. Off: when the world is played it falls and lands on what is below it (the ground, objects, other parts).")
            .changed()
        {
            run(
                build,
                ops::modify(&build.doc, if anchored { "Anchor" } else { "Unanchor" }, &[id], |n| {
                    n.anchored = anchored
                }),
            );
        }
    });
    for (what, name) in [
        (
            "Shape",
            match &part.shape {
                sa_scene::world_layout::PartShape::Other(name) => Some(name),
                _ => None,
            },
        ),
        (
            "Material",
            match &part.material {
                sa_scene::world_layout::PartMaterial::Other(name) => Some(name),
                _ => None,
            },
        ),
    ] {
        if let Some(name) = name {
            ui.label(
                RichText::new(format!(
                    "{what} \"{name}\" is from a newer SARE; drawn as the default."
                ))
                .small()
                .color(WARNING),
            );
        }
    }
    if transparency != part.transparency {
        build.preview("Part transparency", &[id], |n| {
            if let Some(part) = &mut n.part {
                part.transparency = transparency.clamp(0.0, 1.0);
            }
        });
    }
    if let Some(edit) = edit {
        edit_parts(build, &[id], edit);
    }
}

/// A terrain object: its size and colour, and how to sculpt it.
fn terrain_section(build: &mut BuildMode, ui: &mut Ui, id: NodeId, node: &Node) {
    let Some(terrain) = &node.terrain else {
        return;
    };
    section(ui, "Terrain");
    ui.label(format!(
        "{0:.0} × {0:.0} m in {1} × {1} squares of {2:.1} m",
        terrain.size,
        terrain.cells,
        terrain.spacing()
    ));
    let (low, high) = terrain
        .heights
        .iter()
        .fold((f32::MAX, f32::MIN), |(lo, hi), h| (lo.min(*h), hi.max(*h)));
    ui.label(format!("Heights {low:.1} m to {high:.1} m"));
    let holes = terrain
        .paint
        .iter()
        .filter(|m| **m == sa_scene::world_layout::Terrain::HOLE)
        .count();
    let painted = terrain.paint.iter().filter(|m| **m != 0).count() - holes;
    if painted + holes > 0 {
        ui.label(format!("{painted} squares painted, {holes} cut out"));
    }
    let locked = build.doc.layout.effective_locked(id);
    let mut edited = terrain.clone();
    ui.add_enabled_ui(!locked, |ui| {
        ui.horizontal(|ui| {
            ui.label("Colour");
            color_button(ui, &mut edited.color);
            if ui.small_button("Make flat").clicked() {
                edited.heights.iter_mut().for_each(|h| *h = 0.0);
            }
            if !edited.paint.is_empty()
                && ui
                    .small_button("Clear paint")
                    .on_hover_text("Back to the terrain's own colour everywhere, holes filled")
                    .clicked()
            {
                edited.paint.clear();
            }
            if ui
                .button("^ Sculpt")
                .on_hover_text("Sculpt tool (T)")
                .clicked()
            {
                build.tool = super::Tool::Sculpt;
            }
        });
    });
    ui.label(
        RichText::new(
            "Sculpt (T): hold the left mouse button on the terrain to raise, lower, smooth, flatten or paint it, or cut holes. Each stroke is one undo step.",
        )
        .small()
        .color(MUTED),
    );
    edited.color = edited.color.map(|c| c.clamp(0.0, 1.0));
    // A colour drag is one undo step, like every other Inspector drag.
    if edited.heights == terrain.heights && edited.paint == terrain.paint && edited != *terrain {
        build.preview("Edit terrain", &[id], |n| n.terrain = Some(edited.clone()));
        return;
    }
    if edited != *terrain {
        run(
            build,
            ops::modify(&build.doc, "Edit terrain", &[id], |n| {
                n.terrain = Some(edited.clone())
            }),
        );
    }
}

/// A road object: its width and points, and drawing more of it.
fn road_section(build: &mut BuildMode, ui: &mut Ui, id: NodeId, node: &Node) {
    use sa_scene::world_layout::Road;
    let Some(road) = &node.road else {
        return;
    };
    section(ui, "Road");
    ui.label(format!(
        "{} points, {:.0} m long",
        road.points.len(),
        road.length()
    ));
    let locked = build.doc.layout.effective_locked(id);
    let mut edited = road.clone();
    let mut remove = None;
    ui.add_enabled_ui(!locked, |ui| {
        ui.horizontal(|ui| {
            ui.label("Width");
            ui.add(
                egui::DragValue::new(&mut edited.width)
                    .range(Road::MIN_WIDTH..=Road::MAX_WIDTH)
                    .speed(0.1)
                    .suffix(" m"),
            );
        });
        ui.horizontal(|ui| {
            let drawing = build.ui.road_draw == Some(id);
            if ui
                .selectable_label(drawing, "✏ Draw points")
                .on_hover_text(
                    "Click in the world to add points at the end of the road (Esc stops)",
                )
                .clicked()
            {
                build.ui.road_draw = if drawing { None } else { Some(id) };
            }
            let next = edited.extended(16.0);
            let room = edited.points.len() < Road::MAX_POINTS
                && next.iter().all(|v| v.abs() <= Road::MAX_REACH);
            if ui
                .add_enabled(room, egui::Button::new("+ Add point").small())
                .on_hover_text("A point 16 m on from the last one")
                .on_disabled_hover_text(format!(
                    "A road has at most {} points, each with X, Y and Z within ±{} m",
                    Road::MAX_POINTS,
                    Road::MAX_REACH
                ))
                .clicked()
            {
                edited.points.push(next);
            }
        });
        ui.add_enabled(
            edited.points.len() >= 3,
            egui::Checkbox::new(&mut edited.closed, "Closed loop"),
        )
        .on_hover_text("The road runs on from its last point back to its first (a ring road, a race track). In Draw points, clicking the first point closes it too.")
        .on_disabled_hover_text("A loop needs three points or more");
        egui::Grid::new(("road-points", id))
            .num_columns(5)
            .spacing([4.0, 2.0])
            .show(ui, |ui| {
                for (i, point) in edited.points.iter_mut().enumerate() {
                    ui.label(RichText::new(format!("{}", i + 1)).color(MUTED));
                    for value in point.iter_mut() {
                        ui.add(
                            egui::DragValue::new(value)
                                .range(-Road::MAX_REACH..=Road::MAX_REACH)
                                .speed(0.1)
                                .max_decimals(2),
                        );
                    }
                    if ui
                        .add_enabled(road.points.len() > 2, egui::Button::new("×").small())
                        .on_hover_text("Remove this point")
                        .clicked()
                    {
                        remove = Some(i);
                    }
                    ui.end_row();
                }
            });
    });
    ui.label(
        RichText::new(
            "Drag the white points in the world to bend the road (Snap keeps them on the grid). Points are in the road's own metres: X east, Y north, Z up.",
        )
        .small()
        .color(MUTED),
    );
    if let Some(i) = remove {
        edited.points.remove(i);
    }
    // Two points cannot make a loop: unticked, so a point added later does
    // not close it again unasked.
    if edited.points.len() < 3 {
        edited.closed = false;
    }
    if !edited.is_valid() || edited == *road {
        return;
    }
    if edited.points.len() == road.points.len() && edited.closed == road.closed {
        // A drag is one undo step, like every other Properties drag.
        build.preview("Edit road", &[id], |n| n.road = Some(edited.clone()));
    } else {
        run(
            build,
            ops::modify(&build.doc, "Edit road", &[id], |n| {
                n.road = Some(edited.clone())
            }),
        );
    }
}

/// The object's light: a corona and a pool of light on the ground.
fn light_section(build: &mut BuildMode, ui: &mut Ui, id: NodeId, node: &Node) {
    use sa_scene::world_layout::PointLight;
    section(ui, "Light");
    let locked = build.doc.layout.effective_locked(id);
    let Some(light) = &node.light else {
        if ui
            .add_enabled(!locked, egui::Button::new("+ Light"))
            .on_hover_text("A glowing lamp and a pool of light on the ground, from dusk to dawn")
            .clicked()
        {
            run(
                build,
                ops::modify(&build.doc, "Add light", &[id], |n| {
                    n.light = Some(PointLight::lamp())
                }),
            );
        }
        return;
    };
    let mut edited = light.clone();
    ui.add_enabled_ui(!locked, |ui| {
        ui.horizontal(|ui| {
            ui.label("Colour");
            color_button(ui, &mut edited.color);
            if ui.small_button("Remove").clicked() {
                run(
                    build,
                    ops::modify(&build.doc, "Remove light", &[id], |n| n.light = None),
                );
            }
        });
        ui.add(egui::Slider::new(&mut edited.size, 0.1..=PointLight::MAX_SIZE).text("glow (m)"));
        ui.add(
            egui::Slider::new(&mut edited.radius, 0.0..=PointLight::MAX_RADIUS)
                .text("ground light (m)"),
        );
        axis_row(ui, "Lamp at", &mut edited.offset, 0.05, " m");
    });
    ui.label(
        RichText::new(
            "Glows from dusk to dawn: set a fixed time of day after 20:00 to see it. It does not light up other objects.",
        )
        .small()
        .color(MUTED),
    );
    edited.color = edited.color.map(|c| c.clamp(0.0, 1.0));
    if edited != *light && edited.is_valid() && build.doc.layout.node(id).is_some() {
        build.preview("Edit light", &[id], |n| n.light = Some(edited.clone()));
    }
}

/// A change applied to every selected object.
type NodeEdit = fn(&mut Node);

/// Several objects selected: change them all at once (one undo step each).
fn batch_edit(build: &mut BuildMode, ui: &mut Ui) {
    let ids = build.selection.clone();
    let count = |get: fn(&Node) -> bool| {
        ids.iter()
            .filter_map(|id| build.doc.layout.node(*id))
            .filter(|n| get(n))
            .count()
    };
    let (visible, locked, solid) = (
        count(|n| n.visible),
        count(|n| n.locked),
        count(|n| n.collision),
    );
    section(ui, "All selected");
    let mut edit: Option<(&str, NodeEdit)> = None;
    let row = |ui: &mut Ui, label: &str, on: usize| {
        ui.label(format!("{label}: {on} of {}", ids.len()));
    };
    ui.horizontal(|ui| {
        row(ui, "Visible", visible);
        if ui.small_button("Show all").clicked() {
            edit = Some(("Show objects", |n| n.visible = true));
        }
        if ui.small_button("Hide all").clicked() {
            edit = Some(("Hide objects", |n| n.visible = false));
        }
    });
    ui.horizontal(|ui| {
        row(ui, "Locked", locked);
        if ui.small_button("Lock all").clicked() {
            edit = Some(("Lock objects", |n| n.locked = true));
        }
        if ui.small_button("Unlock all").clicked() {
            edit = Some(("Unlock objects", |n| n.locked = false));
        }
    });
    ui.horizontal(|ui| {
        row(ui, "Collision", solid);
        if ui.small_button("On").clicked() {
            edit = Some(("Collision on", |n| n.collision = true));
        }
        if ui.small_button("Off").clicked() {
            edit = Some(("Collision off", |n| n.collision = false));
        }
    });
    ui.horizontal(|ui| {
        if ui.small_button("Reset rotation").clicked() {
            edit = Some(("Reset rotation", |n| n.transform.rotation = [0.0; 3]));
        }
        if ui.small_button("Reset scale").clicked() {
            edit = Some(("Reset scale", |n| n.transform.scale = [1.0; 3]));
        }
        if ui.small_button("Drop to ground").clicked() {
            edit = Some(("Drop to ground", |n| n.transform.position[2] = 0.0));
        }
    });
    let parts: Vec<NodeId> = ids
        .iter()
        .copied()
        .filter(|id| {
            !build.doc.layout.effective_locked(*id)
                && build.doc.layout.node(*id).is_some_and(|n| n.part.is_some())
        })
        .collect();
    if !parts.is_empty() {
        section(ui, &format!("Parts ({} of {})", parts.len(), ids.len()));
        // Shows the first part's look; a pick applies to every part.
        let first = build.doc.layout.node(parts[0]).and_then(|n| n.part.clone());
        if let Some(change) = part_pickers(ui, first.as_ref()) {
            edit_parts(build, &parts, change);
        }
    }
    if let Some((label, change)) = edit {
        // Locked objects keep their transform; flags apply to all.
        let transform = label.starts_with("Reset") || label.starts_with("Drop");
        let targets: Vec<NodeId> = ids
            .iter()
            .copied()
            .filter(|id| !transform || !build.doc.layout.effective_locked(*id))
            .collect();
        run(build, ops::modify(&build.doc, label, &targets, change));
    }
}

pub(super) fn axis_row(
    ui: &mut Ui,
    label: &str,
    values: &mut [f32; 3],
    speed: f32,
    suffix: &str,
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized(
            [58.0, 20.0],
            egui::Label::new(RichText::new(label).color(MUTED)),
        );
        for (i, axis) in ["X", "Y", "Z"].iter().enumerate() {
            ui.label(RichText::new(*axis).color(
                [
                    Color32::from_rgb(226, 74, 74),
                    Color32::from_rgb(110, 196, 84),
                    Color32::from_rgb(76, 138, 236),
                ][i],
            ));
            let response = ui.add_sized(
                [62.0, 20.0],
                egui::DragValue::new(&mut values[i])
                    .speed(speed)
                    .max_decimals(3)
                    .suffix(suffix),
            );
            changed |= response.changed();
        }
    });
    changed
}

fn object(build: &mut BuildMode, ui: &mut Ui, id: NodeId) {
    let Some(node) = build.doc.layout.node(id).cloned() else {
        return;
    };
    // Name
    ui.horizontal(|ui| {
        let icon = if node.model {
            "📦"
        } else if node.is_group() {
            "📁"
        } else {
            "■"
        };
        ui.label(RichText::new(icon).size(18.0));
        let text = match &mut build.ui.name_edit {
            Some((edit_id, text)) if *edit_id == id => text,
            slot => {
                *slot = Some((id, node.name.clone()));
                &mut slot.as_mut().unwrap().1
            }
        };
        let response = ui.add(
            egui::TextEdit::singleline(text)
                .desired_width(200.0)
                .font(egui::TextStyle::Heading),
        );
        if response.lost_focus() && *text != node.name {
            let name = text.clone();
            run(build, ops::rename(&build.doc, id, &name));
            build.ui.name_edit = None;
        }
    });
    let locked = build.doc.layout.effective_locked(id);
    if !node.is_group() {
        section(ui, "Transform");
        if locked {
            ui.label(RichText::new("🔒 Locked: unlock it in the Explorer to edit.").color(WARNING));
        }
        ui.add_enabled_ui(!locked, |ui| {
            let mut t = node.transform;
            let mut changed = axis_row(ui, "Position", &mut t.position, 0.05, " m");
            changed |= axis_row(ui, "Rotation", &mut t.rotation, 0.5, "°");
            let before_scale = t.scale;
            // Parts are sized in metres; their scale stays as it is.
            if let Some(part) = &node.part {
                let mut size = part.size;
                if axis_row(ui, "Size", &mut size, 0.05, " m") {
                    let size = size.map(|s| {
                        s.clamp(
                            sa_scene::world_layout::Part::MIN_SIZE,
                            sa_scene::world_layout::Part::MAX_SIZE,
                        )
                    });
                    build.preview("Resize part", &[id], |n| {
                        if let Some(part) = &mut n.part {
                            part.size = size;
                        }
                    });
                }
            }
            if node.part.is_none() && axis_row(ui, "Scale", &mut t.scale, 0.01, "") {
                for (v, old) in t.scale.iter_mut().zip(before_scale) {
                    if v.abs() < 0.01 {
                        *v = 0.01_f32.copysign(if old == 0.0 { 1.0 } else { old });
                    }
                }
                changed = true;
            }
            if changed && t.is_valid() {
                build.preview("Edit transform", &[id], |n| n.transform = t);
            }
            ui.horizontal(|ui| {
                if ui.small_button("Reset rotation").clicked() {
                    run(
                        build,
                        ops::modify(&build.doc, "Reset rotation", &[id], |n| {
                            n.transform.rotation = [0.0; 3]
                        }),
                    );
                }
                if node.part.is_none() && ui.small_button("Reset scale").clicked() {
                    run(
                        build,
                        ops::modify(&build.doc, "Reset scale", &[id], |n| {
                            n.transform.scale = [1.0; 3]
                        }),
                    );
                }
                if ui
                    .small_button("Drop to ground")
                    .on_hover_text("Height 0")
                    .clicked()
                {
                    run(
                        build,
                        ops::modify(&build.doc, "Drop to ground", &[id], |n| {
                            n.transform.position[2] = 0.0
                        }),
                    );
                }
            });
            // Server scripts (an RP job's Config.Locations) use world
            // coordinates; copy this spot ready to paste.
            let vector =
                sa_editor::world_position::vector4(&build.doc.layout.world_transform(id));
            ui.horizontal(|ui| {
                if ui
                    .small_button("📋 Copy position")
                    .on_hover_text(format!(
                        "Copies {vector}\nWorld coordinates and heading, ready to paste into a server resource's Config.Locations (for example an RP job)."
                    ))
                    .clicked()
                {
                    ui.ctx().copy_text(vector.clone());
                    build.info(format!("Copied {vector}"));
                }
                ui.label(RichText::new(&vector).monospace().small().color(MUTED));
            });
        });
    }
    if node.is_group() {
        pivot_section(build, ui, id, &node, locked);
        saved_model_section(build, ui, id, &node, locked);
    }
    section(ui, "Object");
    let mut flags = (node.visible, node.locked, node.collision);
    ui.checkbox(&mut flags.0, "Visible")
        .on_hover_text("Drawn in the world. Hidden objects still collide when Collision is on.");
    ui.checkbox(&mut flags.1, "Locked")
        .on_hover_text("Cannot be picked in the viewport or moved");
    ui.checkbox(&mut flags.2, "Collision")
        .on_hover_text("Players and vehicles bump into its solid meshes");
    if flags != (node.visible, node.locked, node.collision) {
        run(
            build,
            ops::modify(&build.doc, "Object properties", &[id], |n| {
                (n.visible, n.locked, n.collision) = flags;
            }),
        );
    }
    if !node.is_group() {
        part_section(build, ui, id, &node);
        super::gameplay_ui::inspector(build, ui, id, &node);
        light_section(build, ui, id, &node);
        terrain_section(build, ui, id, &node);
        road_section(build, ui, id, &node);
        super::script_ui::object_section(build, ui, id, &node);
    }
    super::attributes_ui::section_ui(build, ui, id, &node, locked);
    if let Some(asset) = &node.asset {
        section(ui, "Asset");
        ui.label(RichText::new(asset.to_string()).monospace().small());
        match build.catalog.find(asset).map(|e| e.status.clone()) {
            Some(AssetStatus::Ready) | None if !build.gpu.is_placeholder(asset) => {
                ui.label(RichText::new("Loaded").color(SUCCESS));
            }
            Some(AssetStatus::Missing(why)) | Some(AssetStatus::Invalid(why)) => {
                ui.label(RichText::new(why).color(ERROR));
            }
            _ => {
                ui.label(
                    RichText::new(
                        "Could not be loaded; shown as a magenta cube. See Errors and warnings.",
                    )
                    .color(ERROR),
                );
            }
        }
        if let Some(object) = build.gpu.objects.get(&id) {
            egui::CollapsingHeader::new("Advanced")
                .default_open(false)
                .show(ui, |ui| {
                    let size = object.bounds.1 - object.bounds.0;
                    ui.label(format!(
                        "Size {:.2} × {:.2} × {:.2} m",
                        size.x, size.z, size.y
                    ));
                    ui.label(format!("{} triangles", object.triangles.len()));
                    ui.label("Materials:");
                    let mut keys: Vec<&str> = object
                        .batches
                        .iter()
                        .map(|b| b.texture_key.as_str())
                        .collect();
                    keys.dedup();
                    for key in keys {
                        ui.label(RichText::new(key).monospace().small().color(MUTED));
                    }
                    ui.label(format!("Object id {id}"));
                    if !node.extra.is_empty() {
                        ui.label("Extra data (kept as is):");
                        for (key, value) in &node.extra {
                            ui.label(RichText::new(format!("{key}: {value}")).monospace().small());
                        }
                    }
                });
        }
    } else {
        let count = build.doc.layout.descendants(id).len();
        ui.label(format!("Group with {count} objects inside. Its Visible, Locked and Collision apply to all of them."));
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui.button("🗐 Duplicate").clicked() {
            duplicate(build);
        }
        if ui.button("🗑 Delete").clicked() {
            delete(build);
        }
    });
}

fn world_settings(build: &mut BuildMode, ui: &mut Ui) {
    ui.label(
        RichText::new(
            "The whole world. Select an object to edit it, or Lighting in the Explorer for the time, weather and sky.",
        )
        .color(MUTED),
    );
    section(ui, "Data");
    let info = build.doc.info.clone();
    ui.label("Name");
    let name = build.ui.world_name.get_or_insert_with(|| info.name.clone());
    if ui.text_edit_singleline(name).lost_focus()
        && name.trim() != info.name
        && !name.trim().is_empty()
    {
        let after = WorldInfo {
            name: name.trim().to_owned(),
            ..info.clone()
        };
        build.ui.world_name = None;
        build.execute(Command::Info {
            before: info.clone(),
            after,
        });
    }
    ui.label("Description");
    let description = build
        .ui
        .world_description
        .get_or_insert_with(|| info.description.clone());
    if ui.text_edit_multiline(description).lost_focus() && *description != info.description {
        let after = WorldInfo {
            description: description.clone(),
            ..info.clone()
        };
        build.ui.world_description = None;
        build.execute(Command::Info {
            before: info.clone(),
            after,
        });
    }
    ui.label(
        RichText::new(format!(
            "Pack id: {} (folder {})",
            build.project.id,
            build.project.folder.display()
        ))
        .small()
        .color(MUTED),
    );
    section(ui, "Spawn point");
    let mut spawn = info.spawn;
    if axis_row(ui, "Spawn", &mut spawn, 0.1, " m") && spawn.iter().all(|v| v.is_finite()) {
        build.execute(Command::Info {
            before: info.clone(),
            after: WorldInfo {
                spawn,
                ..info.clone()
            },
        });
    }
    if ui.button("Set spawn at camera focus").clicked() {
        let focus = sa_scene::world_layout::gltf_to_blender(build.camera.focus);
        let spawn = [focus[0], focus[1], focus[2].max(0.0)];
        build.execute(Command::Info {
            before: info.clone(),
            after: WorldInfo { spawn, ..info },
        });
    }
}

/// The world's colour look: a ready-made one, fine-tuned with sliders.
fn look_section(
    ui: &mut Ui,
    look: Option<sa_scene::world_layout::WorldLook>,
) -> Option<sa_scene::world_layout::WorldLook> {
    use sa_scene::world_layout::WorldLook;
    section(ui, "Look");
    let mut value = look.unwrap_or_default();
    let shown = value.preset_name().unwrap_or("Custom");
    ui.horizontal(|ui| {
        ui.label("Look");
        egui::ComboBox::from_id_salt("world-look")
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for (name, preset) in WorldLook::presets() {
                    ui.selectable_value(&mut value, preset, name);
                }
            })
            .response
            .on_hover_text("How the world's colours look on every player's screen");
    });
    egui::CollapsingHeader::new("Adjust the look")
        .id_salt("world-look-adjust")
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut value.exposure, 0.5..=2.0).text("exposure"));
            ui.add(egui::Slider::new(&mut value.contrast, 0.5..=1.6).text("contrast"));
            ui.add(egui::Slider::new(&mut value.saturation, 0.0..=2.0).text("colour"));
            ui.add(
                egui::Slider::new(&mut value.warmth, -1.0..=1.0)
                    .text("warmth")
                    .custom_formatter(|v, _| match v {
                        v if v < -0.01 => format!("cold {:.2}", -v),
                        v if v > 0.01 => format!("warm {v:.2}"),
                        _ => "neutral".into(),
                    })
                    .custom_parser(|text| {
                        let text = text.trim().to_lowercase();
                        if text == "neutral" {
                            return Some(0.0);
                        }
                        if let Some(v) = text.strip_prefix("cold") {
                            return v.trim().parse::<f64>().ok().map(|v| -v.abs());
                        }
                        text.trim_start_matches("warm").trim().parse::<f64>().ok()
                    }),
            );
            ui.add(egui::Slider::new(&mut value.bloom, 0.0..=3.0).text("× glow"));
            ui.add(egui::Slider::new(&mut value.vignette, 0.0..=0.8).text("dark corners"));
        });
    ui.label(
        RichText::new("Works on top of each player's own picture settings: a player who turned glow off keeps it off, so any look runs on any PC.")
            .small()
            .color(MUTED),
    );
    (value != WorldLook::default()).then_some(value)
}

/// The Explorer's Lighting row: the world's time, weather, fog, sky and
/// brightness.
fn lighting(build: &mut BuildMode, ui: &mut Ui) {
    section(ui, "Environment");
    let settings = build.doc.layout.settings.clone();
    let mut fixed = settings.hour.is_some();
    let mut hour = settings.hour.unwrap_or(12);
    ui.checkbox(&mut fixed, "Fixed time of day")
        .on_hover_text("Off: the clock runs while playing");
    ui.add_enabled(fixed, egui::Slider::new(&mut hour, 0..=23).text("hour"));
    let mut weather = settings.weather.clone();
    {
        use sa_scene::world_logic::Weather;
        let shown = match weather.as_deref() {
            None => "Game default".to_owned(),
            Some(text) => {
                Weather::parse(text).map_or_else(|| text.to_owned(), |w| w.label().into())
            }
        };
        ui.horizontal(|ui| {
            ui.label("Weather");
            egui::ComboBox::from_id_salt("world-weather")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut weather, None, "Game default");
                    for option in Weather::ALL {
                        ui.selectable_value(
                            &mut weather,
                            Some(option.as_str().to_owned()),
                            option.label(),
                        );
                    }
                });
        });
    }
    // The renderer's own haze, sky colour and ambient light.
    let mut fog = settings.fog;
    let mut sky = settings.sky;
    let mut brightness = settings.brightness;
    ui.horizontal(|ui| {
        let mut own = fog.is_some();
        ui.checkbox(&mut own, "Fog");
        let mut value = fog.unwrap_or(1.0);
        ui.add_enabled(
            own,
            egui::Slider::new(&mut value, 0.2..=10.0)
                .logarithmic(true)
                .text("× density"),
        );
        fog = own.then_some(value);
    });
    ui.horizontal(|ui| {
        let mut own = sky.is_some();
        ui.checkbox(&mut own, "Sky colour")
            .on_hover_text("Also the colour of the haze in the distance");
        let mut value = sky.unwrap_or([0.55, 0.7, 0.9]);
        ui.add_enabled_ui(own, |ui| color_button(ui, &mut value));
        sky = own.then_some(value.map(|c| c.clamp(0.0, 1.0)));
    });
    ui.horizontal(|ui| {
        let mut own = brightness.is_some();
        ui.checkbox(&mut own, "Brightness");
        let mut value = brightness.unwrap_or(1.0);
        ui.add_enabled(own, egui::Slider::new(&mut value, 0.3..=1.5));
        brightness = own.then_some(value);
    });
    let look = look_section(ui, settings.look);
    let after = WorldSettings {
        hour: fixed.then_some(hour),
        weather,
        fog,
        sky,
        brightness,
        look,
        ..settings.clone()
    };
    let dragging = ui.ctx().egui_is_using_pointer();
    if after != settings {
        if dragging {
            build.ui.settings_before.get_or_insert(settings.clone());
            build.doc.layout.settings = after;
        } else {
            build.execute(Command::Settings {
                before: settings,
                after,
            });
        }
    }
    if !dragging {
        if let Some(before) = build.ui.settings_before.take() {
            let after = build.doc.layout.settings.clone();
            if before != after {
                build.history.record(Command::Settings { before, after });
            }
        }
    }
    ui.label(
        RichText::new("Shown here while you edit and used when the world is played or hosted. Gameplay actions can change the time and weather while playing. Lights on objects glow from dusk to dawn.")
            .small()
            .color(MUTED),
    );
}

impl BuildMode {
    fn drag_free(&self) -> bool {
        self.drag.is_none()
    }
}

/// Where new worlds go: beside the one being edited.
fn worlds_folder(build: &BuildMode) -> std::path::PathBuf {
    build
        .project
        .folder
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_else(|| sa_scene::content_pack::DEFAULT_DIR.into())
}

/// Every world (content pack) that can be opened, by name.
fn worlds(build: &BuildMode) -> Vec<(std::path::PathBuf, String)> {
    let mut dirs = vec![worlds_folder(build)];
    dirs.extend(sa_scene::content::pack_dirs());
    let mut found: Vec<(std::path::PathBuf, String)> = Vec::new();
    for dir in dirs {
        for (folder, manifest) in sa_scene::content_pack::list(&dir) {
            let same = |a: &std::path::Path| {
                std::fs::canonicalize(a).ok() == std::fs::canonicalize(&folder).ok()
            };
            if same(&build.project.folder) || found.iter().any(|(f, _)| same(f)) {
                continue;
            }
            found.push((folder, manifest.name));
        }
    }
    found
}

/// Open another world, asking first about unsaved changes.
fn switch_world(build: &mut BuildMode, folder: std::path::PathBuf) {
    if build.history.dirty() {
        build.ui.confirm_switch = Some(folder);
    } else {
        build.switch_to = Some(folder);
    }
}

fn world_dialogs(build: &mut BuildMode, ctx: &egui::Context) {
    if let Some(name) = &mut build.ui.new_world {
        let mut action = None;
        egui::Window::new("New world")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                compact(ui);
                ui.label("A new, empty world with a ground plate. It opens right away.");
                let field = ui.add(
                    egui::TextEdit::singleline(name)
                        .hint_text("My World")
                        .desired_width(260.0),
                );
                let enter = (field.has_focus() || field.lost_focus())
                    && ui.input(|i| i.key_pressed(Key::Enter));
                if !field.lost_focus() {
                    field.request_focus();
                }
                ui.horizontal(|ui| {
                    if ui.button("Create").clicked() || enter {
                        action = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        action = Some(false);
                    }
                });
            });
        match action {
            Some(true) => {
                let name = match name.trim() {
                    "" => "My World".to_owned(),
                    name => name.to_owned(),
                };
                let root = worlds_folder(build);
                let id = sa_editor::project::new_id(&name, &root);
                match sa_editor::project::Project::create(&root.join(&id), &id, &name) {
                    Ok((project, _)) => {
                        build.info(format!("Created {}", project.folder.display()));
                        build.ui.new_world = None;
                        switch_world(build, project.folder);
                    }
                    Err(error) => build.error(format!("Could not create the world: {error:#}")),
                }
            }
            Some(false) => build.ui.new_world = None,
            None => {}
        }
    }
    if let Some(found) = &build.ui.open_world {
        let mut chosen = None;
        let mut open = true;
        egui::Window::new("Open world")
            .collapsible(false)
            .open(&mut open)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                compact(ui);
                if found.is_empty() {
                    ui.label(
                        RichText::new("No other worlds found. Make one with File → New world.")
                            .color(MUTED),
                    );
                }
                egui::ScrollArea::vertical()
                    .max_height(360.0)
                    .show(ui, |ui| {
                        for (folder, name) in found {
                            let label = format!(
                                "{name}   {}",
                                folder
                                    .file_name()
                                    .map(|f| f.to_string_lossy())
                                    .unwrap_or_default()
                            );
                            if ui
                                .add_sized([320.0, 24.0], egui::Button::new(label))
                                .on_hover_text(folder.display().to_string())
                                .clicked()
                            {
                                chosen = Some(folder.clone());
                            }
                        }
                    });
            });
        if let Some(folder) = chosen {
            build.ui.open_world = None;
            switch_world(build, folder);
        } else if !open {
            build.ui.open_world = None;
        }
    }
    if let Some(folder) = build.ui.confirm_switch.clone() {
        egui::Window::new("Save changes first?")
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                compact(ui);
                ui.label("This world has unsaved changes.");
                ui.horizontal(|ui| {
                    if ui.button("Save and open").clicked() {
                        build.save();
                        if !build.history.dirty() {
                            build.switch_to = Some(folder.clone());
                        }
                        build.ui.confirm_switch = None;
                    }
                    if ui
                        .button("Open without saving")
                        .on_hover_text("The autosave keeps your work for recovery")
                        .clicked()
                    {
                        build.autosave();
                        build.switch_to = Some(folder.clone());
                        build.ui.confirm_switch = None;
                    }
                    if ui.button("Cancel").clicked() {
                        build.ui.confirm_switch = None;
                    }
                });
            });
    }
}

fn dialogs(build: &mut BuildMode, root: &mut Ui) {
    super::script_ui::window(build, &root.ctx().clone());
    let ctx = root.ctx().clone();
    world_dialogs(build, &ctx);
    if let Some(recovery) = &build.recovery {
        let saved = recovery.document.layout.nodes.len();
        let mut choice = None;
        egui::Window::new("Recover unsaved work?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(&ctx, |ui| {
                compact(ui);
                ui.label(format!(
                    "SARE closed before this world was saved. The autosave has {saved} objects."
                ));
                ui.horizontal(|ui| {
                    if ui.button(RichText::new("Recover").strong()).clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Discard autosave").clicked() {
                        choice = Some(false);
                    }
                });
            });
        if let Some(keep) = choice {
            build.recover(keep);
        }
    }
    if let Some(name) = &mut build.ui.save_as {
        let mut action = None;
        egui::Window::new("Save As")
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(&ctx, |ui| {
                compact(ui);
                ui.label("Save a copy of this world (with its imported models) under a new name. You continue in the copy.");
                ui.text_edit_singleline(name);
                ui.horizontal(|ui| {
                    if ui.button("Save copy").clicked() && !name.trim().is_empty() {
                        action = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        action = Some(false);
                    }
                });
            });
        match action {
            Some(true) => {
                let name = name.clone();
                build.ui.save_as = None;
                build.save_as(&name);
            }
            Some(false) => build.ui.save_as = None,
            None => {}
        }
    }
    if let Some((path, category)) = &mut build.ui.import {
        let mut action = None;
        egui::Window::new("Import model")
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(&ctx, |ui| {
                compact(ui);
                ui.label("Path to a .glb file on this computer. It is copied into this world's assets folder, so the world stays portable. Tip: you can also drag .glb files from your file explorer onto this window.");
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(path).hint_text("C:/Users/me/Desktop/bench.glb").desired_width(300.0));
                    if ui.button("Browse…").on_hover_text("Choose the file with your system's file picker").clicked() {
                        if let Some(picked) = pick_model() {
                            *path = picked;
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Category");
                    ui.text_edit_singleline(category);
                });
                ui.horizontal(|ui| {
                    if ui.button("Import").clicked() {
                        action = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        action = Some(false);
                    }
                });
            });
        match action {
            Some(true) => {
                let source = std::path::PathBuf::from(path.trim().trim_matches('"'));
                let category = category.clone();
                match build.project.import_model(&source, &category) {
                    Ok(asset) => {
                        build.info(format!("Imported {asset}"));
                        build.refresh_catalog();
                        build.ui.import = None;
                        let label = source
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        build.placing = Some(Placing {
                            asset,
                            label,
                            turn: 0.0,
                            dragged: false,
                        });
                    }
                    Err(error) => build.error(format!("Import failed: {error:#}")),
                }
            }
            Some(false) => build.ui.import = None,
            None => {}
        }
    }
    if build.ui.confirm_quit {
        egui::Window::new("Quit Build Mode?")
            .collapsible(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(&ctx, |ui| {
                compact(ui);
                ui.label("This world has unsaved changes.");
                ui.horizontal(|ui| {
                    if ui.button("Save and quit").clicked() {
                        build.save();
                        if !build.history.dirty() {
                            build.requests.push(Request::Quit);
                        }
                        build.ui.confirm_quit = false;
                    }
                    if ui
                        .button("Quit without saving")
                        .on_hover_text("The autosave keeps your work for recovery")
                        .clicked()
                    {
                        build.autosave();
                        build.requests.push(Request::Quit);
                        build.ui.confirm_quit = false;
                    }
                    if ui.button("Cancel").clicked() {
                        build.ui.confirm_quit = false;
                    }
                });
            });
    }
    if build.ui.show_help {
        let mut open = true;
        egui::Window::new("Build Mode controls")
            .open(&mut open)
            .show(&ctx, |ui| {
                compact(ui);
                for (keys, what) in [
                    (
                        "Right mouse + WASD/QE",
                        "Fly (Shift faster, wheel changes speed)",
                    ),
                    ("Middle mouse drag", "Pan"),
                    ("Alt + left drag", "Orbit"),
                    ("Mouse wheel", "Zoom"),
                    ("F / double-click", "Focus selection"),
                    ("Home", "Reset camera"),
                    ("Ctrl+1 2 3 4", "Select, Move, Scale, Rotate"),
                    ("Q W E R", "Select, Move, Rotate, Scale"),
                    ("L / Ctrl+L", "World or local gizmo axes"),
                    ("Ctrl+C / X / V", "Copy / cut / paste"),
                    ("F5", "Play (Playtest)"),
                    ("G", "Snap on or off (Ctrl while dragging inverts it)"),
                    ("Ctrl+click", "Add to or remove from selection"),
                    ("Ctrl+D / Del", "Duplicate / delete"),
                    ("Ctrl+N / Ctrl+O", "New world / open another world"),
                    ("Ctrl+G / Ctrl+Shift+G", "Group / ungroup"),
                    ("Ctrl+Z / Ctrl+Y", "Undo / redo"),
                    ("Ctrl+S", "Save"),
                    ("[ and ]", "Turn the object being placed"),
                    ("F6", "Playtest and back"),
                ] {
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            [170.0, 18.0],
                            egui::Label::new(RichText::new(keys).strong()),
                        );
                        ui.label(what);
                    });
                }
            });
        build.ui.show_help = open;
    }
}

/// While playtesting: a small banner with the way back.
pub fn playtest_overlay(build: &mut BuildMode, root: &mut Ui) {
    egui::Area::new("build-playtest".into())
        .anchor(egui::Align2::CENTER_TOP, [0.0, 10.0])
        .show(root.ctx(), |ui| {
            compact(ui);
            egui::Frame::popup(ui.style())
                .fill(BACKGROUND.gamma_multiply(0.9))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("▶ Playtest").color(SUCCESS).strong());
                        ui.label(
                            RichText::new(format!(
                                "{}: press F6 to return to Build Mode",
                                build.doc.info.name
                            ))
                            .color(TEXT),
                        );
                        if ui.button("Back to Build Mode").clicked() {
                            build.requests.push(Request::Playtest);
                        }
                    });
                });
        });
}
