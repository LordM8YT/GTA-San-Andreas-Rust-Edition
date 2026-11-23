//! The Build Mode viewport: camera controls, picking, placement, transform
//! gizmos, grid and markers. The 3D scene is drawn by the normal renderer
//! behind the interface; this module handles input over it and paints the
//! editor overlays with egui.
use super::{BuildMode, Tool};
use crate::ui_theme;
use egui::{Color32, Pos2, Sense, Stroke, Ui};
use glam::{Mat4, Vec2, Vec3, Vec4};
use sa_editor::ops;
use sa_editor::viewport::{
    angle_around, blender_axis, gizmo_axes, moved, pick, rotated, snap, Pickable, Ray,
};
use sa_scene::world_layout::{gltf_to_blender, Node, NodeId, Transform};
use sa_scene::world_logic::{self, Action, Component};

/// The camera of the frame being drawn.
pub struct Frame {
    pub view_projection: Mat4,
    pub eye: Vec3,
}

const AXIS_COLORS: [Color32; 3] = [
    Color32::from_rgb(226, 74, 74),
    Color32::from_rgb(110, 196, 84),
    Color32::from_rgb(76, 138, 236),
];
/// How close (points) the mouse must be to grab a gizmo handle.
const GRAB: f32 = 9.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Handle {
    Axis(usize),
    /// Move on the ground plane through the object.
    Plane,
    Ring(usize),
    ScaleAxis(usize),
    ScaleAll,
}

pub struct Drag {
    handle: Handle,
    origin: Vec3,
    axis: Vec3,
    /// Axis parameter, plane point or mouse position when grabbed.
    start: f32,
    start_point: Vec3,
    start_mouse: Pos2,
    starts: Vec<(NodeId, Transform)>,
}

struct View {
    view_projection: Mat4,
    size: Vec2,
    eye: Vec3,
}
impl View {
    fn project(&self, p: Vec3) -> Option<Pos2> {
        let clip = self.view_projection * p.extend(1.0);
        if clip.w <= 0.05 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        Some(Pos2::new(
            (ndc.x + 1.0) * 0.5 * self.size.x,
            (1.0 - ndc.y) * 0.5 * self.size.y,
        ))
    }
    /// A 3D segment on screen, cut at the near plane.
    fn segment(&self, a: Vec3, b: Vec3) -> Option<[Pos2; 2]> {
        let (ca, cb) = (
            self.view_projection * a.extend(1.0),
            self.view_projection * b.extend(1.0),
        );
        const NEAR: f32 = 0.05;
        if ca.w <= NEAR && cb.w <= NEAR {
            return None;
        }
        let cut = |inside: Vec4, outside: Vec4| {
            let t = (inside.w - NEAR) / (inside.w - outside.w);
            inside + (outside - inside) * t
        };
        let (ca, cb) = if ca.w <= NEAR {
            (cut(cb, ca), cb)
        } else if cb.w <= NEAR {
            (ca, cut(ca, cb))
        } else {
            (ca, cb)
        };
        let to_screen = |c: Vec4| {
            let ndc = c.truncate() / c.w;
            Pos2::new(
                (ndc.x + 1.0) * 0.5 * self.size.x,
                (1.0 - ndc.y) * 0.5 * self.size.y,
            )
        };
        Some([to_screen(ca), to_screen(cb)])
    }
    fn ray(&self, pointer: Pos2) -> Ray {
        Ray::from_screen(
            Vec2::new(pointer.x, pointer.y),
            self.size,
            self.view_projection,
            self.eye,
        )
    }
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let t = ((p - a).dot(ab) / ab.length_sq().max(1e-6)).clamp(0.0, 1.0);
    p.distance(a + ab * t)
}

