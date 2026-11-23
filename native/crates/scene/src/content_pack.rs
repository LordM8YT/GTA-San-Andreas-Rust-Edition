//! Content packs: a folder with `content.json` and a glTF world, so anyone can
//! make a playable SARE world in Blender without any San Andreas files.
//! The format is documented in docs/content-packs.md.
use crate::character::CharacterModel;
use crate::{
    collision::CollisionWorld, content::WorldSource, gltf_model, pack_vehicle, vehicle,
    world_layout, Scene,
};
use anyhow::{ensure, Context, Result};
use glam::Vec3;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The file that marks a folder as a content pack.
pub const MANIFEST: &str = "content.json";
/// Where `--content <id>` looks for packs, next to `mods/`.
pub const DEFAULT_DIR: &str = "content";
/// Where a pack's origin (Blender 0, 0, 0) sits in world coordinates: the
/// runtime's default origin, like SARE Testbed, at height 0.
pub const ANCHOR: [f32; 2] = crate::testbed::CENTER;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    /// Short machine name: lowercase letters, digits and `-`/`_`.
    pub id: String,
    pub name: String,
    pub version: String,
    /// Required: who may share the files, e.g. "CC0-1.0", "CC-BY-4.0", "MIT".
    pub license: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    pub world: WorldSpec,
    /// Optional customisable characters (see `character`), a `.glb` file.
    #[serde(default)]
    pub characters: Option<String>,
    /// Optional drivable vehicles (see `pack_vehicle`).
    #[serde(default)]
    pub vehicles: Vec<VehicleSpec>,
}

