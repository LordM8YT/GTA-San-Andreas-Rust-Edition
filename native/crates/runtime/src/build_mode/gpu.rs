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
    /// Colour multiplier a script set (white: as modelled).
    pub tint: [f32; 3],
}

#[derive(Default)]
pub struct Gpu {
    assets: HashMap<AssetRef, AssetMesh>,
    pub objects: HashMap<NodeId, ObjectMesh>,
    /// The placement preview, drawn like an object.
    pub ghost: Option<ObjectMesh>,
    /// Objects' lights: their glow, drawn in the night glow pass.
    pub lights: HashMap<NodeId, LightMesh>,
    /// The terrain or road each generated object's mesh was built from.
    generated: HashMap<NodeId, Generated>,
    /// Where objects a script moved are drawn while playtesting, instead
    /// of their place in the layout.
    pub moved: HashMap<NodeId, Transform>,
    /// Model-space meshes of moving terrain and roads, re-placed as they
    /// move instead of rebuilt.
    moving_meshes: HashMap<NodeId, AssetMesh>,
    /// Colours scripts gave objects while playtesting (models only).
    pub tints: HashMap<NodeId, [f32; 3]>,
    /// Copies scripts made while playtesting: copy to the object it copies.
    /// Drawn like it, where `moved` says (or where it was placed).
    pub copies: HashMap<NodeId, NodeId>,
}

/// What a terrain or road object's own mesh is made from.
#[derive(Clone, PartialEq)]
enum Generated {
    Terrain(sa_scene::world_layout::Terrain),
    Road(sa_scene::world_layout::Road),
}
impl Generated {
    fn of(node: &sa_scene::world_layout::Node) -> Option<Self> {
        match (&node.terrain, &node.road) {
            (Some(terrain), _) => Some(Self::Terrain(terrain.clone())),
            (None, Some(road)) => Some(Self::Road(road.clone())),
            (None, None) => None,
        }
    }
    /// The node is still what this was made from (compared in place: no
    /// copy of a big height grid every frame).
    fn matches(&self, node: &sa_scene::world_layout::Node) -> bool {
        match (self, &node.terrain, &node.road) {
            (Self::Terrain(made), Some(now), _) => made == now,
            (Self::Road(made), None, Some(now)) => made == now,
            _ => false,
        }
    }
    fn model(&self) -> sa_scene::gltf_model::Model {
        match self {
            Self::Terrain(terrain) => sa_scene::terrain::model(terrain),
            Self::Road(road) => sa_scene::road::model(road),
        }
    }
}

/// The glow of one object's light, where the object stands.
pub struct LightMesh {
    light: sa_scene::world_layout::PointLight,
    transform: Transform,
    pub object: ObjectMesh,
}