/// Run the viewport for one frame inside the central panel.
pub fn show(build: &mut BuildMode, ui: &mut Ui, frame: &Frame) {
    let rect = ui.max_rect();
    let screen = ui.ctx().content_rect();
    let view = View {
        view_projection: frame.view_projection,
        size: Vec2::new(screen.width(), screen.height()),
        eye: frame.eye,
    };
    let response = ui.interact(
        rect,
        ui.id().with("build viewport"),
        Sense::click_and_drag(),
    );
    // The viewport never keeps keyboard focus: shortcuts must keep working.
    if response.has_focus() {
        response.surrender_focus();
    }
    let painter = ui.painter_at(rect);
    let pointer = response.hover_pos();
    let (modifiers, dt) = ui.input(|i| (i.modifiers, i.stable_dt.min(0.1)));

    camera_controls(build, ui, &response, dt);

    if build.snap.show_grid {
        draw_grid(build, &view, &painter);
    }
    if build.ui.show_collision {
        draw_collision(build, &view, &painter);
    }
    if !build.ui.hide_gameplay {
        draw_gameplay(build, &view, &painter);
    }
    draw_spawn(build, &view, &painter);

    // Placement preview follows the mouse until a click places it.
    build.ui.ghost_transform = None;
    if build.placing.as_ref().is_some_and(|p| p.dragged) {
        // Dragged in from the Asset Browser: the drag belongs to the browser
        // tile, so follow the raw pointer and place where it is let go.
        let pointer = ui
            .input(|i| i.pointer.latest_pos())
            .filter(|p| rect.contains(*p));
        let transform = pointer.map(|p| placement_at(build, &view, p));
        build.ui.ghost_transform = transform;
        if !ui.input(|i| i.pointer.primary_down()) {
            match transform {
                Some(transform) => place(build, transform, false),
                None => build.placing = None,
            }
        }
        draw_selection(build, &view, &painter);
        return;
    }
    if build.placing.is_some() {
        if let Some(pointer) = pointer {
            let transform = placement_at(build, &view, pointer);
            build.ui.ghost_transform = Some(transform);
            // Placing goes on until right-click or Esc, so rows of walls or
            // trees are one click each.
            if response.clicked_by(egui::PointerButton::Primary) {
                place(build, transform, true);
            }
        }
        if response.clicked_by(egui::PointerButton::Secondary) {
            build.placing = None;
        }
        draw_selection(build, &view, &painter);
        return;
    }

    let handle_hover = pointer.and_then(|p| gizmo(build, &view, p));
    // Grab a handle, drag, release.
    if response.drag_started_by(egui::PointerButton::Primary) && !modifiers.alt {
        // The handle under the press, not under the already moved mouse.
        if let Some(p) = ui.input(|i| i.pointer.press_origin()) {
            if let Some(handle) = gizmo(build, &view, p) {
                begin_drag(build, &view, handle, p);
            }
        }
    }
    if let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) {
        if build.drag.is_some() && response.dragged_by(egui::PointerButton::Primary) {
            continue_drag(build, &view, pointer, modifiers.ctrl);
        }
    }
    if build.drag.is_some() && !ui.input(|i| i.pointer.primary_down()) {
        build.drag = None;
        build.commit_preview();
    }
    let active = build.drag.as_ref().map(|d| d.handle).or(handle_hover);
    draw_gizmo(build, &view, &painter, active);

    // Click to select (Ctrl adds or removes).
    if response.clicked_by(egui::PointerButton::Primary) && handle_hover.is_none() && !modifiers.alt
    {
        if let Some(pointer) = pointer {
            let hit = pick_object(build, &view.ray(pointer));
            match (hit, modifiers.ctrl || modifiers.command) {
                (Some(id), true) => {
                    if let Some(i) = build.selection.iter().position(|s| *s == id) {
                        build.selection.remove(i);
                    } else {
                        build.selection.push(id);
                    }
                }
                (Some(id), false) => build.selection = vec![id],
                (None, false) => build.selection.clear(),
                (None, true) => {}
            }
        }
    }
    if response.double_clicked() && !build.selection.is_empty() {
        build.focus_selection();
    }
    draw_selection(build, &view, &painter);
    if let Some(pointer) = pointer {
        if let Some(id) = pick_object(build, &view.ray(pointer)) {
            if let Some(node) = build.doc.layout.node(id) {
                response.clone().on_hover_text_at_pointer(node.name.clone());
            }
        }
    }
}

