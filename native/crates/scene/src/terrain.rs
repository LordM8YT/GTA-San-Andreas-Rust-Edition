//! Build Mode terrain: a square height grid on a layout object, sculpted in
//! the editor and drawn and walked on like any model.
//!
//! The grid is `cells` × `cells` squares over `size` metres, centred on the
//! object, with one height (metres, up) per corner, row by row from south to
//! north and west to east within a row. Heights live in the layout, so a
//! terrain is portable like everything else in a pack. The object's
//! transform places, turns and scales it.
//!
//! The brushes ([`raise`], [`smooth`], [`flatten`]) work in the terrain's
//! own metres, so the editor and tests share them.
use crate::gltf_model::Model;
use crate::world_layout::Terrain;
use crate::{Batch, Vertex};
use glam::{Vec2, Vec3};
use std::collections::HashMap;

/// Texture key of terrain (plain white, vertex coloured).
pub const KEY: &str = crate::builtin_assets::KEY;

impl Terrain {
    /// Corners per side.
    pub fn side(&self) -> usize {
        usize::from(self.cells) + 1
    }
    /// Metres between two corners.
    pub fn spacing(&self) -> f32 {
        self.size / f32::from(self.cells.max(1))
    }
    /// The corner at (column, row) in the terrain's own metres (x east,
    /// y north), height included.
    pub fn corner(&self, column: usize, row: usize) -> Vec3 {
        let half = self.size * 0.5;
        Vec3::new(
            -half + column as f32 * self.spacing(),
            -half + row as f32 * self.spacing(),
            self.heights[row * self.side() + column],
        )
    }
    /// Height at a point in the terrain's own metres, between the corners
    /// (None outside the terrain).
    pub fn height_at(&self, x: f32, y: f32) -> Option<f32> {
        let half = self.size * 0.5;
        let (u, v) = ((x + half) / self.spacing(), (y + half) / self.spacing());
        let last = f32::from(self.cells);
        if !(0.0..=last).contains(&u) || !(0.0..=last).contains(&v) {
            return None;
        }
        let (c, r) = (
            (u.floor() as usize).min(self.side() - 2),
            (v.floor() as usize).min(self.side() - 2),
        );
        let (fu, fv) = (u - c as f32, v - r as f32);
        let h = |c: usize, r: usize| self.heights[r * self.side() + c];
        let south = h(c, r) + (h(c + 1, r) - h(c, r)) * fu;
        let north = h(c, r + 1) + (h(c + 1, r + 1) - h(c, r + 1)) * fu;
        Some(south + (north - south) * fv)
    }
    /// Change every corner within `radius` of `center` (terrain metres):
    /// `edit(corner index, height, weight)` returns the new height, weight
    /// being 1 at the centre falling smoothly to 0 at the edge. True if
    /// anything changed.
    fn brush(
        &mut self,
        center: Vec2,
        radius: f32,
        mut edit: impl FnMut(usize, f32, f32) -> f32,
    ) -> bool {
        let radius = radius.max(0.01);
        let mut changed = false;
        for row in 0..self.side() {
            for column in 0..self.side() {
                let corner = self.corner(column, row);
                let distance = Vec2::new(corner.x, corner.y).distance(center);
                if distance >= radius {
                    continue;
                }
                let t = distance / radius;
                let weight = 1.0 - t * t * (3.0 - 2.0 * t);
                let index = row * self.side() + column;
                let height = edit(index, self.heights[index], weight)
                    .clamp(-Terrain::MAX_HEIGHT, Terrain::MAX_HEIGHT);
                if height != self.heights[index] {
                    self.heights[index] = height;
                    changed = true;
                }
            }
        }
        changed
    }
}

/// Paint every square whose middle is within `radius` of `center` with
/// `material` (an index in [`crate::world_layout::TERRAIN_MATERIALS`], or
/// [`Terrain::HOLE`] to cut it out). Painting a hole fills it again. True if
/// anything changed.
pub fn paint(terrain: &mut Terrain, center: Vec2, radius: f32, material: u8) -> bool {
    let cells = usize::from(terrain.cells);
    if terrain.paint.len() != cells * cells {
        if material == 0 {
            return false; // Unpainted ground is already ground.
        }
        terrain.paint = vec![0; cells * cells];
    }
    let before = terrain.paint.clone();
    let half = terrain.size * 0.5;
    let spacing = terrain.spacing();
    // A brush smaller than a square still paints the square it is in.
    let radius = radius.max(spacing * 0.5);
    let mut changed = false;
    for row in 0..cells {
        for column in 0..cells {
            let middle = Vec2::new(
                -half + (column as f32 + 0.5) * spacing,
                -half + (row as f32 + 0.5) * spacing,
            );
            let slot = &mut terrain.paint[row * cells + column];
            if middle.distance(center) <= radius && *slot != material {
                *slot = material;
                changed = true;
            }
        }
    }
    if terrain.paint.iter().all(|m| *m == Terrain::HOLE) {
        // A terrain cut away completely would be nothing to select or
        // paint again: keep the last of it.
        terrain.paint = before;
        return false;
    }
    if terrain.paint.iter().all(|m| *m == 0) {
        terrain.paint.clear();
    }
    changed
}