/// Most vehicles one pack may list.
pub const MAX_VEHICLES: usize = 64;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VehicleSpec {
    /// Model name scripts use, e.g. `CreateVehicle("sedan", ...)`: 1-32
    /// lowercase letters, digits, `-` or `_`.
    pub model: String,
    /// Name shown in menus.
    pub name: String,
    /// The vehicle's `.glb` file inside the pack.
    pub scene: String,
    /// Default paint as sRGB, for primitives using the `paint` material.
    #[serde(default)]
    pub color: Option<[u8; 3]>,
    /// Handling overrides; omitted fields keep the defaults.
    #[serde(default)]
    pub handling: Option<vehicle::Handling>,
    /// Driver seat in Blender coordinates relative to the vehicle origin
    /// ([right, forward, up], metres): where the seated driver's feet go.
    #[serde(default)]
    pub seat: Option<[f32; 3]>,
}
impl VehicleSpec {
    /// The seat in model space (glTF axes: -Z front, -X left).
    pub fn seat_model_space(&self) -> Vec3 {
        let [right, forward, up] = self.seat.unwrap_or([-0.38, 0.0, -0.45]);
        Vec3::new(right, up, -forward)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorldSpec {
    /// The drawn world, a `.glb` (or `.gltf`) file inside the pack. Optional
    /// when the pack has a Build Mode `layout`.
    #[serde(default)]
    pub scene: Option<String>,
    /// Optional separate collision model. When set, only this file collides
    /// and `scene` is drawn only.
    #[serde(default)]
    pub collision: Option<String>,
    /// Start point in Blender coordinates: [east, north, up] in metres.
    pub spawn: [f32; 3],
    /// Objects placed in Build Mode, a layout file inside the pack (see
    /// `world_layout`), drawn on top of `scene`.
    #[serde(default)]
    pub layout: Option<String>,
}

impl Manifest {
    pub fn read(folder: &Path) -> Result<Self> {
        let path = folder.join(MANIFEST);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let manifest: Self = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .with_context(|| format!("{} is not a valid content pack manifest", path.display()))?;
        manifest.validate()?;
        Ok(manifest)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.id.is_empty()
                && self.id.len() <= 64
                && self.id.bytes().all(|b| b.is_ascii_lowercase()
                    || b.is_ascii_digit()
                    || b == b'-'
                    || b == b'_'),
            "pack id {:?} must be 1-64 lowercase letters, digits, - or _",
            self.id
        );
        ensure!(!self.name.trim().is_empty(), "the pack needs a name");
        ensure!(
            !self.license.trim().is_empty(),
            "the pack needs a license, for example \"CC0-1.0\""
        );
        ensure!(
            self.world.scene.is_some() || self.world.layout.is_some(),
            "world needs a scene (.glb) or a Build Mode layout"
        );
        ensure!(
            self.world.spawn.iter().all(|v| v.is_finite()),
            "world.spawn must be three numbers"
        );
        ensure!(
            self.vehicles.len() <= MAX_VEHICLES,
            "a pack may list at most {MAX_VEHICLES} vehicles"
        );
        let mut seen = std::collections::HashSet::new();
        for spec in &self.vehicles {
            ensure!(
                !spec.model.is_empty()
                    && spec.model.len() <= 32
                    && spec.model.bytes().all(|b| b.is_ascii_lowercase()
                        || b.is_ascii_digit()
                        || b == b'-'
                        || b == b'_'),
                "vehicle model {:?} must be 1-32 lowercase letters, digits, - or _",
                spec.model
            );
            ensure!(
                seen.insert(spec.model.as_str()),
                "vehicle model {:?} is listed twice",
                spec.model
            );
            ensure!(
                !spec.name.trim().is_empty(),
                "vehicle {} needs a name",
                spec.model
            );
            ensure!(
                spec.seat
                    .is_none_or(|s| s.iter().all(|v| v.is_finite() && v.abs() < 10.0)),
                "vehicle {} has an odd seat",
                spec.model
            );
            if let Some(handling) = &spec.handling {
                handling
                    .validate()
                    .with_context(|| format!("vehicle {} handling", spec.model))?;
            }
        }
        Ok(())
    }
}

/// Find a pack: a folder with `content.json`, the manifest itself, or an id
/// inside `packs_dir`.
pub fn resolve(name: &str, packs_dir: &Path) -> Result<PathBuf> {
    let direct = PathBuf::from(name);
    if direct.file_name().is_some_and(|f| f == MANIFEST) && direct.is_file() {
        return Ok(direct.parent().unwrap_or(Path::new(".")).to_path_buf());
    }
    if direct.join(MANIFEST).is_file() {
        return Ok(direct);
    }
    let by_id = packs_dir.join(name);
    if by_id.join(MANIFEST).is_file() {
        return Ok(by_id);
    }
    // A pack whose folder name differs from its id.
    for (folder, manifest) in list(packs_dir) {
        if manifest.id == name {
            return Ok(folder);
        }
    }
    anyhow::bail!(
        "No content pack {name:?} found (looked for {MANIFEST} in {name} and {})",
        packs_dir.display()
    )
}

/// Every valid pack directly inside `packs_dir`, sorted by name. Invalid
/// packs are reported and skipped.
pub fn list(packs_dir: &Path) -> Vec<(PathBuf, Manifest)> {
    let Ok(entries) = std::fs::read_dir(packs_dir) else {
        return Vec::new();
    };
    let mut packs: Vec<_> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join(MANIFEST).is_file())
        .filter_map(|p| match Manifest::read(&p) {
            Ok(m) => Some((p, m)),
            Err(error) => {
                eprintln!("Skipping content pack: {error:#}");
                None
            }
        })
        .collect();
    packs.sort_by_key(|(_, manifest)| manifest.name.to_lowercase());
    packs
}

/// A pack's paths must stay inside the pack folder.
pub fn inside(folder: &Path, relative: &str) -> Result<PathBuf> {
    let path = Path::new(relative);
    ensure!(
        path.components()
            .all(|c| matches!(c, std::path::Component::Normal(_))),
        "{relative} must be a plain path inside the pack folder"
    );
    Ok(folder.join(path))
}

/// The pack's characters, if it has any.
pub fn load_characters(folder: &Path, manifest: &Manifest) -> Result<Option<CharacterModel>> {
    manifest
        .characters
        .as_ref()
        .map(|file| CharacterModel::load(&inside(folder, file)?))
        .transpose()
}

/// The pack's vehicles as car scenes, in manifest order. A broken vehicle is
/// reported and left out, so one bad file does not stop the world.
pub fn load_vehicles(folder: &Path, manifest: &Manifest) -> Vec<(VehicleSpec, Scene)> {
    let (loaded, failed) = load_vehicles_reporting(folder, manifest);
    for (model, error) in failed {
        eprintln!("Content pack vehicle {model} skipped: {error}");
    }
    loaded
}