fn camera_controls(build: &mut BuildMode, ui: &Ui, response: &egui::Response, dt: f32) {
    let (delta, scroll, keys, alt, shift) = ui.input(|i| {
        let key = |k| if i.key_down(k) { 1.0 } else { 0.0 };
        (
            i.pointer.delta(),
            i.smooth_scroll_delta.y,
            Vec3::new(
                key(egui::Key::D) - key(egui::Key::A),
                key(egui::Key::E) - key(egui::Key::Q),
                key(egui::Key::W) - key(egui::Key::S),
            ),
            i.modifiers.alt,
            i.modifiers.shift,
        )
    });
    let delta = Vec2::new(delta.x, delta.y);
    let flying = response.dragged_by(egui::PointerButton::Secondary)
        || (response.is_pointer_button_down_on() && ui.input(|i| i.pointer.secondary_down()));
    build.ui.flying = flying;
    if flying {
        build.camera.look(delta);
        build.camera.fly(keys, dt, shift);
        if response.hovered() && scroll != 0.0 {
            build.camera.speed =
                (build.camera.speed * 1.15f32.powf(scroll / 50.0)).clamp(1.0, 400.0);
        }
    } else if response.dragged_by(egui::PointerButton::Middle) {
        build.camera.pan(delta);
    } else if alt && response.dragged_by(egui::PointerButton::Primary) {
        build.camera.orbit(delta);
    } else if response.hovered() && scroll != 0.0 {
        build.camera.zoom(scroll / 50.0);
    }
}

/// Where a new object goes: on the surface under the mouse (or the ground
/// plane), snapped to the grid, turned by the placement turn.
fn placement_at(build: &mut BuildMode, view: &View, pointer: Pos2) -> Transform {
    let ray = view.ray(pointer);
    let surface = build
        .snap
        .surface
        .then(|| surface_hit(build, &ray))
        .flatten();
    let point = surface
        .or_else(|| ray.plane(Vec3::ZERO, Vec3::Y).map(|t| ray.at(t)))
        .unwrap_or(build.camera.focus);
    let mut position = gltf_to_blender(point);
    if build.snap.enabled {
        position[0] = snap(position[0], build.snap.step);
        position[1] = snap(position[1], build.snap.step);
        if surface.is_none() {
            position[2] = snap(position[2], build.snap.step);
        }
    }
    let turn = build.placing.as_ref().map_or(0.0, |p| p.turn);
    Transform {
        position,
        rotation: [0.0, 0.0, turn],
        scale: [1.0; 3],
    }
}

fn surface_hit(build: &BuildMode, ray: &Ray) -> Option<Vec3> {
    let objects = build.gpu.objects.iter().filter_map(|(id, o)| {
        build.doc.layout.effective_visible(*id).then_some(Pickable {
            id: (),
            triangles: &o.triangles,
            bounds: o.bounds,
        })
    });
    let base = build.base_bounds.map(|bounds| Pickable {
        id: (),
        triangles: &build.base_pick,
        bounds,
    });
    pick(ray, objects.chain(base)).map(|(_, t)| ray.at(t))
}

fn pick_object(build: &BuildMode, ray: &Ray) -> Option<NodeId> {
    let layout = &build.doc.layout;
    let items = build.gpu.objects.iter().filter_map(|(id, o)| {
        (layout.effective_visible(*id) && !layout.effective_locked(*id)).then_some(Pickable {
            id: *id,
            triangles: &o.triangles,
            bounds: o.bounds,
        })
    });
    let id = pick(ray, items)?.0;
    // Clicking an object inside a group picks the object; Alt-free double
    // selection of groups happens in the outliner.
    Some(id)
}

fn place(build: &mut BuildMode, transform: Transform, keep_placing: bool) {
    let Some(placing) = &build.placing else {
        return;
    };
    let (asset, label) = (placing.asset.clone(), placing.label.clone());
    let parent = build
        .ui
        .place_into
        .filter(|g| build.doc.layout.node(*g).is_some_and(Node::is_group));
    match ops::place(&mut build.doc, asset, &label, transform, parent) {
        Ok((command, id)) => {
            build.execute(command);
            build.selection = vec![id];
            if !keep_placing {
                build.placing = None;
            }
        }
        Err(error) => build.error(format!("{error:#}")),
    }
}

