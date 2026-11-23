//! Build Mode roads: a road drawn along points on a layout object.
//!
//! The road runs through its points (the object's own metres: x east,
//! y north, z up) along a smooth curve (Catmull-Rom), as wide as
//! [`Road::width`]. It is drawn with asphalt, edge lines and a dashed centre
//! line, has a short kerb down each side so it never floats over uneven
//! ground, and is solid like any model. The object's transform places,
//! turns and scales it.
use crate::gltf_model::Model;
use crate::world_layout::Road;
use crate::{Batch, Vertex};
use glam::{Vec2, Vec3};
use std::collections::HashMap;

/// Texture key of roads (plain white, vertex coloured).
pub const KEY: &str = crate::builtin_assets::KEY;

const ASPHALT: [f32; 3] = [0.2, 0.2, 0.21];
const LINE: [f32; 3] = [0.9, 0.85, 0.5];
const EDGE: [f32; 3] = [0.85, 0.85, 0.82];
/// The road surface over its points, so it covers flat ground there.
const LIFT: f32 = 0.02;
/// Lines over the surface.
const PAINT: f32 = 0.015;
/// How far the kerb reaches down below the surface.
const KERB: f32 = 0.35;
/// About how far apart the curve's samples are, metres.
const STEP: f32 = 1.0;
const DASH: f32 = 3.0;

impl Road {
    /// The smooth centre line through the points, sampled about every
    /// metre (each point is on it).
    pub fn curve(&self) -> Vec<Vec3> {
        let points: Vec<Vec3> = self.points.iter().map(|p| Vec3::from_array(*p)).collect();
        let Some(first) = points.first() else {
            return Vec::new();
        };
        let mut out = vec![*first];
        for i in 0..points.len().saturating_sub(1) {
            let p0 = points[i.saturating_sub(1)];
            let (p1, p2) = (points[i], points[i + 1]);
            let p3 = points[(i + 2).min(points.len() - 1)];
            // Points are at most a few km apart: about a metre each keeps
            // long bends smooth and dashes short.
            let steps = ((p2.distance(p1) / STEP).ceil() as usize).clamp(1, 8192);
            for k in 1..=steps {
                out.push(catmull_rom(p0, p1, p2, p3, k as f32 / steps as f32));
            }
        }
        out.dedup_by(|a, b| a.distance(*b) < 1e-4);
        out
    }
    /// Length along the curve, metres.
    pub fn length(&self) -> f32 {
        self.curve().windows(2).map(|w| w[0].distance(w[1])).sum()
    }
    /// A new point past the last one, `distance` metres on in the road's
    /// direction there (east for a road of one point).
    pub fn extended(&self, distance: f32) -> [f32; 3] {
        let n = self.points.len();
        let last = self
            .points
            .last()
            .map_or(Vec3::ZERO, |p| Vec3::from_array(*p));
        let direction = if n >= 2 {
            let before = Vec3::from_array(self.points[n - 2]);
            Vec2::new(last.x - before.x, last.y - before.y).normalize_or(Vec2::X)
        } else {
            Vec2::X
        };
        (last + (direction * distance).extend(0.0)).to_array()
    }
}

/// The bend through the points is a Catmull-Rom curve; the height eases
/// from one point's height to the next without overshooting, so a road
/// never dips below (or bumps over) the points around it.
fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let mut p = bend(p0, p1, p2, p3, t);
    p.z = p1.z + (p2.z - p1.z) * t * t * (3.0 - 2.0 * t);
    p
}

fn bend(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let (t2, t3) = (t * t, t * t * t);
    0.5 * (2.0 * p1
        + (p2 - p0) * t
        + (2.0 * p0 - 5.0 * p1 + 4.0 * p2 - p3) * t2
        + (3.0 * p1 - p0 - 3.0 * p2 + p3) * t3)
}

fn shaded(color: [f32; 3], normal: Vec3) -> [f32; 4] {
    // The same light as terrain, so roads sit in it naturally.
    let light = Vec3::new(-0.6, 0.45, 0.66).normalize();
    let shade = 0.4 + 0.6 * normal.dot(light).max(0.0);
    [color[0] * shade, color[1] * shade, color[2] * shade, 1.0]
}

