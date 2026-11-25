//! Studio parts: basic shapes (block, ball, cylinder, wedge, corner wedge)
//! built in code from a [`Part`]'s size, colour, material and transparency.
//!
//! Like the built-in assets, a part's origin is the middle of its base, so
//! placing one on the ground needs no offset. Materials are small patterns
//! generated here (no image files), tiled about every 2 m in world metres
//! so a long wall shows more bricks rather than stretched ones. The part's
//! colour is painted over the pattern.
use crate::gltf_model::Model;
use crate::world_layout::{Part, PartMaterial, PartShape};
use crate::{Batch, Vertex};
use glam::Vec3;
use sa_assets::Texture;
use std::collections::HashMap;

/// Metres one copy of a material's pattern covers.
const TILE: f32 = 2.0;
/// Pixels along each side of a material's pattern.
const PIXELS: u32 = 128;
/// Steps around a ball or cylinder.
const ROUND: usize = 24;
/// Most a Glass part lets through stays visible.
const GLASS_ALPHA: f32 = 0.45;

/// Texture key of a material's pattern.
pub fn texture_key(material: &PartMaterial) -> String {
    format!(
        "builtin:material/{}",
        known(material).label().to_lowercase()
    )
}

/// The material drawn: an unknown one as Plastic.
fn known(material: &PartMaterial) -> &PartMaterial {
    match material {
        PartMaterial::Other(_) => &PartMaterial::Plastic,
        m => m,
    }
}

/// How opaque the part is drawn, 0-1.
pub fn opacity(part: &Part) -> f32 {
    let alpha = 1.0 - part.transparency.clamp(0.0, 1.0);
    if *known(&part.material) == PartMaterial::Glass {
        alpha.min(GLASS_ALPHA)
    } else {
        alpha
    }
}

/// The part as a model in glTF space (Y up), drawn and solid.
pub fn model(part: &Part) -> Model {
    let triangles = shape_triangles(&part.shape, part.size.map(|s| s.max(Part::MIN_SIZE)));
    let alpha = opacity(part);
    let key = texture_key(&part.material);
    let to_gltf = |p: Vec3| Vec3::new(p.x, p.z, -p.y);
    let mut vertices = Vec::with_capacity(triangles.len() * 3);
    for triangle in &triangles {
        let face = (triangle[1].0 - triangle[0].0)
            .cross(triangle[2].0 - triangle[0].0)
            .normalize_or_zero();
        for &(position, normal) in triangle {
            let shade = shade(normal);
            let color = part.color.map(|c| c * shade);
            vertices.push(Vertex {
                position: to_gltf(position).to_array(),
                uv: planar_uv(position, face),
                color: [color[0], color[1], color[2], alpha],
            });
        }
    }
    let batch = |vertices: Vec<Vertex>, alpha: bool| Batch {
        key: key.clone(),
        vertices,
        alpha,
        animated: false,
        uv_animation: None,
        map_door: None,
        mlo_entity: None,
    };
    let count = vertices.len() / 3;
    Model {
        collision: vec![batch(vertices.clone(), false)],
        drawn: if alpha > 0.0 {
            vec![batch(vertices, alpha < 1.0)]
        } else {
            Vec::new()
        },
        textures: HashMap::from([(key.clone(), texture(&part.material))]),
        triangles: if alpha > 0.0 { count } else { 0 },
    }
}

/// Light baked into the colour by which way a surface faces (the renderer
/// lights on top), like the built-in shapes: tops brightest, undersides
/// darkest, the sides in between.
fn shade(normal: Vec3) -> f32 {
    let up = normal.z;
    let side = 0.73 + 0.06 * normal.x - 0.03 * normal.y;
    if up >= 0.0 {
        side + (1.0 - side) * up
    } else {
        side + (side - 0.5) * up
    }
}

/// Texture coordinates from the position in metres, projected along the
/// face's main axis, so patterns keep their size on any part.
fn planar_uv(position: Vec3, face: Vec3) -> [f32; 2] {
    let a = face.abs();
    let (u, v) = if a.z >= a.x && a.z >= a.y {
        (position.x, position.y)
    } else if a.x >= a.y {
        (position.y, position.z)
    } else {
        (position.x, position.z)
    };
    // Upwards in the picture is upwards on walls.
    [u / TILE, -v / TILE]
}

