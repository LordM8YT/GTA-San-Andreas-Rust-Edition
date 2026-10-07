//! Manifest-ordered SA world placement and small bounded scene assembly.
pub mod collision;
mod mods;
pub mod ped;
mod texture;
pub mod vehicle;
pub mod water;
use anyhow::{ensure, Context, Result};
use sa_assets::{decode_dff, decode_txd, Geometry, Img, Texture};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone)]
struct Definition {
    model: String,
    txd: String,
}
#[derive(Clone)]
struct Placement {
    id: i32,
    pos: [f32; 3],
    quat: [f32; 4],
    interior: i32,
    lod: i32,
    is_lod: bool,
}
#[derive(Clone, Copy, Default)]
pub struct Vertex {
    pub position: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
}
pub struct Batch {
    pub key: String,
    pub vertices: Vec<Vertex>,
    pub alpha: bool,
    pub animated: bool,
}
pub struct Scene {
    pub water: Option<std::sync::Arc<water::WaterMap>>,
    pub batches: Vec<Batch>,
    pub textures: HashMap<String, Texture>,
    pub placements: usize,
    pub triangles: usize,
    pub animation: Option<sa_script::CutAnimation>,
    pub collision: Option<collision::CollisionWorld>,
}

pub struct RadarTile {
    pub index: usize,
    pub texture: Texture,
}

/// Decode the original 12x12 radar texture grid from the game installation.
pub fn load_radar_tiles(game: &Path) -> Result<Vec<RadarTile>> {
    let mut archive = Img::open(&sa_assets::game_path::resolve(game, "models/gta3.img")?)?;
    (0..144)
        .map(|index| {
            let name = format!("radar{index:02}");
            let dictionary = archive.read(&format!("{name}.txd"))?;
            Ok(RadarTile {
                index,
                texture: decode_txd(&dictionary, &name)
                    .with_context(|| format!("decoding original radar tile {name}"))?,
            })
        })
        .collect()
}

