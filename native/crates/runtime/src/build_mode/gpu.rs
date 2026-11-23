//! Build Mode objects on the GPU. Each asset is uploaded once (textures and
//! model-space vertices); each placed object gets its own vertex buffers,
//! rewritten in place when the object moves, the way cars and peds are
//! posed. The renderer itself is unchanged: objects are ordinary batches.
use crate::{GpuBatch, State};
use glam::{Mat4, Vec3};
use sa_scene::content_pack::AssetResolver;
use sa_scene::world_layout::{AssetRef, Layout, NodeId, Transform};
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Floats per vertex: position 3, uv 2, colour 4.
const STRIDE: usize = 9;

/// An uploaded asset, in its own model space.
pub struct AssetMesh {
    parts: Vec<GpuBatch>,
    /// Model-space triangles of the drawn meshes, for picking.
    triangles: Arc<Vec<[Vec3; 3]>>,
    pub bounds: (Vec3, Vec3),
    /// Shown instead of a model that is missing or failed to load.
    pub placeholder: bool,
}

/// One placed object on the GPU.
pub struct ObjectMesh {
    pub asset: AssetRef,
    pub transform: Transform,
    pub batches: Vec<GpuBatch>,
    /// Render-space triangles and bounds, for picking.
    pub triangles: Vec<[Vec3; 3]>,
    pub bounds: (Vec3, Vec3),
    /// Model-space bounds and the render matrix, for the selection box.
    pub local_bounds: (Vec3, Vec3),
    pub matrix: Mat4,
    pub placeholder: bool,
}

#[derive(Default)]
pub struct Gpu {
    assets: HashMap<AssetRef, AssetMesh>,
    pub objects: HashMap<NodeId, ObjectMesh>,
    /// The placement preview, drawn like an object.
    pub ghost: Option<ObjectMesh>,
    /// Objects' lights: their glow, drawn in the night glow pass.
    pub lights: HashMap<NodeId, LightMesh>,
    /// The heights each terrain object's mesh was built from.
    terrains: HashMap<NodeId, sa_scene::world_layout::Terrain>,
}

/// The glow of one object's light, where the object stands.
pub struct LightMesh {
    light: sa_scene::world_layout::PointLight,
    transform: Transform,
    pub object: ObjectMesh,
}

/// Render space from pack space: the pack origin is the runtime origin
/// shifted by the pack anchor (zero for packs today).
pub fn render_matrix(transform: &Transform) -> Mat4 {
    let anchor = sa_scene::content_pack::ANCHOR;
    let origin = crate::ORIGIN;
    let offset = Vec3::new(anchor[0] - origin[0], 0.0, origin[1] - anchor[1]);
    Mat4::from_translation(offset) * transform.matrix()
}

impl ObjectMesh {
    /// A gameplay marker: drawn while editing, not while playing.
    pub fn editor_only(&self) -> bool {
        matches!(&self.asset, AssetRef::Builtin(name) if sa_scene::builtin_assets::editor_only(name))
    }
}