/// Position and normal of one corner, in Blender axes (x east, y north,
/// z up) with the origin in the middle of the base.
type Corner = (Vec3, Vec3);

fn shape_triangles(shape: &PartShape, size: [f32; 3]) -> Vec<[Corner; 3]> {
    let [x, y, z] = size;
    let (hx, hy) = (x / 2.0, y / 2.0);
    let mut out = Vec::new();
    match shape {
        PartShape::Ball => ball(&mut out, Vec3::new(hx, hy, z / 2.0)),
        PartShape::Cylinder => cylinder(&mut out, hx, hy, z),
        PartShape::Wedge => {
            let p = |e: f32, n: f32, u: f32| Vec3::new(e, n, u);
            let slope = Vec3::new(0.0, -z, y).normalize();
            flat_quad(
                &mut out,
                [
                    p(-hx, -hy, 0.0),
                    p(hx, -hy, 0.0),
                    p(hx, hy, z),
                    p(-hx, hy, z),
                ],
                slope,
            );
            flat_quad(
                &mut out,
                [p(-hx, hy, 0.0), p(hx, hy, 0.0), p(hx, hy, z), p(-hx, hy, z)],
                Vec3::Y,
            );
            flat_quad(
                &mut out,
                [
                    p(-hx, -hy, 0.0),
                    p(hx, -hy, 0.0),
                    p(hx, hy, 0.0),
                    p(-hx, hy, 0.0),
                ],
                -Vec3::Z,
            );
            for (e, outward) in [(hx, Vec3::X), (-hx, -Vec3::X)] {
                flat_triangle(
                    &mut out,
                    [p(e, -hy, 0.0), p(e, hy, 0.0), p(e, hy, z)],
                    outward,
                );
            }
        }
        PartShape::CornerWedge => {
            let p = |e: f32, n: f32, u: f32| Vec3::new(e, n, u);
            let top = p(hx, hy, z);
            let (sw, se, ne, nw) = (
                p(-hx, -hy, 0.0),
                p(hx, -hy, 0.0),
                p(hx, hy, 0.0),
                p(-hx, hy, 0.0),
            );
            flat_quad(&mut out, [sw, se, ne, nw], -Vec3::Z);
            flat_triangle(&mut out, [se, ne, top], Vec3::X);
            flat_triangle(&mut out, [ne, nw, top], Vec3::Y);
            // The two slopes face south and west; the outward test only
            // needs a rough direction.
            flat_triangle(&mut out, [sw, se, top], Vec3::new(0.0, -1.0, 1.0));
            flat_triangle(&mut out, [nw, sw, top], Vec3::new(-1.0, 0.0, 1.0));
        }
        PartShape::Block | PartShape::Other(_) => {
            let p = |e: f32, n: f32, u: f32| Vec3::new(e, n, u);
            let (e0, e1, n0, n1) = (-hx, hx, -hy, hy);
            for (corners, outward) in [
                (
                    [p(e0, n0, z), p(e1, n0, z), p(e1, n1, z), p(e0, n1, z)],
                    Vec3::Z,
                ),
                (
                    [
                        p(e0, n0, 0.0),
                        p(e1, n0, 0.0),
                        p(e1, n1, 0.0),
                        p(e0, n1, 0.0),
                    ],
                    -Vec3::Z,
                ),
                (
                    [p(e1, n0, 0.0), p(e1, n1, 0.0), p(e1, n1, z), p(e1, n0, z)],
                    Vec3::X,
                ),
                (
                    [p(e0, n0, 0.0), p(e0, n1, 0.0), p(e0, n1, z), p(e0, n0, z)],
                    -Vec3::X,
                ),
                (
                    [p(e0, n1, 0.0), p(e1, n1, 0.0), p(e1, n1, z), p(e0, n1, z)],
                    Vec3::Y,
                ),
                (
                    [p(e0, n0, 0.0), p(e1, n0, 0.0), p(e1, n0, z), p(e0, n0, z)],
                    -Vec3::Y,
                ),
            ] {
                flat_quad(&mut out, corners, outward);
            }
        }
    }
    out
}