/// The objects the gizmo moves: selected, unlocked, not groups' contents
/// twice. Groups move everything inside them.
fn movable(build: &BuildMode) -> Vec<NodeId> {
    let layout = &build.doc.layout;
    ops::with_descendants(&build.doc, &build.selection)
        .into_iter()
        .filter(|id| {
            layout.node(*id).is_some_and(|n| !n.is_group()) && !layout.effective_locked(*id)
        })
        .collect()
}

/// Gizmo origin (render space) and the transform it follows.
fn gizmo_anchor(build: &BuildMode) -> Option<(Vec3, Transform)> {
    let primary = *build.selection.last()?;
    let node = build.doc.layout.node(primary)?;
    if node.is_group() {
        let ids = movable(build);
        let points = ids
            .iter()
            .filter_map(|id| build.gpu.objects.get(id))
            .flat_map(|o| [o.bounds.0, o.bounds.1]);
        let (lo, hi) = sa_editor::viewport::bounds(points)?;
        let center = (lo + hi) * 0.5;
        return Some((center, Transform::default()));
    }
    if build.doc.layout.effective_locked(primary) {
        return None;
    }
    Some((
        super::gpu::render_matrix(&node.transform).transform_point3(Vec3::ZERO),
        node.transform,
    ))
}

/// The gizmo handle under `pointer`, if any.
fn gizmo(build: &BuildMode, view: &View, pointer: Pos2) -> Option<Handle> {
    if build.tool == Tool::Select {
        return None;
    }
    let (origin, transform) = gizmo_anchor(build)?;
    let size = gizmo_size(view, origin);
    let axes = gizmo_axes(&transform, build.local || build.tool == Tool::Scale);
    let center = view.project(origin)?;
    match build.tool {
        Tool::Move | Tool::Scale => {
            if pointer.distance(center) < GRAB + 2.0 {
                return Some(if build.tool == Tool::Move {
                    Handle::Plane
                } else {
                    Handle::ScaleAll
                });
            }
            let mut best = None;
            for (i, axis) in axes.iter().enumerate() {
                if let Some([a, b]) = view.segment(origin, origin + *axis * size) {
                    let d = distance_to_segment(pointer, a, b);
                    if d < GRAB && best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, i));
                    }
                }
            }
            best.map(|(_, i)| {
                if build.tool == Tool::Move {
                    Handle::Axis(i)
                } else {
                    Handle::ScaleAxis(i)
                }
            })
        }
        Tool::Rotate => {
            let mut best = None;
            for (i, axis) in axes.iter().enumerate() {
                for [a, b] in ring(view, origin, *axis, size) {
                    let d = distance_to_segment(pointer, a, b);
                    if d < GRAB && best.is_none_or(|(bd, _)| d < bd) {
                        best = Some((d, i));
                    }
                }
            }
            best.map(|(_, i)| Handle::Ring(i))
        }
        Tool::Select => None,
    }
}

fn gizmo_size(view: &View, origin: Vec3) -> f32 {
    view.eye.distance(origin).max(0.5) * 0.16
}

fn ring(view: &View, origin: Vec3, axis: Vec3, radius: f32) -> Vec<[Pos2; 2]> {
    let u = axis.any_orthonormal_vector();
    let v = axis.cross(u);
    let point = |k: usize| {
        let a = k as f32 / 48.0 * std::f32::consts::TAU;
        origin + (u * a.cos() + v * a.sin()) * radius
    };
    (0..48)
        .filter_map(|k| view.segment(point(k), point(k + 1)))
        .collect()
}