fn lines(path: &Path) -> Result<Vec<String>> {
    let data = fs::read(path)?;
    ensure!(
        data.len() <= 16 * 1024 * 1024,
        "metadata file exceeds 16 MiB"
    );
    Ok(String::from_utf8_lossy(&data)
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}
fn registered(root: &Path, kind: &str, extension: &str) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for manifest in ["default.dat", "gta.dat"] {
        let path = sa_assets::game_path::resolve(root, &format!("data/{manifest}"))?;
        ensure!(!path.is_symlink(), "symlink manifest rejected");
        for line in lines(&path)? {
            let mut parts = line.split_whitespace();
            let (Some(k), Some(relative)) = (parts.next(), parts.next()) else {
                continue;
            };
            if !k.eq_ignore_ascii_case(kind) || !relative.to_ascii_lowercase().ends_with(extension)
            {
                continue;
            }
            let relative = relative.replace('\\', "/");
            let path = sa_assets::game_path::resolve(root, &relative)?;
            let resolved = path
                .canonicalize()
                .with_context(|| format!("registered {kind} absent: {}", path.display()))?;
            ensure!(
                resolved.starts_with(root) && !path.is_symlink(),
                "registered path escapes installation"
            );
            out.push(resolved);
        }
    }
    Ok(out)
}
fn definitions(root: &Path) -> Result<HashMap<i32, Definition>> {
    let mut out = HashMap::new();
    for path in registered(root, "IDE", ".ide")? {
        let mut section = String::new();
        for line in lines(&path)? {
            if !line.contains(',') {
                section = if line.eq_ignore_ascii_case("end") {
                    String::new()
                } else {
                    line.to_ascii_lowercase()
                };
                continue;
            }
            if section != "objs" && section != "tobj" && section != "anim" {
                continue;
            }
            let fields: Vec<_> = line.split(',').map(str::trim).collect();
            if fields.len() < 3 {
                continue;
            }
            if let Ok(id) = fields[0].parse::<i32>() {
                out.insert(
                    id,
                    Definition {
                        model: fields[1].to_ascii_lowercase(),
                        txd: fields[2].to_ascii_lowercase(),
                    },
                );
            }
        }
    }
    Ok(out)
}
fn placement(id: i32, interior: i32, pos: [f32; 3], quat: [f32; 4]) -> Result<Placement> {
    ensure!(
        id >= 0 && interior >= 0 && pos.iter().chain(quat.iter()).all(|v| v.is_finite()),
        "invalid IPL instance"
    );
    let norm = quat.iter().map(|v| v * v).sum::<f32>();
    ensure!((0.98..=1.02).contains(&norm), "invalid IPL quaternion");
    Ok(Placement {
        id,
        interior: interior & 0xff,
        pos,
        quat,
        lod: -1,
        is_lod: false,
    })
}
fn binary_ipl(data: &[u8]) -> Result<Vec<Placement>> {
    ensure!(
        data.len() >= 32 && &data[..4] == b"bnry",
        "invalid binary IPL"
    );
    let count = u32::from_le_bytes(data[4..8].try_into()?) as usize;
    let offset = u32::from_le_bytes(data[28..32].try_into()?) as usize;
    ensure!(
        count <= 200_000
            && offset >= 32
            && offset
                .checked_add(count * 40)
                .is_some_and(|n| n <= data.len()),
        "binary IPL bounds"
    );
    let mut out = Vec::with_capacity(count);
    for row in data[offset..offset + count * 40].as_chunks::<40>().0.iter() {
        let f = |p: usize| f32::from_le_bytes(row[p..p + 4].try_into().unwrap());
        let i = |p: usize| i32::from_le_bytes(row[p..p + 4].try_into().unwrap());
        let mut row_placement = placement(
            i(28),
            i(32),
            [f(0), f(4), f(8)],
            [f(12), f(16), f(20), f(24)],
        )?;
        row_placement.lod = i(36);
        out.push(row_placement);
    }
    Ok(out)
}
struct WorldArchive {
    exterior: Img,
    interior: Option<Img>,
}
impl WorldArchive {
    fn has(&self, name: &str) -> bool {
        self.exterior.has(name) || self.interior.as_ref().is_some_and(|img| img.has(name))
    }
    fn names(&self) -> impl Iterator<Item = &String> {
        self.exterior.names().chain(
            self.interior
                .iter()
                .flat_map(|img| img.names())
                .filter(|name| !self.exterior.has(name)),
        )
    }
    fn read(&mut self, name: &str) -> Result<Vec<u8>> {
        if self.exterior.has(name) {
            self.exterior.read(name)
        } else {
            self.interior
                .as_mut()
                .context("interior archive unavailable")?
                .read(name)
        }
    }
}
fn placements(
    root: &Path,
    img: &mut WorldArchive,
    center: [f32; 2],
    radius: f32,
) -> Result<Vec<Placement>> {
    let mut out = Vec::new();
    let names: Vec<_> = img.names().cloned().collect();
    for path in registered(root, "IPL", ".ipl")? {
        let mut base = Vec::new();
        let mut children = Vec::new();
        let mut section = String::new();
        for line in lines(&path)? {
            if !line.contains(',') {
                section = if line.eq_ignore_ascii_case("end") {
                    String::new()
                } else {
                    line.to_ascii_lowercase()
                };
                continue;
            }
            if section != "inst" {
                continue;
            }
            let f: Vec<_> = line.split(',').map(str::trim).collect();
            ensure!(f.len() == 11, "unsupported text IPL record");
            let pos = [f[3].parse()?, f[4].parse()?, f[5].parse()?];
            let quat = [f[6].parse()?, f[7].parse()?, f[8].parse()?, f[9].parse()?];
            let mut row = placement(f[0].parse()?, f[2].parse()?, pos, quat)?;
            row.lod = f[10].parse()?;
            base.push(row);
        }
        let stem = path
            .file_stem()
            .context("IPL filename")?
            .to_string_lossy()
            .to_ascii_lowercase();
        let prefix = format!("{stem}_stream");
        let mut streams: Vec<_> = names
            .iter()
            .filter(|n| {
                n.starts_with(&prefix)
                    && n.ends_with(".ipl")
                    && n[prefix.len()..n.len() - 4]
                        .chars()
                        .all(|c| c.is_ascii_digit())
            })
            .collect();
        streams.sort();
        for stream in streams {
            children.extend(binary_ipl(&img.read(stream)?)?);
        }
        let parents: Vec<_> = base
            .iter()
            .chain(children.iter())
            .filter_map(|r| usize::try_from(r.lod).ok())
            .collect();
        for parent in parents {
            if let Some(row) = base.get_mut(parent) {
                row.is_lod = true;
            }
        }
        out.extend(base);
        out.extend(children);
    }
    Ok(out
        .into_iter()
        .filter(|r| {
            let x = r.pos[0] - center[0];
            let y = r.pos[1] - center[1];
            x * x + y * y <= radius * radius
        })
        .collect())
}
fn rotate(point: [f32; 3], q: [f32; 4]) -> [f32; 3] {
    let len = q.iter().map(|v| v * v).sum::<f32>().sqrt();
    let (x, y, z, w) = (-q[0] / len, -q[1] / len, -q[2] / len, q[3] / len);
    let t = [
        2.0 * (y * point[2] - z * point[1]),
        2.0 * (z * point[0] - x * point[2]),
        2.0 * (x * point[1] - y * point[0]),
    ];
    [
        point[0] + w * t[0] + y * t[2] - z * t[1],
        point[1] + w * t[1] + z * t[0] - x * t[2],
        point[2] + w * t[2] + x * t[1] - y * t[0],
    ]
}
fn place(point: [f32; 3], r: &Placement, center: [f32; 2]) -> [f32; 3] {
    let v = rotate(point, r.quat);
    [
        v[0] + r.pos[0] - center[0],
        v[2] + r.pos[2],
        -(v[1] + r.pos[1] - center[1]),
    ]
}
fn add_model(
    batches: &mut HashMap<String, Batch>,
    geometry: &[Geometry],
    r: &Placement,
    center: [f32; 2],
    txd: &str,
) -> Result<usize> {
    let mut triangles = 0;
    for g in geometry {
        for t in &g.triangles {
            let material = &g.materials[t[3] as usize];
            let key = material
                .texture
                .as_ref()
                .map(|n| format!("{txd}:{n}"))
                .unwrap_or_else(|| "runtime:white".into());
            let batch = batches.entry(key.clone()).or_insert_with(|| Batch {
                key,
                vertices: Vec::new(),
                alpha: false,
                animated: false,
            });
            for index in [t[0], t[1], t[2]] {
                let i = index as usize;
                let c = g.colors[i];
                let m = material.color;
                let color = std::array::from_fn(|j| c[j] as f32 * m[j] as f32 / 65025.0);
                batch.alpha |= color[3] < 1.0;
                batch.vertices.push(Vertex {
                    position: place(g.positions[i], r, center),
                    uv: g.uvs[i],
                    color,
                });
            }
            triangles += 1;
        }
    }
    Ok(triangles)
}
pub fn load_world(game: &Path, radius: f32) -> Result<Scene> {
    load_world_at(game, [2500.0, -1670.0], radius)
}
/// Reusable read-only world index. Streaming keeps a fixed origin so player
/// positions and collision do not jump when a new neighbourhood is installed.
pub struct WorldLoader {
    img: WorldArchive,
    defs: HashMap<i32, Definition>,
    rows: Vec<Placement>,
    collision_models: HashMap<String, sa_assets::col::CollisionModel>,
    resources: mods::Resources,
    texture_parents: HashMap<String, String>,
    water: std::sync::Arc<water::WaterMap>,
    water_texture: Option<Texture>,
}
pub const DISTANT_RADIUS: f32 = 2500.0;
/// Standalone taxi mesh; no script or cutscene timeline is loaded.
pub fn load_car(game: &Path) -> Result<Scene> {
    let mut main = Img::open(&sa_assets::game_path::resolve(game, "models/gta3.img")?)?;
    let geometry = sa_assets::decode_vehicle_dff(&main.read("taxi.dff")?)?;
    let taxi = main.read("taxi.txd")?;
    let common = fs::read(sa_assets::game_path::resolve(
        game,
        "models/generic/vehicle.txd",
    )?)?;
    car_scene(&geometry, |name| {
        decode_txd(&taxi, name).or_else(|_| decode_txd(&common, name))
    })
}
fn car_scene(
    geometry: &[Geometry],
    mut texture: impl FnMut(&str) -> Result<Texture>,
) -> Result<Scene> {
    let local = placement(0, 0, [0.0; 3], [0.0, 0.0, 0.0, 1.0])?;
    let mut batches = HashMap::new();
    let mut triangles = 0;
    // Separate opaque bodywork and glass even when they share a texture.
    for (index, g) in geometry.iter().enumerate() {
        for material_index in 0..g.materials.len() {
            let mut part = g.clone();
            part.triangles
                .retain(|t| usize::from(t[3]) == material_index);
            if part.triangles.is_empty() {
                continue;
            }
            let material = &mut part.materials[material_index];
            if material.color[..3] == [60, 255, 0] {
                material.color[..3].copy_from_slice(&[255, 205, 30]);
            }
            if material.texture.is_none() {
                material.texture = Some("runtime_white".into());
            }
            triangles += add_model(
                &mut batches,
                &[part],
                &local,
                [0.0; 2],
                &format!("car{index}_{material_index}"),
            )?;
        }
    }
    let mut textures = HashMap::new();
    for key in batches.keys() {
        let mut image = if key.ends_with(":runtime_white") {
            Texture {
                width: 1,
                height: 1,
                rgba: vec![255; 4],
                has_alpha: false,
            }
        } else {
            let (_, name) = key.split_once(':').context("car texture key")?;
            texture(name)?
        };
        if !batches[key].alpha {
            // Vehicle texture alpha also carries reflection/dirt masks; it
            // must not punch holes through opaque bodywork.
            for pixel in image.rgba.as_chunks_mut::<4>().0.iter_mut() {
                pixel[3] = 255;
            }
            image.has_alpha = false;
        }
        textures.insert(key.clone(), image);
    }
    let mut batches: Vec<_> = batches.into_values().collect();
    for batch in &mut batches {
        batch.alpha |= textures[&batch.key].needs_blending();
        batch.animated = true;
    }
    batches.sort_by_key(|b| b.alpha);
    Ok(Scene {
        water: None,
        batches,
        textures,
        placements: 1,
        triangles,
        animation: None,
        collision: None,
    })
}
fn visible_region(distance_squared: f32, is_lod: bool, radius: f32) -> bool {
    if is_lod {
        distance_squared > (radius + 100.0).powi(2) && distance_squared <= DISTANT_RADIUS.powi(2)
    } else {
        distance_squared <= (radius + 100.0).powi(2)
    }
}
impl WorldLoader {
    pub fn open(game: &Path) -> Result<Self> {
        let game = game.canonicalize()?;
        let archive = sa_assets::game_path::resolve(&game, "models/gta3.img")?;
        ensure!(!archive.is_symlink(), "symlink archive rejected");
        let interior = sa_assets::game_path::resolve(&game, "models/gta_int.img")?;
        ensure!(!interior.is_symlink(), "symlink interior archive rejected");
        let mut img = WorldArchive {
            exterior: Img::open(&archive)?,
            interior: if interior.exists() {
                Some(Img::open(&interior)?)
            } else {
                None
            },
        };
        let defs = definitions(&game)?;
        let water_file = sa_assets::game_path::resolve(&game, "data/water.dat")?;
        let water = std::sync::Arc::new(if water_file.exists() {
            water::WaterMap::parse(lines(&water_file)?)?
        } else {
            water::WaterMap::default()
        });
        let water_texture = if water_file.exists() {
            Some(decode_txd(
                &fs::read(sa_assets::game_path::resolve(&game, "models/particle.txd")?)?,
                "waterclear256",
            )?)
        } else {
            None
        };
        let mut texture_parents = HashMap::new();
        for path in registered(&game, "IDE", ".ide")? {
            texture::add_parents(lines(&path)?, &mut texture_parents);
        }
        let rows = placements(&game, &mut img, [0.0, 0.0], 20000.0)?;
        let mut collision_models = HashMap::new();
        let mut files: Vec<_> = img
            .names()
            .filter(|n| n.ends_with(".col"))
            .cloned()
            .collect();
        files.sort();
        for file in files {
            match sa_assets::col::decode_col(&img.read(&file)?) {
                Ok(models) => {
                    for model in models {
                        collision_models.insert(model.name.clone(), model);
                    }
                }
                Err(error) => eprintln!("Skipping collision archive {file}: {error:#}"),
            }
        }
        eprintln!(
            "Indexed {} original collision models",
            collision_models.len()
        );
        Ok(Self {
            img,
            defs,
            rows,
            collision_models,
            resources: mods::Resources::default(),
            texture_parents,
            water,
            water_texture,
        })
    }
    pub fn placement_count(&self) -> usize {
        self.rows.iter().filter(|r| r.interior == 0).count()
    }
    pub fn interior_placement_count(&self) -> usize {
        self.rows.iter().filter(|r| r.interior != 0).count()
    }
    pub fn load(&mut self, center: [f32; 2], origin: [f32; 2], radius: f32) -> Result<Scene> {
        self.load_area(center, origin, radius, 0)
    }
    /// Load one SA interior dimension, without exterior geometry or water.
    pub fn load_interior(
        &mut self,
        center: [f32; 2],
        origin: [f32; 2],
        radius: f32,
        interior: u8,
    ) -> Result<Scene> {
        ensure!(interior != 0, "interior dimension must be nonzero");
        self.load_area(center, origin, radius, i32::from(interior))
    }
    fn load_area(
        &mut self,
        center: [f32; 2],
        origin: [f32; 2],
        radius: f32,
        interior: i32,
    ) -> Result<Scene> {
        ensure!(
            (50.0..=800.0).contains(&radius)
                && center.iter().chain(origin.iter()).all(|v| v.is_finite()),
            "invalid streaming region"
        );
        let img = &mut self.img;
        let defs = &self.defs;
        // Padding includes large road/building meshes whose origin is outside
        // the visible neighbourhood. Geometry is never clipped at the edge.
        let rows: Vec<_> = self
            .rows
            .iter()
            .filter(|r| {
                if r.interior != interior {
                    return false;
                }
                let x = r.pos[0] - center[0];
                let y = r.pos[1] - center[1];
                let is_lod =
                    r.is_lod || defs.get(&r.id).is_some_and(|d| d.model.starts_with("lod"));
                let range = if is_lod { DISTANT_RADIUS } else { radius };
                x * x + y * y <= (range + 1600.0).powi(2)
            })
            .collect();
        let mut models: HashMap<String, Vec<Geometry>> = HashMap::new();
        let mut batches = HashMap::new();
        let mut physical = Batch {
            key: "collision".into(),
            vertices: Vec::new(),
            alpha: false,
            animated: false,
        };
        let mut fallback = HashMap::new();
        let mut count = 0;
        let mut lod_count = 0;
        let mut triangles = 0;
        for r in rows {
            if self.resources.excluded.contains(&r.id) {
                continue;
            }
            let Some(def) = defs.get(&r.id) else { continue };
            let is_lod = r.is_lod || def.model.starts_with("lod");
            if let Some(model) = self.collision_models.get(&def.model).filter(|_| !is_lod) {
                // Collision-only objects and transparent surfaces remain solid.
                let mut min = [f32::INFINITY; 2];
                let mut max = [f32::NEG_INFINITY; 2];
                for corner in 0..8 {
                    let point = std::array::from_fn(|a| {
                        if corner & (1 << a) == 0 {
                            model.min[a]
                        } else {
                            model.max[a]
                        }
                    });
                    let v = rotate(point, r.quat);
                    for a in 0..2 {
                        min[a] = min[a].min(v[a] + r.pos[a]);
                        max[a] = max[a].max(v[a] + r.pos[a]);
                    }
                }
                let dist: f32 = (0..2)
                    .map(|a| (center[a] - center[a].clamp(min[a], max[a])).powi(2))
                    .sum();
                if dist <= (radius + 100.0).powi(2) {
                    for face in &model.triangles {
                        for point in face {
                            physical.vertices.push(Vertex {
                                position: place(*point, r, origin),
                                ..Default::default()
                            });
                        }
                    }
                }
            }
            let dff = format!("{}.dff", def.model);
            let txd = format!("{}.txd", def.txd);
            if let Some(custom) = self.resources.geometry.get(&dff) {
                models.entry(dff.clone()).or_insert_with(|| custom.clone());
            } else if !img.has(&dff) || !img.has(&txd) {
                continue;
            }
            if !models.contains_key(&dff) {
                models.insert(
                    dff.clone(),
                    match decode_dff(&img.read(&dff)?) {
                        Ok(model) => model,
                        Err(error) => {
                            eprintln!("skipping unsupported {dff}: {error}");
                            continue;
                        }
                    },
                );
            }
            // Select by transformed mesh bounds, not the IPL origin. Rural
            // terrain meshes can span hundreds of metres from their origin.
            let mut min = [f32::INFINITY; 2];
            let mut max = [f32::NEG_INFINITY; 2];
            for geometry in &models[&dff] {
                for point in &geometry.positions {
                    let v = rotate(*point, r.quat);
                    for axis in 0..2 {
                        let value = v[axis] + r.pos[axis];
                        min[axis] = min[axis].min(value);
                        max[axis] = max[axis].max(value);
                    }
                }
            }
            let distance_squared: f32 = (0..2)
                .map(|axis| (center[axis] - center[axis].clamp(min[axis], max[axis])).powi(2))
                .sum();
            if !visible_region(distance_squared, is_lod, radius) {
                continue;
            }
            if !is_lod && !self.collision_models.contains_key(&def.model) {
                add_model(&mut fallback, &models[&dff], r, origin, &def.txd)?;
            }
            triangles += add_model(&mut batches, &models[&dff], r, origin, &def.txd)?;
            count += 1;
            lod_count += usize::from(is_lod);
            ensure!(
                count <= 30_000 && triangles <= 4_000_000,
                "streaming region exceeds geometry budget"
            );
        }
        ensure!(count > 0, "no detailed placements");
        let mut textures = HashMap::new();
        eprintln!("{lod_count} distant LOD placements included without collision");
        textures.insert(
            "runtime:white".into(),
            Texture {
                width: 1,
                height: 1,
                rgba: vec![255; 4],
                has_alpha: false,
            },
        );
        let mut fallback_textures = 0;
        let mut inherited_textures = 0;
        let mut dictionaries = HashMap::<String, Vec<u8>>::new();
        let mut dictionary_bytes = 0;
        for key in batches.keys() {
            if key == "runtime:white" {
                continue;
            }
            let (txd, tex) = key.split_once(':').context("invalid texture key")?;
            let file = format!("{txd}.txd");
            let texture = if let Some(image) = self.resources.textures.get(key) {
                image.clone()
            } else {
                let result = texture::resolve(txd, &self.texture_parents, |dictionary| {
                    let image_key = format!("{dictionary}:{tex}");
                    let image = if let Some(image) = self.resources.textures.get(&image_key) {
                        Ok(image.clone())
                    } else if let Some(bytes) = self.resources.dictionaries.get(dictionary) {
                        decode_txd(bytes, tex)
                    } else {
                        if !dictionaries.contains_key(dictionary) {
                            let bytes = img.read(&format!("{dictionary}.txd"))?;
                            if dictionary_bytes + bytes.len() > 64 * 1024 * 1024 {
                                dictionaries.clear();
                                dictionary_bytes = 0;
                            }
                            dictionary_bytes += bytes.len();
                            dictionaries.insert(dictionary.to_string(), bytes);
                        }
                        decode_txd(&dictionaries[dictionary], tex)
                    };
                    if image.is_ok() && dictionary != txd {
                        inherited_textures += 1;
                    }
                    image
                });
                match result {
                    Ok(texture) => texture,
                    Err(error) => {
                        fallback_textures += 1;
                        if fallback_textures <= 8 {
                            eprintln!("unsupported original texture {file}:{tex}: {error}");
                        }
                        Texture {
                            width: 1,
                            height: 1,
                            rgba: vec![190, 175, 160, 255],
                            has_alpha: false,
                        }
                    }
                }
            };
            textures.insert(key.clone(), texture);
        }
        if fallback_textures > 0 {
            eprintln!("{fallback_textures} map materials use fallback color");
        }
        if inherited_textures > 0 {
            eprintln!("{inherited_textures} textures resolved from declared TXD parents");
        }
        let mut batches: Vec<_> = batches.into_values().collect();
        batches.sort_by(|a, b| a.key.cmp(&b.key));
        for batch in &mut batches {
            batch.alpha |= textures[&batch.key].needs_blending();
        }
        let mut collision_batches: Vec<_> = fallback.into_values().collect();
        for batch in &mut collision_batches {
            batch.alpha |= textures[&batch.key].has_alpha;
        }
        collision_batches.push(physical);
        let collision = Some(collision::CollisionWorld::from_batches(&collision_batches));
        let water_batch = if interior == 0 {
            self.water.batch(center, origin, radius + 100.0)
        } else {
            Batch {
                key: "runtime:water".into(),
                vertices: Vec::new(),
                alpha: true,
                animated: false,
            }
        };
        if !water_batch.vertices.is_empty() {
            triangles += water_batch.vertices.len() / 3;
            textures.insert(
                "runtime:water".into(),
                self.water_texture
                    .clone()
                    .context("water texture missing")?,
            );
            batches.push(water_batch);
        }
        Ok(Scene {
            water: (interior == 0).then(|| self.water.clone()),
            batches,
            textures,
            placements: count,
            triangles,
            animation: None,
            collision,
        })
    }
}
pub fn load_world_at(game: &Path, center: [f32; 2], radius: f32) -> Result<Scene> {
    WorldLoader::open(game)?.load(center, center, radius)
}
pub fn load_first_model(game: &Path) -> Result<Scene> {
    let game = game.canonicalize()?;
    let mut img = Img::open(&sa_assets::game_path::resolve(&game, "models/gta3.img")?)?;
    let geometry = decode_dff(&img.read("cj_wastebin.dff")?)?;
    let r = Placement {
        id: 0,
        interior: 0,
        lod: -1,
        is_lod: false,
        pos: [2500.0, -1670.0, 0.0],
        quat: [0.0, 0.0, 0.0, 1.0],
    };
    let mut batches = HashMap::new();
    let triangles = add_model(&mut batches, &geometry, &r, [2500.0, -1670.0], "cj_bins")?;
    let mut textures = HashMap::new();
    for key in batches.keys() {
        let (_, tex) = key
            .split_once(':')
            .context("first model material untextured")?;
        textures.insert(key.clone(), decode_txd(&img.read("cj_bins.txd")?, tex)?);
    }
    let mut batches: Vec<_> = batches.into_values().collect();
    for batch in &mut batches {
        batch.alpha |= textures[&batch.key].needs_blending();
    }
    Ok(Scene {
        water: None,
        batches,
        textures,
        placements: 1,
        triangles,
        animation: None,
        collision: None,
    })
}
pub fn load_cuttest(game: &Path) -> Result<Scene> {
    let game = game.canonicalize()?;
    let animation = sa_script::load_cuttest_animation(&game)?;
    let mut img = Img::open(&sa_assets::game_path::resolve(
        &game,
        "models/cutscene.img",
    )?)?;
    let geometry = decode_dff(&img.read("csgoldrec.dff")?)?;
    let r = Placement {
        id: 0,
        interior: 0,
        lod: -1,
        is_lod: false,
        pos: [0.0, 0.0, 0.0],
        quat: [0.0, 0.0, 0.0, 1.0],
    };
    let mut batches = HashMap::new();
    let triangles = add_model(&mut batches, &geometry, &r, [0.0, 0.0], "csgoldrec")?;
    let mut textures = HashMap::new();
    for key in batches.keys() {
        let (_, tex) = key.split_once(':').context("cuttest material untextured")?;
        textures.insert(key.clone(), decode_txd(&img.read("csgoldrec.txd")?, tex)?);
    }
    let mut batches: Vec<_> = batches.into_values().collect();
    batches.sort_by(|a, b| a.key.cmp(&b.key));
    for batch in &mut batches {
        batch.alpha |= textures[&batch.key].needs_blending();
        batch.animated = true;
    }
    Ok(Scene {
        water: None,
        batches,
        textures,
        placements: 1,
        triangles,
        animation: Some(animation),
        collision: None,
    })
}