fn flat_quad(out: &mut Vec<[Corner; 3]>, corners: [Vec3; 4], outward: Vec3) {
    let [a, b, c, d] = corners;
    flat_triangle(out, [a, b, c], outward);
    flat_triangle(out, [a, c, d], outward);
}

/// One triangle facing `outward` (its corners are swapped if needed).
fn flat_triangle(out: &mut Vec<[Corner; 3]>, corners: [Vec3; 3], outward: Vec3) {
    let [a, mut b, mut c] = corners;
    if (b - a).cross(c - a).dot(outward) < 0.0 {
        std::mem::swap(&mut b, &mut c);
    }
    let normal = (b - a).cross(c - a).normalize_or_zero();
    out.push([(a, normal), (b, normal), (c, normal)]);
}

/// A triangle of a round surface: each corner keeps its own normal.
fn smooth_triangle(out: &mut Vec<[Corner; 3]>, corners: [Corner; 3]) {
    let [a, mut b, mut c] = corners;
    let average = a.1 + b.1 + c.1;
    if (b.0 - a.0).cross(c.0 - a.0).dot(average) < 0.0 {
        std::mem::swap(&mut b, &mut c);
    }
    out.push([a, b, c]);
}

/// A ball with radii `r` (east, north, up), resting on the origin.
fn ball(out: &mut Vec<[Corner; 3]>, r: Vec3) {
    let rings = ROUND / 2;
    let at = |ring: usize, step: usize| -> Corner {
        let lat = std::f32::consts::PI * ring as f32 / rings as f32;
        let lon = std::f32::consts::TAU * step as f32 / ROUND as f32;
        let unit = Vec3::new(lat.sin() * lon.cos(), lat.sin() * lon.sin(), -lat.cos());
        let position = unit * r + Vec3::new(0.0, 0.0, r.z);
        // The normal of a stretched ball.
        let normal = (unit / r.max(Vec3::splat(1e-3))).normalize_or_zero();
        (position, normal)
    };
    for ring in 0..rings {
        for step in 0..ROUND {
            let (a, b) = (at(ring, step), at(ring, step + 1));
            let (c, d) = (at(ring + 1, step + 1), at(ring + 1, step));
            if ring > 0 {
                smooth_triangle(out, [a, b, c]);
            }
            if ring + 1 < rings {
                smooth_triangle(out, [a, c, d]);
            }
        }
    }
}

/// An upright cylinder, `rx` by `ry` across and `height` tall.
fn cylinder(out: &mut Vec<[Corner; 3]>, rx: f32, ry: f32, height: f32) {
    let rim = |step: usize| {
        let angle = std::f32::consts::TAU * step as f32 / ROUND as f32;
        let (s, c) = angle.sin_cos();
        let normal = Vec3::new(c / rx.max(1e-3), s / ry.max(1e-3), 0.0).normalize_or_zero();
        (Vec3::new(c * rx, s * ry, 0.0), normal)
    };
    let up = Vec3::new(0.0, 0.0, height);
    for step in 0..ROUND {
        let (a, na) = rim(step);
        let (b, nb) = rim(step + 1);
        smooth_triangle(out, [(a, na), (b, nb), (b + up, nb)]);
        smooth_triangle(out, [(a, na), (b + up, nb), (a + up, na)]);
        flat_triangle(out, [up, a + up, b + up], Vec3::Z);
        flat_triangle(out, [Vec3::ZERO, a, b], -Vec3::Z);
    }
}