/// A vehicle left out of a pack: its model and why.
pub type SkippedVehicle = (String, String);

/// Like [`load_vehicles`], also returning each skipped vehicle's model and
/// the reason, for the runtime log.
pub fn load_vehicles_reporting(
    folder: &Path,
    manifest: &Manifest,
) -> (Vec<(VehicleSpec, Scene)>, Vec<SkippedVehicle>) {
    let mut failed = Vec::new();
    let loaded = manifest
        .vehicles
        .iter()
        .filter_map(|spec| {
            let loaded = inside(folder, &spec.scene).and_then(|path| {
                pack_vehicle::load(
                    &path,
                    &format!("pack:{}:{}", manifest.id, spec.model),
                    spec.color.unwrap_or([200, 200, 200]),
                    spec.handling.unwrap_or_default(),
                )
            });
            match loaded {
                Ok(scene) => Some((spec.clone(), scene)),
                Err(error) => {
                    failed.push((spec.model.clone(), format!("{error:#}")));
                    None
                }
            }
        })
        .collect();
    (loaded, failed)
}

/// A loaded pack world, kept in glTF space and moved into place per load.
pub struct PackWorld {
    pub manifest: Manifest,
    model: gltf_model::Model,
}
impl PackWorld {
    /// The whole world: the pack's scene plus every Build Mode object.
    pub fn open(folder: &Path) -> Result<Self> {
        Self::open_with(folder, true)
    }
    /// Only the pack's own scene, without layout objects. Build Mode draws
    /// those itself, so it can move them without reloading the world.
    pub fn open_base(folder: &Path) -> Result<Self> {
        Self::open_with(folder, false)
    }
    fn open_with(folder: &Path, with_layout: bool) -> Result<Self> {
        let manifest = Manifest::read(folder)?;
        let mut model = base_model(folder, &manifest)?;
        if with_layout {
            if let Some(file) = &manifest.world.layout {
                let layout = world_layout::Layout::read(&inside(folder, file)?)?;
                let mut assets = AssetResolver::new(folder);
                let report = place_layout(&mut model, &layout, &mut assets);
                for problem in &report.problems {
                    eprintln!("Content pack {}: {problem}", manifest.id);
                }
            }
        }
        let scene_name = manifest.world.scene.as_deref().unwrap_or("the layout");
        // Build Mode may start from an empty world and add everything itself.
        ensure!(
            !with_layout || !model.drawn.is_empty(),
            "{scene_name} has no visible meshes"
        );
        if with_layout && model.collision.is_empty() {
            eprintln!(
                "Content pack {}: nothing is solid, the player will fall",
                manifest.id
            );
        }
        Ok(Self { manifest, model })
    }
}

/// The pack's own scene (and separate collision file), or an empty model
/// when the world is made only in Build Mode.
pub fn base_model(folder: &Path, manifest: &Manifest) -> Result<gltf_model::Model> {
    let key = format!("pack:{}", manifest.id);
    let Some(scene) = &manifest.world.scene else {
        return Ok(gltf_model::Model::default());
    };
    let mut model = gltf_model::load(&inside(folder, scene)?, &key)?;
    if let Some(collision) = &manifest.world.collision {
        let solid = gltf_model::load(&inside(folder, collision)?, &format!("{key}:col"))?;
        model.collision = solid.collision;
        model.collision.extend(solid.drawn);
    }
    Ok(model)
}

/// What [`place_layout`] could not place, so the caller can show it.
#[derive(Default, Debug)]
pub struct PlaceReport {
    pub placed: usize,
    pub problems: Vec<String>,
}

