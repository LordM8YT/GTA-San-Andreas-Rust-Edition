//! Drivable vehicles from content packs: one glTF file per vehicle, made in
//! Blender like the pack world (tools/blender/make_rp_vehicles.py builds the
//! SARE RP Town set). See "Vehicles" in docs/content-packs.md.
//!
//! The model's nose points along glTF -Z (Blender +Y) and its origin is the
//! point the car simulation moves; the lowest vertex sets the ground
//! clearance. Mesh nodes named `wheel_lf`, `wheel_rf`, `wheel_lb` and
//! `wheel_rb` (left is -X; `lr`/`rr` also work for the back) are wheels:
//! their node origin is the hub, front wheels steer and all of them roll.
//! Primitives using a material named `paint` take the vehicle's colour.
use crate::vehicle::{Handling, RigPiece, VehiclePart};
use crate::{gltf_model, Batch, Scene, Vertex};
use anyhow::{ensure, Context, Result};
use glam::{Mat4, Vec3};
use sa_assets::Texture;
use std::collections::HashMap;
use std::path::Path;

/// Marks the batch holding painted panels; the default colour follows as
/// six hex digits, e.g. `pack:sedan:paint#3c5a96`.
pub const PAINT_TAG: &str = ":paint#";

/// The default paint of a painted batch, from its key.
pub fn paint_default(key: &str) -> Option<[u8; 3]> {
    let hex = &key[key.find(PAINT_TAG)? + PAINT_TAG.len()..];
    let value = u32::from_str_radix(hex.get(..6)?, 16).ok()?;
    Some([(value >> 16) as u8, (value >> 8) as u8, value as u8])
}

/// Recolour vertices of a painted batch (`raw` is position, uv, rgba per
/// vertex, as uploaded) from their default paint to `paint`.
pub fn repaint(raw: &mut [f32], default: [u8; 3], paint: [u8; 3]) {
    let ratio: [f32; 3] = std::array::from_fn(|i| paint[i] as f32 / (default[i].max(1)) as f32);
    for vertex in raw.as_chunks_mut::<9>().0.iter_mut() {
        for i in 0..3 {
            vertex[5 + i] *= ratio[i];
        }
    }
}

/// A wheel node: its lowercase name, front, left.
type Wheel = (String, bool, bool);

fn wheel(name: &str) -> Option<(bool, bool)> {
    let lower = name.to_ascii_lowercase();
    let tag = lower.strip_prefix("wheel_")?.get(..2)?;
    match tag {
        "lf" => Some((true, true)),
        "rf" => Some((true, false)),
        "lb" | "lr" => Some((false, true)),
        "rb" | "rr" => Some((false, false)),
        _ => None,
    }
}

fn srgb_to_linear(c: u8) -> f32 {
    let c = c as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}
/// Linear colour channel to sRGB, as materials store linear values.
pub fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Load one pack vehicle as a car scene: body batches by material kind, one
/// batch per wheel with its rig piece, the given default paint and handling.
pub fn load(path: &Path, key: &str, paint: [u8; 3], handling: Handling) -> Result<Scene> {
    let bytes = gltf_model::read_file(path)?;
    let folder = path.parent().unwrap_or(Path::new("."));
    parse(&bytes, folder, key, paint, handling)
        .with_context(|| format!("loading vehicle {}", path.display()))
}

