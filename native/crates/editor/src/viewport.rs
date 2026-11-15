//! Viewport maths for Build Mode, independent of the renderer: mouse rays,
//! picking, gizmo drags, snapping and the orbit/fly editor camera.
//!
//! Space: everything here is in the runtime's render space (glTF axes: X
//! east, Y up, -Z north; metres), except values stored in the layout, which
//! use Blender axes. [`blender_axis`] gives a Blender axis in render space.
use glam::{EulerRot, Mat4, Quat, Vec2, Vec3};
use sa_scene::world_layout::{Transform, BLENDER_TO_GLTF};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ray {
    pub origin: Vec3,
    pub direction: Vec3,
}
impl Ray {
    /// The ray under a screen point (`pixel` from the top-left of a
    /// `size` viewport) for a camera at `eye`.
    pub fn from_screen(pixel: Vec2, size: Vec2, view_projection: Mat4, eye: Vec3) -> Self {
        let ndc = Vec2::new(
            pixel.x / size.x.max(1.0) * 2.0 - 1.0,
            1.0 - pixel.y / size.y.max(1.0) * 2.0,
        );
        // Any depth inside the frustum works; the renderer uses reversed Z.
        let point = view_projection.inverse().project_point3(ndc.extend(0.5));
        Self {
            origin: eye,
            direction: (point - eye).normalize_or(Vec3::NEG_Z),
        }
    }
    pub fn at(&self, t: f32) -> Vec3 {
        self.origin + self.direction * t
    }
    /// Distance to a triangle, Möller–Trumbore, both faces.
    pub fn triangle(&self, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
        let (e1, e2) = (b - a, c - a);
        let p = self.direction.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-9 {
            return None;
        }
        let inv = 1.0 / det;
        let s = self.origin - a;
        let u = s.dot(p) * inv;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = s.cross(e1);
        let v = self.direction.dot(q) * inv;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        let t = e2.dot(q) * inv;
        (t > 1e-4).then_some(t)
    }
    /// Entry distance into a box, or None when missed.
    pub fn aabb(&self, min: Vec3, max: Vec3) -> Option<f32> {
        let inv = self.direction.recip();
        let a = (min - self.origin) * inv;
        let b = (max - self.origin) * inv;
        let near = a.min(b).max_element();
        let far = a.max(b).min_element();
        (far >= near.max(0.0)).then_some(near.max(0.0))
    }
    /// Where the ray meets the plane through `point` with `normal`.
    pub fn plane(&self, point: Vec3, normal: Vec3) -> Option<f32> {
        let denom = self.direction.dot(normal);
        if denom.abs() < 1e-6 {
            return None;
        }
        let t = (point - self.origin).dot(normal) / denom;
        (t > 0.0).then_some(t)
    }
    /// Parameter along the line `origin + s * axis` closest to this ray.
    pub fn closest_on_axis(&self, origin: Vec3, axis: Vec3) -> Option<f32> {
        let w = origin - self.origin;
        let (a, b, c) = (
            axis.dot(axis),
            axis.dot(self.direction),
            self.direction.dot(self.direction),
        );
        let (d, e) = (axis.dot(w), self.direction.dot(w));
        let denom = a * c - b * b;
        // Looking straight down the axis gives no usable drag.
        if denom.abs() < 1e-4 {
            return None;
        }
        Some((b * e - c * d) / denom)
    }
}

/// Bounds of a set of points.
pub fn bounds(points: impl IntoIterator<Item = Vec3>) -> Option<(Vec3, Vec3)> {
    points.into_iter().fold(None, |acc, p| match acc {
        None => Some((p, p)),
        Some((lo, hi)) => Some((lo.min(p), hi.max(p))),
    })
}

