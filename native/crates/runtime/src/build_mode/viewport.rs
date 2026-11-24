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
use sa_scene::world_layout::{gltf_to_blender, Node, NodeId, Road, Terrain, Transform};
use sa_scene::world_logic::{self, Action, Component};
use std::collections::HashMap;

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
    /// Sizes of the parts among them: scaling a part changes its size.
    sizes: HashMap<NodeId, [f32; 3]>,
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
    if build.tool == Tool::Sculpt || build.placing.is_some() {
        end_road_editing(build);
    }
    // Placing an asset works with every tool; sculpting otherwise.
    if build.tool == Tool::Sculpt && build.placing.is_none() {
        sculpt(build, ui, &view, &painter, pointer, dt);
        return;
    }

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

    // A selected road: its points are handles, and in draw mode a click
    // adds a point. They come before the gizmo and selection.
    if road_points(build, ui, &view, &painter, &response, pointer, modifiers) {
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

    // Click to select (Ctrl adds or removes). A click on part of a model
    // selects the model; Alt+click the object itself. (Alt+drag orbits.)
    if response.clicked_by(egui::PointerButton::Primary) && handle_hover.is_none() {
        if let Some(pointer) = pointer {
            let hit = pick_object(build, &view.ray(pointer)).map(|id| {
                if modifiers.alt {
                    id
                } else {
                    model_of(build, id)
                }
            });
            match (hit, modifiers.ctrl || modifiers.command) {
                (Some(id), true) => {
                    if let Some(i) = build.selection.iter().position(|s| *s == id) {
                        build.selection.remove(i);
                    } else {
                        build.selection.push(id);
                    }
                }
                (Some(id), false) => build.selection = vec![id],
                (None, false) => {
                    // Empty space: the world's own settings, as before.
                    build.selection.clear();
                    build.ui.world_page = super::panels::WorldPage::Workspace;
                }
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
            let model = model_of(build, id);
            let name = |id| build.doc.layout.node(id).map(|n| n.name.clone());
            let text = match (name(model), name(id)) {
                (Some(m), Some(o)) if model != id => {
                    Some(format!("{m} › {o}\nAlt+click selects {o}"))
                }
                (_, o) => o,
            };
            if let Some(text) = text {
                response.clone().on_hover_text_at_pointer(text);
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

/// The outermost model `id` is part of (unlocked), or `id` itself.
fn model_of(build: &BuildMode, id: NodeId) -> NodeId {
    let layout = &build.doc.layout;
    layout
        .ancestry(id)
        .skip(1)
        .filter(|n| n.model && !layout.effective_locked(n.id))
        .last()
        .map_or(id, |n| n.id)
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

/// Where `ray` first meets the surface of an editable terrain object,
/// holes included (they have no mesh to pick): its heights, turned and
/// scaled with the object. The nearest such terrain and point.
fn terrain_surface_hit(build: &BuildMode, ray: &Ray) -> Option<(NodeId, Vec3)> {
    let layout = &build.doc.layout;
    // Render space to the pack's Blender axes, without a matrix per sample.
    let offset = crate::play_logic::blender_to_render([0.0; 3]);
    layout
        .nodes
        .iter()
        .filter(|n| {
            n.terrain.is_some() && layout.effective_visible(n.id) && !layout.effective_locked(n.id)
        })
        .filter_map(|n| {
            let terrain = n.terrain.as_ref()?;
            let t = &layout.world_transform(n.id);
            let (turn, position) = (t.rotation_quat().inverse(), Vec3::from_array(t.position));
            let scale = Vec3::from_array(t.scale);
            // How far the ray at `along` is above the ground under it (None
            // outside the terrain's square).
            let above = |along: f32| {
                let world = Vec3::from_array(gltf_to_blender(ray.at(along) - offset));
                let local = (turn * (world - position)) / scale;
                terrain.height_at(local.x, local.y).map(|h| local.z - h)
            };
            let step =
                (terrain.spacing() * 0.5 * scale.x.abs().min(scale.y.abs())).clamp(0.05, 1.0);
            // Only where the ray crosses the terrain's box (a metre more all
            // round): nothing to sample over the sky.
            let (lo, hi) = build.gpu.objects.get(&n.id)?.bounds;
            let inv = ray.direction.recip();
            let (a, b) = (
                (lo - Vec3::ONE - ray.origin) * inv,
                (hi + Vec3::ONE - ray.origin) * inv,
            );
            let near = a.min(b).max_element().max(0.0);
            let far = a.max(b).min_element().min(3000.0);
            if far < near {
                return None;
            }
            let mut previous: Option<(f32, f32)> = None;
            let mut along = near;
            while along <= far + step {
                match (above(along), previous) {
                    (Some(now), Some((before_t, before))) if before > 0.0 && now <= 0.0 => {
                        // Crossed the surface: narrow it down.
                        let (mut low, mut high) = (before_t, along);
                        for _ in 0..12 {
                            let middle = (low + high) * 0.5;
                            if above(middle).is_some_and(|d| d > 0.0) {
                                low = middle;
                            } else {
                                high = middle;
                            }
                        }
                        return Some((n.id, ray.at(high), high));
                    }
                    (now, _) => previous = now.map(|d| (along, d)),
                }
                along += step;
            }
            None
        })
        .min_by(|a, b| a.2.total_cmp(&b.2))
        .map(|(id, at, _)| (id, at))
}

/// The Sculpt tool: the brush follows the mouse over terrain objects, and
/// holding the left button shapes the terrain under it. A stroke changes the
/// layout live and becomes one undo step when the button is let go.
fn sculpt(
    build: &mut BuildMode,
    ui: &Ui,
    view: &View,
    painter: &egui::Painter,
    pointer: Option<Pos2>,
    dt: f32,
) {
    use super::Brush;
    use sa_scene::terrain;
    let held = ui.input(|i| i.pointer.primary_down() && !i.modifiers.alt) && !build.ui.flying;
    let hit = pointer.and_then(|pointer| {
        let ray = view.ray(pointer);
        let layout = &build.doc.layout;
        let items = build.gpu.objects.iter().filter_map(|(id, o)| {
            let node = layout.node(*id)?;
            (node.terrain.is_some()
                && layout.effective_visible(*id)
                && !layout.effective_locked(*id))
            .then_some(Pickable {
                id: *id,
                triangles: &o.triangles,
                bounds: o.bounds,
            })
        });
        pick(&ray, items)
            .map(|(id, t)| (id, ray.at(t)))
            // Over a hole there is no mesh to hit: Paint finds the
            // terrain's own surface there, so it can fill the hole again.
            .or_else(|| {
                (build.sculpt.brush == Brush::Paint)
                    .then(|| terrain_surface_hit(build, &ray))
                    .flatten()
            })
    });
    let Some((id, point)) = hit else {
        if !held && build.ui.sculpt_stroke.take().is_some() {
            build.commit_preview();
        }
        return;
    };
    let Some(node) = build.doc.layout.node(id).cloned() else {
        return;
    };
    // The brush in the terrain's own metres.
    let at = Vec3::from_array(crate::play_logic::render_to_blender(point));
    let t = build.doc.layout.world_transform(id);
    let local = t.rotation_quat().inverse() * (at - Vec3::from_array(t.position));
    let scale = ((t.scale[0].abs() + t.scale[1].abs()) * 0.5).max(0.001);
    let center = Vec2::new(local.x / t.scale[0], local.y / t.scale[1]);
    let radius = build.sculpt.radius / scale;
    // The brush circle on the ground.
    let ring: Vec<Vec3> = (0..=40)
        .map(|k| {
            let a = k as f32 / 40.0 * std::f32::consts::TAU;
            crate::play_logic::blender_to_render(
                (at + Vec3::new(a.cos(), a.sin(), 0.0) * build.sculpt.radius + Vec3::Z * 0.1)
                    .to_array(),
            )
        })
        .collect();
    for pair in ring.windows(2) {
        if let Some(segment) = view.segment(pair[0], pair[1]) {
            painter.line_segment(segment, Stroke::new(2.0, ui_theme::ACCENT));
        }
    }
    if !held {
        if build.ui.sculpt_stroke.take().is_some() {
            build.commit_preview();
        }
        return;
    }
    let Some(terrain_now) = &node.terrain else {
        return;
    };
    // Flatten keeps the height where the stroke began.
    let level = *build
        .ui
        .sculpt_stroke
        .get_or_insert_with(|| terrain_now.height_at(center.x, center.y).unwrap_or(0.0));
    let sculpt = build.sculpt;
    let rate = sculpt.strength * dt;
    build.preview("Sculpt terrain", &[id], |n| {
        if let Some(terrain) = &mut n.terrain {
            match sculpt.brush {
                Brush::Raise => terrain::raise(terrain, center, radius, rate * 6.0),
                Brush::Lower => terrain::raise(terrain, center, radius, -rate * 6.0),
                Brush::Smooth => terrain::smooth(terrain, center, radius, (rate * 8.0).min(1.0)),
                Brush::Flatten => {
                    terrain::flatten(terrain, center, radius, level, (rate * 8.0).min(1.0))
                }
                Brush::Paint => terrain::paint(terrain, center, radius, sculpt.material),
                Brush::Hole => terrain::paint(terrain, center, radius, Terrain::HOLE),
            };
        }
    });
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

/// The objects the gizmo moves: selected and unlocked. Moving or turning
/// a group moves its pivot and everything inside comes along; scaling
/// scales everything inside it (groups have no scale of their own).
fn movable(build: &BuildMode) -> Vec<NodeId> {
    let layout = &build.doc.layout;
    if build.tool == Tool::Scale {
        return ops::with_descendants(&build.doc, &build.selection)
            .into_iter()
            .filter(|id| {
                layout.node(*id).is_some_and(|n| !n.is_group()) && !layout.effective_locked(*id)
            })
            .collect();
    }
    // The topmost selected: an object inside a selected group goes along
    // with it already.
    build
        .selection
        .iter()
        .copied()
        .filter(|id| {
            !layout
                .ancestry(*id)
                .skip(1)
                .any(|a| build.selection.contains(&a.id))
                && !layout.effective_locked(*id)
                && layout.node(*id).is_some()
        })
        .collect()
}

/// The group's pivot is used: it has one, and the tool moves or turns it.
fn uses_pivot(build: &BuildMode, node: &Node) -> bool {
    node.is_group() && build.tool != Tool::Scale && !node.transform.is_unmoved()
}

/// Stop dragging a road point (keeping the move as one undo step) and
/// drawing a road.
pub(super) fn end_road_editing(build: &mut BuildMode) {
    if build.ui.road_drag.take().is_some() {
        build.commit_preview();
    }
    build.ui.road_draw = None;
}

/// The one selected road that can be edited: its id, transform and road.
fn selected_road(build: &BuildMode) -> Option<(NodeId, Transform, Road)> {
    let [id] = build.selection[..] else {
        return None;
    };
    let layout = &build.doc.layout;
    let node = layout.node(id)?;
    let road = node.road.clone()?;
    (layout.effective_visible(id) && !layout.effective_locked(id)).then_some((
        id,
        layout.world_transform(id),
        road,
    ))
}

/// A road point (the road's own metres) in the pack's Blender axes.
fn road_to_world(transform: &Transform, point: [f32; 3]) -> Vec3 {
    let scaled = Vec3::from_array(point) * Vec3::from_array(transform.scale);
    Vec3::from_array(transform.position) + transform.rotation_quat() * scaled
}

/// A point in the pack's Blender axes in a road's own metres.
fn world_to_road(transform: &Transform, world: Vec3) -> [f32; 3] {
    let local =
        transform.rotation_quat().inverse() * (world - Vec3::from_array(transform.position));
    let scale = Vec3::from_array(transform.scale);
    (local / scale).to_array()
}

/// The selected road's point handles: drag one to move it (on the level of
/// the point, snapped to the grid with Snap on), and in draw mode click
/// to add a point at the end. True when the road took the mouse.
fn road_points(
    build: &mut BuildMode,
    ui: &Ui,
    view: &View,
    painter: &egui::Painter,
    response: &egui::Response,
    pointer: Option<Pos2>,
    modifiers: egui::Modifiers,
) -> bool {
    let Some((id, transform, road)) = selected_road(build) else {
        end_road_editing(build);
        return false;
    };
    let drawing = build.ui.road_draw == Some(id);
    // The curve and its points.
    let render =
        |p: [f32; 3]| crate::play_logic::blender_to_render(road_to_world(&transform, p).to_array());
    let curve: Vec<Vec3> = road.curve().iter().map(|p| render(p.to_array())).collect();
    for pair in curve.windows(2) {
        if let Some(segment) = view.segment(pair[0], pair[1]) {
            painter.line_segment(segment, Stroke::new(2.0, ui_theme::ACCENT));
        }
    }
    let handles: Vec<Option<Pos2>> = road
        .points
        .iter()
        .map(|p| view.project(render(*p)))
        .collect();
    let under = |at: Pos2| {
        handles
            .iter()
            .enumerate()
            .filter_map(|(i, h)| h.map(|h| (i, h.distance(at))))
            .filter(|(_, d)| *d < 9.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    };
    // A gizmo drag going on keeps the mouse, even over a point.
    let hover = pointer.and_then(under).filter(|_| build.drag.is_none());

    // Drag a point.
    if response.drag_started_by(egui::PointerButton::Primary)
        && !modifiers.alt
        && build.drag.is_none()
    {
        let index = ui.input(|i| i.pointer.press_origin()).and_then(under);
        if let Some(index) = index {
            build.commit_preview();
            build.ui.road_drag = Some((id, index));
        }
    }
    let dragging = build.ui.road_drag.filter(|(road_id, _)| *road_id == id);
    if let (Some((_, index)), Some(at)) = (dragging, ui.input(|i| i.pointer.interact_pos())) {
        if let Some(point) = road.points.get(index) {
            let level = render(*point);
            let ray = view.ray(at);
            if let Some(t) = ray.plane(level, Vec3::Y) {
                let mut world = Vec3::from_array(crate::play_logic::render_to_blender(ray.at(t)));
                if build.snap.enabled != modifiers.ctrl {
                    world.x = snap(world.x, build.snap.step);
                    world.y = snap(world.y, build.snap.step);
                }
                let moved = world_to_road(&transform, world);
                let mut edited = road.clone();
                edited.points[index] = moved;
                if edited.is_valid() {
                    build.preview("Move road point", &[id], |n| n.road = Some(edited.clone()));
                }
            }
        }
    }
    if build.ui.road_drag.is_some() && !ui.input(|i| i.pointer.primary_down()) {
        build.ui.road_drag = None;
        build.commit_preview();
    }

    // Draw mode: a click on the first point closes the road into a loop.
    let closes = drawing && !road.is_loop() && road.points.len() >= 3 && hover == Some(0);
    let closed_now = closes && response.clicked_by(egui::PointerButton::Primary) && !modifiers.alt;
    if closed_now {
        let mut edited = road.clone();
        edited.closed = true;
        super::panels::run(
            build,
            ops::modify(&build.doc, "Close road loop", &[id], |n| {
                n.road = Some(edited.clone())
            }),
        );
        build.ui.road_draw = None;
    }
    let drawing = drawing && build.ui.road_draw == Some(id);
    // Draw mode: a click adds a point where it lands.
    // Only a point drag or draw mode keep the mouse from the gizmo and
    // selection; hovering a point does not.
    // The click that closed a loop is not also a selection click.
    let mut took = build.ui.road_drag.is_some() || closed_now;
    if drawing {
        took = true;
        if response.clicked_by(egui::PointerButton::Primary) && hover.is_none() && !modifiers.alt {
            if let Some(at) = pointer {
                let ray = view.ray(at);
                let hit = surface_hit(build, &ray).or_else(|| {
                    ray.plane(render(*road.points.last().unwrap_or(&[0.0; 3])), Vec3::Y)
                        .map(|t| ray.at(t))
                });
                if let Some(hit) = hit {
                    let mut world = Vec3::from_array(crate::play_logic::render_to_blender(hit));
                    if build.snap.enabled != modifiers.ctrl {
                        world.x = snap(world.x, build.snap.step);
                        world.y = snap(world.y, build.snap.step);
                    }
                    let mut edited = road.clone();
                    edited.points.push(world_to_road(&transform, world));
                    if edited.is_valid() {
                        super::panels::run(
                            build,
                            ops::modify(&build.doc, "Add road point", &[id], |n| {
                                n.road = Some(edited.clone())
                            }),
                        );
                    } else {
                        build.error(format!(
                            "A road has at most {} points, each with X, Y and Z within ±{} m of the object (in its own metres)",
                            Road::MAX_POINTS,
                            Road::MAX_REACH
                        ));
                    }
                }
            }
        }
        if response.clicked_by(egui::PointerButton::Secondary) {
            build.ui.road_draw = None;
        }
        if pointer.is_some() {
            response.clone().on_hover_text_at_pointer(if closes {
                "Click to close the road into a loop."
            } else {
                "Click to add a road point. Right-click or Esc to stop drawing."
            });
        }
    }
    let active = build.ui.road_drag.map(|(_, i)| i).or(hover);
    for (i, handle) in handles.iter().enumerate() {
        let Some(handle) = handle else { continue };
        let radius = if active == Some(i) { 7.0 } else { 5.0 };
        painter.circle(
            *handle,
            radius,
            if active == Some(i) {
                ui_theme::ACCENT
            } else {
                Color32::WHITE
            },
            Stroke::new(1.5, Color32::BLACK),
        );
    }
    took
}

/// Gizmo origin (render space) and the transform it follows.
fn gizmo_anchor(build: &BuildMode) -> Option<(Vec3, Transform)> {
    let primary = *build.selection.last()?;
    let node = build.doc.layout.node(primary)?;
    if node.is_group() && !uses_pivot(build, node) {
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
    let world = build.doc.layout.world_transform(primary);
    Some((
        super::gpu::render_matrix(&world).transform_point3(Vec3::ZERO),
        world,
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
        Tool::Sculpt => None,
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
        Tool::Sculpt => {}
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
    // A group without a pivot of its own gets one in the middle of its
    // contents before it moves or turns, so it turns about itself.
    if build.tool != Tool::Scale {
        let unpivoted: Vec<NodeId> = movable(build)
            .into_iter()
            .filter(|id| {
                build
                    .doc
                    .layout
                    .node(*id)
                    .is_some_and(|n| n.is_group() && n.transform.is_unmoved())
            })
            .collect();
        for group in unpivoted {
            if let Ok(command) = ops::center_pivot(&build.doc, group) {
                build.execute(command);
            }
        }
    }
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
    // Dragged in world space; written back relative to each one's group.
    let starts = ids
        .iter()
        .filter(|id| build.doc.layout.node(**id).is_some())
        .map(|id| (*id, build.doc.layout.world_transform(*id)))
        .collect();
    let sizes = ids
        .iter()
        .filter_map(|id| Some((*id, build.doc.layout.node(*id)?.part.as_ref()?.size)))
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
        sizes,
    });
}

fn continue_drag(build: &mut BuildMode, view: &View, pointer: Pos2, fine: bool) {
    let Some(drag) = &build.drag else { return };
    let ray = view.ray(pointer);
    let snap_on = build.snap.enabled != fine;
    let (step, angle_step, scale_step) = (build.snap.step, build.snap.angle, build.snap.scale);
    let local = build.local;
    let starts = drag.starts.clone();
    let sizes = drag.sizes.clone();
    let label;
    let mut updates: Vec<(NodeId, Transform)> = Vec::new();
    // New sizes of parts being scaled.
    let mut resized: Vec<(NodeId, [f32; 3])> = Vec::new();
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
            let together = starts.len() > 1;
            for (id, start) in &starts {
                let mut t = *start;
                t.rotation = if together && local {
                    // All turn about the primary object's own axis.
                    sa_editor::viewport::turned_about(start.rotation, drag.axis, angle)
                } else {
                    rotated(start.rotation, i, angle, local)
                };
                if together {
                    // A group (or several objects) turns as one about the
                    // gizmo: each object also swings around it.
                    let at = super::gpu::render_matrix(start).transform_point3(Vec3::ZERO);
                    let to = sa_editor::viewport::orbit(at, drag.origin, drag.axis, angle);
                    t.position = moved(start.position, to - at);
                }
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
            let together = starts.len() > 1;
            for (id, start) in &starts {
                let mut t = *start;
                if let Some(size) = sizes.get(id) {
                    // A part grows in metres (snapped to the move step),
                    // from its other side: the dragged face follows the
                    // mouse. Its base is its origin, so it grows upwards.
                    let mut new = *size;
                    let value = size[i] * ratio;
                    new[i] = clamp_size(if snap_on { snap(value, step) } else { value });
                    // Only when the handle runs along the part's own side
                    // (always with local axes; with world axes for a part
                    // that is not turned off them).
                    let mut unit = [0.0; 3];
                    unit[i] = 1.0;
                    let own = super::gpu::render_matrix(start)
                        .transform_vector3(sa_scene::world_layout::blender_to_gltf(unit))
                        .normalize_or_zero();
                    if !together && i < 2 && own.dot(drag.axis).abs() > 0.999 {
                        let grown = (new[i] - size[i]) * start.scale[i].abs();
                        t.position = moved(start.position, drag.axis * (grown / 2.0));
                    }
                    resized.push((*id, new));
                } else {
                    let value = start.scale[i] * ratio;
                    t.scale[i] = clamp_scale(if snap_on {
                        snap(value, scale_step)
                    } else {
                        value
                    });
                }
                if together {
                    // Objects spread out (or close up) from the gizmo too.
                    t.position = spread(start, drag.origin, Some(drag.axis), ratio);
                }
                updates.push((*id, t));
            }
        }
        Handle::ScaleAll => {
            let ratio = (-(pointer.y - drag.start_mouse.y) * 0.01).exp();
            label = "Scale";
            let together = starts.len() > 1;
            for (id, start) in &starts {
                let mut t = *start;
                if together {
                    t.position = spread(start, drag.origin, None, ratio);
                }
                if let Some(size) = sizes.get(id) {
                    resized.push((*id, size.map(|s| clamp_size(s * ratio))));
                } else {
                    for axis in 0..3 {
                        let value = start.scale[axis] * ratio;
                        t.scale[axis] = clamp_scale(if snap_on {
                            snap(value, scale_step)
                        } else {
                            value
                        });
                    }
                }
                updates.push((*id, t));
            }
        }
    }
    for (id, world) in updates {
        let parent = build.doc.layout.node(id).and_then(|n| n.parent);
        let transform = build.doc.layout.local_in(parent, &world);
        if transform.is_valid() {
            build.preview(label, &[id], |n| n.transform = transform);
        }
    }
    for (id, size) in resized {
        build.preview("Resize part", &[id], |n| {
            if let Some(part) = &mut n.part {
                part.size = size;
            }
        });
    }
}

fn clamp_size(value: f32) -> f32 {
    use sa_scene::world_layout::Part;
    if value.is_finite() {
        value.abs().clamp(Part::MIN_SIZE, Part::MAX_SIZE)
    } else {
        1.0
    }
}

/// The position of an object of a group scaled by `ratio` from `pivot`.
fn spread(start: &Transform, pivot: Vec3, axis: Option<Vec3>, ratio: f32) -> [f32; 3] {
    let at = super::gpu::render_matrix(start).transform_point3(Vec3::ZERO);
    let to = sa_editor::viewport::scaled_about(at, pivot, axis, ratio);
    moved(start.position, to - at)
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
    let world = layout.world_transforms();
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
        let at = world[&node.id];
        let mut top = to_render(at.position) + Vec3::Y * 1.8;
        let zone = component
            .size()
            .map(|size| world_logic::ZoneShape::of(&at, size));
        if let Some(zone) = zone {
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
            let to = to_render(world[&goal.id].position) + Vec3::Y * 0.2;
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