pub fn load_prologue(game: &Path) -> Result<Scene> {
    let game = game.canonicalize()?;
    let metadata = sa_script::inspect_cutscene(&game, "prolog1")?;
    let center = [metadata.offset[0], metadata.offset[1]];
    let mut world = load_world_at(&game, center, 180.0)?;
    let mut animation = sa_script::load_prologue_animation(&game)?;
    // The original cutscene is staged about a kilometre above its map X/Y.
    // Keep its relative camera and model motion, but stage it at local road level.
    let taxi_start = &animation.keys[0].translation;
    let ground = world
        .collision
        .as_ref()
        .and_then(|collision| {
            collision.ground_below(glam::Vec3::new(taxi_start[0], 100.0, -taxi_start[1]), 50.0)
        })
        .unwrap_or(10.0);
    animation.height_offset = ground - taxi_start[2] + 0.69;
    let mut img = Img::open(&sa_assets::game_path::resolve(
        &game,
        "models/cutscene.img",
    )?)?;
    let mut gta3 = Img::open(&sa_assets::game_path::resolve(&game, "models/gta3.img")?)?;
    let taxi_txd = gta3.read("taxi.txd")?;
    let vehicle_txd = fs::read(sa_assets::game_path::resolve(
        &game,
        "models/generic/vehicle.txd",
    )?)?;
    let geometry = decode_dff(&img.read("cstaxi92.dff")?)?;
    let placement = Placement {
        id: 0,
        interior: 0,
        lod: -1,
        is_lod: false,
        pos: [0.0; 3],
        quat: [0.0, 0.0, 0.0, 1.0],
    };
    let mut batches = HashMap::new();
    let triangles = add_model(&mut batches, &geometry, &placement, [0.0, 0.0], "cstaxi92")?;
    let mut textures = HashMap::new();
    for key in batches.keys() {
        if key == "runtime:white" {
            textures.insert(
                key.clone(),
                Texture {
                    width: 1,
                    height: 1,
                    rgba: vec![255; 4],
                    has_alpha: false,
                },
            );
        } else {
            let (_, tex) = key.split_once(':').context("prologue material key")?;
            textures.insert(
                key.clone(),
                decode_txd(&taxi_txd, tex)
                    .or_else(|_| decode_txd(&vehicle_txd, tex))
                    .with_context(|| format!("missing original taxi material {tex}"))?,
            );
        }
    }
    let mut batches: Vec<_> = batches.into_values().collect();
    batches.sort_by(|a, b| a.key.cmp(&b.key));
    for batch in &mut batches {
        batch.alpha |= textures[&batch.key].needs_blending();
        batch.animated = true;
        batch.key = format!("cutscene:{}", batch.key);
    }
    let textures: HashMap<_, _> = textures
        .into_iter()
        .map(|(key, texture)| (format!("cutscene:{key}"), texture))
        .collect();
    world.batches.extend(batches);
    world.textures.extend(textures);
    world.placements += 1;
    world.triangles += triangles;
    world.animation = Some(animation);
    world.collision = None;
    Ok(world)
}