pub fn parse(
    bytes: &[u8],
    folder: &Path,
    key: &str,
    paint: [u8; 3],
    handling: Handling,
) -> Result<Scene> {
    handling.validate()?;
    let (gltf, buffers) = gltf_model::open_document(bytes, folder)?;
    let scene = gltf
        .document
        .default_scene()
        .or_else(|| gltf.document.scenes().next())
        .context("the file has no scene")?;
    let paint_key = format!(
        "{key}{PAINT_TAG}{:02x}{:02x}{:02x}",
        paint[0], paint[1], paint[2]
    );
    let body_key = format!("{key}:body");
    let mut batches: Vec<Batch> = Vec::new();
    let mut slots: HashMap<String, usize> = HashMap::new();
    let mut pieces = HashMap::new();
    let light = Vec3::new(0.35, 1.0, 0.25).normalize();
    let mut stack: Vec<(gltf::Node, Mat4, Option<Wheel>)> =
        scene.nodes().map(|n| (n, Mat4::IDENTITY, None)).collect();
    let mut triangles = 0;
    while let Some((node, parent, parent_wheel)) = stack.pop() {
        ensure!(stack.len() < 10_000, "the node tree is too large");
        let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        let name = node.name().unwrap_or("");
        let this_wheel = wheel(name).map(|(front, left)| (name.to_ascii_lowercase(), front, left));
        let wheel_of = this_wheel.clone().or(parent_wheel);
        if let Some((wheel_name, front, left)) = &this_wheel {
            let wheel_key = format!("{key}:{wheel_name}");
            pieces.insert(
                wheel_key,
                RigPiece {
                    part: VehiclePart::Wheel {
                        front: *front,
                        left: *left,
                    },
                    damaged: false,
                    pivot: transform.transform_point3(Vec3::ZERO),
                    radius: 0.0,
                },
            );
        }
        for child in node.children() {
            stack.push((child, transform, wheel_of.clone()));
        }
        let Some(mesh) = node.mesh() else { continue };
        let normal_matrix = transform.inverse().transpose();
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            let reader = primitive.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
            let Some(positions) = reader.read_positions() else {
                continue;
            };
            let positions: Vec<Vec3> = positions
                .map(|p| transform.transform_point3(Vec3::from_array(p)))
                .collect();
            let indices: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            ensure!(
                indices.iter().all(|&i| (i as usize) < positions.len()),
                "a mesh index is out of range"
            );
            triangles += indices.len() / 3;
            ensure!(
                triangles <= 200_000,
                "the vehicle has over 200000 triangles"
            );
            let normals: Option<Vec<Vec3>> = reader.read_normals().map(|n| {
                n.map(|n| {
                    normal_matrix
                        .transform_vector3(Vec3::from_array(n))
                        .normalize_or_zero()
                })
                .collect()
            });
            let material = primitive.material();
            let painted = material.name() == Some("paint");
            let alpha = !matches!(material.alpha_mode(), gltf::material::AlphaMode::Opaque);
            let mut factor = material.pbr_metallic_roughness().base_color_factor();
            if painted {
                for i in 0..3 {
                    factor[i] *= srgb_to_linear(paint[i]);
                }
            }
            let batch_key = match (&wheel_of, painted, alpha) {
                (Some((wheel_name, ..)), _, _) => format!("{key}:{wheel_name}"),
                (None, true, _) => paint_key.clone(),
                (None, false, true) => format!("{key}:glass"),
                (None, false, false) => body_key.clone(),
            };
            let slot = *slots.entry(batch_key.clone()).or_insert_with(|| {
                batches.push(Batch {
                    key: batch_key,
                    vertices: Vec::new(),
                    alpha,
                    animated: false,
                    uv_animation: None,
                    map_door: None,
                    mlo_entity: None,
                });
                batches.len() - 1
            });
            let out = &mut batches[slot].vertices;
            for tri in indices.as_chunks::<3>().0 {
                let [a, b, c] = tri.map(|i| i as usize);
                let face = (positions[b] - positions[a])
                    .cross(positions[c] - positions[a])
                    .normalize_or_zero();
                for i in [a, b, c] {
                    let n = normals
                        .as_ref()
                        .and_then(|n| n.get(i).copied())
                        .filter(|n| *n != Vec3::ZERO)
                        .unwrap_or(face);
                    let shade = 0.6 + 0.4 * n.dot(light).max(0.0);
                    out.push(Vertex {
                        position: positions[i].to_array(),
                        uv: [0.0; 2],
                        // Linear material colours to the sRGB the renderer
                        // draws, with the light baked in.
                        color: [
                            linear_to_srgb(factor[0]) * shade,
                            linear_to_srgb(factor[1]) * shade,
                            linear_to_srgb(factor[2]) * shade,
                            factor[3],
                        ],
                    });
                }
            }
        }
    }
    batches.retain(|b| !b.vertices.is_empty());
    ensure!(!batches.is_empty(), "the vehicle has no meshes");
    // Tyre radius: how far the wheel mesh reaches below its hub.
    for batch in &batches {
        if let Some(piece) = pieces.get_mut(&batch.key) {
            let low = batch
                .vertices
                .iter()
                .map(|v| v.position[1])
                .fold(f32::INFINITY, f32::min);
            piece.radius = (piece.pivot.y - low).max(0.1);
        }
    }
    pieces.retain(|k, _| batches.iter().any(|b| &b.key == k));
    let textures = batches
        .iter()
        .map(|b| {
            (
                b.key.clone(),
                Texture {
                    width: 1,
                    height: 1,
                    rgba: vec![255; 4],
                    has_alpha: false,
                },
            )
        })
        .collect();
    Ok(Scene {
        vehicle_handling: handling,
        water: None,
        triangles,
        batches,
        textures,
        placements: 1,
        animation: None,
        collision: None,
        vehicle_pieces: pieces,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content_pack::{load_vehicles, Manifest};
    #[test]
    fn town_vehicles_load_with_four_wheels_paint_and_glass() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../content/sare-rp-town");
        let manifest = Manifest::read(&folder).unwrap();
        let vehicles = load_vehicles(&folder, &manifest);
        let models: Vec<&str> = vehicles.iter().map(|(s, _)| s.model.as_str()).collect();
        assert_eq!(
            models,
            [
                "sedan",
                "compact",
                "suv",
                "pickup",
                "sports",
                "police",
                "taxi",
                "ambulance",
                "towtruck"
            ]
        );
        for (spec, scene) in &vehicles {
            let wheels: Vec<_> = scene.vehicle_pieces.values().collect();
            assert_eq!(wheels.len(), 4, "{}", spec.model);
            for piece in wheels {
                let VehiclePart::Wheel { front, left } = piece.part else {
                    panic!("not a wheel")
                };
                // Left is -X, the nose is -Z.
                assert_eq!(left, piece.pivot.x < 0.0, "{}", spec.model);
                assert_eq!(front, piece.pivot.z < 0.0, "{}", spec.model);
                assert!(
                    (0.25..0.5).contains(&piece.radius),
                    "{} {}",
                    spec.model,
                    piece.radius
                );
            }
            let paint = scene
                .batches
                .iter()
                .find_map(|b| paint_default(&b.key))
                .expect("painted batch");
            assert_eq!(Some(paint), spec.color, "{}", spec.model);
            assert!(
                scene.batches.iter().any(|b| b.alpha),
                "{} glass",
                spec.model
            );
            assert_eq!(scene.vehicle_handling, spec.handling.unwrap());
            for batch in &scene.batches {
                assert!(scene.textures.contains_key(&batch.key));
            }
            // Wheels touch the ground at the lowest point of the model.
            let low = scene
                .batches
                .iter()
                .flat_map(|b| &b.vertices)
                .map(|v| v.position[1])
                .fold(f32::MAX, f32::min);
            let wheel_low = scene
                .vehicle_pieces
                .values()
                .map(|p| p.pivot.y - p.radius)
                .fold(f32::MAX, f32::min);
            assert!((low - wheel_low).abs() < 0.02, "{}", spec.model);
        }
    }
    #[test]
    fn repaint_swaps_the_default_colour() {
        assert_eq!(
            paint_default("pack:x:sedan:paint#3c5a96"),
            Some([60, 90, 150])
        );
        assert_eq!(paint_default("pack:x:sedan:body"), None);
        let mut raw = vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.25, 1.0, 1.0];
        repaint(&mut raw, [200, 100, 255], [100, 100, 0]);
        assert!((raw[5] - 0.25).abs() < 1e-6 && (raw[6] - 0.25).abs() < 1e-6 && raw[7] == 0.0);
    }
}
