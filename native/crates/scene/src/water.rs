//! Static water polygons and surface queries in original GTA coordinates.
use crate::{Batch, Vertex};
use anyhow::{ensure, Result};

#[derive(Default)]
pub struct WaterMap {
    triangles: Vec<([[f32; 3]; 3], bool)>,
}
impl WaterMap {
    pub fn parse(lines: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut map = Self::default();
        for line in lines {
            if line.eq_ignore_ascii_case("processed") {
                continue;
            }
            let fields: Vec<_> = line.split_whitespace().collect();
            ensure!(matches!(fields.len(), 22 | 29), "invalid water polygon");
            let count = (fields.len() - 1) / 7;
            let flags: u32 = fields[fields.len() - 1].parse()?;
            let mut points = Vec::new();
            for row in fields[..fields.len() - 1].as_chunks::<7>().0.iter() {
                let values: Vec<f32> = row
                    .iter()
                    .map(|v| v.parse())
                    .collect::<std::result::Result<_, _>>()?;
                ensure!(
                    values.iter().all(|v| v.is_finite() && v.abs() <= 100_000.0),
                    "invalid water coordinate"
                );
                points.push([values[0], values[1], values[2]]);
            }
            let center = [
                points.iter().map(|p| p[0]).sum::<f32>() / count as f32,
                points.iter().map(|p| p[1]).sum::<f32>() / count as f32,
            ];
            points.sort_by(|a, b| {
                (a[1] - center[1])
                    .atan2(a[0] - center[0])
                    .total_cmp(&(b[1] - center[1]).atan2(b[0] - center[0]))
            });
            for i in 1..count - 1 {
                map.triangles
                    .push(([points[0], points[i], points[i + 1]], flags & 1 != 0));
            }
            ensure!(map.triangles.len() <= 40_000, "water polygon budget");
        }
        Ok(map)
    }
    pub fn level_at(&self, xy: [f32; 2]) -> Option<f32> {
        let mut level = None;
        for (p, _) in &self.triangles {
            let ab = [p[1][0] - p[0][0], p[1][1] - p[0][1]];
            let ac = [p[2][0] - p[0][0], p[2][1] - p[0][1]];
            let d = ab[0] * ac[1] - ac[0] * ab[1];
            if d.abs() < 1e-6 {
                continue;
            }
            let x = xy[0] - p[0][0];
            let y = xy[1] - p[0][1];
            let u = (x * ac[1] - ac[0] * y) / d;
            let v = (ab[0] * y - x * ab[1]) / d;
            if u >= -0.001 && v >= -0.001 && u + v <= 1.001 {
                let z = p[0][2] + u * (p[1][2] - p[0][2]) + v * (p[2][2] - p[0][2]);
                if level.is_none_or(|old| z > old) {
                    level = Some(z);
                }
            }
        }
        level
    }
    pub fn batch(&self, center: [f32; 2], origin: [f32; 2], radius: f32) -> Batch {
        let mut batch = Batch {
            key: "runtime:water".into(),
            vertices: Vec::new(),
            alpha: true,
            animated: false,
        };
        for (points, visible) in &self.triangles {
            if !visible {
                continue;
            }
            let min = [
                points.iter().map(|p| p[0]).fold(f32::INFINITY, f32::min),
                points.iter().map(|p| p[1]).fold(f32::INFINITY, f32::min),
            ];
            let max = [
                points
                    .iter()
                    .map(|p| p[0])
                    .fold(f32::NEG_INFINITY, f32::max),
                points
                    .iter()
                    .map(|p| p[1])
                    .fold(f32::NEG_INFINITY, f32::max),
            ];
            if (0..2).any(|i| max[i] < center[i] - radius || min[i] > center[i] + radius) {
                continue;
            }
            for p in points {
                batch.vertices.push(Vertex {
                    position: [p[0] - origin[0], p[2], origin[1] - p[1]],
                    uv: [p[0] / 25.0, p[1] / 25.0],
                    color: [0.25, 0.55, 0.65, 0.8],
                });
            }
        }
        batch
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn player_floats_moves_and_can_jump_from_surface() {
        let water = WaterMap::parse([
            "-100 -100 0 0 0 1 0 100 -100 0 0 0 1 0 -100 100 0 0 0 1 0 100 100 0 0 0 1 0 1".into(),
        ])
        .unwrap();
        let world = crate::collision::CollisionWorld::from_batches(&[]);
        let mut player = crate::collision::Player {
            feet: glam::Vec3::new(0.0, -5.0, 0.0),
            vertical_speed: -10.0,
            grounded: false,
        };
        for _ in 0..120 {
            player.step_in_water(
                &world,
                &water,
                [0.0, 0.0],
                glam::Vec3::X * 4.5,
                false,
                1.0 / 60.0,
            );
        }
        assert!((player.feet.y + 1.2).abs() < 0.001);
        assert!(player.feet.x > 4.0);
        assert!(!player.grounded);
        player.step_in_water(
            &world,
            &water,
            [0.0, 0.0],
            glam::Vec3::ZERO,
            true,
            1.0 / 60.0,
        );
        assert!(player.feet.y > -1.2);
        let paused = player.feet;
        player.step_in_water(&world, &water, [0.0, 0.0], glam::Vec3::X, true, 0.0);
        assert_eq!(player.feet, paused);
    }
    #[test]
    fn quad_covers_both_halves_and_water_is_not_solid_ground() {
        let map = WaterMap::parse(
            [
                "processed",
                "0 0 2 0 0 1 0 10 0 2 0 0 1 0 0 10 2 0 0 1 0 10 10 2 0 0 1 0 1",
            ]
            .map(str::to_string),
        )
        .unwrap();
        assert_eq!(map.level_at([1.0, 9.0]), Some(2.0));
        assert_eq!(map.level_at([9.0, 1.0]), Some(2.0));
        assert_eq!(map.level_at([11.0, 5.0]), None);
        let batch = map.batch([5.0, 5.0], [0.0, 0.0], 20.0);
        assert_eq!(batch.vertices.len(), 6);
        assert_eq!(
            crate::collision::CollisionWorld::from_batches(&[batch]).triangle_count(),
            0
        );
        assert!(
            WaterMap::parse(["NaN 0 2 0 0 1 0 10 0 2 0 0 1 0 0 10 2 0 0 1 0 1".into()]).is_err()
        );
    }
}
