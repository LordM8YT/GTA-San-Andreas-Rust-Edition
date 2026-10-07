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
    player: Option<PlayerModel>,
    #[serde(default)]
    placements: Vec<Instance>,
    #[serde(default)]
    exclude_model_ids: Vec<i32>,
    #[serde(default)]
    texture_overrides: HashMap<String, String>,
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
struct Vehicle {
    dff: String,
    #[serde(default)]
    txd: Option<String>,
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
#[derive(Default)]
pub(super) struct Resources {
    pub geometry: HashMap<String, Vec<Geometry>>,
    pub dictionaries: HashMap<String, Vec<u8>>,
    pub textures: HashMap<String, Texture>,
    pub excluded: HashSet<i32>,
    pub names: Vec<String>,
    pub vehicle: Option<Scene>,
    pub player: Option<ped::Source>,
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
    pub fn enable_mods(&mut self, directory: &Path) -> Result<()> {
        if !directory.exists() {
            return Ok(());
        }
        let root = directory.canonicalize()?;
        let mut folders: Vec<_> = fs::read_dir(&root)?.collect::<std::io::Result<Vec<_>>>()?;
        folders.sort_by_key(|entry| entry.file_name());
        ensure!(folders.len() <= 128, "too many mod resources");
        for entry in folders {
            if !entry.file_type()?.is_dir() {
                continue;
            }
            let folder = entry.path().canonicalize()?;
            ensure!(folder.starts_with(&root), "mod directory escapes root");
            if !folder.join("mod.json").is_file() {
                continue;
            }
            let manifest: Manifest = serde_json::from_slice(&resource(&folder, "mod.json")?)
                .with_context(|| format!("mod manifest {}", folder.display()))?;
            let folder_name = entry.file_name().to_string_lossy().into_owned();
            let name = if manifest.name.is_empty() {
                folder_name
            } else {
                manifest.name.clone()
            };
            if !manifest.enabled {
                self.resources.names.push(format!("[disabled] {name}"));
                continue;
            }
            ensure!(
                manifest.models.len() <= 256 && manifest.placements.len() <= 2000,
                "mod model/placement budget"
            );
            ensure!(
                manifest.vehicles.len() + usize::from(self.resources.vehicle.is_some()) <= 1,
                "only one custom drivable vehicle is supported; disable other vehicle resources"
            );
            for vehicle in manifest.vehicles {
                let geometry = sa_assets::decode_vehicle_dff(&resource(&folder, &vehicle.dff)?)
                    .with_context(|| format!("custom vehicle {}", vehicle.dff))?;
                let dictionary = vehicle
                    .txd
                    .as_deref()
                    .map(|path| resource(&folder, path))
                    .transpose()?;
                let scene = car_scene(&geometry, |name| {
                    decode_txd(
                        dictionary
                            .as_deref()
                            .context("textured custom vehicle requires a TXD")?,
                        name,
                    )
                })?;
                ensure!(scene.triangles > 0, "empty custom vehicle");
                self.resources.vehicle = Some(scene);
            }
            if let Some(player) = manifest.player {
                ensure!(
                    self.resources.player.is_none(),
                    "only one custom player resource is supported"
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
                self.resources.player = Some(source);
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
                let key = format!(
                    "mod_{}_{}",
                    entry.file_name().to_string_lossy().to_ascii_lowercase(),
                    model.id
                );
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
            let vehicle_bytes = self.resources.vehicle.as_ref().map_or(0, |scene| {
                scene
                    .batches
                    .iter()
                    .map(|b| b.vertices.len() * std::mem::size_of::<Vertex>())
                    .sum::<usize>()
                    + scene.textures.values().map(|t| t.rgba.len()).sum::<usize>()
            });
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
                    <= 256 * 1024 * 1024,
                "total mod memory budget (256 MiB)"
            );
            eprintln!("Enabled resource: {name}");
            self.resources.names.push(format!("[enabled] {name}"));
        }
        Ok(())
    }
    pub fn mod_names(&self) -> &[String] {
        &self.resources.names
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
    fn custom_room_loads_and_player_can_enter() {
        let root = std::env::temp_dir().join(format!("sa-native-room-{}", std::process::id()));
        let folder = root.join("mods/room");
        let disabled = root.join("mods/disabled-room");
        fs::create_dir_all(&folder).unwrap();
        fs::create_dir_all(&disabled).unwrap();
        fs::write(
            disabled.join("mod.json"),
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
        manifest["player"] =
            serde_json::json!({"dff":"ped.dff","ifp":"ped.ifp","clothes":[{"dff":"clothes.dff"}]});
        fs::write(
            folder.join("mod.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        loader.enable_mods(&root.join("mods")).unwrap();
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
        fs::remove_file(disabled.join("mod.json")).unwrap();
        drop(loader);
        fs::remove_file(root.join("empty.img")).unwrap();
        fs::remove_dir(folder).unwrap();
        fs::remove_dir(disabled).unwrap();
        fs::remove_dir(root.join("mods")).unwrap();
        fs::remove_dir(root).unwrap();
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
