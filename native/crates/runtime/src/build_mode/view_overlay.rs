//! What floats on the 3D view, as in a game engine: a toolbar with the
//! tools and their switches along the top, and an axis gizmo in the corner
//! that shows which way the camera looks (click an axis to look along it).
use super::{BuildMode, Tool};
use crate::ui_theme::{ACCENT, BACKGROUND, TEXT};
use egui::{Color32, Pos2, RichText, Stroke, Ui, Vec2};
use glam::Vec3;

const X_COLOR: Color32 = Color32::from_rgb(230, 80, 80);
const Y_COLOR: Color32 = Color32::from_rgb(110, 200, 90);
const Z_COLOR: Color32 = Color32::from_rgb(80, 140, 240);

pub fn show(build: &mut BuildMode, ui: &mut Ui) {
    let rect = ui.max_rect();
    if build.playtesting || rect.width() < 200.0 {
        return;
    }
    toolbar(build, ui, rect);
    axis_gizmo(build, ui, rect);
}

fn toggle(ui: &mut Ui, on: bool, text: &str, hover: &str) -> bool {
    let text = RichText::new(text)
        .size(13.0)
        .color(if on { BACKGROUND } else { TEXT });
    let button = egui::Button::new(text)
        .fill(if on { ACCENT } else { Color32::TRANSPARENT })
        .min_size(Vec2::new(26.0, 22.0));
    ui.add(button).on_hover_text(hover).clicked()
}

fn toolbar(build: &mut BuildMode, ui: &mut Ui, rect: egui::Rect) {
    egui::Area::new(ui.id().with("view toolbar"))
        .fixed_pos(rect.min + Vec2::new(8.0, 8.0))
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(BACKGROUND.gamma_multiply(0.88))
                .inner_margin(egui::Margin::symmetric(4, 3))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.horizontal(|ui| {
                        for (tool, icon, hover) in [
                            (Tool::Select, "⬉", "Select (Q)"),
                            (Tool::Move, "↔", "Move (W)"),
                            (Tool::Rotate, "⟲", "Rotate (E)"),
                            (Tool::Scale, "⛶", "Scale (R)"),
                        ] {
                            if toggle(ui, build.tool == tool, icon, hover) {
                                build.tool = tool;
                            }
                        }
                        ui.separator();
                        if toggle(
                            ui,
                            build.local,
                            if build.local { "Local" } else { "World" },
                            "Gizmo along the world's axes or the object's own (L)",
                        ) {
                            build.local = !build.local;
                        }
                        if toggle(
                            ui,
                            build.ui.pivot_mode,
                            "Pivot",
                            "Edit the pivot (P): move or turn a model's pivot and leave what is inside where it is",
                        ) {
                            build.ui.pivot_mode = !build.ui.pivot_mode;
                        }
                        ui.separator();
                        let snap = if build.snap.enabled {
                            format!("Snap {} m", build.snap.step)
                        } else {
                            "Snap".to_string()
                        };
                        if toggle(ui, build.snap.enabled, &snap, "Snap moves to the grid and turns to steps (G); Ctrl while dragging does the opposite") {
                            build.snap.enabled = !build.snap.enabled;
                        }
                        if toggle(ui, build.snap.show_grid, "#", "Show the grid") {
                            build.snap.show_grid = !build.snap.show_grid;
                        }
                        ui.separator();
                        let any = !build.selection.is_empty();
                        if ui
                            .add_enabled(any, egui::Button::new(RichText::new("Focus").size(13.0)).fill(Color32::TRANSPARENT))
                            .on_hover_text("Frame the selection (F)")
                            .clicked()
                        {
                            build.focus_selection();
                        }
                        if ui
                            .add(egui::Button::new(RichText::new("Home").size(13.0)).fill(Color32::TRANSPARENT))
                            .on_hover_text("Back to the starting view (Home)")
                            .clicked()
                        {
                            build.reset_camera();
                        }
                    });
                });
        });
}

/// The view's axes in the top right: X east (red), Y north (green), Z up
/// (blue). A click on one looks along it, towards the focus.
fn axis_gizmo(build: &mut BuildMode, ui: &mut Ui, rect: egui::Rect) {
    let radius = 34.0;
    let center = Pos2::new(rect.max.x - radius - 14.0, rect.min.y + radius + 14.0);
    let painter = ui.painter_at(rect);
    let camera = &build.camera;
    let right = camera.right();
    let forward = camera.forward();
    let up = right.cross(forward).normalize_or(Vec3::Y);
    // Render space: x east, y up, z south.
    let axes = [
        ("X", Vec3::X, X_COLOR),
        ("Y", Vec3::NEG_Z, Y_COLOR),
        ("Z", Vec3::Y, Z_COLOR),
    ];
    let to_screen = |a: Vec3| center + Vec2::new(a.dot(right), -a.dot(up)) * (radius - 9.0);
    let back = ui.interact(
        egui::Rect::from_center_size(center, Vec2::splat(radius * 2.0)),
        ui.id().with("axis gizmo"),
        egui::Sense::click(),
    );
    painter.circle_filled(
        center,
        radius,
        BACKGROUND.gamma_multiply(if back.hovered() { 0.75 } else { 0.45 }),
    );
    // Far ends first, so near ones draw on top.
    let mut ends: Vec<(f32, &str, Vec3, Color32, bool)> = Vec::new();
    for (name, axis, color) in axes {
        ends.push((axis.dot(forward), name, axis, color, true));
        ends.push((-axis.dot(forward), name, -axis, color, false));
    }
    ends.sort_by(|a, b| b.0.total_cmp(&a.0));
    let pointer = back.hover_pos();
    let mut clicked = None;
    for (_, name, axis, color, positive) in ends {
        let at = to_screen(axis);
        let hovered = pointer.is_some_and(|p| p.distance(at) < 9.0);
        if positive {
            painter.line_segment([center, at], Stroke::new(2.0, color));
            painter.circle_filled(at, if hovered { 9.0 } else { 7.5 }, color);
            painter.text(
                at,
                egui::Align2::CENTER_CENTER,
                name,
                egui::FontId::proportional(10.5),
                Color32::BLACK,
            );
        } else {
            painter.circle_stroke(at, 5.5, Stroke::new(1.5, color.gamma_multiply(0.7)));
        }
        if hovered && back.clicked() {
            clicked = Some(axis);
        }
    }
    back.on_hover_text(
        "Which way the camera looks: X east, Y north, Z up. Click an axis to look from that side; Home goes back.",
    );
    if let Some(axis) = clicked {
        // Look from that side, towards the focus.
        let look = -axis;
        build.camera.pitch = look.y.clamp(-1.0, 1.0).asin().clamp(-1.54, 1.54);
        if look.x.abs() + look.z.abs() > 1e-3 {
            build.camera.yaw = look.x.atan2(look.z);
        }
    }
}