/// What an object's mesh is uploaded as: its asset, or for a part a key
/// made from everything its mesh is built from (parts that look the same
/// share one upload).
pub fn mesh_key(node: &sa_scene::world_layout::Node) -> Option<AssetRef> {
    match &node.part {
        Some(part) => {
            let mut look = part.clone();
            look.extra.clear();
            Some(AssetRef::Builtin(format!(
                "part#{}",
                serde_json::to_string(&look).unwrap_or_default()
            )))
        }
        None => node.asset.clone(),
    }
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
        let copies = &self.copies;
        self.objects.retain(|id, object| {
            layout
                .node(copies.get(id).copied().unwrap_or(*id))
                .is_some_and(|n| mesh_key(n).as_ref() == Some(&object.asset))
        });
        self.generated.retain(|id, _| {
            layout
                .node(*id)
                .is_some_and(|n| n.terrain.is_some() || n.road.is_some())
        });
        let moved = &self.moved;
        self.moving_meshes.retain(|id, _| moved.contains_key(id));
        // Objects inside groups with a pivot are placed in the group.
        let world = layout.world_transforms();
        for node in &layout.nodes {
            let Some(asset) = &node.asset else { continue };
            let transform = self.moved.get(&node.id).copied().unwrap_or(world[&node.id]);
            if node.terrain.is_some() || node.road.is_some() {
                let same_shape = self
                    .generated
                    .get(&node.id)
                    .is_some_and(|g| g.matches(node));
                if let (true, Some(mesh), Some(object)) = (
                    same_shape,
                    self.moving_meshes.get(&node.id),
                    self.objects.get_mut(&node.id),
                ) {
                    if object.transform != transform {
                        object.transform = transform;
                        place(state, mesh, object);
                    }
                    continue;
                }
                // Its own mesh, rebuilt when sculpted, its points change or
                // it moves.
                let fresh = self
                    .generated
                    .get(&node.id)
                    .is_some_and(|g| g.matches(node))
                    && self
                        .objects
                        .get(&node.id)
                        .is_some_and(|o| o.transform == transform);
                if !fresh {
                    let Some(generated) = Generated::of(node) else {
                        continue;
                    };
                    match upload(state, &generated.model(), false) {
                        Ok(mesh) => {
                            let object = instance(state, &mesh, asset.clone(), transform);
                            self.objects.insert(node.id, object);
                            self.generated.insert(node.id, generated);
                            if self.moved.contains_key(&node.id) {
                                self.moving_meshes.insert(node.id, mesh);
                            }
                        }
                        Err(error) => problems.push(format!("{asset}: {error:#}")),
                    }
                }
                continue;
            }
            let asset = self
                .part_mesh(state, node, problems)
                .unwrap_or_else(|| asset.clone());
            self.sync_model(state, resolver, node.id, &asset, transform, problems);
        }
        let copies: Vec<(NodeId, NodeId)> = self.copies.iter().map(|(c, s)| (*c, *s)).collect();
        for (copy, source) in copies {
            let Some(node) = layout.node(source) else {
                continue;
            };
            // Only models and parts are copied (not terrain or roads).
            let (Some(asset), None, None) = (&node.asset, &node.terrain, &node.road) else {
                continue;
            };
            let asset = self
                .part_mesh(state, node, problems)
                .unwrap_or_else(|| asset.clone());
            let transform = self.moved.get(&copy).copied().unwrap_or(world[&source]);
            self.sync_model(state, resolver, copy, &asset, transform, problems);
        }
    }
    /// A part's mesh, uploaded once for every part of the same shape, size,
    /// colour and material; None for objects that are not parts.
    fn part_mesh(
        &mut self,
        state: &State,
        node: &sa_scene::world_layout::Node,
        problems: &mut Vec<String>,
    ) -> Option<AssetRef> {
        let part = node.part.as_ref()?;
        let key = mesh_key(node)?;
        if !self.assets.contains_key(&key) {
            match upload(state, &sa_scene::part::model(part), false) {
                Ok(mesh) => {
                    self.assets.insert(key.clone(), mesh);
                }
                Err(error) => {
                    problems.push(format!("{}: {error:#}", node.name));
                    return None;
                }
            }
        }
        Some(key)
    }
    /// Draw model object `id` with `transform`, in its colour.
    fn sync_model(
        &mut self,
        state: &State,
        resolver: &mut AssetResolver,
        id: NodeId,
        asset: &AssetRef,
        transform: Transform,
        problems: &mut Vec<String>,
    ) {
        let tint = self.tints.get(&id).copied().unwrap_or([1.0; 3]);
        match self.objects.get_mut(&id) {
            Some(object) if object.transform == transform && object.tint == tint => {}
            Some(object) => {
                object.transform = transform;
                object.tint = tint;
                let mesh = &self.assets[asset];
                place(state, mesh, object);
            }
            None => {
                let mesh = self.asset(state, resolver, asset, problems);
                let mut object = instance(state, mesh, asset.clone(), transform);
                if tint != [1.0; 3] {
                    object.tint = tint;
                    place(state, mesh, &mut object);
                }
                self.objects.insert(id, object);
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
        let world = layout.world_transforms();
        for node in &layout.nodes {
            let Some(light) = &node.light else { continue };
            let transform = world[&node.id];
            if self
                .lights
                .get(&node.id)
                .is_some_and(|mesh| mesh.transform == transform)
            {
                continue;
            }
            let model = sa_scene::lights::point_light_model(light);
            match upload(state, &model, false) {
                Ok(mesh) => {
                    let object =
                        instance(state, &mesh, AssetRef::Builtin("light".into()), transform);
                    self.lights.insert(
                        node.id,
                        LightMesh {
                            light: light.clone(),
                            transform,
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
            let raw = transformed(&part.base, matrix, [1.0; 3]);
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
        tint: [1.0; 3],
    };
    pick_data(mesh, &mut object);
    object
}

/// Move an existing object: rewrite its vertex buffers in place.
fn place(state: &State, mesh: &AssetMesh, object: &mut ObjectMesh) {
    object.matrix = render_matrix(&object.transform);
    for (part, batch) in mesh.parts.iter().zip(&object.batches) {
        let raw = transformed(&part.base, object.matrix, object.tint);
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

fn transformed(base: &[f32], matrix: Mat4, tint: [f32; 3]) -> Vec<f32> {
    let mut raw = base.to_vec();
    let tinted = tint != [1.0; 3];
    for vertex in raw.as_chunks_mut::<STRIDE>().0 {
        let p = matrix.transform_point3(Vec3::new(vertex[0], vertex[1], vertex[2]));
        vertex[..3].copy_from_slice(&p.to_array());
        if tinted {
            // Position 3, uv 2, then the colour.
            for (c, t) in vertex[5..8].iter_mut().zip(tint) {
                *c *= t;
            }
        }
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