fn draw_gizmo(build: &BuildMode, view: &View, painter: &egui::Painter, active: Option<Handle>) {
    if build.tool == Tool::Select {
        return;
    }
    let Some((origin, transform)) = gizmo_anchor(build) else {
        return;
    };
    let size = gizmo_size(view, origin);
    let axes = gizmo_axes(&transform, build.local || build.tool == Tool::Scale);
    let color = |i: usize, handle: Handle| {
        if active == Some(handle) {
            ui_theme::ACCENT
        } else {
            AXIS_COLORS[i]
        }
    };
    let Some(center) = view.project(origin) else {
        return;
    };
    match build.tool {
        Tool::Move | Tool::Scale => {
            for (i, axis) in axes.iter().enumerate() {
                let handle = if build.tool == Tool::Move {
                    Handle::Axis(i)
                } else {
                    Handle::ScaleAxis(i)
                };
                let c = color(i, handle);
                if let Some([a, b]) = view.segment(origin, origin + *axis * size) {
                    painter.line_segment([a, b], Stroke::new(3.0, c));
                    if build.tool == Tool::Move {
                        let dir = (b - a).normalized();
                        let side = egui::vec2(-dir.y, dir.x) * 6.0;
                        painter.add(egui::Shape::convex_polygon(
                            vec![b + dir * 12.0, b + side, b - side],
                            c,
                            Stroke::NONE,
                        ));
                    } else {
                        painter.rect_filled(
                            egui::Rect::from_center_size(b, egui::vec2(10.0, 10.0)),
                            1.0,
                            c,
                        );
                    }
                }
            }
            let center_handle = if build.tool == Tool::Move {
                Handle::Plane
            } else {
                Handle::ScaleAll
            };
            let c = if active == Some(center_handle) {
                ui_theme::ACCENT
            } else {
                Color32::from_gray(230)
            };
            painter.circle(center, 6.0, c.gamma_multiply(0.6), Stroke::new(1.5, c));
        }
        Tool::Rotate => {
            for (i, axis) in axes.iter().enumerate() {
                let c = color(i, Handle::Ring(i));
                for segment in ring(view, origin, *axis, size) {
                    painter.line_segment(segment, Stroke::new(2.5, c));
                }
            }
        }
        Tool::Select => {}
    }
}

fn begin_drag(build: &mut BuildMode, view: &View, handle: Handle, pointer: Pos2) {
    let Some((origin, transform)) = gizmo_anchor(build) else {
        return;
    };
    let ids = movable(build);
    if ids.is_empty() {
        return;
    }
    let local = build.local || build.tool == Tool::Scale;
    let axes = gizmo_axes(&transform, local);
    let ray = view.ray(pointer);
    let (axis, start, start_point) = match handle {
        Handle::Axis(i) | Handle::ScaleAxis(i) => {
            let Some(s) = ray.closest_on_axis(origin, axes[i]) else {
                return;
            };
            (axes[i], s, origin)
        }
        Handle::Plane => {
            let Some(t) = ray.plane(origin, Vec3::Y) else {
                return;
            };
            (Vec3::Y, 0.0, ray.at(t))
        }
        Handle::Ring(i) => {
            let Some(t) = ray.plane(origin, axes[i]) else {
                return;
            };
            (axes[i], 0.0, ray.at(t))
        }
        Handle::ScaleAll => (Vec3::Y, 0.0, origin),
    };
    let starts = ids
        .iter()
        .filter_map(|id| build.doc.layout.node(*id).map(|n| (*id, n.transform)))
        .collect();
    build.commit_preview();
    build.drag = Some(Drag {
        handle,
        origin,
        axis,
        start,
        start_point,
        start_mouse: pointer,
        starts,
    });
}

