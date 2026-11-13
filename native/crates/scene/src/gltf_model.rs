//! glTF 2.0 models (`.glb`, or `.gltf` with external `.bin`/`.png` files),
//! the open format Blender and most 3D tools export. This lets creators make
//! SARE content without any RenderWare tooling. See docs/content-packs.md.
//!
//! Coordinates: glTF is Y-up, metres, with -Z pointing "forward". SARE's
//! runtime space is also Y-up with -Z pointing north, so a model exported
//! from Blender with the default "+Y Up" setting keeps Blender's axes:
//! Blender X = east, Blender Y = north, Blender Z = up.
//!
//! Supported: triangle meshes with node transforms, vertex colours, the base
//! colour factor and base colour texture (PNG) of each material, and the
//! OPAQUE/MASK/BLEND alpha modes. Lighting is baked into vertex colours from
//! the normals, as the renderer expects pre-lit geometry. Not supported yet:
//! skins, animation, morph targets, JPEG/KTX2 textures and data: URIs.
use crate::{Batch, Vertex};
use anyhow::{bail, ensure, Context, Result};
use glam::{Mat4, Vec3};
use sa_assets::Texture;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Largest model file read, in bytes.
pub const MAX_FILE_BYTES: u64 = 256 * 1024 * 1024;
/// Largest number of triangles one model may add.
pub const MAX_TRIANGLES: usize = 4_000_000;

/// What a mesh node is used for, from its name in the 3D tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// Drawn and solid (the default).
    Solid,
    /// `COL_…`: invisible collision only.
    CollisionOnly,
    /// `NOCOL_…`: drawn, but players and cars pass through it.
    DrawOnly,
}
impl Role {
    pub fn from_name(name: &str) -> Self {
        let upper = name.to_ascii_uppercase();
        if upper.starts_with("NOCOL_") || upper.starts_with("NOCOL.") {
            Self::DrawOnly
        } else if upper.starts_with("COL_") || upper.starts_with("COL.") {
            Self::CollisionOnly
        } else {
            Self::Solid
        }
    }
}

/// A loaded model in glTF space (Y up, -Z north), already lit.
#[derive(Default)]
pub struct Model {
    /// Drawn geometry, one batch per texture and alpha mode.
    pub drawn: Vec<Batch>,
    /// Geometry that should collide: solid drawn meshes plus `COL_` meshes.
    pub collision: Vec<Batch>,
    pub textures: HashMap<String, Texture>,
    /// Drawn triangles.
    pub triangles: usize,
}
impl Model {
    /// Move every vertex by `offset` (glTF/runtime space).
    pub fn translated(&self, offset: Vec3) -> (Vec<Batch>, Vec<Batch>) {
        let shift = |batches: &[Batch]| {
            batches
                .iter()
                .map(|b| Batch {
                    key: b.key.clone(),
                    vertices: b
                        .vertices
                        .iter()
                        .map(|v| Vertex {
                            position: (Vec3::from_array(v.position) + offset).to_array(),
                            ..*v
                        })
                        .collect(),
                    alpha: b.alpha,
                    animated: false,
                    uv_animation: None,
                    map_door: None,
                    mlo_entity: None,
                })
                .collect::<Vec<_>>()
        };
        (shift(&self.drawn), shift(&self.collision))
    }
}

/// Is this path a glTF model (by extension)?
pub fn is_gltf_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".glb") || lower.ends_with(".gltf")
}

impl Model {
    /// The drawn meshes as map-model geometry for native mods: Z-up model
    /// space like a DFF (glTF -Z north becomes +Y north), one material per
    /// batch, split into pieces of at most 65535 vertices. Texture names are
    /// the part of each batch key after `key_prefix:`, so the textures are
    /// found as `<key_prefix>:<name>` in the mod texture table.
    pub fn to_geometry(&self, key_prefix: &str) -> Vec<sa_assets::Geometry> {
        let mut out = Vec::new();
        let white = format!("{key_prefix}:white");
        for batch in &self.drawn {
            let texture = (batch.key != white)
                .then(|| batch.key.strip_prefix(&format!("{key_prefix}:")))
                .flatten()
                .map(str::to_owned);
            // Alpha below 255 marks the material as see-through, which also
            // keeps it out of the visible-triangle collision fallback.
            let alpha = if batch.alpha { 254 } else { 255 };
            for chunk in batch.vertices.chunks(65535 / 3 * 3) {
                let to_u8 = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                out.push(sa_assets::Geometry {
                    positions: chunk
                        .iter()
                        .map(|v| [v.position[0], -v.position[2], v.position[1]])
                        .collect(),
                    normals: Vec::new(),
                    uvs: chunk.iter().map(|v| v.uv).collect(),
                    colors: chunk.iter().map(|v| v.color.map(to_u8)).collect(),
                    triangles: (0..chunk.len() as u16 / 3)
                        .map(|t| [t * 3, t * 3 + 1, t * 3 + 2, 0])
                        .collect(),
                    materials: vec![sa_assets::Material {
                        color: [255, 255, 255, alpha],
                        texture: texture.clone(),
                        uv_animation: None,
                    }],
                    road_signs: Vec::new(),
                    lights: Vec::new(),
                });
            }
        }
        out
    }
}