/// Add every visible (or solid) layout object to `model`, transformed into
/// pack space. Objects whose asset is missing are reported, not dropped from
/// the layout: the layout file is never changed here. Objects gameplay can
/// show or hide ([`crate::world_logic::switchable`]) are left out: the game
/// draws them, and switches their collision, one by one.
pub fn place_layout(
    model: &mut gltf_model::Model,
    layout: &world_layout::Layout,
    assets: &mut AssetResolver,
) -> PlaceReport {
    let mut report = PlaceReport::default();
    let switchable = crate::world_logic::switchable(layout);
    for node in &layout.nodes {
        let Some(asset) = &node.asset else { continue };
        if switchable.contains(&node.id) {
            continue;
        }
        if matches!(asset, world_layout::AssetRef::Builtin(name) if crate::builtin_assets::editor_only(name))
        {
            // Gameplay markers only show in Build Mode.
            continue;
        }
        let visible = layout.effective_visible(node.id);
        let solid = layout.effective_collision(node.id);
        if !visible && !solid {
            continue;
        }
        let source = match node_model(node, assets) {
            Ok(source) => source,
            Err(error) => {
                report.problems.push(format!(
                    "object {} ({}) is missing its model: {error:#}",
                    node.id, node.name
                ));
                continue;
            }
        };
        let (drawn, collision) = source.transformed(node.transform.matrix());
        if let Some(light) = node.light.as_ref().filter(|_| visible) {
            let glow = crate::lights::point_light_model(light);
            model
                .drawn
                .extend(glow.transformed(node.transform.matrix()).0);
            for (key, texture) in glow.textures {
                model.textures.entry(key).or_insert(texture);
            }
        }
        if visible {
            model.triangles += source.triangles;
            model.drawn.extend(drawn);
            for (key, texture) in &source.textures {
                model
                    .textures
                    .entry(key.clone())
                    .or_insert_with(|| texture.clone());
            }
        }
        if solid {
            model.collision.extend(collision);
        }
        report.placed += 1;
    }
    report
}

/// The model a layout object shows: its sculpted terrain, its road, or its
/// asset.
pub fn node_model(
    node: &world_layout::Node,
    assets: &mut AssetResolver,
) -> Result<std::sync::Arc<gltf_model::Model>> {
    if let Some(terrain) = &node.terrain {
        return Ok(std::sync::Arc::new(crate::terrain::model(terrain)));
    }
    if let Some(road) = &node.road {
        return Ok(std::sync::Arc::new(crate::road::model(road)));
    }
    let asset = node.asset.as_ref().context("a group has no model")?;
    assets.model(asset)
}

/// Finds and caches the models layout objects refer to.
pub struct AssetResolver {
    own: PathBuf,
    pack_dirs: Vec<PathBuf>,
    cache: HashMap<
        world_layout::AssetRef,
        std::result::Result<std::sync::Arc<gltf_model::Model>, String>,
    >,
}
impl AssetResolver {
    /// Resolve `self:` paths in `own_folder` and `pack:` ids in the usual
    /// content folders plus the folder that holds this pack.
    pub fn new(own_folder: &Path) -> Self {
        let mut pack_dirs = crate::content::pack_dirs();
        if let Some(parent) = own_folder.parent() {
            pack_dirs.insert(0, parent.to_path_buf());
        }
        Self::with_dirs(own_folder, pack_dirs)
    }
    pub fn with_dirs(own_folder: &Path, pack_dirs: Vec<PathBuf>) -> Self {
        Self {
            own: own_folder.to_path_buf(),
            pack_dirs,
            cache: HashMap::new(),
        }
    }
    pub fn own_folder(&self) -> &Path {
        &self.own
    }
    /// Where a file asset lives on this computer, or None for built-ins.
    pub fn path(&self, asset: &world_layout::AssetRef) -> Result<Option<PathBuf>> {
        use world_layout::AssetRef;
        Ok(match asset {
            AssetRef::Builtin(_) => None,
            AssetRef::Own(path) => Some(inside(&self.own, path)?),
            AssetRef::Pack { id, path } => {
                let folder = self
                    .pack_dirs
                    .iter()
                    .find_map(|dir| resolve(id, dir).ok())
                    .with_context(|| format!("content pack {id:?} is not installed"))?;
                Some(inside(&folder, path)?)
            }
        })
    }
    /// The asset's model in its own space (glTF axes, origin at its origin).
    pub fn model(
        &mut self,
        asset: &world_layout::AssetRef,
    ) -> Result<std::sync::Arc<gltf_model::Model>> {
        if let Some(cached) = self.cache.get(asset) {
            return cached.clone().map_err(anyhow::Error::msg);
        }
        let loaded = self.load(asset).map(std::sync::Arc::new);
        let stored = loaded
            .as_ref()
            .map(Clone::clone)
            .map_err(|e| format!("{e:#}"));
        self.cache.insert(asset.clone(), stored);
        loaded
    }
    /// Forget cached models, so files changed on disk load again.
    pub fn clear(&mut self) {
        self.cache.clear();
    }
    fn load(&self, asset: &world_layout::AssetRef) -> Result<gltf_model::Model> {
        match self.path(asset)? {
            None => {
                let world_layout::AssetRef::Builtin(name) = asset else {
                    unreachable!()
                };
                crate::builtin_assets::model(name)
                    .with_context(|| format!("SARE has no built-in asset {name:?}"))
            }
            Some(path) => {
                ensure!(path.is_file(), "{} does not exist", path.display());
                gltf_model::load(&path, &format!("asset:{asset}"))
            }
        }
    }
}