impl Gpu {
    /// Upload an asset once; failures become a visible placeholder.
    pub fn asset(
        &mut self,
        state: &State,
        resolver: &mut AssetResolver,
        asset: &AssetRef,
        problems: &mut Vec<String>,
    ) -> &AssetMesh {
        if !self.assets.contains_key(asset) {
            let mesh = resolver
                .model(asset)
                .and_then(|model| upload(state, &model, false))
                .or_else(|error| {
                    problems.push(format!("{asset}: {error:#}"));
                    placeholder(state)
                });
            match mesh {
                Ok(mesh) => {
                    self.assets.insert(asset.clone(), mesh);
                }
                Err(error) => {
                    // Even the placeholder failed: the GPU is in trouble.
                    crate::runtime_log::line(format!("Build Mode upload failed: {error:#}"));
                    return self
                        .assets
                        .entry(asset.clone())
                        .or_insert_with(|| AssetMesh {
                            parts: Vec::new(),
                            triangles: Arc::new(Vec::new()),
                            bounds: (Vec3::ZERO, Vec3::ZERO),
                            placeholder: true,
                        });
                }
            }
        }
        &self.assets[asset]
    }
    /// Forget uploaded assets and objects (models changed on disk).
    pub fn clear(&mut self) {
        self.assets.clear();
        self.objects.clear();
        self.ghost = None;
    }
    /// Make the GPU objects match the layout: add new ones, drop deleted
    /// ones and move the ones whose transform or asset changed.
    pub fn sync(
        &mut self,
        state: &State,
        resolver: &mut AssetResolver,
        layout: &Layout,
        problems: &mut Vec<String>,
    ) {
        self.objects.retain(|id, object| {
            layout
                .node(*id)
                .is_some_and(|n| n.asset.as_ref() == Some(&object.asset))
        });
        self.terrains
            .retain(|id, _| layout.node(*id).is_some_and(|n| n.terrain.is_some()));
        for node in &layout.nodes {
            let Some(asset) = &node.asset else { continue };
            if let Some(terrain) = &node.terrain {
                // Its own mesh, rebuilt when sculpted or moved.
                let fresh = self.terrains.get(&node.id) == Some(terrain)
                    && self
                        .objects
                        .get(&node.id)
                        .is_some_and(|o| o.transform == node.transform);
                if !fresh {
                    match upload(state, &sa_scene::terrain::model(terrain), false) {
                        Ok(mesh) => {
                            let object = instance(state, &mesh, asset.clone(), node.transform);
                            self.objects.insert(node.id, object);
                            self.terrains.insert(node.id, terrain.clone());
                        }
                        Err(error) => problems.push(format!("{asset}: {error:#}")),
                    }
                }
                continue;
            }
            match self.objects.get_mut(&node.id) {
                Some(object) if object.transform == node.transform => {}
                Some(object) => {
                    object.transform = node.transform;
                    let mesh = &self.assets[asset];
                    place(state, mesh, object);
                }
                None => {
                    let mesh = self.asset(state, resolver, asset, problems);
                    let object = instance(state, mesh, asset.clone(), node.transform);
                    self.objects.insert(node.id, object);
                }
            }
        }
    }
    /// Show (or move) the placement preview.
    pub fn set_ghost(
        &mut self,
        state: &State,
        resolver: &mut AssetResolver,
        asset: &AssetRef,
        transform: Transform,
        problems: &mut Vec<String>,
    ) {
        match &mut self.ghost {
            Some(ghost) if &ghost.asset == asset => {
                if ghost.transform != transform {
                    ghost.transform = transform;
                    let mesh = &self.assets[asset];
                    place(state, mesh, ghost);
                }
            }
            _ => {
                let mesh = self.asset(state, resolver, asset, problems);
                self.ghost = Some(instance(state, mesh, asset.clone(), transform));
            }
        }
    }
    pub fn is_placeholder(&self, asset: &AssetRef) -> bool {
        self.assets.get(asset).is_some_and(|a| a.placeholder)
    }
}

impl Gpu {
    /// Keep every object's light glow in step with the layout.
    pub fn sync_lights(&mut self, state: &State, layout: &Layout, problems: &mut Vec<String>) {
        self.lights.retain(|id, mesh| {
            layout
                .node(*id)
                .and_then(|n| n.light.as_ref())
                .is_some_and(|light| *light == mesh.light)
        });
        for node in &layout.nodes {
            let Some(light) = &node.light else { continue };
            if self
                .lights
                .get(&node.id)
                .is_some_and(|mesh| mesh.transform == node.transform)
            {
                continue;
            }
            let model = sa_scene::lights::point_light_model(light);
            match upload(state, &model, false) {
                Ok(mesh) => {
                    let object = instance(
                        state,
                        &mesh,
                        AssetRef::Builtin("light".into()),
                        node.transform,
                    );
                    self.lights.insert(
                        node.id,
                        LightMesh {
                            light: light.clone(),
                            transform: node.transform,
                            object,
                        },
                    );
                }
                Err(error) => problems.push(format!("light of {}: {error:#}", node.name)),
            }
        }
    }
}