impl Model {
    /// The solid part (`COL_` meshes plus opaque, non-`NOCOL_` meshes) as a
    /// Z-up collision model like a COL file, or `None` if nothing is solid.
    pub fn to_collision(&self, name: &str) -> Option<sa_assets::col::CollisionModel> {
        let triangles: Vec<[[f32; 3]; 3]> = self
            .collision
            .iter()
            .flat_map(|b| b.vertices.chunks_exact(3))
            .map(|t| {
                std::array::from_fn(|i| {
                    let p = t[i].position;
                    [p[0], -p[2], p[1]]
                })
            })
            .collect();
        let points = || triangles.iter().flatten();
        let min = std::array::from_fn(|a| points().map(|p| p[a]).fold(f32::INFINITY, f32::min));
        let max = std::array::from_fn(|a| points().map(|p| p[a]).fold(f32::NEG_INFINITY, f32::max));
        (!triangles.is_empty()).then(|| sa_assets::col::CollisionModel {
            name: name.to_owned(),
            triangles,
            min,
            max,
        })
    }
}

/// Load a `.glb` or `.gltf` file. Texture keys start with `key_prefix`, so
/// two models never share a texture by accident.
pub fn load(path: &Path, key_prefix: &str) -> Result<Model> {
    let size = fs::metadata(path)
        .with_context(|| format!("reading {}", path.display()))?
        .len();
    ensure!(
        size <= MAX_FILE_BYTES,
        "{} is larger than 256 MiB",
        path.display()
    );
    let bytes = fs::read(path)?;
    let folder = path.parent().unwrap_or(Path::new("."));
    parse(&bytes, folder, key_prefix).with_context(|| format!("loading {}", path.display()))
}

/// Read a file next to the model, refusing paths that leave its folder.
fn sibling(folder: &Path, uri: &str) -> Result<Vec<u8>> {
    ensure!(
        !uri.starts_with("data:"),
        "embedded data: URIs are not supported; export as .glb instead"
    );
    let relative = Path::new(uri);
    ensure!(
        relative
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_))),
        "{uri} must be a plain path inside the model's folder"
    );
    let path = folder.join(relative);
    ensure!(
        fs::metadata(&path)
            .with_context(|| format!("missing {}", path.display()))?
            .len()
            <= MAX_FILE_BYTES,
        "{uri} is larger than 256 MiB"
    );
    Ok(fs::read(path)?)
}