fn continue_drag(build: &mut BuildMode, view: &View, pointer: Pos2, fine: bool) {
    let Some(drag) = &build.drag else { return };
    let ray = view.ray(pointer);
    let snap_on = build.snap.enabled != fine;
    let (step, angle_step, scale_step) = (build.snap.step, build.snap.angle, build.snap.scale);
    let local = build.local;
    let starts = drag.starts.clone();
    let label;
    let mut updates: Vec<(NodeId, Transform)> = Vec::new();
    match drag.handle {
        Handle::Axis(i) => {
            let Some(s) = ray.closest_on_axis(drag.origin, drag.axis) else {
                return;
            };
            let delta = s - drag.start;
            label = "Move";
            for (id, start) in &starts {
                let mut t = *start;
                if local {
                    let d = if snap_on { snap(delta, step) } else { delta };
                    t.position = moved(start.position, drag.axis * d);
                } else {
                    let world_axis = blender_axis(i);
                    let d = delta * drag.axis.dot(world_axis).signum();
                    let value = start.position[i] + d;
                    t.position[i] = if snap_on { snap(value, step) } else { value };
                }
                updates.push((*id, t));
            }
        }
        Handle::Plane => {
            let Some(t) = ray.plane(drag.origin, Vec3::Y) else {
                return;
            };
            let delta = ray.at(t) - drag.start_point;
            label = "Move";
            for (id, start) in &starts {
                let mut t = *start;
                t.position = moved(start.position, delta);
                if snap_on {
                    t.position[0] = snap(t.position[0], step);
                    t.position[1] = snap(t.position[1], step);
                }
                updates.push((*id, t));
            }
        }
        Handle::Ring(i) => {
            let Some(t) = ray.plane(drag.origin, drag.axis) else {
                return;
            };
            let mut angle = angle_around(
                drag.axis,
                drag.start_point - drag.origin,
                ray.at(t) - drag.origin,
            );
            if snap_on {
                angle = snap(angle, angle_step);
            }
            label = "Rotate";
            for (id, start) in &starts {
                let mut t = *start;
                t.rotation = rotated(start.rotation, i, angle, local);
                updates.push((*id, t));
            }
        }
        Handle::ScaleAxis(i) => {
            let Some(s) = ray.closest_on_axis(drag.origin, drag.axis) else {
                return;
            };
            let size = gizmo_size(view, drag.origin);
            let ratio = ((s - drag.start) / size + 1.0).max(0.01);
            label = "Scale";
            for (id, start) in &starts {
                let mut t = *start;
                let value = start.scale[i] * ratio;
                t.scale[i] = clamp_scale(if snap_on {
                    snap(value, scale_step)
                } else {
                    value
                });
                updates.push((*id, t));
            }
        }
        Handle::ScaleAll => {
            let ratio = (-(pointer.y - drag.start_mouse.y) * 0.01).exp();
            label = "Scale";
            for (id, start) in &starts {
                let mut t = *start;
                for axis in 0..3 {
                    let value = start.scale[axis] * ratio;
                    t.scale[axis] = clamp_scale(if snap_on {
                        snap(value, scale_step)
                    } else {
                        value
                    });
                }
                updates.push((*id, t));
            }
        }
    }
    for (id, transform) in updates {
        if transform.is_valid() {
            build.preview(label, &[id], |n| n.transform = transform);
        }
    }
}

fn clamp_scale(value: f32) -> f32 {
    if value.abs() < 0.01 {
        0.01_f32.copysign(if value == 0.0 { 1.0 } else { value })
    } else {
        value.clamp(-1000.0, 1000.0)
    }
}

fn draw_selection(build: &BuildMode, view: &View, painter: &egui::Painter) {
    let ids = ops::with_descendants(&build.doc, &build.selection);
    for id in ids {
        let Some(object) = build.gpu.objects.get(&id) else {
            continue;
        };
        let color = if object.placeholder {
            Color32::from_rgb(230, 60, 230)
        } else {
            ui_theme::ACCENT
        };
        draw_box(view, painter, &box_corners(object), Stroke::new(1.5, color));
    }
}

/// An object's model bounds in render space; bit 0 = +x, 1 = +y, 2 = +z.
fn box_corners(object: &super::gpu::ObjectMesh) -> [Vec3; 8] {
    let (lo, hi) = object.local_bounds;
    std::array::from_fn(|k| {
        object.matrix.transform_point3(Vec3::new(
            if k & 1 == 0 { lo.x } else { hi.x },
            if k & 2 == 0 { lo.y } else { hi.y },
            if k & 4 == 0 { lo.z } else { hi.z },
        ))
    })
}