/// Something pickable: an id, its world-space triangles and their bounds.
pub struct Pickable<'a, Id> {
    pub id: Id,
    pub triangles: &'a [[Vec3; 3]],
    pub bounds: (Vec3, Vec3),
}
/// The nearest object under the ray and its distance.
pub fn pick<'a, Id: Copy>(
    ray: &Ray,
    items: impl IntoIterator<Item = Pickable<'a, Id>>,
) -> Option<(Id, f32)> {
    let mut best: Option<(Id, f32)> = None;
    for item in items {
        let Some(entry) = ray.aabb(item.bounds.0, item.bounds.1) else {
            continue;
        };
        if best.is_some_and(|(_, t)| entry > t) {
            continue;
        }
        for [a, b, c] in item.triangles {
            if let Some(t) = ray.triangle(*a, *b, *c) {
                if best.is_none_or(|(_, bt)| t < bt) {
                    best = Some((item.id, t));
                }
            }
        }
    }
    best
}

/// Blender axis 0 (east), 1 (north) or 2 (up) in render space.
pub fn blender_axis(index: usize) -> Vec3 {
    BLENDER_TO_GLTF.col(index)
}
/// The three gizmo axes in render space, world or local to `transform`.
pub fn gizmo_axes(transform: &Transform, local: bool) -> [Vec3; 3] {
    let rotation = if local {
        Quat::from_mat3(
            &(BLENDER_TO_GLTF
                * glam::Mat3::from_quat(transform.rotation_quat())
                * BLENDER_TO_GLTF.transpose()),
        )
    } else {
        Quat::IDENTITY
    };
    [0, 1, 2].map(|i| (rotation * blender_axis(i)).normalize())
}

/// Round to the nearest multiple of `step` (no snapping when `step` <= 0).
pub fn snap(value: f32, step: f32) -> f32 {
    if step > 0.0 {
        (value / step).round() * step
    } else {
        value
    }
}

/// Move a Blender-axes position by a render-space offset.
pub fn moved(position: [f32; 3], render_delta: Vec3) -> [f32; 3] {
    let blender = BLENDER_TO_GLTF.transpose() * render_delta;
    [
        position[0] + blender.x,
        position[1] + blender.y,
        position[2] + blender.z,
    ]
}

/// Rotate Blender Euler angles (degrees) by `degrees` about gizmo axis
/// `axis` (0 X, 1 Y, 2 Z), in world or local orientation. World Z (turning
/// left/right) and local X only change one angle, so typed values stay tidy.
pub fn rotated(rotation: [f32; 3], axis: usize, degrees: f32, local: bool) -> [f32; 3] {
    if (!local && axis == 2) || (local && axis == 0) {
        let mut r = rotation;
        r[axis] = wrap_degrees(r[axis] + degrees);
        return r;
    }
    let current = Transform {
        rotation,
        ..Transform::default()
    }
    .rotation_quat();
    let unit = [Vec3::X, Vec3::Y, Vec3::Z][axis];
    let turn = Quat::from_axis_angle(unit, degrees.to_radians());
    let result = if local {
        current * turn
    } else {
        turn * current
    };
    let (z, y, x) = result.normalize().to_euler(EulerRot::ZYX);
    [x, y, z].map(|r| tidy(r.to_degrees()))
}
fn wrap_degrees(d: f32) -> f32 {
    let wrapped = (d + 180.0).rem_euclid(360.0) - 180.0;
    tidy(if wrapped == -180.0 { 180.0 } else { wrapped })
}
/// Remove float noise such as 89.99999 from computed angles.
fn tidy(d: f32) -> f32 {
    let rounded = (d * 1000.0).round() / 1000.0;
    if rounded == -0.0 {
        0.0
    } else {
        rounded
    }
}

/// Signed angle (degrees) from `from` to `to` around `axis`.
pub fn angle_around(axis: Vec3, from: Vec3, to: Vec3) -> f32 {
    let a = from.reject_from(axis).normalize_or_zero();
    let b = to.reject_from(axis).normalize_or_zero();
    a.cross(b).dot(axis).atan2(a.dot(b)).to_degrees()
}