fn upload(
    state: &State,
    model: &sa_scene::gltf_model::Model,
    placeholder: bool,
) -> anyhow::Result<AssetMesh> {
    let triangles: Vec<[Vec3; 3]> = model
        .drawn
        .iter()
        .flat_map(|b| b.vertices.as_chunks::<3>().0)
        .map(|t| [0, 1, 2].map(|i| Vec3::from_array(t[i].position)))
        .collect();
    let bounds = editor_bounds(&triangles);
    let scene = sa_scene::Scene {
        vehicle_handling: Default::default(),
        water: None,
        // "Animated" keeps the model-space vertices on the CPU side, so
        // each placed copy can be transformed from them.
        batches: model
            .drawn
            .iter()
            .map(|b| sa_scene::Batch {
                key: b.key.clone(),
                vertices: b.vertices.clone(),
                alpha: b.alpha,
                animated: true,
                uv_animation: None,
                map_door: None,
                mlo_entity: None,
            })
            .collect(),
        textures: model.textures.clone(),
        placements: 1,
        triangles: model.triangles,
        animation: None,
        collision: None,
        vehicle_pieces: HashMap::new(),
    };
    let parts = State::upload_scene(
        &state.device,
        &state.queue,
        &state.image_layout,
        &state.sampler,
        &state.mipmaps,
        scene,
    )?;
    Ok(AssetMesh {
        parts,
        triangles: Arc::new(triangles),
        bounds,
        placeholder,
    })
}

/// A magenta cube marking an object whose model could not be loaded.
fn placeholder(state: &State) -> anyhow::Result<AssetMesh> {
    let mut model = sa_scene::builtin_assets::model("cube").expect("built-in cube");
    for batch in &mut model.drawn {
        for v in &mut batch.vertices {
            let shade = v.color[0];
            v.color = [shade, 0.1 * shade, shade, 1.0];
        }
    }
    upload(state, &model, true)
}

fn editor_bounds(triangles: &[[Vec3; 3]]) -> (Vec3, Vec3) {
    sa_editor::viewport::bounds(triangles.iter().flatten().copied())
        .unwrap_or((Vec3::splat(-0.5), Vec3::splat(0.5)))
}

fn instance(state: &State, mesh: &AssetMesh, asset: AssetRef, transform: Transform) -> ObjectMesh {
    let matrix = render_matrix(&transform);
    let batches = mesh
        .parts
        .iter()
        .map(|part| {
            let raw = transformed(&part.base, matrix);
            let buffer = state
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("build mode object"),
                    contents: bytemuck::cast_slice(&raw),
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                });
            GpuBatch {
                bounds: None,
                texture_key: part.texture_key.clone(),
                buffer,
                count: part.count,
                texture: part.texture.clone(),
                alpha: part.alpha,
                animated: true,
                uv_animation: None,
                base: Vec::new(),
                piece: None,
                map_door: None,
            }
        })
        .collect();
    let mut object = ObjectMesh {
        asset,
        transform,
        batches,
        triangles: Vec::new(),
        bounds: (Vec3::ZERO, Vec3::ZERO),
        local_bounds: mesh.bounds,
        matrix,
        placeholder: mesh.placeholder,
    };
    pick_data(mesh, &mut object);
    object
}

/// Move an existing object: rewrite its vertex buffers in place.
fn place(state: &State, mesh: &AssetMesh, object: &mut ObjectMesh) {
    object.matrix = render_matrix(&object.transform);
    for (part, batch) in mesh.parts.iter().zip(&object.batches) {
        let raw = transformed(&part.base, object.matrix);
        state
            .queue
            .write_buffer(&batch.buffer, 0, bytemuck::cast_slice(&raw));
    }
    pick_data(mesh, object);
}

fn pick_data(mesh: &AssetMesh, object: &mut ObjectMesh) {
    let m = object.matrix;
    object.triangles = mesh
        .triangles
        .iter()
        .map(|t| t.map(|p| m.transform_point3(p)))
        .collect();
    object.bounds = editor_bounds(&object.triangles);
}

fn transformed(base: &[f32], matrix: Mat4) -> Vec<f32> {
    let mut raw = base.to_vec();
    for vertex in raw.as_chunks_mut::<STRIDE>().0 {
        let p = matrix.transform_point3(Vec3::new(vertex[0], vertex[1], vertex[2]));
        vertex[..3].copy_from_slice(&p.to_array());
    }
    if matrix.determinant() < 0.0 {
        // Mirrored: swap two corners so faces keep pointing outwards.
        for triangle in raw.as_chunks_mut::<{ STRIDE * 3 }>().0 {
            let (first, rest) = triangle.split_at_mut(STRIDE * 2);
            first[STRIDE..].swap_with_slice(&mut rest[..STRIDE]);
        }
    }
    raw
}