/// Raise (or with a negative `amount`, lower) the ground around `center` by
/// up to `amount` metres.
pub fn raise(terrain: &mut Terrain, center: Vec2, radius: f32, amount: f32) -> bool {
    terrain.brush(center, radius, |_, h, w| h + amount * w)
}
/// Move heights around `center` towards `height` by `strength` (0-1).
pub fn flatten(
    terrain: &mut Terrain,
    center: Vec2,
    radius: f32,
    height: f32,
    strength: f32,
) -> bool {
    let strength = strength.clamp(0.0, 1.0);
    terrain.brush(center, radius, |_, h, w| h + (height - h) * strength * w)
}
/// Even out bumps around `center`: each corner moves towards the average of
/// its neighbours by `strength` (0-1).
pub fn smooth(terrain: &mut Terrain, center: Vec2, radius: f32, strength: f32) -> bool {
    let before = terrain.heights.clone();
    let side = terrain.side();
    let average = |index: usize| {
        let (row, column) = (index / side, index % side);
        let mut sum = 0.0;
        let mut count = 0.0;
        for (dr, dc) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
            let (r, c) = (row as i32 + dr, column as i32 + dc);
            if (0..side as i32).contains(&r) && (0..side as i32).contains(&c) {
                sum += before[r as usize * side + c as usize];
                count += 1.0;
            }
        }
        sum / count
    };
    let strength = strength.clamp(0.0, 1.0);
    terrain.brush(center, radius, |index, h, w| {
        h + (average(index) - h) * strength * w
    })
}

/// Shade of a ground colour for a surface facing `normal` (Blender axes),
/// so slopes read without textures.
fn shaded(color: [f32; 3], normal: Vec3) -> [f32; 4] {
    // Light from the north-west, low enough that hills read clearly.
    let light = Vec3::new(-0.6, 0.45, 0.66).normalize();
    let shade = 0.4 + 0.6 * normal.dot(light).max(0.0);
    [color[0] * shade, color[1] * shade, color[2] * shade, 1.0]
}