pub fn parse(bytes: &[u8], folder: &Path, key_prefix: &str) -> Result<Model> {
    let gltf = gltf::Gltf::from_slice(bytes).context("not a valid glTF 2.0 file")?;
    let mut buffers = Vec::new();
    for buffer in gltf.document.buffers() {
        let data = match buffer.source() {
            gltf::buffer::Source::Bin => gltf
                .blob
                .clone()
                .context("the .glb binary chunk is missing")?,
            gltf::buffer::Source::Uri(uri) => sibling(folder, uri)?,
        };
        ensure!(
            data.len() >= buffer.length(),
            "buffer {} is shorter than declared",
            buffer.index()
        );
        buffers.push(data);
    }

    check_ranges(&gltf.document, &buffers)?;

    let mut images: HashMap<usize, Option<String>> = HashMap::new();
    let mut model = Model::default();
    let white = format!("{key_prefix}:white");
    model.textures.insert(
        white.clone(),
        Texture {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
            has_alpha: false,
        },
    );
    // (texture key, alpha) -> batch index, separately for drawn/collision.
    let mut drawn_slots: HashMap<(String, bool), usize> = HashMap::new();
    let mut collision_vertices: Vec<Vertex> = Vec::new();

    let scene = gltf
        .document
        .default_scene()
        .or_else(|| gltf.document.scenes().next())
        .context("the file has no scene")?;
    let mut stack: Vec<(gltf::Node, Mat4, Role)> = scene
        .nodes()
        .map(|n| (n, Mat4::IDENTITY, Role::Solid))
        .collect();
    let light = Vec3::new(0.35, 1.0, 0.25).normalize();
    let mut read_triangles = 0;
    while let Some((node, parent, parent_role)) = stack.pop() {
        ensure!(stack.len() < 100_000, "the node tree is too large");
        let transform = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        let mut role = Role::from_name(node.name().unwrap_or(""));
        if role == Role::Solid {
            role = parent_role;
        }
        for child in node.children() {
            stack.push((child, transform, role));
        }
        let Some(mesh) = node.mesh() else { continue };
        if role == Role::Solid {
            role = Role::from_name(mesh.name().unwrap_or(""));
        }
        let normal_matrix = transform.inverse().transpose();
        for primitive in mesh.primitives() {
            if primitive.mode() != gltf::mesh::Mode::Triangles {
                eprintln!(
                    "glTF: skipping a non-triangle primitive in {:?}",
                    mesh.name()
                );
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
            let triangles = indices.len() / 3;
            read_triangles += triangles;
            ensure!(
                read_triangles <= MAX_TRIANGLES,
                "the model has more than {MAX_TRIANGLES} triangles"
            );
            let alpha = match primitive.material().alpha_mode() {
                gltf::material::AlphaMode::Opaque => false,
                gltf::material::AlphaMode::Mask | gltf::material::AlphaMode::Blend => true,
            };
            // See-through materials (glass, leaves) are drawn but not solid,
            // like alpha batches in the original map; COL_ is always solid.
            if role == Role::CollisionOnly || (role == Role::Solid && !alpha) {
                collision_vertices.extend(indices.iter().map(|&i| Vertex {
                    position: positions[i as usize].to_array(),
                    uv: [0.0, 0.0],
                    color: [1.0; 4],
                }));
            }
            if role == Role::CollisionOnly {
                continue;
            }
            model.triangles += triangles;

            let material = primitive.material();
            let pbr = material.pbr_metallic_roughness();
            let factor = pbr.base_color_factor();
            let mut uv_set = 0;
            let mut key = white.clone();
            if let Some(info) = pbr.base_color_texture() {
                uv_set = info.tex_coord();
                let image = info.texture().source();
                let entry = images.entry(image.index()).or_insert_with(|| {
                    match decode_image(&image, &buffers, folder) {
                        Ok(texture) => {
                            let key = format!("{key_prefix}:image{}", image.index());
                            model.textures.insert(key.clone(), texture);
                            Some(key)
                        }
                        Err(error) => {
                            eprintln!("glTF: texture {} ignored: {error:#}", image.index());
                            None
                        }
                    }
                });
                if let Some(k) = entry {
                    key = k.clone();
                }
            }
            let normals: Option<Vec<Vec3>> = reader.read_normals().map(|n| {
                n.map(|n| {
                    normal_matrix
                        .transform_vector3(Vec3::from_array(n))
                        .normalize_or_zero()
                })
                .collect()
            });
            let uvs: Option<Vec<[f32; 2]>> = reader
                .read_tex_coords(uv_set)
                .map(|t| t.into_f32().collect());
            let colors: Option<Vec<[f32; 4]>> =
                reader.read_colors(0).map(|c| c.into_rgba_f32().collect());

            let slot = *drawn_slots.entry((key.clone(), alpha)).or_insert_with(|| {
                model.drawn.push(Batch {
                    key: key.clone(),
                    vertices: Vec::new(),
                    alpha,
                    animated: false,
                    uv_animation: None,
                    map_door: None,
                    mlo_entity: None,
                });
                model.drawn.len() - 1
            });
            let out = &mut model.drawn[slot].vertices;
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
                    // Soft sky light plus one sun-like direction.
                    let shade = 0.6 + 0.4 * n.dot(light).max(0.0);
                    let vc = colors
                        .as_ref()
                        .and_then(|c| c.get(i).copied())
                        .unwrap_or([1.0; 4]);
                    out.push(Vertex {
                        position: positions[i].to_array(),
                        uv: uvs
                            .as_ref()
                            .and_then(|u| u.get(i).copied())
                            .unwrap_or([0.0; 2]),
                        color: [
                            factor[0] * vc[0] * shade,
                            factor[1] * vc[1] * shade,
                            factor[2] * vc[2] * shade,
                            factor[3] * vc[3],
                        ],
                    });
                }
            }
        }
    }
    model.drawn.retain(|b| !b.vertices.is_empty());
    if !collision_vertices.is_empty() {
        model.collision.push(Batch {
            key: white,
            vertices: collision_vertices,
            alpha: false,
            animated: false,
            uv_animation: None,
            map_door: None,
            mlo_entity: None,
        });
    }
    Ok(model)
}