/// A material's pattern: grey levels near white, so the part's colour
/// shows true; Glass and unknown materials are plain.
pub fn texture(material: &PartMaterial) -> Texture {
    let size = PIXELS as usize;
    let mut rgba = Vec::with_capacity(size * size * 4);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (x as f32 / size as f32, y as f32 / size as f32);
            let value = match known(material) {
                PartMaterial::Smooth | PartMaterial::Glass | PartMaterial::Other(_) => 1.0,
                PartMaterial::Plastic => 0.96 + 0.04 * noise(x / 4, y / 4, 1),
                PartMaterial::Concrete => {
                    0.78 + 0.12 * noise(x / 3, y / 3, 2) + 0.08 * noise(x, y, 3)
                        - 0.18 * f32::from(noise(x, y, 4) > 0.97)
                }
                PartMaterial::Wood => {
                    // Planks across the picture with wavy grain along them.
                    let plank = (v * 4.0).floor();
                    let grain = ((v * 4.0 + 0.08 * (u * 9.0 + plank).sin()) * 22.0).sin();
                    let seam = f32::from((v * 4.0).fract() < 0.03);
                    0.8 + 0.08 * grain
                        + 0.07 * noise(plank as usize, 0, 5)
                        + 0.03 * noise(x, y / 2, 6)
                        - 0.3 * seam
                }
                PartMaterial::Brick => {
                    // 8 rows of bricks, every other row shifted half a brick.
                    let row = (v * 8.0).floor();
                    let shift = if row as i32 % 2 == 0 { 0.0 } else { 0.125 };
                    let column = ((u + shift) * 4.0).floor();
                    let mortar = (v * 8.0).fract() < 0.1 || ((u + shift) * 4.0).fract() < 0.05;
                    if mortar {
                        0.97
                    } else {
                        0.7 + 0.1 * noise(column as usize, row as usize, 7)
                            + 0.05 * noise(x / 2, y / 2, 8)
                    }
                }
                PartMaterial::Tiles => {
                    let grout = (u * 4.0).fract() < 0.04 || (v * 4.0).fract() < 0.04;
                    if grout {
                        0.7
                    } else {
                        0.95 + 0.03 * noise(x / 8, y / 8, 9)
                    }
                }
                PartMaterial::Metal => {
                    // Fine brushed streaks across.
                    0.86 + 0.07 * noise(0, y, 10) + 0.03 * noise(x / 16, y, 11)
                }
            };
            let level = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
            rgba.extend_from_slice(&[level, level, level, 255]);
        }
    }
    Texture {
        width: PIXELS,
        height: PIXELS,
        rgba,
        has_alpha: false,
    }
}

