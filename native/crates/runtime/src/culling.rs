//! Conservative static-batch visibility using the actual GPU camera matrix.
use glam::{Mat4, Vec3, Vec4};

#[derive(Clone, Copy)]
pub(super) struct Bounds {
    min: Vec3,
    max: Vec3,
    invalid: bool,
}
impl Default for Bounds {
    fn default() -> Self {
        Self {
            min: Vec3::splat(f32::INFINITY),
            max: Vec3::splat(f32::NEG_INFINITY),
            invalid: false,
        }
    }
}
impl Bounds {
    pub fn include(&mut self, vertices: &[sa_scene::Vertex]) {
        for vertex in vertices {
            let point = Vec3::from_array(vertex.position);
            if !point.is_finite() {
                self.invalid = true;
            }
            self.min = self.min.min(point);
            self.max = self.max.max(point);
        }
    }
    pub fn finish(self) -> Option<Self> {
        (!self.invalid && self.min.is_finite() && self.max.is_finite()).then_some(self)
    }
    pub fn from_vertices(vertices: &[sa_scene::Vertex]) -> Option<Self> {
        let mut bounds = Self::default();
        bounds.include(vertices);
        bounds.finish()
    }
}

#[derive(Default)]
pub(super) struct Frustum {
    planes: Option<[Vec4; 6]>,
}
impl Frustum {
    pub fn new(matrix: Mat4) -> Self {
        // WebGPU clip depth is 0..w, unlike OpenGL's -w..w near plane.
        let w = matrix.row(3);
        let planes = [
            w + matrix.row(0),
            w - matrix.row(0),
            w + matrix.row(1),
            w - matrix.row(1),
            matrix.row(2),
            w - matrix.row(2),
        ];
        Self {
            planes: planes
                .iter()
                .all(|p| p.is_finite() && p.truncate().length() > 1e-8)
                .then_some(planes),
        }
    }
    pub fn visible(&self, bounds: Option<Bounds>) -> bool {
        let (Some(planes), Some(bounds)) = (&self.planes, bounds) else {
            return true;
        };
        planes.iter().all(|plane| {
            let positive = Vec3::new(
                if plane.x >= 0.0 {
                    bounds.max.x
                } else {
                    bounds.min.x
                },
                if plane.y >= 0.0 {
                    bounds.max.y
                } else {
                    bounds.min.y
                },
                if plane.z >= 0.0 {
                    bounds.max.z
                } else {
                    bounds.min.z
                },
            );
            // A small world-space margin retains objects touching a clip edge.
            plane.dot(positive.extend(1.0)) >= -0.05 * plane.truncate().length()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn box_at(center: Vec3, size: f32) -> Option<Bounds> {
        Some(Bounds {
            min: center - Vec3::splat(size),
            max: center + Vec3::splat(size),
            invalid: false,
        })
    }
    #[test]
    fn perspective_planes_keep_edges_and_camera_enclosing_boxes() {
        let frustum = Frustum::new(Mat4::perspective_rh(
            std::f32::consts::FRAC_PI_2,
            1.0,
            0.1,
            1000.0,
        ));
        for point in [
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::new(2.0, 0.0, -2.0),
            Vec3::new(0.0, 2.0, -2.0),
            Vec3::new(0.0, 0.0, -0.1),
        ] {
            assert!(frustum.visible(box_at(point, 0.01)));
        }
        assert!(frustum.visible(box_at(Vec3::ZERO, 2.0)));
        for point in [
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::new(5.0, 0.0, -2.0),
            Vec3::new(0.0, 5.0, -2.0),
            Vec3::new(0.0, 0.0, -2000.0),
        ] {
            assert!(!frustum.visible(box_at(point, 0.01)));
        }
        assert!(!frustum.visible(box_at(Vec3::new(0.0, 0.0, -0.01), 0.005)));
        assert!(frustum.visible(None));
        assert!(Frustum::new(Mat4::ZERO).visible(box_at(Vec3::splat(10000.0), 1.0)));
    }
    #[test]
    fn translated_rotated_camera_and_incremental_bounds_are_conservative() {
        let eye = Vec3::new(30.0, 10.0, 40.0);
        let frustum = Frustum::new(
            Mat4::perspective_rh(1.0, 1.6, 0.1, 1000.0)
                * Mat4::look_at_rh(eye, eye + Vec3::X, Vec3::Y),
        );
        assert!(frustum.visible(box_at(eye + Vec3::X * 10.0, 1.0)));
        assert!(!frustum.visible(box_at(eye - Vec3::X * 10.0, 1.0)));
        let vertex = |point: Vec3| sa_scene::Vertex {
            position: point.to_array(),
            uv: [0.0; 2],
            color: [1.0; 4],
        };
        let vertices = [vertex(eye + Vec3::X * 9.0), vertex(eye + Vec3::X * 11.0)];
        let mut bounds = Bounds::default();
        assert!(bounds.finish().is_none());
        bounds.include(&vertices[..1]);
        bounds.include(&vertices[1..]);
        assert!(frustum.visible(bounds.finish()));
        bounds.include(&[vertex(Vec3::splat(f32::NAN))]);
        assert!(bounds.finish().is_none());
    }
}