/// The terrain as a model in glTF space (Y up), drawn and solid.
pub fn model(terrain: &Terrain) -> Model {
    let side = terrain.side();
    let cells = side - 1;
    let to_gltf = |p: Vec3| crate::world_layout::blender_to_gltf(p.to_array());
    // Each corner takes the average colour of the squares around it (holes
    // left out), so painted areas blend into each other.
    let corner_colors: Vec<Vec3> = (0..side * side)
        .map(|index| {
            let (row, column) = (index / side, index % side);
            let mut sum = Vec3::ZERO;
            let mut count = 0.0;
            for (r, c) in [(row, column), (row, column.wrapping_sub(1))]
                .into_iter()
                .chain([
                    (row.wrapping_sub(1), column),
                    (row.wrapping_sub(1), column.wrapping_sub(1)),
                ])
            {
                if r < cells && c < cells {
                    let material = terrain.material(c, r);
                    if material != Terrain::HOLE {
                        sum += Vec3::from_array(terrain.material_color(material));
                        count += 1.0;
                    }
                }
            }
            if count > 0.0 {
                sum / count
            } else {
                Vec3::from_array(terrain.color)
            }
        })
        .collect();
    let mut vertices = Vec::with_capacity(cells.pow(2) * 6);
    for row in 0..cells {
        for column in 0..cells {
            if terrain.material(column, row) == Terrain::HOLE {
                continue;
            }
            let corner = |c: usize, r: usize| (terrain.corner(c, r), corner_colors[r * side + c]);
            let a = corner(column, row);
            let b = corner(column + 1, row);
            let c = corner(column + 1, row + 1);
            let d = corner(column, row + 1);
            for [p, q, r] in [[a, b, c], [a, c, d]] {
                let normal = (q.0 - p.0).cross(r.0 - p.0).normalize_or_zero();
                for (corner, tint) in [p, q, r] {
                    let color = shaded(tint.to_array(), normal);
                    let local = Vec2::new(corner.x, corner.y) / terrain.size + 0.5;
                    vertices.push(Vertex {
                        position: to_gltf(corner).to_array(),
                        uv: local.to_array(),
                        color,
                    });
                }
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
    let triangles = vertices.len() / 3;
    Model {
        collision: vec![batch(vertices.clone())],
        drawn: vec![batch(vertices)],
        textures: HashMap::from([(KEY.into(), crate::builtin_assets::white())]),
        triangles,
        animation: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flat_terrain_is_a_walkable_grid() {
        let terrain = Terrain::flat(32.0, 16);
        assert_eq!(terrain.heights.len(), 17 * 17);
        assert_eq!(terrain.height_at(0.0, 0.0), Some(0.0));
        assert_eq!(terrain.height_at(16.1, 0.0), None);
        let model = model(&terrain);
        assert_eq!(model.triangles, 16 * 16 * 2);
        let world = crate::collision::CollisionWorld::from_batches(&model.collision);
        // glTF: up is Y.
        assert!(world.standing_at(Vec3::new(3.0, 0.0, -3.0), 1.0).is_some());
    }

    #[test]
    fn brushes_raise_lower_flatten_and_smooth() {
        let mut terrain = Terrain::flat(32.0, 16);
        assert!(raise(&mut terrain, Vec2::ZERO, 8.0, 4.0));
        let peak = terrain.height_at(0.0, 0.0).unwrap();
        assert!((peak - 4.0).abs() < 1e-4, "{peak}");
        assert!(
            terrain.height_at(4.0, 0.0).unwrap() > 0.5,
            "falls off gently"
        );
        assert_eq!(terrain.height_at(12.0, 0.0), Some(0.0), "outside the brush");
        // Between corners the height is interpolated.
        let between = terrain.height_at(1.0, 0.0).unwrap();
        assert!(between < peak && between > terrain.height_at(2.0, 0.0).unwrap());
        // Lowering is raising by a negative amount.
        raise(&mut terrain, Vec2::ZERO, 8.0, -1.0);
        assert!((terrain.height_at(0.0, 0.0).unwrap() - 3.0).abs() < 1e-4);
        // Smoothing pulls the peak down towards its neighbours.
        let before = terrain.height_at(0.0, 0.0).unwrap();
        assert!(smooth(&mut terrain, Vec2::ZERO, 4.0, 1.0));
        assert!(terrain.height_at(0.0, 0.0).unwrap() < before);
        // Flattening at full strength makes the centre exactly that height.
        flatten(&mut terrain, Vec2::ZERO, 20.0, 1.0, 1.0);
        assert!((terrain.height_at(0.0, 0.0).unwrap() - 1.0).abs() < 1e-4);
        // Heights stay within limits.
        raise(&mut terrain, Vec2::ZERO, 8.0, 1e6);
        assert!(terrain
            .heights
            .iter()
            .all(|h| h.abs() <= Terrain::MAX_HEIGHT));
        assert!(terrain.is_valid());
    }

    #[test]
    fn paint_colours_squares_and_holes_cut_through() {
        let mut terrain = Terrain::flat(32.0, 16);
        let flat = model(&terrain).triangles;
        // Painting the terrain's own material on unpainted ground is a no-op.
        assert!(!paint(&mut terrain, Vec2::ZERO, 4.0, 0));
        assert!(terrain.paint.is_empty());
        // Sand around the middle: the colour changes there, not far away.
        assert!(paint(&mut terrain, Vec2::ZERO, 4.0, 3));
        assert!(terrain.is_valid());
        assert_eq!(terrain.material(8, 8), 3);
        assert_eq!(terrain.material(0, 0), 0);
        let sandy = model(&terrain);
        let near = |m: &Model, x: f32, y: f32| {
            m.drawn[0]
                .vertices
                .iter()
                .find(|v| (v.position[0] - x).abs() < 0.01 && (v.position[2] + y).abs() < 0.01)
                .map(|v| v.color)
                .unwrap()
        };
        assert_ne!(
            near(&sandy, 0.0, 0.0),
            near(&model(&Terrain::flat(32.0, 16)), 0.0, 0.0)
        );
        // A hole: fewer triangles, and nothing to stand on there.
        assert!(paint(&mut terrain, Vec2::new(8.0, 8.0), 2.0, Terrain::HOLE));
        let holed = model(&terrain);
        assert!(holed.triangles < flat);
        let world = crate::collision::CollisionWorld::from_batches(&holed.collision);
        // Blender (9, 9) is glTF (9, -9).
        assert!(world
            .ground_below(Vec3::new(9.0, 5.0, -9.0), 10.0)
            .is_none());
        assert!(world
            .ground_below(Vec3::new(-9.0, 5.0, 9.0), 10.0)
            .is_some());
        // Painting over it fills it again.
        paint(&mut terrain, Vec2::new(8.0, 8.0), 2.0, 1);
        assert_eq!(model(&terrain).triangles, flat);
        // Painting it all back to ground drops the paint list.
        paint(&mut terrain, Vec2::ZERO, 100.0, 0);
        assert!(terrain.paint.is_empty());
        // The Hole brush never cuts the whole terrain away.
        let mut small = Terrain::flat(8.0, 2);
        assert!(!paint(&mut small, Vec2::ZERO, 100.0, Terrain::HOLE));
        assert!(model(&small).triangles > 0);
        // A paint list of the wrong size or an unknown material is invalid.
        terrain.paint = vec![0; 3];
        assert!(!terrain.is_valid());
        terrain.paint = vec![9; 16 * 16];
        assert!(!terrain.is_valid());
    }

    #[test]
    fn sculpted_terrain_is_solid_where_it_is_drawn() {
        let mut terrain = Terrain::flat(32.0, 16);
        raise(&mut terrain, Vec2::new(4.0, 4.0), 6.0, 3.0);
        let world = crate::collision::CollisionWorld::from_batches(&model(&terrain).collision);
        // Blender (4, 4) is glTF (4, -4); the ground there is 3 m up.
        let ground = world
            .ground_below(Vec3::new(4.0, 10.0, -4.0), 10.0)
            .unwrap();
        assert!((ground - 3.0).abs() < 0.05, "{ground}");
    }
}