/// The 12 edges of a box given as 8 corners numbered by bits.
fn draw_box(view: &View, painter: &egui::Painter, corners: &[Vec3; 8], stroke: Stroke) {
    for (a, b) in [
        (0, 1),
        (2, 3),
        (4, 5),
        (6, 7),
        (0, 2),
        (1, 3),
        (4, 6),
        (5, 7),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ] {
        if let Some(segment) = view.segment(corners[a], corners[b]) {
            painter.line_segment(segment, stroke);
        }
    }
}

/// Collision view: solid objects in cyan, objects without collision in red.
/// The outline is the model's bounds; collision itself uses its triangles.
fn draw_collision(build: &BuildMode, view: &View, painter: &egui::Painter) {
    let flags = build.doc.layout.effective_all();
    for (id, object) in &build.gpu.objects {
        let Some(flags) = flags.get(id) else { continue };
        // Hidden but solid objects (invisible walls) are what this view is for.
        let color = if object.editor_only() || !(flags.visible || flags.collision) {
            continue;
        } else if flags.collision {
            Color32::from_rgb(70, 210, 230).gamma_multiply(0.7)
        } else {
            Color32::from_rgb(230, 80, 80).gamma_multiply(0.7)
        };
        draw_box(view, painter, &box_corners(object), Stroke::new(1.0, color));
    }
}

fn gameplay_color(component: &Component) -> Color32 {
    let teleports = component
        .action_lists()
        .into_iter()
        .flatten()
        .any(|a| matches!(a.known(), Some(Action::Teleport { .. })));
    match component {
        _ if teleports => Color32::from_rgb(180, 110, 245),
        Component::Spawn(_) => ui_theme::SUCCESS,
        Component::Trigger(_) => Color32::from_rgb(245, 155, 60),
        Component::Interact(_) => Color32::from_rgb(80, 215, 230),
        Component::Checkpoint(_) => Color32::from_rgb(245, 215, 60),
    }
}

/// Gameplay zones as boxes with a label, and lines to what actions affect.
fn draw_gameplay(build: &BuildMode, view: &View, painter: &egui::Painter) {
    let layout = &build.doc.layout;
    let flags = layout.effective_all();
    let to_render = crate::play_logic::blender_to_render;
    let font = egui::FontId::proportional(12.0);
    for node in &layout.nodes {
        let Some(component) = node.gameplay.as_ref().and_then(|g| g.known()) else {
            continue;
        };
        if !flags.get(&node.id).is_some_and(|f| f.visible) {
            continue;
        }
        let selected = build.selection.contains(&node.id);
        let color = gameplay_color(component);
        let width = if selected { 2.5 } else { 1.2 };
        let label = match component {
            Component::Spawn(_) => "Spawn point".to_owned(),
            Component::Trigger(_) => format!("Trigger: {}", node.name),
            Component::Interact(i) if !i.prompt.is_empty() => format!("E: {}", i.prompt),
            Component::Interact(_) => format!("E: {}", node.name),
            Component::Checkpoint(c) => format!("Checkpoint {}", c.order),
        };
        let mut top = to_render(node.transform.position) + Vec3::Y * 1.8;
        if let Some(zone) = world_logic::zone_of(node) {
            let corners = zone.corners().map(|c| to_render(c.to_array()));
            draw_box(
                view,
                painter,
                &corners,
                Stroke::new(width, color.gamma_multiply(0.85)),
            );
            top = (corners[4] + corners[7]) * 0.5;
        }
        if let Some(at) = view.project(top) {
            painter.text(at, egui::Align2::CENTER_BOTTOM, label, font.clone(), color);
        }
        // Where its actions lead: dashed for teleports, thin for objects
        // it shows, hides or toggles.
        let from = top - Vec3::Y * 0.4;
        for action in component.action_lists().into_iter().flatten() {
            let (target, dashed) = match action.known() {
                Some(Action::Teleport { target: Some(t) }) => (*t, true),
                Some(Action::Show { node: Some(t) })
                | Some(Action::Hide { node: Some(t) })
                | Some(Action::Toggle { node: Some(t) })
                    if *t != node.id =>
                {
                    (*t, false)
                }
                _ => continue,
            };
            let Some(goal) = layout.node(target) else {
                continue;
            };
            let to = to_render(goal.transform.position) + Vec3::Y * 0.2;
            let pieces = 24;
            for k in 0..pieces {
                if dashed && k % 2 == 1 {
                    continue;
                }
                let (a, b) = (k as f32 / pieces as f32, (k + 1) as f32 / pieces as f32);
                if let Some(segment) = view.segment(from.lerp(to, a), from.lerp(to, b)) {
                    painter
                        .line_segment(segment, Stroke::new(if dashed { 2.0 } else { 1.0 }, color));
                }
            }
            if let Some(end) = view.project(to) {
                painter.circle_stroke(end, 5.0, Stroke::new(2.0, color));
                if dashed {
                    painter.text(
                        end + egui::vec2(7.0, 0.0),
                        egui::Align2::LEFT_CENTER,
                        format!("\u{2192} {}", goal.name),
                        font.clone(),
                        color,
                    );
                }
            }
        }
    }
}