/// Orbit/fly editor camera. `yaw` 0 looks south (+Z) like the game camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorCamera {
    pub focus: Vec3,
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
    /// Metres per second when flying.
    pub speed: f32,
}
impl Default for EditorCamera {
    fn default() -> Self {
        Self {
            focus: Vec3::ZERO,
            distance: 25.0,
            yaw: std::f32::consts::PI * 0.75,
            pitch: -0.55,
            speed: 15.0,
        }
    }
}
impl EditorCamera {
    pub fn forward(&self) -> Vec3 {
        Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        )
    }
    pub fn right(&self) -> Vec3 {
        Vec3::new(-self.yaw.cos(), 0.0, self.yaw.sin())
    }
    pub fn eye(&self) -> Vec3 {
        self.focus - self.forward() * self.distance
    }
    /// Turn around the focus point (mouse delta in pixels).
    pub fn orbit(&mut self, delta: Vec2) {
        self.yaw -= delta.x * 0.006;
        self.pitch = (self.pitch - delta.y * 0.006).clamp(-1.54, 1.54);
    }
    /// Look around from the eye, keeping the eye still (fly mode).
    pub fn look(&mut self, delta: Vec2) {
        let eye = self.eye();
        self.orbit(delta);
        self.focus = eye + self.forward() * self.distance;
    }
    /// Slide the view sideways and up (mouse delta in pixels).
    pub fn pan(&mut self, delta: Vec2) {
        let up = self.right().cross(self.forward()).normalize_or(Vec3::Y);
        let scale = self.distance * 0.0015;
        self.focus += (self.right() * -delta.x + up * delta.y) * scale;
    }
    /// Scroll: move towards or away from the focus.
    pub fn zoom(&mut self, steps: f32) {
        self.distance = (self.distance * 0.88f32.powf(steps)).clamp(0.5, 2000.0);
    }
    /// Fly: `input` is (right, up, forward) in -1..1 per axis.
    pub fn fly(&mut self, input: Vec3, dt: f32, fast: bool) {
        let speed = self.speed * if fast { 4.0 } else { 1.0 };
        let motion = self.right() * input.x + Vec3::Y * input.y + self.forward() * input.z;
        self.focus += motion.clamp_length_max(1.0) * speed * dt;
    }
    /// Look at a box from a sensible distance.
    pub fn frame(&mut self, min: Vec3, max: Vec3) {
        self.focus = (min + max) * 0.5;
        self.distance = ((max - min).length() * 1.4).clamp(3.0, 1500.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rays_hit_triangles_boxes_and_planes() {
        let ray = Ray {
            origin: Vec3::new(0.2, 5.0, 0.2),
            direction: Vec3::NEG_Y,
        };
        let t = ray.triangle(Vec3::ZERO, Vec3::X, Vec3::Z).unwrap();
        assert!((t - 5.0).abs() < 1e-5);
        assert!(ray
            .triangle(
                Vec3::new(2.0, 0.0, 0.0),
                Vec3::new(3.0, 0.0, 0.0),
                Vec3::new(2.0, 0.0, 1.0)
            )
            .is_none());
        assert!((ray.aabb(Vec3::splat(-1.0), Vec3::splat(1.0)).unwrap() - 4.0).abs() < 1e-5);
        assert!((ray.plane(Vec3::ZERO, Vec3::Y).unwrap() - 5.0).abs() < 1e-5);
    }

    #[test]
    fn screen_rays_go_through_the_screen_point() {
        let eye = Vec3::new(0.0, 10.0, 20.0);
        let view = Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::Y);
        let projection = Mat4::perspective_rh(1.0, 16.0 / 9.0, 3500.0, 0.1);
        let ray = Ray::from_screen(
            Vec2::new(800.0, 450.0),
            Vec2::new(1600.0, 900.0),
            projection * view,
            eye,
        );
        let expected = (Vec3::ZERO - eye).normalize();
        assert!(ray.direction.distance(expected) < 1e-3, "{}", ray.direction);
    }

    #[test]
    fn picking_takes_the_nearest_object() {
        let near = [[
            Vec3::new(-1.0, 1.0, -1.0),
            Vec3::new(1.0, 1.0, -1.0),
            Vec3::new(0.0, 1.0, 1.0),
        ]];
        let far = [[
            Vec3::new(-1.0, 0.0, -1.0),
            Vec3::new(1.0, 0.0, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
        ]];
        let ray = Ray {
            origin: Vec3::new(0.0, 5.0, 0.0),
            direction: Vec3::NEG_Y,
        };
        let item = |id, tris: &'static [[Vec3; 3]]| Pickable {
            id,
            triangles: tris,
            bounds: bounds(tris.iter().flatten().copied()).unwrap(),
        };
        let near: &'static [[Vec3; 3]] = Box::leak(Box::new(near));
        let far: &'static [[Vec3; 3]] = Box::leak(Box::new(far));
        assert_eq!(pick(&ray, [item(1, far), item(2, near)]).unwrap().0, 2);
        let miss = Ray {
            origin: Vec3::new(9.0, 5.0, 0.0),
            direction: Vec3::NEG_Y,
        };
        assert!(pick(&miss, [item(1, far)]).is_none());
    }

    #[test]
    fn axis_drags_follow_the_mouse() {
        // Looking down at the X axis; the mouse ray passes x = 3.
        let ray = Ray {
            origin: Vec3::new(3.0, 10.0, 0.0),
            direction: Vec3::NEG_Y,
        };
        let s = ray.closest_on_axis(Vec3::ZERO, Vec3::X).unwrap();
        assert!((s - 3.0).abs() < 1e-4);
        // Straight down the axis: no drag.
        let along = Ray {
            origin: Vec3::new(10.0, 0.0, 0.0),
            direction: Vec3::NEG_X,
        };
        assert!(along.closest_on_axis(Vec3::ZERO, Vec3::X).is_none());
        // Blender up is render +Y; Blender north is render -Z.
        assert_eq!(
            moved([1.0, 2.0, 3.0], Vec3::new(0.0, 1.0, -1.0)),
            [1.0, 3.0, 4.0]
        );
    }

    #[test]
    fn rotations_compose_in_world_and_local_space() {
        assert_eq!(
            rotated([0.0, 0.0, 170.0], 2, 20.0, false),
            [0.0, 0.0, -170.0]
        );
        assert_eq!(rotated([10.0, 0.0, 30.0], 0, 15.0, true), [25.0, 0.0, 30.0]);
        // World X on an object turned 90 degrees about Z equals its local Y.
        let world = rotated([0.0, 0.0, 90.0], 0, 30.0, false);
        let local = rotated([0.0, 0.0, 90.0], 1, -30.0, true);
        let q = |r| {
            Transform {
                rotation: r,
                ..Transform::default()
            }
            .rotation_quat()
        };
        assert!(
            q(world).angle_between(q(local)) < 1e-3,
            "{world:?} {local:?}"
        );
        let angle = angle_around(Vec3::Y, Vec3::X, Vec3::NEG_Z);
        assert!((angle - 90.0).abs() < 1e-3);
        assert_eq!(snap(1.26, 0.25), 1.25);
        assert_eq!(snap(1.26, 0.0), 1.26);
    }

    #[test]
    fn local_axes_turn_with_the_object() {
        let t = Transform {
            rotation: [0.0, 0.0, 90.0],
            ..Transform::default()
        };
        let [x, _, z] = gizmo_axes(&t, true);
        // Local X now points north (render -Z); up stays up.
        assert!(x.distance(Vec3::NEG_Z) < 1e-5, "{x}");
        assert!(z.distance(Vec3::Y) < 1e-5);
        assert_eq!(gizmo_axes(&t, false)[0], Vec3::X);
    }

    #[test]
    fn the_camera_orbits_pans_and_frames() {
        let mut camera = EditorCamera::default();
        let focus = camera.focus;
        camera.orbit(Vec2::new(100.0, 20.0));
        assert!((camera.eye().distance(focus) - camera.distance).abs() < 1e-3);
        let eye = camera.eye();
        camera.look(Vec2::new(50.0, 0.0));
        assert!(camera.eye().distance(eye) < 1e-3);
        camera.frame(Vec3::splat(-1.0), Vec3::splat(1.0));
        assert_eq!(camera.focus, Vec3::ZERO);
        camera.zoom(1.0);
        assert!(camera.distance < 4.85);
    }
}
