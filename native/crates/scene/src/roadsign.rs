//! Original sign glyphs share one small atlas and never enter collision geometry.
use super::{Batch, Geometry, Vertex};
use glam::{Quat, Vec3};
use sa_assets::roadsign::{glyph, RoadSign};
use std::collections::HashMap;
pub(super) const KEY: &str = "runtime:roadsignfont";

fn vertices(sign: &RoadSign, origin: [f32; 2]) -> Vec<Vertex> {
    let [x, y, z] = sign.rotation.map(f32::to_radians);
    let rotation = Quat::from_rotation_y(y) * Quat::from_rotation_x(x) * Quat::from_rotation_z(z);
    let height = sign.size[1] / sign.lines() as f32;
    let width = sign.size[0] / sign.letters() as f32;
    let mut out = Vec::new();
    for row in 0..sign.lines() {
        let bottom = sign.size[1] * 0.5 - (row + 1) as f32 * height;
        for col in 0..sign.letters() {
            let Some(cell) = glyph(sign.text[row][col]) else {
                continue;
            };
            let left = -sign.size[0] * 0.5 + col as f32 * width;
            // Half-texel inset prevents adjacent atlas letters bleeding into one another.
            let u0 = ((cell % 4) as f32 * 8.0 + 0.5) / 32.0;
            let u1 = ((cell % 4 + 1) as f32 * 8.0 - 0.5) / 32.0;
            let v0 = ((cell / 4) as f32 * 16.0 + 0.5) / 512.0;
            let v1 = ((cell / 4 + 1) as f32 * 16.0 - 0.5) / 512.0;
            let corners = [
                (left, bottom, [u0, v1]),
                (left + width, bottom, [u1, v1]),
                (left + width, bottom + height * 0.95, [u1, v0]),
                (left, bottom + height * 0.95, [u0, v0]),
            ]
            .map(|(x, y, uv)| Vertex {
                // SA roadsign effects contain baked world coordinates. Unlike
                // the base mesh, the original game does not apply the IPL transform.
                position: {
                    let v = rotation * Vec3::new(x, y, 0.0) + Vec3::from_array(sign.position);
                    [v.x - origin[0], v.z, -(v.y - origin[1])]
                },
                uv,
                color: sign.color(),
            });
            for i in [0, 1, 2, 0, 2, 3] {
                out.push(corners[i]);
            }
        }
    }
    out
}
pub(super) fn add(
    batches: &mut HashMap<String, Batch>,
    geometries: &[Geometry],
    origin: [f32; 2],
) -> usize {
    let mut count = 0;
    for sign in geometries.iter().flat_map(|g| &g.road_signs) {
        let vertices = vertices(sign, origin);
        if vertices.is_empty() {
            continue;
        }
        count += vertices.len() / 3;
        batches
            .entry(KEY.into())
            .or_insert_with(|| Batch {
                key: KEY.into(),
                vertices: Vec::new(),
                alpha: false,
                animated: false,
                uv_animation: None,
            })
            .vertices
            .extend(vertices);
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sign_text_uses_baked_world_coordinates_rows_spacing_and_atlas() {
        let mut sign = RoadSign {
            position: [10.0, 20.0, 30.0],
            size: [4.0, 2.0],
            rotation: [0.0; 3],
            flags: 2 | (2 << 2) | (3 << 4),
            text: [[b'_'; 16]; 4],
        };
        sign.text[0][0] = b'A';
        sign.text[0][2] = b'B';
        sign.text[1][0] = b'^';
        let output = vertices(&sign, [0.0, 0.0]);
        assert_eq!(output.len(), 18);
        assert_eq!(output[0].position, [8.0, 30.0, -20.0]);
        assert_eq!(output[6].position, [10.0, 30.0, -20.0]);
        assert_eq!(output[12].position, [8.0, 30.0, -19.0]);
        assert_eq!(output[0].uv, [0.5 / 32.0, 111.5 / 512.0]);
        assert_eq!(output[0].color, [1.0, 0.0, 0.0, 1.0]);
        let shifted = vertices(&sign, [100.0, 200.0]);
        assert_eq!(shifted[0].position, [-92.0, 30.0, 180.0]);
        sign.rotation = [90.0, 0.0, 0.0];
        let output = vertices(&sign, [0.0, 0.0]);
        assert!((output[2].position[1] - 30.95).abs() < 0.0001);
        assert!((output[2].position[2] + 20.0).abs() < 0.0001);
    }
}
