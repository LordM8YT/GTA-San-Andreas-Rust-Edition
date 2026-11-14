//! Content packs: a folder with `content.json` and a glTF world, so anyone can
//! make a playable SARE world in Blender without any San Andreas files.
//! The format is documented in docs/content-packs.md.
use crate::character::CharacterModel;
use crate::{
    collision::CollisionWorld, content::WorldSource, gltf_model, pack_vehicle, vehicle, Scene,
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
    /// The drawn world, a `.glb` (or `.gltf`) file inside the pack.
    pub scene: String,
    /// Optional separate collision model. When set, only this file collides
    /// and `scene` is drawn only.
    #[serde(default)]
    pub collision: Option<String>,
    /// Start point in Blender coordinates: [east, north, up] in metres.
    pub spawn: [f32; 3],
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
    fn validate(&self) -> Result<()> {
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
fn inside(folder: &Path, relative: &str) -> Result<PathBuf> {
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
    manifest
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
                    eprintln!("Content pack vehicle {} skipped: {error:#}", spec.model);
                    None
                }
            }
        })
        .collect()
}

/// A loaded pack world, kept in glTF space and moved into place per load.
pub struct PackWorld {
    pub manifest: Manifest,
    model: gltf_model::Model,
}
impl PackWorld {
    pub fn open(folder: &Path) -> Result<Self> {
        let manifest = Manifest::read(folder)?;
        let key = format!("pack:{}", manifest.id);
        let mut model = gltf_model::load(&inside(folder, &manifest.world.scene)?, &key)?;
        if let Some(collision) = &manifest.world.collision {
            let solid = gltf_model::load(&inside(folder, collision)?, &format!("{key}:col"))?;
            model.collision = solid.collision;
            model.collision.extend(solid.drawn);
        }
        ensure!(
            !model.drawn.is_empty(),
            "{} has no visible meshes",
            manifest.world.scene
        );
        if model.collision.is_empty() {
            eprintln!(
                "Content pack {}: nothing is solid, the player will fall",
                manifest.id
            );
        }
        Ok(Self { manifest, model })
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
