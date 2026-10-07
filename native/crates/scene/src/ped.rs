//! Animated pedestrian mesh; movement and collision are owned by the runtime.
use super::*;
struct Clothing {
    name: String,
    first: usize,
    count: usize,
    enabled: bool,
}
pub(super) struct Source {
    model: sa_assets::skin::Model,
    clips: Option<sa_assets::ifp::Package>,
    dictionary: Option<Vec<u8>>,
    clothing_textures: HashMap<String, Texture>,
    clothing_count: usize,
    clothing: Vec<Clothing>,
}
impl Source {
    pub fn decode(dff: &[u8], dictionary: Option<Vec<u8>>, ifp: Option<Vec<u8>>) -> Result<Self> {
        Ok(Self {
            model: sa_assets::skin::decode(dff)?,
            clips: ifp
                .map(|bytes| sa_assets::ifp::decode(&bytes))
                .transpose()?,
            dictionary,
            clothing_textures: HashMap::new(),
            clothing_count: 0,
            clothing: Vec::new(),
        })
    }
    pub fn attach_clothing(&mut self, dff: &[u8], dictionary: Option<Vec<u8>>) -> Result<()> {
        ensure!(self.clothing_count < 16, "player clothing limit (16)");
        let clothing = sa_assets::skin::decode(dff)?;
        let base_bind = &self.model.parts[0].skin.inverse_bind;
        let mut mapping = Vec::new();
        for frame in &clothing.bone_frames {
            let id = clothing.frames[*frame]
                .bone_id
                .context("clothing bone has no HAnim ID")?;
            let index = self
                .model
                .bone_frames
                .iter()
                .position(|f| self.model.frames[*f].bone_id == Some(id))
                .with_context(|| format!("clothing bone {id} is absent from player"))?;
            mapping.push(index);
        }
        let mut additions = Vec::new();
        let mut images = HashMap::new();
        for mut part in clothing.parts {
            for (old, new) in mapping.iter().enumerate() {
                ensure!(
                    part.skin.inverse_bind[old]
                        .iter()
                        .zip(&base_bind[*new])
                        .all(|(a, b)| (a - b).abs() < 0.01),
                    "clothing inverse bind differs from player skeleton"
                );
            }
            for (indices, weights) in part.skin.indices.iter_mut().zip(&part.skin.weights) {
                for i in 0..4 {
                    indices[i] = if weights[i] == 0.0 {
                        0
                    } else {
                        mapping[usize::from(indices[i])] as u8
                    };
                }
            }
            part.skin.inverse_bind = base_bind.clone();
            for material in &mut part.geometry.materials {
                if let Some(name) = &material.texture {
                    let key = format!("clothing{}_{}", self.clothing_count, name);
                    if !images.contains_key(&key) {
                        let image = decode_txd(
                            dictionary
                                .as_deref()
                                .context("textured clothing requires a TXD")?,
                            name,
                        )?;
                        images.insert(key.clone(), image);
                        let bytes = images
                            .values()
                            .chain(self.clothing_textures.values())
                            .map(|t| t.rgba.len())
                            .sum::<usize>();
                        ensure!(bytes <= 128 * 1024 * 1024, "clothing texture memory budget");
                    }
                    material.texture = Some(key);
                }
            }
            additions.push(part);
        }
        ensure!(
            self.model.parts.len() + additions.len() <= 256,
            "player geometry part limit"
        );
        self.clothing.push(Clothing {
            name: format!("Plagg {}", self.clothing_count + 1),
            first: self.model.parts.len(),
            count: additions.len(),
            enabled: true,
        });
        self.model.parts.extend(additions);
        self.clothing_textures.extend(images);
        self.clothing_count += 1;
        ensure!(
            self.memory_bytes() <= 256 * 1024 * 1024,
            "player source memory budget"
        );
        Ok(())
    }
    pub fn configure_last_clothing(&mut self, name: String, enabled: bool) {
        if let Some(clothing) = self.clothing.last_mut() {
            if !name.is_empty() {
                clothing.name = name;
            }
            clothing.enabled = enabled;
        }
    }
    pub fn memory_bytes(&self) -> usize {
        self.model
            .parts
            .iter()
            .map(|p| {
                p.geometry.positions.len() * 64
                    + p.geometry.triangles.len() * 8
                    + p.skin.inverse_bind.len() * 48
            })
            .sum::<usize>()
            + self.dictionary.as_ref().map_or(0, Vec::len)
            + self
                .clothing_textures
                .values()
                .map(|t| t.rgba.len())
                .sum::<usize>()
            + self.clips.as_ref().map_or(0, |p| {
                p.clips
                    .iter()
                    .flat_map(|c| &c.tracks)
                    .map(|t| t.keys.len() * std::mem::size_of::<sa_assets::ifp::Key>())
                    .sum()
            })
    }
}
pub struct Ped {
    model: sa_assets::skin::Model,
    clips: sa_assets::ifp::Package,
    height_offset: f32,
    textures: HashMap<String, Texture>,
    clothing: Vec<Clothing>,
}
impl Ped {
    pub fn load(game: &Path) -> Result<Self> {
        Self::load_model(game, "fam1")
    }
    pub fn load_model(game: &Path, model: &str) -> Result<Self> {
        ensure!(
            model.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'),
            "invalid ped model name"
        );
        let mut archive = Img::open(&sa_assets::game_path::resolve(game, "models/gta3.img")?)?;
        Self::from_source(
            Source::decode(
                &archive.read(&format!("{model}.dff"))?,
                Some(archive.read(&format!("{model}.txd"))?),
                None,
            )?,
            game,
        )
    }
    pub(super) fn from_source(source: Source, game: &Path) -> Result<Self> {
        let clips = match source.clips {
            Some(clips) => clips,
            None => sa_assets::ifp::decode(&fs::read(sa_assets::game_path::resolve(
                game,
                "anim/ped.ifp",
            )?)?)?,
        };
        let mut ped = Self {
            model: source.model,
            clips,
            height_offset: 0.0,
            textures: HashMap::new(),
            clothing: source.clothing,
        };
        for name in ["idle_stance", "walk_player", "run_player"] {
            let clip = ped
                .clips
                .clips
                .iter()
                .find(|c| c.name == name)
                .with_context(|| format!("missing player clip {name}"))?;
            for part in &ped.model.parts {
                for (indices, weights) in part.skin.indices.iter().zip(&part.skin.weights) {
                    for i in 0..4 {
                        if weights[i] > 0.0 {
                            let frame =
                                &ped.model.frames[ped.model.bone_frames[usize::from(indices[i])]];
                            ensure!(
                                clip.tracks.iter().any(|t| Some(t.bone_id) == frame.bone_id),
                                "weighted player bone missing from clip {name}"
                            );
                        }
                    }
                }
            }
        }
        let batches = ped.frame("idle_stance", 0.0)?;
        let low = batches
            .iter()
            .flat_map(|b| &b.vertices)
            .map(|v| v.position[1])
            .fold(f32::INFINITY, f32::min);
        let high = batches
            .iter()
            .flat_map(|b| &b.vertices)
            .map(|v| v.position[1])
            .fold(f32::NEG_INFINITY, f32::max);
        ensure!(
            (1.0..=2.5).contains(&(high - low)),
            "animated ped is not upright: height {}",
            high - low
        );
        ped.height_offset = -low;
        let mut texture_bytes = 0usize;
        for batch in &batches {
            let image = if batch.key == "runtime:white" {
                Texture {
                    width: 1,
                    height: 1,
                    rgba: vec![255; 4],
                    has_alpha: false,
                }
            } else {
                let (_, name) = batch.key.split_once(':').context("ped material key")?;
                if let Some(image) = source.clothing_textures.get(name).cloned() {
                    image
                } else {
                    decode_txd(
                        source
                            .dictionary
                            .as_deref()
                            .context("textured player DFF requires a TXD")?,
                        name,
                    )?
                }
            };
            texture_bytes += image.rgba.len();
            ensure!(
                texture_bytes <= 128 * 1024 * 1024,
                "player texture memory budget (128 MiB)"
            );
            ped.textures.insert(batch.key.clone(), image);
        }
        Ok(ped)
    }
    pub fn clothing_options(&self) -> Vec<(String, bool)> {
        self.clothing
            .iter()
            .map(|c| (c.name.clone(), c.enabled))
            .collect()
    }
    pub fn set_clothing(&mut self, index: usize, enabled: bool) -> Result<()> {
        self.clothing
            .get_mut(index)
            .context("unknown clothing selection")?
            .enabled = enabled;
        Ok(())
    }
    pub fn frame(&self, name: &str, seconds: f32) -> Result<Vec<Batch>> {
        let clip = self
            .clips
            .clips
            .iter()
            .find(|c| c.name == name)
            .context("missing ped clip")?;
        let locals = clip.pose(&self.model, seconds, true)?;
        let geometry = self.model.pose(&locals)?;
        let placement = placement(0, 0, [0.0, 0.0, self.height_offset], [0.0, 0.0, 0.0, 1.0])?;
        let mut batches = HashMap::new();
        for (index, part) in geometry.into_iter().enumerate() {
            let hidden = self
                .clothing
                .iter()
                .any(|c| !c.enabled && (c.first..c.first + c.count).contains(&index));
            let counts: HashMap<_, _> = if hidden {
                batches
                    .iter()
                    .map(|(key, batch): (&String, &Batch)| (key.clone(), batch.vertices.len()))
                    .collect()
            } else {
                HashMap::new()
            };
            add_model(
                &mut batches,
                &[part],
                &placement,
                [0.0; 2],
                &format!("ped{index}"),
            )?;
            if hidden {
                for (key, batch) in &mut batches {
                    for vertex in &mut batch.vertices[counts.get(key).copied().unwrap_or(0)..] {
                        vertex.color[3] = 0.0;
                    }
                }
            }
        }
        let mut batches: Vec<_> = batches.into_values().collect();
        batches.sort_by(|a, b| a.key.cmp(&b.key));
        for batch in &mut batches {
            batch.animated = true;
        }
        Ok(batches)
    }
    pub fn scene(&self) -> Result<Scene> {
        let mut batches = self.frame("idle_stance", 0.0)?;
        for batch in &mut batches {
            batch.alpha |= self.textures[&batch.key].needs_blending();
        }
        let triangles = batches.iter().map(|b| b.vertices.len() / 3).sum();
        Ok(Scene {
            water: None,
            batches,
            textures: self.textures.clone(),
            placements: 1,
            triangles,
            animation: None,
            collision: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn source() -> Source {
        Source::decode(
            include_bytes!("../../../../mods/native-ped-demo/ped.dff"),
            None,
            Some(include_bytes!("../../../../mods/native-ped-demo/ped.ifp").to_vec()),
        )
        .unwrap()
    }
    #[test]
    fn own_custom_player_loads_and_animates_without_game_assets() {
        let ped = Ped::from_source(source(), Path::new("not-an-installation")).unwrap();
        let scene = ped.scene().unwrap();
        assert_eq!(scene.triangles, 120);
        let first = ped.frame("walk_player", 0.0).unwrap();
        let after = ped.frame("walk_player", 0.3).unwrap();
        assert_eq!(first.len(), after.len());
        assert!(first
            .iter()
            .zip(&after)
            .flat_map(|(a, b)| a.vertices.iter().zip(&b.vertices))
            .any(|(a, b)| glam::Vec3::from_array(a.position)
                .distance(glam::Vec3::from_array(b.position))
                > 0.1));
        assert!(scene
            .batches
            .iter()
            .flat_map(|b| &b.vertices)
            .all(|v| v.position[1] >= -0.001));
    }
    #[test]
    fn player_rejects_missing_weighted_bone_animation() {
        let mut source = source();
        source.clips.as_mut().unwrap().clips[0]
            .tracks
            .retain(|t| t.bone_id != 41);
        assert!(Ped::from_source(source, Path::new("not-an-installation")).is_err());
    }
    #[test]
    fn clothes_bind_by_id_and_follow_arm_motion() {
        let mut source = source();
        // Different palette orders must not bind garments to the wrong bone.
        source.model.bone_frames.swap(0, 1);
        for part in &mut source.model.parts {
            part.skin.inverse_bind.swap(0, 1);
            for indices in &mut part.skin.indices {
                for index in indices {
                    if *index < 2 {
                        *index = 1 - *index;
                    }
                }
            }
        }
        source
            .attach_clothing(
                include_bytes!("../../../../mods/native-clothing-demo/clothes.dff"),
                None,
            )
            .unwrap();
        let ped = Ped::from_source(source, Path::new("not-an-installation")).unwrap();
        assert_eq!(ped.scene().unwrap().triangles, 168);
        let a = ped.frame("walk_player", 0.0).unwrap();
        let b = ped.frame("walk_player", 0.3).unwrap();
        assert!(a
            .iter()
            .zip(&b)
            .flat_map(|(a, b)| a.vertices.iter().zip(&b.vertices))
            .any(|(a, b)| a.color[0] > 0.7
                && a.color[1] < 0.3
                && glam::Vec3::from_array(a.position)
                    .distance(glam::Vec3::from_array(b.position))
                    > 0.1));
    }
    #[test]
    fn clothes_reject_incompatible_bind_pose_before_adding_parts() {
        let mut source = source();
        let count = source.model.parts.len();
        source.model.parts[0].skin.inverse_bind[2][9] += 0.1;
        assert!(source
            .attach_clothing(
                include_bytes!("../../../../mods/native-clothing-demo/clothes.dff"),
                None
            )
            .is_err());
        assert_eq!(source.model.parts.len(), count);
    }
    #[test]
    fn wardrobe_hides_only_clothing_and_keeps_gpu_layout_stable() {
        let mut source = source();
        source
            .attach_clothing(
                include_bytes!("../../../../mods/native-clothing-demo/clothes.dff"),
                None,
            )
            .unwrap();
        source.configure_last_clothing("Jacket".into(), true);
        let mut ped = Ped::from_source(source, Path::new("not-an-installation")).unwrap();
        let shown = ped.frame("idle_stance", 0.0).unwrap();
        ped.set_clothing(0, false).unwrap();
        let hidden = ped.frame("idle_stance", 0.0).unwrap();
        assert_eq!(ped.clothing_options(), vec![("Jacket".into(), false)]);
        for (a, b) in shown.iter().zip(&hidden) {
            assert_eq!(a.key, b.key);
            assert_eq!(a.vertices.len(), b.vertices.len());
            assert_eq!(a.alpha, b.alpha);
        }
        assert_eq!(
            hidden
                .iter()
                .flat_map(|b| &b.vertices)
                .filter(|v| v.color[3] == 0.0)
                .count(),
            48 * 3
        );
        assert!(hidden
            .iter()
            .flat_map(|b| &b.vertices)
            .any(|v| v.color[3] > 0.0));
        ped.set_clothing(0, true).unwrap();
        assert!(ped
            .frame("idle_stance", 0.0)
            .unwrap()
            .iter()
            .flat_map(|b| &b.vertices)
            .all(|v| v.color[3] > 0.0));
        assert!(ped.set_clothing(1, true).is_err());
    }
}
