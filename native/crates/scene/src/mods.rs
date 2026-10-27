//! Data-only local resources, layered alphabetically. No executable code.
use super::*;
use serde::Deserialize;
use std::collections::HashSet;

#[derive(Default, Deserialize)]
struct Manifest {
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    name: String,
    #[serde(default)]
    models: Vec<Model>,
    #[serde(default)]
    vehicles: Vec<Vehicle>,
    #[serde(default)]
    original_vehicle_handling: HashMap<String, crate::vehicle::Handling>,
    #[serde(default)]
    player: Option<PlayerModel>,
    #[serde(default)]
    placements: Vec<Instance>,
    #[serde(default)]
    exclude_model_ids: Vec<i32>,
    /// MTA:SA `.map` files with object placements and world-object removals.
    #[serde(default)]
    mta_maps: Vec<String>,
    /// Model files an MTA resource's scripts assign to IDs. Unlike `models`,
    /// an entry that fails to load is skipped instead of failing the resource.
    #[serde(default)]
    mta_models: Vec<MtaModel>,
    #[serde(default)]
    texture_overrides: HashMap<String, String>,
}
impl Manifest {
    fn validate_vehicle_tuning(&self) -> Result<()> {
        ensure!(
            self.original_vehicle_handling.len() <= 32,
            "original vehicle tuning limit (32)"
        );
        for (model, tuning) in &self.original_vehicle_handling {
            ensure!(!model.is_empty() && model.len() <= 32
                && model.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'),
                "original vehicle tuning requires lowercase model names (1-32 letters/digits/underscores)");
            tuning.validate()?;
        }
        Ok(())
    }
}
#[derive(Deserialize)]
struct Model {
    id: i32,
    dff: String,
    #[serde(default)]
    txd: Option<String>,
    #[serde(default)]
    col: Option<String>,
}
#[derive(Deserialize)]
struct MtaModel {
    id: i32,
    #[serde(default)]
    dff: Option<String>,
    #[serde(default)]
    txd: Option<String>,
    #[serde(default)]
    col: Option<String>,
}
#[derive(Deserialize)]
struct Vehicle {
    #[serde(default)]
    name: String,
    #[serde(default)]
    handling: crate::vehicle::Handling,
    dff: String,
    #[serde(default)]
    txd: Option<String>,
}
impl Vehicle {
    fn validate(&self) -> Result<()> {
        self.handling.validate()?;
        ensure!(
            self.name.is_empty()
                || (!self.name.trim().is_empty()
                    && self.name.chars().count() <= 48
                    && self.name.chars().all(|ch| !ch.is_control())),
            "vehicle name must contain 1-48 printable characters"
        );
        Ok(())
    }
}
#[derive(Deserialize)]
struct PlayerModel {
    dff: String,
    #[serde(default)]
    txd: Option<String>,
    #[serde(default)]
    ifp: Option<String>,
    #[serde(default)]
    clothes: Vec<ClothingModel>,
}
#[derive(Deserialize)]
struct ClothingModel {
    dff: String,
    #[serde(default)]
    txd: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default = "enabled_clothing")]
    enabled: bool,
}
fn enabled_clothing() -> bool {
    true
}
/// FiveM-style bracket folders group resources; ordinary folders are leaves.
fn resource_folders(root: &Path) -> Result<Vec<PathBuf>> {
    fn collect(
        directory: &Path,
        root: &Path,
        depth: usize,
        folders: &mut Vec<PathBuf>,
    ) -> Result<()> {
        ensure!(depth <= 4, "mod category nesting exceeds four levels");
        let mut entries = fs::read_dir(directory)?.collect::<std::io::Result<Vec<_>>>()?;
        ensure!(entries.len() <= 128, "too many entries in mod directory");
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let folder = entry.path().canonicalize()?;
            ensure!(folder.starts_with(root), "mod directory escapes root");
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('[') && name.ends_with(']') {
                collect(&folder, root, depth + 1, folders)?;
            } else {
                folders.push(folder);
                ensure!(folders.len() <= 128, "too many mod resources");
            }
        }
        Ok(())
    }
    let mut folders = Vec::new();
    collect(root, root, 0, &mut folders)?;
    Ok(folders)
}
#[derive(Deserialize)]
struct Instance {
    model_id: i32,
    position: [f32; 3],
    #[serde(default = "identity")]
    rotation: [f32; 4],
}
fn identity() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}
/// Share only files referenced by enabled native manifests. Category folders,
/// disabled demos, original IMG archives and arbitrary adjacent files stay local.
pub fn share_resources(directory: &Path) -> Result<sa_net::resources::Share> {
    if !directory.exists() {
        return Ok(sa_net::resources::Share::default());
    }
    let root = directory.canonicalize()?;
    share_resource_folders(resource_folders(&root)?, false)
}
/// Share an explicit, ordered resource list, e.g. a server's `ensure` lines.
/// `force_enabled` treats each listed resource as enabled, as `ensure` does.
pub fn share_resource_folders(
    folders: Vec<PathBuf>,
    force_enabled: bool,
) -> Result<sa_net::resources::Share> {
    use sa_net::resources::{Input, Share, MAX_FILES, MAX_PACK_BYTES};
    use std::collections::BTreeSet;
    let mut inputs = Vec::new();
    let (mut total_bytes, mut total_files) = (0_usize, 0_usize);
    for folder in folders {
        let folder = folder.canonicalize()?;
        let Some((filename, raw)) = manifest_bytes(&folder)? else {
            continue;
        };
        let manifest: Manifest = serde_json::from_slice(&raw)?;
        if !manifest.enabled && !force_enabled {
            continue;
        }
        manifest.validate_vehicle_tuning()?;
        let name = if manifest.name.is_empty() {
            folder.file_name().unwrap().to_string_lossy().into_owned()
        } else {
            manifest.name.clone()
        };
        let mut references = BTreeSet::new();
        for model in &manifest.models {
            references.insert(model.dff.clone());
            references.extend(model.txd.iter().cloned());
            references.extend(model.col.iter().cloned());
        }
        for vehicle in &manifest.vehicles {
            vehicle.validate()?;
            references.insert(vehicle.dff.clone());
            references.extend(vehicle.txd.iter().cloned());
        }
        if let Some(player) = &manifest.player {
            references.insert(player.dff.clone());
            references.extend(player.txd.iter().cloned());
            references.extend(player.ifp.iter().cloned());
            for clothing in &player.clothes {
                references.insert(clothing.dff.clone());
                references.extend(clothing.txd.iter().cloned());
            }
        }
        references.extend(manifest.texture_overrides.values().cloned());
        references.extend(manifest.mta_maps.iter().cloned());
        for model in &manifest.mta_models {
            references.extend(
                [&model.dff, &model.txd, &model.col]
                    .into_iter()
                    .flatten()
                    .filter(|path| folder.join(path).is_file())
                    .cloned(),
            );
        }
        total_files += references.len() + 1;
        ensure!(
            total_files <= MAX_FILES,
            "server resource sharing supports at most {MAX_FILES} files"
        );
        let mut exported: serde_json::Value = serde_json::from_slice(&raw)?;
        exported["name"] = serde_json::Value::String(name.clone());
        if force_enabled {
            exported["enabled"] = serde_json::Value::Bool(true);
        }
        fn normalize(value: &mut serde_json::Value) {
            match value {
                serde_json::Value::Object(fields) => {
                    for (key, value) in fields {
                        if ["dff", "txd", "col", "ifp"].contains(&key.as_str()) {
                            if let Some(path) = value.as_str() {
                                *value = serde_json::Value::String(path.replace('\\', "/"));
                            }
                        } else if key == "mta_maps" {
                            if let Some(maps) = value.as_array_mut() {
                                for value in maps {
                                    if let Some(path) = value.as_str() {
                                        *value = serde_json::Value::String(path.replace('\\', "/"));
                                    }
                                }
                            }
                        } else if key == "texture_overrides" {
                            if let Some(textures) = value.as_object_mut() {
                                for value in textures.values_mut() {
                                    if let Some(path) = value.as_str() {
                                        *value = serde_json::Value::String(path.replace('\\', "/"));
                                    }
                                }
                            }
                        } else {
                            normalize(value);
                        }
                    }
                }
                serde_json::Value::Array(values) => {
                    for value in values {
                        normalize(value);
                    }
                }
                _ => {}
            }
        }
        normalize(&mut exported);
        let data = serde_json::to_vec(&exported)?;
        total_bytes += data.len();
        let mut files = vec![(filename.into(), data)];
        for relative in references {
            let normalized = relative.replace('\\', "/");
            sa_net::resources::validate_path(&normalized)?;
            let bytes = resource(&folder, &relative)?;
            total_bytes += bytes.len();
            ensure!(
                total_bytes <= MAX_PACK_BYTES,
                "server resource pack exceeds 128 MiB"
            );
            files.push((normalized, bytes));
        }
        inputs.push(Input { name, files });
        ensure!(
            inputs.len() <= 16,
            "server sharing supports at most 16 enabled resources"
        );
    }
    Ok(Share::build(inputs)?)
}
#[derive(Default)]
pub(super) struct Resources {
    pub original_vehicle_handling: HashMap<String, crate::vehicle::Handling>,
    pub geometry: HashMap<String, Vec<Geometry>>,
    pub dictionaries: HashMap<String, Vec<u8>>,
    pub textures: HashMap<String, Texture>,
    pub excluded: HashSet<i32>,
    /// Spawn points from MTA maps, as (label, IPL position).
    pub map_spawns: Vec<(String, [f32; 3])>,
    pub names: Vec<String>,
    pub vehicle: Option<Scene>,
    pub vehicles: Vec<(String, Scene)>,
    pub car_name: String,
    pub ped_name: String,
    pub player: Option<ped::Source>,
    pub players: Vec<(String, ped::Source)>,
}
/// The native manifest, or one generated from an MTA resource's `meta.xml`
/// when the folder has no native manifest. Shared packs always carry JSON.
fn manifest_bytes(folder: &Path) -> Result<Option<(&'static str, Vec<u8>)>> {
    for filename in ["mod.json", "resource.json"] {
        if folder.join(filename).is_file() {
            return Ok(Some((filename, resource(folder, filename)?)));
        }
    }
    if !folder.join("meta.xml").is_file() {
        return Ok(None);
    }
    let folder_name = folder
        .file_name()
        .context("resource folder name missing")?
        .to_string_lossy();
    let manifest =
        crate::mta_map::meta_manifest(&resource(folder, "meta.xml")?, &folder_name, |path| {
            resource(folder, path)
        })
        .with_context(|| format!("MTA resource {}", folder.display()))?;
    Ok(Some(("mod.json", serde_json::to_vec(&manifest)?)))
}
/// Canonical containment also rejects escapes through directory junctions.
fn resource(root: &Path, relative: &str) -> Result<Vec<u8>> {
    let path = root
        .join(relative)
        .canonicalize()
        .context("mod resource absent")?;
    ensure!(
        path.starts_with(root) && path.is_file(),
        "mod resource escapes its directory"
    );
    ensure!(
        fs::metadata(&path)?.len() <= 16 * 1024 * 1024,
        "mod resource exceeds 16 MiB"
    );
    Ok(fs::read(path)?)
}
fn png_texture(data: &[u8]) -> Result<Texture> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(data));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    ensure!(
        reader.info().width <= 4096 && reader.info().height <= 4096,
        "mod PNG exceeds 4096 pixels"
    );
    let size = reader
        .output_buffer_size()
        .context("PNG output size overflow")?;
    ensure!(size <= 64 * 1024 * 1024, "mod PNG memory budget");
    let mut bytes = vec![0; size];
    let info = reader.next_frame(&mut bytes)?;
    let mut rgba = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    let channels = match info.color_type {
        png::ColorType::Rgba => 4,
        png::ColorType::Rgb => 3,
        png::ColorType::GrayscaleAlpha => 2,
        png::ColorType::Grayscale => 1,
        _ => anyhow::bail!("unsupported mod PNG colour format"),
    };
    for pixel in bytes[..info.buffer_size()].chunks_exact(channels) {
        match channels {
            4 => rgba.extend_from_slice(pixel),
            3 => {
                rgba.extend_from_slice(pixel);
                rgba.push(255);
            }
            2 => rgba.extend([pixel[0], pixel[0], pixel[0], pixel[1]]),
            _ => rgba.extend([pixel[0], pixel[0], pixel[0], 255]),
        }
    }
    let has_alpha = rgba.as_chunks::<4>().0.iter().any(|p| p[3] < 255);
    Ok(Texture {
        width: info.width,
        height: info.height,
        rgba,
        has_alpha,
    })
}
impl WorldLoader {
    /// Original catalog tail after the primary model, consistent across clients.
    pub fn additional_original_car_models(
        &self,
    ) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
        [
            ("Taxi", "taxi"),
            ("Infernus", "infernus"),
            ("Admiral", "admiral"),
        ]
        .into_iter()
        .filter(|(_, model)| !self.resources.car_name.is_empty() || *model != "taxi")
    }
    pub fn additional_original_ped_models(
        &self,
    ) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
        [
            ("Grove Street", "fam1"),
            ("Grove Street 2", "fam2"),
            ("Ballas", "ballas1"),
        ]
        .into_iter()
        .filter(|(_, model)| !self.resources.ped_name.is_empty() || *model != "fam1")
    }
    /// Read an original model from the installation and apply native resource tuning.
    pub fn load_original_car(&self, game: &Path, model: &str) -> Result<Scene> {
        let mut scene = super::load_car_model(game, model)?;
        if let Some(tuning) = self.resources.original_vehicle_handling.get(model) {
            scene.vehicle_handling = *tuning;
        }
        Ok(scene)
    }
    pub fn enable_mods(&mut self, directory: &Path) -> Result<()> {
        if !directory.exists() {
            return Ok(());
        }
        let root = directory.canonicalize()?;
        let mut resource_names = HashSet::new();
        for folder in resource_folders(&root)? {
            let Some((_, raw)) = manifest_bytes(&folder)? else {
                continue;
            };
            let manifest: Manifest = serde_json::from_slice(&raw)
                .with_context(|| format!("mod manifest {}", folder.display()))?;
            let folder_name = folder
                .file_name()
                .context("resource folder name missing")?
                .to_string_lossy()
                .into_owned();
            ensure!(resource_names.insert(folder_name.to_ascii_lowercase()),
                "duplicate resource folder name: {folder_name}; names must be unique across categories");
            let name = if manifest.name.is_empty() {
                folder_name.clone()
            } else {
                manifest.name.clone()
            };
            if !manifest.enabled {
                self.resources.names.push(format!("[disabled] {name}"));
                continue;
            }
            manifest.validate_vehicle_tuning()?;
            self.resources.original_vehicle_handling.extend(
                manifest
                    .original_vehicle_handling
                    .iter()
                    .map(|(name, tuning)| (name.clone(), *tuning)),
            );
            ensure!(
                self.resources.original_vehicle_handling.len() <= 64,
                "total original vehicle tuning limit (64)"
            );
            ensure!(
                manifest.models.len() <= 256 && manifest.placements.len() <= 2000,
                "mod model/placement budget"
            );
            ensure!(
                manifest.vehicles.len()
                    + self.resources.vehicles.len()
                    + usize::from(self.resources.vehicle.is_some())
                    <= 32,
                "custom vehicle limit (32)"
            );
            for vehicle in manifest.vehicles {
                vehicle.validate()?;
                let geometry = sa_assets::decode_vehicle_pieces(&resource(&folder, &vehicle.dff)?)
                    .with_context(|| format!("custom vehicle {}", vehicle.dff))?;
                let dictionary = vehicle
                    .txd
                    .as_deref()
                    .map(|path| resource(&folder, path))
                    .transpose()?;
                let mut scene = car_scene(&geometry, None, |name| {
                    decode_txd(
                        dictionary
                            .as_deref()
                            .context("textured custom vehicle requires a TXD")?,
                        name,
                    )
                })?;
                scene.vehicle_handling = vehicle.handling;
                ensure!(scene.triangles > 0, "empty custom vehicle");
                let car_name = if vehicle.name.is_empty() {
                    if self.resources.vehicle.is_none() {
                        name.clone()
                    } else {
                        format!("{name} / {}", vehicle.dff)
                    }
                } else {
                    vehicle.name.trim().to_owned()
                };
                if self.resources.vehicle.is_none() {
                    self.resources.car_name = car_name;
                    self.resources.vehicle = Some(scene);
                } else {
                    self.resources.vehicles.push((car_name, scene));
                }
            }
            if let Some(player) = manifest.player {
                ensure!(
                    self.resources.players.len() + usize::from(self.resources.player.is_some())
                        < 16,
                    "custom player limit (16)"
                );
                let dictionary = player
                    .txd
                    .as_deref()
                    .map(|p| resource(&folder, p))
                    .transpose()?;
                let clips = player
                    .ifp
                    .as_deref()
                    .map(|p| resource(&folder, p))
                    .transpose()?;
                ensure!(player.clothes.len() <= 16, "player clothing limit (16)");
                let mut source =
                    ped::Source::decode(&resource(&folder, &player.dff)?, dictionary, clips)?;
                for clothing in player.clothes {
                    let dictionary = clothing
                        .txd
                        .as_deref()
                        .map(|p| resource(&folder, p))
                        .transpose()?;
                    source.attach_clothing(&resource(&folder, &clothing.dff)?, dictionary)?;
                    source.configure_last_clothing(
                        if clothing.name.is_empty() {
                            clothing.dff
                        } else {
                            clothing.name
                        },
                        clothing.enabled,
                    );
                }
                if self.resources.player.is_none() {
                    self.resources.ped_name = name.clone();
                    self.resources.player = Some(source);
                } else {
                    self.resources.players.push((name.clone(), source));
                }
            }
            ensure!(
                self.resources.geometry.len() + manifest.models.len() <= 512,
                "total mod model budget"
            );
            ensure!(
                self.resources.textures.len() + manifest.texture_overrides.len() <= 512,
                "total mod texture budget"
            );
            for model in manifest.models {
                ensure!(model.id >= 0, "negative mod model ID");
                let key = format!("mod_{}_{}", folder_name.to_ascii_lowercase(), model.id);
                let geometry = decode_dff(&resource(&folder, &model.dff)?)
                    .with_context(|| format!("mod model {}", model.dff))?;
                if let Some(txd) = model.txd {
                    self.resources
                        .dictionaries
                        .insert(key.clone(), resource(&folder, &txd)?);
                } else {
                    ensure!(
                        geometry
                            .iter()
                            .all(|g| g.materials.iter().all(|m| m.texture.is_none())),
                        "textured mod DFF requires a TXD"
                    );
                }
                // The new definition has its own key, so it cannot accidentally
                // retain the replaced model's collision. Shared old models stay.
                if let Some(col) = model.col {
                    let mut collisions = sa_assets::col::decode_col(&resource(&folder, &col)?)?;
                    ensure!(
                        collisions.len() == 1,
                        "each mod model COL must contain exactly one model"
                    );
                    let mut collision = collisions.remove(0);
                    collision.name = key.clone();
                    self.collision_models.insert(key.clone(), collision);
                }
                self.resources
                    .geometry
                    .insert(format!("{key}.dff"), geometry);
                self.defs.insert(
                    model.id,
                    Definition {
                        model: key.clone(),
                        txd: key,
                    },
                );
            }
            for instance in manifest.placements {
                ensure!(
                    self.defs.contains_key(&instance.model_id),
                    "mod placement uses unknown model {}",
                    instance.model_id
                );
                self.rows.push(placement(
                    instance.model_id,
                    0,
                    instance.position,
                    instance.rotation,
                )?);
            }
            ensure!(
                manifest.mta_models.len() <= 256,
                "MTA model limit (256 per resource)"
            );
            let mut replaced = 0;
            for model in &manifest.mta_models {
                if self.resources.geometry.len() >= 512 {
                    eprintln!("MTA model {}: total mod model budget reached", model.id);
                    break;
                }
                match self.mta_model(&folder, &folder_name, model) {
                    Ok(()) => replaced += 1,
                    Err(error) => eprintln!("MTA model {} skipped: {error:#}", model.id),
                }
            }
            if !manifest.mta_models.is_empty() {
                eprintln!(
                    "{name}: {replaced} of {} script model replacements loaded",
                    manifest.mta_models.len()
                );
            }
            ensure!(
                manifest.mta_maps.len() <= 16,
                "MTA map limit (16 per resource)"
            );
            let mut map_objects = 0;
            for relative in &manifest.mta_maps {
                let map = crate::mta_map::parse(&resource(&folder, relative)?)
                    .with_context(|| format!("MTA map {relative}"))?;
                for removal in &map.removals {
                    self.rows.retain(|row| {
                        let model = row.id == removal.model || Some(row.id) == removal.lod_model;
                        let interior = removal.interior.is_none_or(|i| i == row.interior);
                        let distance = row
                            .pos
                            .iter()
                            .zip(removal.position)
                            .map(|(a, b)| (a - b).powi(2))
                            .sum::<f32>();
                        !(model && interior && distance <= removal.radius.powi(2))
                    });
                }
                let mut unknown = 0;
                for object in map.objects {
                    if !self.defs.contains_key(&object.model) {
                        unknown += 1;
                        continue;
                    }
                    map_objects += 1;
                    ensure!(
                        map_objects <= crate::mta_map::MAX_MAP_ELEMENTS,
                        "MTA map placement budget ({})",
                        crate::mta_map::MAX_MAP_ELEMENTS
                    );
                    let mut row = placement(
                        object.model,
                        object.interior,
                        object.position,
                        object.rotation,
                    )?;
                    row.scale = object.scale;
                    row.solid = object.solid;
                    self.rows.push(row);
                }
                for position in map.spawns {
                    if self.resources.map_spawns.len() < 64 {
                        let label = format!("{name} {}", self.resources.map_spawns.len() + 1);
                        self.resources.map_spawns.push((label, position));
                    }
                }
                eprintln!(
                    "MTA map {relative}: {} removals; skipped {unknown} unknown models, {} unsupported elements",
                    map.removals.len(),
                    map.skipped
                );
            }
            self.resources.excluded.extend(manifest.exclude_model_ids);
            for (key, relative) in manifest.texture_overrides {
                let image = png_texture(&resource(&folder, &relative)?)?;
                self.resources
                    .textures
                    .insert(key.to_ascii_lowercase(), image);
            }
            let geometry_bytes: usize = self
                .resources
                .geometry
                .values()
                .flatten()
                .map(|g| {
                    g.positions.len() * 12
                        + g.normals.len() * 12
                        + g.uvs.len() * 8
                        + g.colors.len() * 4
                        + g.triangles.len() * 8
                })
                .sum();
            let dictionary_bytes: usize = self.resources.dictionaries.values().map(Vec::len).sum();
            let texture_bytes: usize = self.resources.textures.values().map(|t| t.rgba.len()).sum();
            let vehicle_bytes: usize = self
                .resources
                .vehicle
                .iter()
                .chain(self.resources.vehicles.iter().map(|(_, s)| s))
                .map(|scene| {
                    scene
                        .batches
                        .iter()
                        .map(|b| b.vertices.len() * std::mem::size_of::<Vertex>())
                        .sum::<usize>()
                        + scene.textures.values().map(|t| t.rgba.len()).sum::<usize>()
                })
                .sum();
            ensure!(
                geometry_bytes
                    + dictionary_bytes
                    + texture_bytes
                    + vehicle_bytes
                    + self
                        .resources
                        .player
                        .as_ref()
                        .map_or(0, ped::Source::memory_bytes)
                    + self
                        .resources
                        .players
                        .iter()
                        .map(|(_, p)| p.memory_bytes())
                        .sum::<usize>()
                    <= 256 * 1024 * 1024,
                "total mod memory budget (256 MiB)"
            );
            eprintln!("Enabled resource: {name}");
            self.resources.names.push(format!("[enabled] {name}"));
        }
        Ok(())
    }
    /// Loads one script model replacement; nothing is changed on error.
    fn mta_model(&mut self, folder: &Path, folder_name: &str, model: &MtaModel) -> Result<()> {
        ensure!(model.id >= 0, "negative model ID");
        let key = format!("mod_{}_{}", folder_name.to_ascii_lowercase(), model.id);
        let original = self.defs.get(&model.id);
        let geometry = model
            .dff
            .as_deref()
            .map(|path| decode_dff(&resource(folder, path)?).with_context(|| path.to_owned()))
            .transpose()?;
        let dictionary = model
            .txd
            .as_deref()
            .map(|path| resource(folder, path))
            .transpose()?;
        let mesh = match (&geometry, original) {
            (Some(_), _) => key.clone(),
            (None, Some(def)) => def.model.clone(),
            (None, None) => anyhow::bail!("texture-only replacement of an unknown model"),
        };
        // Without its own TXD a new mesh keeps the replaced model's textures.
        let txd = match (&dictionary, original) {
            (Some(_), _) | (None, None) => key.clone(),
            (None, Some(def)) => def.txd.clone(),
        };
        let collision = model
            .col
            .as_deref()
            .map(|path| -> Result<_> {
                let mut models = sa_assets::col::decode_col(&resource(folder, path)?)?;
                ensure!(!models.is_empty(), "empty COL {path}");
                // A multi-model COL contributes its first model, as for one model ID.
                let mut collision = models.swap_remove(0);
                collision.name = mesh.clone();
                Ok(collision)
            })
            .transpose()?;
        if let Some(geometry) = geometry {
            self.resources
                .geometry
                .insert(format!("{key}.dff"), geometry);
        }
        if let Some(dictionary) = dictionary {
            self.resources.dictionaries.insert(key.clone(), dictionary);
        }
        if let Some(collision) = collision {
            self.collision_models.insert(mesh.clone(), collision);
        }
        self.defs.insert(model.id, Definition { model: mesh, txd });
        Ok(())
    }
    /// MTA map spawn points, as (label, IPL position).
    pub fn map_spawns(&self) -> &[(String, [f32; 3])] {
        &self.resources.map_spawns
    }
    pub fn mod_names(&self) -> &[String] {
        &self.resources.names
    }
    pub fn model_names(&self) -> (String, String) {
        (
            if self.resources.car_name.is_empty() {
                "Taxi".into()
            } else {
                self.resources.car_name.clone()
            },
            if self.resources.ped_name.is_empty() {
                "Grove Street".into()
            } else {
                self.resources.ped_name.clone()
            },
        )
    }
    pub fn take_car_catalog(&mut self) -> Vec<(String, Scene)> {
        std::mem::take(&mut self.resources.vehicles)
    }
    pub fn take_ped_catalog(&mut self, game: &Path) -> Result<Vec<(String, ped::Ped)>> {
        std::mem::take(&mut self.resources.players)
            .into_iter()
            .map(|(name, source)| Ok((name, ped::Ped::from_source(source, game)?)))
            .collect()
    }
    pub fn take_custom_car(&mut self) -> Option<Scene> {
        self.resources.vehicle.take()
    }
    pub fn take_custom_player(&mut self, game: &Path) -> Result<Option<ped::Ped>> {
        self.resources
            .player
            .take()
            .map(|source| ped::Ped::from_source(source, game))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_handling_pack_shares_only_manifest_and_rejects_invalid_models() {
        let root = std::env::temp_dir().join(format!("sa-handling-only-{}", std::process::id()));
        let folder = root.join("handling");
        fs::create_dir_all(&folder).unwrap();
        let mut manifest = serde_json::json!({"enabled":true,"original_vehicle_handling":{"taxi":{"acceleration":7.0}}});
        fs::write(
            folder.join("resource.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let shared = share_resources(&root).unwrap();
        assert_eq!(shared.manifest.resources.len(), 1);
        assert_eq!(shared.manifest.resources[0].files.len(), 1);
        assert_eq!(shared.manifest.resources[0].files[0].path, "resource.json");
        for model in ["../taxi", "Taxi", "", "taxi.dff"] {
            manifest["original_vehicle_handling"] = serde_json::json!({model:{"acceleration":7.0}});
            fs::write(
                folder.join("resource.json"),
                serde_json::to_vec(&manifest).unwrap(),
            )
            .unwrap();
            assert!(share_resources(&root).is_err(), "{model}");
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn custom_room_loads_and_player_can_enter() {
        let root = std::env::temp_dir().join(format!("sa-native-room-{}", std::process::id()));
        let folder = root.join("mods/[maps]/room");
        let disabled = root.join("mods/[maps]/disabled-room");
        fs::create_dir_all(&folder).unwrap();
        fs::create_dir_all(&disabled).unwrap();
        fs::write(
            disabled.join("resource.json"),
            br#"{"enabled":false,"name":"Disabled sample"}"#,
        )
        .unwrap();
        fs::write(root.join("empty.img"), b"VER2\0\0\0\0").unwrap();
        fs::write(
            folder.join("room.dff"),
            include_bytes!("../../../../mods/native-room-demo/room.dff"),
        )
        .unwrap();
        fs::write(
            folder.join("car.dff"),
            include_bytes!("../../../../mods/native-car-demo/car.dff"),
        )
        .unwrap();
        fs::write(folder.join("mod.json"),br#"{"enabled":true,"name":"Test room","models":[{"id":30000,"dff":"room.dff"}],"placements":[{"model_id":30000,"position":[0,0,0]}],"vehicles":[{"dff":"car.dff"}]}"#).unwrap();
        let mut loader = WorldLoader {
            water: std::sync::Arc::new(crate::water::WaterMap::default()),
            water_texture: None,
            roadsign_font: None,
            uv_tracks: HashMap::new(),
            texture_parents: HashMap::new(),
            img: WorldArchive {
                exterior: Img::open(&root.join("empty.img")).unwrap(),
                interior: None,
            },
            defs: HashMap::new(),
            rows: Vec::new(),
            collision_models: HashMap::new(),
            resources: Resources::default(),
        };
        assert_eq!(
            loader.additional_original_car_models().collect::<Vec<_>>(),
            vec![("Infernus", "infernus"), ("Admiral", "admiral")]
        );
        assert_eq!(
            loader.additional_original_ped_models().collect::<Vec<_>>(),
            vec![("Grove Street 2", "fam2"), ("Ballas", "ballas1")]
        );
        fs::write(
            folder.join("ped.dff"),
            include_bytes!("../../../../mods/native-ped-demo/ped.dff"),
        )
        .unwrap();
        fs::write(
            folder.join("ped.ifp"),
            include_bytes!("../../../../mods/native-ped-demo/ped.ifp"),
        )
        .unwrap();
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(folder.join("mod.json")).unwrap()).unwrap();
        fs::write(
            folder.join("clothes.dff"),
            include_bytes!("../../../../mods/native-clothing-demo/clothes.dff"),
        )
        .unwrap();
        manifest["original_vehicle_handling"] =
            serde_json::json!({"taxi":{"acceleration":7.0},"infernus":{"acceleration":10.0}});
        manifest["player"] =
            serde_json::json!({"dff":"ped.dff","ifp":"ped.ifp","clothes":[{"dff":"clothes.dff"}]});
        fs::write(
            folder.join("mod.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        manifest["vehicles"] = serde_json::json!([{"name":"Custom coupe","dff":"car.dff","handling":{"acceleration":9.0}},{"name":"Custom sedan","dff":"car.dff","handling":{"acceleration":4.0}}]);
        fs::write(
            folder.join("mod.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let extra = root.join("mods/second-ped");
        fs::create_dir_all(&extra).unwrap();
        fs::write(
            extra.join("ped.dff"),
            include_bytes!("../../../../mods/native-ped-demo/ped.dff"),
        )
        .unwrap();
        fs::write(
            extra.join("ped.ifp"),
            include_bytes!("../../../../mods/native-ped-demo/ped.ifp"),
        )
        .unwrap();
        fs::write(
            extra.join("resource.json"),
            br#"{"enabled":true,"name":"Second ped","player":{"dff":"ped.dff","ifp":"ped.ifp"}}"#,
        )
        .unwrap();
        loader.enable_mods(&root.join("mods")).unwrap();
        fs::write(extra.join("private.txt"), b"Do not share adjacent files").unwrap();
        let shared = share_resources(&root.join("mods")).unwrap();
        assert_eq!(shared.manifest.resources.len(), 2);
        assert!(shared.manifest.resources.iter().all(|r| {
            r.files.iter().all(|f| f.path != "private.txt")
                && r.files.iter().any(|f| f.path.ends_with(".json"))
        }));
        assert!(shared
            .manifest
            .resources
            .iter()
            .any(|r| r.name == "Test room"));
        fs::remove_file(extra.join("private.txt")).unwrap();
        let cars = loader.take_car_catalog();
        assert_eq!(cars.len(), 1);
        assert_eq!(cars[0].0, "Custom sedan");
        assert_eq!(loader.resources.car_name, "Custom coupe");
        assert_eq!(loader.additional_original_car_models().count(), 3);
        assert_eq!(loader.additional_original_ped_models().count(), 3);
        assert_eq!(
            loader.resources.original_vehicle_handling["taxi"].acceleration,
            7.0
        );
        assert_eq!(
            loader.resources.original_vehicle_handling["infernus"].acceleration,
            10.0
        );
        assert_eq!(cars[0].1.vehicle_handling.acceleration, 4.0);
        assert_eq!(
            loader
                .resources
                .vehicle
                .as_ref()
                .unwrap()
                .vehicle_handling
                .acceleration,
            9.0
        );
        manifest["vehicles"][0]["handling"]["acceleration"] = serde_json::json!(-1.0);
        fs::write(
            folder.join("mod.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        assert!(share_resources(&root.join("mods")).is_err());
        manifest["vehicles"][0]["handling"]["acceleration"] = serde_json::json!(8.0);
        fs::write(
            folder.join("mod.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let changed = share_resources(&root.join("mods")).unwrap();
        assert_ne!(shared.manifest, changed.manifest);
        assert_eq!(
            loader
                .take_ped_catalog(Path::new("not-an-installation"))
                .unwrap()
                .len(),
            1
        );
        assert!(loader
            .mod_names()
            .iter()
            .any(|name| name == "[disabled] Disabled sample"));
        assert!(loader
            .mod_names()
            .iter()
            .any(|name| name == "[enabled] Test room"));
        let ped = loader
            .take_custom_player(Path::new("not-an-installation"))
            .unwrap()
            .unwrap();
        assert_eq!(ped.scene().unwrap().triangles, 168);
        assert!(loader
            .take_custom_player(Path::new("not-an-installation"))
            .unwrap()
            .is_none());
        let scene = loader.load([0.0, 0.0], [0.0, 0.0], 100.0).unwrap();
        assert_eq!(scene.placements, 1);
        assert_eq!(scene.triangles, 108);
        let mut interior_room = loader.rows[0].clone();
        interior_room.interior = 3;
        interior_room.pos[2] = 1000.0;
        loader.rows.push(interior_room);
        assert_eq!(loader.placement_count(), 1);
        assert_eq!(loader.interior_placement_count(), 1);
        assert_eq!(
            loader.load([0.0; 2], [0.0; 2], 100.0).unwrap().placements,
            1
        );
        let interior = loader.load_interior([0.0; 2], [0.0; 2], 100.0, 3).unwrap();
        assert_eq!(interior.placements, 1);
        assert_eq!(interior.triangles, 108);
        assert!(interior.water.is_none());
        assert_eq!(
            interior
                .collision
                .unwrap()
                .ground_below(glam::Vec3::new(0.0, 1002.0, 0.0), 1002.0),
            Some(1000.1)
        );
        assert!(loader.load_interior([0.0; 2], [0.0; 2], 100.0, 0).is_err());
        assert!(loader.load_interior([0.0; 2], [0.0; 2], 100.0, 4).is_err());
        let world = scene.collision.unwrap();
        let custom = loader.take_custom_car().unwrap();
        assert_eq!(custom.triangles, 108);
        assert!(custom.batches.iter().all(|b| b.animated));
        assert!(loader.take_custom_car().is_none());
        let mut car = crate::vehicle::Car::new(glam::Vec3::new(0.0, 0.65, 0.0), 0.55);
        for _ in 0..30 {
            car.step(&world, 1.0, 0.0, false, 1.0, 1.0 / 60.0);
        }
        assert!(car.speed > 2.0 && car.position.z > 0.5);
        assert_eq!(
            world.ground_below(glam::Vec3::new(0.0, 2.0, 0.0), 2.0),
            Some(0.1)
        );
        let mut player = collision::Player::spawn(&world, glam::Vec3::new(0.0, 1.7, 3.9));
        for _ in 0..60 {
            player.step(&world, -glam::Vec3::Z * 4.0, false, 1.0 / 60.0);
        }
        assert!(
            player.feet.z < 0.1 && player.grounded,
            "custom room door blocked or floor missing"
        );
        fs::remove_file(folder.join("clothes.dff")).unwrap();
        fs::remove_file(folder.join("ped.dff")).unwrap();
        fs::remove_file(folder.join("ped.ifp")).unwrap();
        fs::remove_file(folder.join("room.dff")).unwrap();
        fs::remove_file(folder.join("car.dff")).unwrap();
        fs::remove_file(folder.join("mod.json")).unwrap();
        fs::remove_file(disabled.join("resource.json")).unwrap();
        drop(loader);
        fs::remove_file(root.join("empty.img")).unwrap();
        fs::remove_dir(folder).unwrap();
        fs::remove_dir(disabled).unwrap();
        fs::remove_dir(root.join("mods/[maps]")).unwrap();
        for name in ["ped.dff", "ped.ifp", "resource.json"] {
            fs::remove_file(extra.join(name)).unwrap();
        }
        fs::remove_dir(extra).unwrap();
        fs::remove_dir(root.join("mods")).unwrap();
        fs::remove_dir(root).unwrap();
    }
    #[test]
    fn mta_map_places_known_objects_and_removes_world_objects() {
        let root = std::env::temp_dir().join(format!("sa-mta-map-{}", std::process::id()));
        let folder = root.join("mods/[maps]/stunt");
        fs::create_dir_all(&folder).unwrap();
        fs::write(root.join("empty.img"), b"VER2\0\0\0\0").unwrap();
        fs::write(
            folder.join("stunt.map"),
            br#"<map>
<object id="a" model="1634" interior="0" posX="1" posY="2" posZ="3" rotZ="90"/>
<object id="b" model="99999" posX="0" posY="0" posZ="0"/>
<removeWorldObject id="r" radius="5" model="17699" lodModel="17979" posX="10" posY="20" posZ="30"/>
</map>"#,
        )
        .unwrap();
        fs::write(
            folder.join("mod.json"),
            br#"{"enabled":true,"name":"Stunt","mta_maps":["stunt.map"]}"#,
        )
        .unwrap();
        let park = root.join("mods/[maps]/park");
        fs::create_dir_all(park.join("maps")).unwrap();
        fs::write(
            park.join("meta.xml"),
            br#"<meta><info name="Park" type="map"/><map src="maps/park.map"/><script src="c.lua" type="client"/></meta>"#,
        )
        .unwrap();
        fs::write(
            park.join("maps/park.map"),
            br#"<map><object model="1634" posX="7" posY="8" posZ="9" scale="2"/><spawnpoint posX="7" posY="8" posZ="12"/></map>"#,
        )
        .unwrap();
        fs::write(
            park.join("room.dff"),
            include_bytes!("../../../../mods/native-room-demo/room.dff"),
        )
        .unwrap();
        fs::write(
            park.join("c.lua"),
            b"local d = engineLoadDFF('room.dff')\nengineReplaceModel(d, 1634)\nengineReplaceModel(engineLoadDFF('missing.dff'), 1490)",
        )
        .unwrap();
        let definition = || Definition {
            model: "m".into(),
            txd: "m".into(),
        };
        let mut loader = WorldLoader {
            water: std::sync::Arc::new(crate::water::WaterMap::default()),
            water_texture: None,
            roadsign_font: None,
            uv_tracks: HashMap::new(),
            texture_parents: HashMap::new(),
            img: WorldArchive {
                exterior: Img::open(&root.join("empty.img")).unwrap(),
                interior: None,
            },
            defs: [
                (1634, definition()),
                (17699, definition()),
                (17979, definition()),
            ]
            .into(),
            rows: vec![
                placement(17699, 0, [12.0, 20.0, 30.0], [0.0, 0.0, 0.0, 1.0]).unwrap(),
                placement(17979, 0, [10.0, 20.0, 30.0], [0.0, 0.0, 0.0, 1.0]).unwrap(),
                placement(17699, 0, [100.0, 20.0, 30.0], [0.0, 0.0, 0.0, 1.0]).unwrap(),
            ],
            collision_models: HashMap::new(),
            resources: Resources::default(),
        };
        loader.enable_mods(&root.join("mods")).unwrap();
        let rows: Vec<_> = loader.rows.iter().map(|r| (r.id, r.pos)).collect();
        assert_eq!(
            rows,
            vec![
                (17699, [100.0, 20.0, 30.0]),
                (1634, [7.0, 8.0, 9.0]),
                (1634, [1.0, 2.0, 3.0])
            ]
        );
        assert!(loader.mod_names().contains(&"[enabled] Park".to_owned()));
        assert_eq!(loader.defs[&1634].model, "mod_park_1634");
        assert!(
            !loader.defs.contains_key(&1490),
            "a missing script model is skipped"
        );
        assert_eq!(loader.rows[1].scale, 2.0);
        assert_eq!(
            loader.map_spawns(),
            [("Park 1".to_owned(), [7.0, 8.0, 12.0])]
        );
        let shared = share_resources(&root.join("mods")).unwrap();
        let files: Vec<_> = shared
            .manifest
            .resources
            .iter()
            .flat_map(|r| r.files.iter().map(|f| f.path.as_str()))
            .collect();
        assert!(files.contains(&"stunt.map") && files.contains(&"maps/park.map"));
        assert!(files.contains(&"room.dff") && !files.contains(&"missing.dff"));
        assert!(!files.contains(&"meta.xml"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rejects_parent_path_escape() {
        let root = std::env::temp_dir().join(format!("sa-mod-containment-{}", std::process::id()));
        fs::create_dir_all(root.join("resource")).unwrap();
        fs::write(root.join("outside.bin"), b"private").unwrap();
        let folder = root.join("resource").canonicalize().unwrap();
        assert!(resource(&folder, "../outside.bin").is_err());
        fs::remove_file(root.join("outside.bin")).unwrap();
        fs::remove_dir(root.join("resource")).unwrap();
        fs::remove_dir(root).unwrap();
    }
    #[test]
    fn png_rgba_round_trip() {
        let mut data = Vec::new();
        {
            let mut encoder = png::Encoder::new(&mut data, 1, 1);
            encoder.set_color(png::ColorType::Rgba);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[10, 20, 30, 128])
                .unwrap();
        }
        let image = png_texture(&data).unwrap();
        assert_eq!(image.rgba, [10, 20, 30, 128]);
        assert!(image.has_alpha);
    }
}