#[derive(Default)]
struct Mesh {
    vertices: Vec<Vertex>,
}
impl Mesh {
    /// A quad a-b-c-d (counter-clockwise seen from where it faces), in
    /// Blender metres.
    fn quad(&mut self, [a, b, c, d]: [Vec3; 4], color: [f32; 3]) {
        let normal = (b - a).cross(c - a).normalize_or_zero();
        let color = shaded(color, normal);
        for (p, uv) in [
            (a, [0.0, 0.0]),
            (b, [1.0, 0.0]),
            (c, [1.0, 1.0]),
            (a, [0.0, 0.0]),
            (c, [1.0, 1.0]),
            (d, [0.0, 1.0]),
        ] {
            self.vertices.push(Vertex {
                position: crate::world_layout::blender_to_gltf(p.to_array()).to_array(),
                uv,
                color,
            });
        }
    }
    /// A strip from `across[0]` to `across[1]` metres sideways (0 is the
    /// centre, positive is left) over the curve's segments `segments`, `up`
    /// over the surface.
    fn strip(
        &mut self,
        frames: &[Frame],
        segments: std::ops::Range<usize>,
        across: [f32; 2],
        up: f32,
        color: [f32; 3],
    ) {
        let end = segments.end.min(frames.len().saturating_sub(1));
        for i in segments.start..end {
            let (f, g) = (&frames[i], &frames[i + 1]);
            let lift = Vec3::Z * up;
            self.quad(
                [
                    f.at(across[0]) + lift,
                    g.at(across[0]) + lift,
                    g.at(across[1]) + lift,
                    f.at(across[1]) + lift,
                ],
                color,
            );
        }
    }
}

/// A sample of the centre line with the level direction to its left.
struct Frame {
    center: Vec3,
    left: Vec3,
    /// Metres along the road.
    distance: f32,
}
impl Frame {
    fn at(&self, across: f32) -> Vec3 {
        self.center + self.left * across
    }
}

fn frames(road: &Road) -> Vec<Frame> {
    let curve = road.curve();
    let mut distance = 0.0;
    let mut previous_left = Vec3::Y;
    curve
        .iter()
        .enumerate()
        .map(|(i, &center)| {
            let before = curve[i.saturating_sub(1)];
            let after = curve[(i + 1).min(curve.len() - 1)];
            let ahead = Vec2::new(after.x - before.x, after.y - before.y);
            // Level sideways, so the road never tilts across.
            let left = if ahead.length() > 1e-4 {
                Vec2::new(-ahead.y, ahead.x).normalize().extend(0.0)
            } else {
                previous_left
            };
            previous_left = left;
            if i > 0 {
                distance += curve[i - 1].distance(center);
            }
            Frame {
                center: center + Vec3::Z * LIFT,
                left,
                distance,
            }
        })
        .collect()
}