/// Repeatable noise 0-1 for a pixel (wrapping, so patterns tile).
fn noise(x: usize, y: usize, seed: u32) -> f32 {
    let (x, y) = (x % PIXELS as usize, y % PIXELS as usize);
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1)
        ^ (y as u32).wrapping_mul(0x85EB_CA77)
        ^ seed.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    (h & 0xFFFF) as f32 / 65535.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::CollisionWorld;

    fn bounds(model: &Model) -> (Vec3, Vec3) {
        let points = model
            .drawn
            .iter()
            .flat_map(|b| &b.vertices)
            .map(|v| Vec3::from_array(v.position));
        points.fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(lo, hi), p| (lo.min(p), hi.max(p)),
        )
    }

    #[test]
    fn every_shape_fills_its_size_and_is_closed() {
        for shape in PartShape::ALL {
            let part = Part::new(shape.clone(), [4.0, 2.0, 1.0]);
            let model = model(&part);
            assert!(model.triangles > 0, "{shape:?}");
            let (lo, hi) = bounds(&model);
            // glTF: x east, y up, -z north.
            assert!(
                (lo.x + 2.0).abs() < 1e-3 && (hi.x - 2.0).abs() < 1e-3,
                "{shape:?} {lo} {hi}"
            );
            assert!(
                lo.y.abs() < 1e-3 && (hi.y - 1.0).abs() < 1e-3,
                "{shape:?} {lo} {hi}"
            );
            assert!(
                (lo.z + 1.0).abs() < 1e-3 && (hi.z - 1.0).abs() < 1e-3,
                "{shape:?} {lo} {hi}"
            );
            // Closed: every edge is shared by two triangles, in opposite
            // directions (faces point outwards consistently).
            let key = |p: [f32; 3]| p.map(|v| (v * 1000.0).round() as i32);
            let mut edges: HashMap<([i32; 3], [i32; 3]), i32> = HashMap::new();
            for tri in model.drawn[0].vertices.chunks(3) {
                for i in 0..3 {
                    let (a, b) = (key(tri[i].position), key(tri[(i + 1) % 3].position));
                    if a == b {
                        continue;
                    }
                    *edges.entry((a, b)).or_default() += 1;
                    *edges.entry((b, a)).or_default() -= 1;
                }
            }
            assert!(
                edges.values().all(|&n| n == 0),
                "{shape:?} has an open or flipped edge"
            );
        }
    }

    #[test]
    fn a_block_is_solid_to_stand_on() {
        let model = model(&Part::new(PartShape::Block, [4.0, 4.0, 1.0]));
        let world = CollisionWorld::from_batches(&model.collision);
        let player = world.standing_at(Vec3::new(0.0, 3.0, 0.0), 3.0).unwrap();
        assert!((player.feet.y - 1.0).abs() < 1e-3, "{}", player.feet);
    }

    #[test]
    fn colour_material_and_transparency() {
        let mut part = Part::new(PartShape::Block, [1.0, 1.0, 1.0]);
        part.color = [1.0, 0.0, 0.0];
        part.material = PartMaterial::Brick;
        let solid = model(&part);
        assert_eq!(solid.drawn[0].key, "builtin:material/brick");
        assert!(!solid.drawn[0].alpha);
        // The top keeps its colour, the sides are darker, green stays out.
        let top = solid.drawn[0]
            .vertices
            .iter()
            .map(|v| v.color)
            .fold(0.0_f32, |m, c| m.max(c[0]));
        assert!((top - 1.0).abs() < 1e-3);
        assert!(solid.drawn[0].vertices.iter().all(|v| v.color[1] == 0.0));

        part.transparency = 0.5;
        let half = model(&part);
        assert!(half.drawn[0].alpha);
        assert!(half.drawn[0]
            .vertices
            .iter()
            .all(|v| (v.color[3] - 0.5).abs() < 1e-6));

        // Glass is always see-through; an invisible part is still solid.
        part.transparency = 0.0;
        part.material = PartMaterial::Glass;
        assert!(model(&part).drawn[0].alpha);
        part.transparency = 1.0;
        let invisible = model(&part);
        assert!(invisible.drawn.is_empty());
        assert!(!invisible.collision[0].vertices.is_empty());
    }

    #[test]
    fn patterns_tile_by_the_metre() {
        // A bigger wall repeats the pattern more often instead of stretching it.
        let small = model(&Part::new(PartShape::Block, [2.0, 0.2, 2.0]));
        let big = model(&Part::new(PartShape::Block, [8.0, 0.2, 2.0]));
        let span = |m: &Model| {
            let us = m.drawn[0].vertices.iter().map(|v| v.uv[0]);
            let (lo, hi) = us.fold((f32::MAX, f32::MIN), |(lo, hi), u| (lo.min(u), hi.max(u)));
            hi - lo
        };
        assert!((span(&big) / span(&small) - 4.0).abs() < 1e-3);
        for material in PartMaterial::ALL {
            let texture = texture(&material);
            assert_eq!(texture.rgba.len(), (PIXELS * PIXELS * 4) as usize);
            // Mostly light: the colour on top reads true.
            let mean =
                texture.rgba.chunks(4).map(|p| p[0] as f32).sum::<f32>() / (PIXELS * PIXELS) as f32;
            assert!(mean > 170.0, "{material:?} {mean}");
        }
    }

    #[test]
    fn unknown_shapes_and_materials_are_kept() {
        let json = r#"{"shape":"torus","size":[1,2,3],"material":"lava","glow":2}"#;
        let part: Part = serde_json::from_str(json).unwrap();
        assert_eq!(part.shape, PartShape::Other("torus".into()));
        assert_eq!(part.material, PartMaterial::Other("lava".into()));
        // Drawn as a plastic block.
        let drawn = model(&part);
        assert_eq!(drawn.drawn[0].key, "builtin:material/plastic");
        let back = serde_json::to_value(&part).unwrap();
        assert_eq!(back["shape"], "torus");
        assert_eq!(back["material"], "lava");
        assert_eq!(back["glow"], 2);
        let known: Part =
            serde_json::from_str(r#"{"shape":"corner_wedge","size":[1,1,1]}"#).unwrap();
        assert_eq!(known.shape, PartShape::CornerWedge);
        assert_eq!(serde_json::to_value(&known).unwrap()["material"], "plastic");
    }
}