fn draw_grid(build: &BuildMode, view: &View, painter: &egui::Painter) {
    // Wider spacing when far away keeps the grid readable.
    let height = (view.eye.y).abs().max(build.camera.distance * 0.5);
    let mut spacing = build.snap.step.max(0.25);
    while height / spacing > 40.0 {
        spacing *= 2.0;
    }
    let lines = 40;
    let center = Vec3::new(
        snap(build.camera.focus.x, spacing),
        0.0,
        snap(build.camera.focus.z, spacing),
    );
    let half = spacing * lines as f32 / 2.0;
    for k in 0..=lines {
        let offset = -half + k as f32 * spacing;
        let major = ((center.x + offset) / spacing).round() as i64 % 10 == 0;
        let alpha = if major { 70 } else { 32 };
        for (a, b) in [
            (
                Vec3::new(center.x + offset, 0.0, center.z - half),
                Vec3::new(center.x + offset, 0.0, center.z + half),
            ),
            (
                Vec3::new(center.x - half, 0.0, center.z + offset),
                Vec3::new(center.x + half, 0.0, center.z + offset),
            ),
        ] {
            // Short pieces so lines behind the camera are cut cleanly.
            for piece in 0..8 {
                let (t0, t1) = (piece as f32 / 8.0, (piece + 1) as f32 / 8.0);
                if let Some(segment) = view.segment(a.lerp(b, t0), a.lerp(b, t1)) {
                    painter
                        .line_segment(segment, Stroke::new(1.0, Color32::from_white_alpha(alpha)));
                }
            }
        }
    }
    // World axes through the pack origin: east (red) and north (green).
    let origin = super::gpu::render_matrix(&Transform::default()).transform_point3(Vec3::ZERO);
    for (i, color) in [(0, AXIS_COLORS[0]), (1, AXIS_COLORS[1])] {
        let axis = blender_axis(i) * half;
        if let Some(segment) = view.segment(origin - axis, origin + axis) {
            painter.line_segment(segment, Stroke::new(1.5, color.gamma_multiply(0.7)));
        }
    }
}

fn draw_spawn(build: &BuildMode, view: &View, painter: &egui::Painter) {
    // A Spawn point object replaces the world's spawn; its marker shows it.
    let has_spawn_object = build.doc.layout.nodes.iter().any(|n| {
        matches!(
            n.gameplay.as_ref().and_then(|g| g.known()),
            Some(Component::Spawn(_))
        )
    });
    if has_spawn_object {
        return;
    }
    let spawn = build.spawn_render();
    let top = spawn + Vec3::Y * 1.8;
    if let Some(segment) = view.segment(spawn, top) {
        painter.line_segment(segment, Stroke::new(3.0, ui_theme::SUCCESS));
        painter.circle_stroke(segment[0], 5.0, Stroke::new(2.0, ui_theme::SUCCESS));
        painter.text(
            segment[1] + egui::vec2(6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "Spawn",
            egui::FontId::proportional(12.0),
            ui_theme::SUCCESS,
        );
    }
}