/// The road as a model in glTF space (Y up), drawn and solid.
pub fn model(road: &Road) -> Model {
    let frames = frames(road);
    let half = road.width * 0.5;
    let n = frames.len();
    let mut solid = Mesh::default();
    // Surface, then a kerb down each side (outward facing).
    solid.strip(&frames, 0..n, [-half, half], 0.0, ASPHALT);
    let down = Vec3::Z * -KERB;
    for pair in frames.windows(2) {
        let (f, g) = (&pair[0], &pair[1]);
        let (fl, gl, fr, gr) = (f.at(half), g.at(half), f.at(-half), g.at(-half));
        solid.quad([fl, gl, gl + down, fl + down], ASPHALT);
        solid.quad([fr, fr + down, gr + down, gr], ASPHALT);
    }
    // Paint: edge lines on wider roads, and a dashed centre line.
    let mut paint = Mesh::default();
    if road.width >= 5.0 {
        let edge = half - 0.35;
        paint.strip(&frames, 0..n, [edge - 0.12, edge], PAINT, EDGE);
        paint.strip(&frames, 0..n, [-edge, -edge + 0.12], PAINT, EDGE);
    }
    if road.width >= 3.0 {
        for i in 0..n.saturating_sub(1) {
            let middle = (frames[i].distance + frames[i + 1].distance) * 0.5;
            if ((middle / DASH) as u32).is_multiple_of(2) {
                paint.strip(&frames, i..i + 1, [-0.08, 0.08], PAINT, LINE);
            }
        }
    }
    let batch = |vertices: Vec<Vertex>| Batch {
        key: KEY.into(),
        vertices,
        alpha: false,
        animated: false,
        uv_animation: None,
        map_door: None,
        mlo_entity: None,
    };
    let collision = batch(solid.vertices.clone());
    let mut drawn = solid.vertices;
    drawn.extend(paint.vertices);
    let triangles = drawn.len() / 3;
    Model {
        collision: vec![collision],
        drawn: vec![batch(drawn)],
        textures: HashMap::from([(KEY.into(), crate::builtin_assets::white())]),
        triangles,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn road(points: &[[f32; 3]], width: f32) -> Road {
        Road {
            points: points.to_vec(),
            width,
            extra: Default::default(),
        }
    }

    #[test]
    fn the_curve_passes_through_every_point_smoothly() {
        let r = road(&[[0.0, 0.0, 0.0], [20.0, 0.0, 0.0], [20.0, 20.0, 2.0]], 8.0);
        let curve = r.curve();
        for p in &r.points {
            let p = Vec3::from_array(*p);
            assert!(
                curve.iter().any(|c| c.distance(p) < 1e-3),
                "{p} on the curve"
            );
        }
        // About a metre between samples, and longer than the straight legs
        // would be if it cut corners.
        assert!(curve.windows(2).all(|w| w[0].distance(w[1]) < 1.6));
        assert!(r.length() > 38.0 && r.length() < 45.0, "{}", r.length());
        assert_eq!(road(&[[1.0, 2.0, 3.0]], 8.0).curve().len(), 1);
    }

    #[test]
    fn a_road_is_solid_and_level_across() {
        let r = road(&[[0.0, 0.0, 0.0], [30.0, 0.0, 0.0], [60.0, 10.0, 3.0]], 8.0);
        let model = model(&r);
        assert!(model.triangles > 100);
        let world = crate::collision::CollisionWorld::from_batches(&model.collision);
        // Blender (10, 3) is glTF (10, -3): on the road, 3 m left of centre.
        let ground = world
            .ground_below(Vec3::new(10.0, 5.0, -3.0), 10.0)
            .unwrap();
        assert!((ground - LIFT).abs() < 0.05, "{ground}");
        // Beside the road there is nothing.
        assert!(world
            .ground_below(Vec3::new(10.0, 5.0, -6.0), 10.0)
            .is_none());
        // Climbing towards the last point.
        let high = world
            .ground_below(Vec3::new(59.0, 10.0, -9.7), 10.0)
            .unwrap();
        assert!(high > 2.0, "{high}");
    }

    #[test]
    fn roads_must_go_somewhere_and_long_ones_stay_smooth() {
        assert!(!road(&[[5.0, 5.0, 0.0], [5.0, 5.0, 0.0]], 8.0).is_valid());
        assert!(road(&[[0.0; 3], [1.0, 0.0, 0.0]], 8.0).is_valid());
        let long = road(
            &[[-1900.0, 0.0, 0.0], [0.0, 300.0, 0.0], [1900.0, 0.0, 0.0]],
            8.0,
        );
        assert!(long.is_valid());
        assert!(long.curve().windows(2).all(|w| w[0].distance(w[1]) < 1.6));
    }

    #[test]
    fn extending_goes_on_in_the_road_direction() {
        let r = road(&[[0.0, 0.0, 0.0], [0.0, 10.0, 1.0]], 8.0);
        assert_eq!(r.extended(16.0), [0.0, 26.0, 1.0]);
        assert_eq!(road(&[[2.0, 0.0, 0.0]], 8.0).extended(5.0), [7.0, 0.0, 0.0]);
    }
}