#[cfg(test)]
mod streaming_tests {
    use super::*;
    #[test]
    fn distant_models_stay_outside_detail_region() {
        assert!(visible_region(400.0f32.powi(2), false, 400.0));
        assert!(!visible_region(400.0f32.powi(2), true, 400.0));
        assert!(visible_region(900.0f32.powi(2), true, 400.0));
        assert!(!visible_region(900.0f32.powi(2), false, 400.0));
        assert!(!visible_region(2600.0f32.powi(2), true, 400.0));
    }
    #[test]
    fn animated_map_objects_are_registered_as_geometry() {
        let folder = std::env::temp_dir().join(format!("sa-ide-animation-{}", std::process::id()));
        fs::create_dir_all(folder.join("data")).unwrap();
        fs::write(folder.join("data/default.dat"), b"").unwrap();
        fs::write(folder.join("data/gta.dat"), b"IDE data/world.ide\n").unwrap();
        fs::write(folder.join("data/world.ide"),b"objs\n100, road, streets, 100, 0\nend\ntobj\n101, lights, windows, 100, 0, 20, 6\nend\nanim\n102, Windmill, FarmStuff, windmill, 150, 0\nend\ntxdp\nfarmstuff, shared\nend\n").unwrap();
        let definitions = definitions(&folder.canonicalize().unwrap()).unwrap();
        assert_eq!(definitions.len(), 3);
        assert_eq!(definitions[&102].model, "windmill");
        assert_eq!(definitions[&102].txd, "farmstuff");
        for name in ["default.dat", "gta.dat", "world.ide"] {
            fs::remove_file(folder.join("data").join(name)).unwrap();
        }
        fs::remove_dir(folder.join("data")).unwrap();
        fs::remove_dir(folder).unwrap();
    }
    #[test]
    fn exterior_flags_are_not_interior_ids() {
        for flags in [0, 256, 512, 1024, 2048, 4096] {
            assert_eq!(
                placement(100, flags, [0.0; 3], [0.0, 0.0, 0.0, 1.0])
                    .unwrap()
                    .interior,
                0
            );
        }
        assert_eq!(
            placement(100, 1024 + 5, [0.0; 3], [0.0, 0.0, 0.0, 1.0])
                .unwrap()
                .interior,
            5
        );
    }
    #[test]
    fn world_archives_include_interior_assets_and_preserve_exterior_precedence() {
        let folder = std::env::temp_dir().join(format!("sa-world-archives-{}", std::process::id()));
        fs::create_dir_all(&folder).unwrap();
        for (file, unique, marker) in [("outer.img", "outer.dff", 1), ("inner.img", "inner.dff", 2)]
        {
            let mut bytes = vec![marker; 4096];
            bytes[..2048].fill(0);
            bytes[..4].copy_from_slice(b"VER2");
            bytes[4..8].copy_from_slice(&2u32.to_le_bytes());
            for (index, name) in ["common.txd", unique].iter().enumerate() {
                let row = 8 + index * 32;
                bytes[row..row + 4].copy_from_slice(&1u32.to_le_bytes());
                bytes[row + 4..row + 6].copy_from_slice(&1u16.to_le_bytes());
                bytes[row + 8..row + 8 + name.len()].copy_from_slice(name.as_bytes());
            }
            fs::write(folder.join(file), bytes).unwrap();
        }
        let mut archive = WorldArchive {
            exterior: Img::open(&folder.join("outer.img")).unwrap(),
            interior: Some(Img::open(&folder.join("inner.img")).unwrap()),
        };
        assert_eq!(archive.names().count(), 3);
        assert!(archive.has("inner.dff"));
        assert_eq!(archive.read("common.txd").unwrap()[0], 1);
        assert_eq!(archive.read("inner.dff").unwrap()[0], 2);
        assert!(archive.read("missing.dff").is_err());
        drop(archive);
        fs::remove_file(folder.join("outer.img")).unwrap();
        fs::remove_file(folder.join("inner.img")).unwrap();
        fs::remove_dir(folder).unwrap();
    }
}