/// Refuse buffer views and accessors that point outside their data, so a
/// broken or hostile file is an error instead of a crash while reading.
fn check_ranges(document: &gltf::Document, buffers: &[Vec<u8>]) -> Result<()> {
    for view in document.views() {
        let end = view.offset().checked_add(view.length());
        ensure!(
            end.is_some_and(|end| end <= buffers[view.buffer().index()].len()),
            "buffer view {} is out of range",
            view.index()
        );
    }
    for accessor in document.accessors() {
        ensure!(
            accessor.sparse().is_none(),
            "sparse accessors are not supported"
        );
        let Some(view) = accessor.view() else {
            continue;
        };
        let element = accessor.size();
        let stride = view.stride().unwrap_or(element);
        ensure!(
            stride >= element,
            "accessor {} has a too small stride",
            accessor.index()
        );
        let needed = match accessor.count() {
            0 => Some(0),
            count => (count - 1)
                .checked_mul(stride)
                .and_then(|n| n.checked_add(element))
                .and_then(|n| n.checked_add(accessor.offset())),
        };
        ensure!(
            needed.is_some_and(|n| n <= view.length()),
            "accessor {} reads past its buffer view",
            accessor.index()
        );
    }
    Ok(())
}

fn decode_image(image: &gltf::Image, buffers: &[Vec<u8>], folder: &Path) -> Result<Texture> {
    let (data, mime): (Vec<u8>, Option<&str>) = match image.source() {
        gltf::image::Source::View { view, mime_type } => {
            let buffer = buffers
                .get(view.buffer().index())
                .context("image buffer missing")?;
            let end = view
                .offset()
                .checked_add(view.length())
                .filter(|&end| end <= buffer.len())
                .context("image data is out of range")?;
            (buffer[view.offset()..end].to_vec(), Some(mime_type))
        }
        gltf::image::Source::Uri { uri, mime_type } => (sibling(folder, uri)?, mime_type),
    };
    if !data.starts_with(b"\x89PNG") {
        bail!(
            "only PNG textures are supported (got {}); re-export textures as PNG",
            mime.unwrap_or("an unknown format")
        );
    }
    crate::mods::png_texture(&data)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Build a tiny .glb in memory: one quad, optionally named.
    pub(crate) fn quad_glb(node_name: &str, translation: [f32; 3]) -> Vec<u8> {
        quad_glb_edited(node_name, translation, |json| json)
    }

    /// The quad, with its JSON changed by `edit` before packing.
    fn quad_glb_edited(
        node_name: &str,
        translation: [f32; 3],
        edit: impl Fn(String) -> String,
    ) -> Vec<u8> {
        let positions: [[f32; 3]; 4] = [
            [-1.0, 0.0, -1.0],
            [1.0, 0.0, -1.0],
            [1.0, 0.0, 1.0],
            [-1.0, 0.0, 1.0],
        ];
        let indices: [u16; 6] = [0, 3, 2, 0, 2, 1];
        let mut bin: Vec<u8> = bytemuck::cast_slice(&positions).to_vec();
        bin.extend_from_slice(bytemuck::cast_slice(&indices));
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"scene":0,"scenes":[{{"nodes":[0]}}],
            "nodes":[{{"name":"{node_name}","mesh":0,"translation":[{},{},{}]}}],
            "meshes":[{{"primitives":[{{"attributes":{{"POSITION":0}},"indices":1,"material":0}}]}}],
            "materials":[{{"pbrMetallicRoughness":{{"baseColorFactor":[1,0,0,1]}}}}],
            "buffers":[{{"byteLength":{}}}],
            "bufferViews":[{{"buffer":0,"byteOffset":0,"byteLength":48}},{{"buffer":0,"byteOffset":48,"byteLength":12}}],
            "accessors":[
              {{"bufferView":0,"componentType":5126,"count":4,"type":"VEC3","min":[-1,0,-1],"max":[1,0,1]}},
              {{"bufferView":1,"componentType":5123,"count":6,"type":"SCALAR"}}]}}"#,
            translation[0],
            translation[1],
            translation[2],
            bin.len()
        );
        glb(edit(json).as_bytes(), &bin)
    }

    pub(crate) fn glb(json: &[u8], bin: &[u8]) -> Vec<u8> {
        let pad = |mut v: Vec<u8>, with: u8| {
            while !v.len().is_multiple_of(4) {
                v.push(with);
            }
            v
        };
        let json = pad(json.to_vec(), b' ');
        let bin = pad(bin.to_vec(), 0);
        let total = 12 + 8 + json.len() + 8 + bin.len();
        let mut out = Vec::new();
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&json);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }

    #[test]
    fn a_quad_loads_lit_coloured_and_solid() {
        let model = parse(&quad_glb("Floor", [0.0, 2.0, 0.0]), Path::new("."), "t").unwrap();
        assert_eq!(model.triangles, 2);
        assert_eq!(model.drawn.len(), 1);
        let v = &model.drawn[0].vertices;
        assert_eq!(v.len(), 6);
        assert!(v.iter().all(|v| v.position[1] == 2.0));
        // Red base colour, facing up: fully lit by the sky term and the sun.
        assert!(v[0].color[0] > 0.9 && v[0].color[1] == 0.0);
        assert_eq!(model.collision[0].vertices.len(), 6);
        let (drawn, _) = model.translated(Vec3::new(10.0, 0.0, 0.0));
        assert!(drawn[0].vertices.iter().all(|v| v.position[0].abs() >= 9.0));
    }

    #[test]
    fn mod_geometry_is_z_up_with_textures_by_name() {
        assert!(is_gltf_path("models/Chair.GLB") && !is_gltf_path("chair.dff"));
        let model = parse(
            &quad_glb("Sign", [1.0, 2.0, -3.0]),
            Path::new("."),
            "mod_x_1",
        )
        .unwrap();
        let geometry = model.to_geometry("mod_x_1");
        assert_eq!(geometry.len(), 1);
        let g = &geometry[0];
        // glTF (1, 2, -3) is 1 m east, 3 m north and 2 m up.
        assert!(g.positions.iter().all(|p| p[2] == 2.0));
        assert!(g.positions.iter().any(|p| (p[1] - 4.0).abs() < 1e-5));
        assert_eq!(g.triangles, vec![[0, 1, 2, 0], [3, 4, 5, 0]]);
        assert!(g.materials[0].texture.is_none() && g.materials[0].color[3] == 255);
        assert!(g.colors[0][0] > 240 && g.colors[0][1] == 0);
        let col = model.to_collision("x").unwrap();
        assert_eq!(col.triangles.len(), 2);
        assert_eq!((col.min[2], col.max[2]), (2.0, 2.0));
        assert_eq!((col.min[1], col.max[1]), (2.0, 4.0));
        let deco = parse(&quad_glb("NOCOL_x", [0.0; 3]), Path::new("."), "m").unwrap();
        assert!(deco.to_collision("m").is_none());
    }

    #[test]
    fn name_prefixes_choose_collision_roles() {
        let col = parse(&quad_glb("COL_wall", [0.0; 3]), Path::new("."), "t").unwrap();
        assert!(col.drawn.is_empty() && col.collision.len() == 1);
        let deco = parse(&quad_glb("nocol_flowers", [0.0; 3]), Path::new("."), "t").unwrap();
        assert!(deco.drawn.len() == 1 && deco.collision.is_empty());
        assert_eq!(Role::from_name("Collider"), Role::Solid);
    }

    #[test]
    fn out_of_range_accessors_are_errors_not_crashes() {
        // Claim 400 positions in a 48-byte view.
        let bad = quad_glb_edited("Floor", [0.0; 3], |json| {
            json.replace(r#""count":4,"type":"VEC3""#, r#""count":400,"type":"VEC3""#)
        });
        let error = parse(&bad, Path::new("."), "t").err().unwrap();
        assert!(format!("{error:#}").contains("reads past"), "{error:#}");
        // A material that blends is drawn but not solid.
        let glass = quad_glb_edited("Window", [0.0; 3], |json| {
            json.replace(r#""materials":[{"#, r#""materials":[{"alphaMode":"BLEND","#)
        });
        let glass = parse(&glass, Path::new("."), "t").unwrap();
        assert!(glass.drawn[0].alpha && glass.collision.is_empty());
    }

    #[test]
    fn garbage_and_escaping_uris_are_refused() {
        assert!(parse(b"not a model", Path::new("."), "t").is_err());
        assert!(sibling(Path::new("."), "../secret.bin").is_err());
        assert!(sibling(Path::new("."), "data:application/octet-stream;base64,AA").is_err());
    }
}