impl WorldSource for PackWorld {
    fn name(&self) -> &str {
        &self.manifest.name
    }
    fn load(&mut self, _center: [f32; 2], origin: [f32; 2], _radius: f32) -> Result<Scene> {
        // Packs load whole for now; streaming cells come later.
        let offset = Vec3::new(ANCHOR[0] - origin[0], 0.0, origin[1] - ANCHOR[1]);
        let (batches, collision) = self.model.translated(offset);
        let collision = CollisionWorld::from_batches(&collision);
        let textures: HashMap<_, _> = self.model.textures.clone();
        Ok(Scene {
            vehicle_handling: vehicle::Handling::default(),
            water: None,
            triangles: self.model.triangles,
            batches,
            textures,
            placements: 1,
            animation: None,
            collision: Some(collision),
            vehicle_pieces: HashMap::new(),
        })
    }
    fn spawn(&self) -> Option<[f32; 3]> {
        let [east, north, up] = self.manifest.world.spawn;
        Some([ANCHOR[0] + east, ANCHOR[1] + north, up])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gltf_model::tests::quad_glb;

    fn pack(dir: &Path, manifest: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(MANIFEST), manifest).unwrap();
        std::fs::write(dir.join("world.glb"), quad_glb("Ground", [0.0, 0.0, 0.0])).unwrap();
    }
    const GOOD: &str = r#"{"id":"tiny","name":"Tiny","version":"1.0.0","license":"CC0-1.0",
        "world":{"scene":"world.glb","spawn":[0,0,0]}}"#;

    #[test]
    fn a_pack_loads_resolves_and_spawns_on_solid_ground() {
        let root = std::env::temp_dir().join(format!("sare-pack-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        pack(&root.join("my-folder"), GOOD);
        let folder = resolve("tiny", &root).unwrap();
        assert_eq!(folder, root.join("my-folder"));
        assert_eq!(
            resolve(folder.to_str().unwrap(), Path::new("nowhere")).unwrap(),
            folder
        );
        assert!(resolve("missing", &root).is_err());
        assert_eq!(list(&root).len(), 1);

        let mut world = PackWorld::open(&folder).unwrap();
        assert_eq!(world.name(), "Tiny");
        let origin = [ANCHOR[0] - 5.0, ANCHOR[1]];
        let scene = world.load(origin, origin, 300.0).unwrap();
        let [x, y, z] = world.spawn().unwrap();
        let point = Vec3::new(x - origin[0], z, origin[1] - y);
        assert!((point.x - 5.0).abs() < 1e-4);
        let collision = scene.collision.unwrap();
        assert!(collision.standing_at(point, z + 2.0).is_some());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn manifests_need_an_id_licence_and_known_fields() {
        let bad = |text: &str| -> bool {
            serde_json::from_str::<Manifest>(text)
                .map_err(anyhow::Error::from)
                .and_then(|m| m.validate())
                .is_err()
        };
        assert!(!bad(GOOD));
        assert!(bad(&GOOD.replace("CC0-1.0", " ")));
        assert!(bad(&GOOD.replace("\"tiny\"", "\"Tiny Pack\"")));
        assert!(bad(&GOOD.replace("\"version\"", "\"verison\"")));
        assert!(inside(Path::new("p"), "../x.glb").is_err());
        assert!(inside(Path::new("p"), "/abs.glb").is_err());
    }
}
