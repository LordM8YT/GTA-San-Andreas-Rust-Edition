//! PC RenderWare skin weights, inverse bind matrices and HAnim frame mapping.
//! Matrix columns follow the same convention as the rigid DFF decoder.
use super::*;

pub type Matrix = [f32; 12];
#[derive(Clone)]
pub struct Frame {
    pub local: Matrix,
    pub parent: Option<usize>,
    pub bone_id: Option<i32>,
}
pub struct Skin {
    pub indices: Vec<[u8; 4]>,
    pub weights: Vec<[f32; 4]>,
    pub inverse_bind: Vec<Matrix>,
}
pub struct Part {
    pub geometry: Geometry,
    pub skin: Skin,
}
pub struct Model {
    pub frames: Vec<Frame>,
    /// Skin matrix index to frame index, identified by HAnim bone IDs.
    pub bone_frames: Vec<usize>,
    pub parts: Vec<Part>,
}
fn decode_skin(data: &[u8], vertices: usize) -> Result<Skin> {
    let header = slice(data, 0, 4)?;
    let bones = usize::from(header[0]);
    let used = usize::from(header[1]);
    ensure!(
        bones > 0 && used <= bones && header[2] <= 4,
        "invalid skin bone counts"
    );
    let old = used == 0;
    let used_indices = slice(data, 4, used)?;
    ensure!(
        used_indices.iter().all(|i| usize::from(*i) < bones),
        "invalid used skin bone"
    );
    let mut offset = 4 + used;
    let mut indices: Vec<[u8; 4]> = Vec::with_capacity(vertices);
    for _ in 0..vertices {
        indices.push(slice(data, offset, 4)?.try_into()?);
        offset += 4;
    }
    let mut weights = Vec::with_capacity(vertices);
    for index in &indices {
        let mut weight = [0.0; 4];
        for i in 0..4 {
            weight[i] = f32at(data, offset)?;
            offset += 4;
            ensure!((0.0..=1.001).contains(&weight[i]), "invalid skin weight");
            ensure!(
                weight[i] == 0.0 || usize::from(index[i]) < bones,
                "invalid weighted bone index"
            );
        }
        let sum: f32 = weight.iter().sum();
        ensure!(
            (0.99..=1.01).contains(&sum),
            "skin weights do not sum to one"
        );
        for value in &mut weight {
            *value /= sum;
        }
        weights.push(weight);
    }
    let mut inverse_bind = Vec::with_capacity(bones);
    for _ in 0..bones {
        if old {
            slice(data, offset, 4)?;
            offset += 4;
        }
        let mut matrix = [0.0; 12];
        for (out, source) in [0, 1, 2, 4, 5, 6, 8, 9, 10, 12, 13, 14]
            .into_iter()
            .enumerate()
        {
            matrix[out] = f32at(data, offset + source * 4)?;
        }
        slice(data, offset, 64)?;
        offset += 64;
        inverse_bind.push(matrix);
    }
    if !old {
        ensure!(
            slice(data, offset, 12)?.iter().all(|v| *v == 0),
            "split skin palettes unsupported"
        );
        offset += 12;
    }
    ensure!(offset == data.len(), "unsupported skin tail");
    Ok(Skin {
        indices,
        weights,
        inverse_bind,
    })
}
pub fn decode(data: &[u8]) -> Result<Model> {
    let clump = root(data, 16)?;
    let frame_list = one(clump, 14)?;
    let structure = one(frame_list, 1)?;
    let count = u32at(structure, 0)? as usize;
    ensure!(
        (1..=256).contains(&count) && structure.len() == 4 + count * 56,
        "invalid skin frame list"
    );
    let extensions: Vec<_> = chunks(frame_list)?
        .into_iter()
        .filter(|c| c.tag == 3)
        .collect();
    ensure!(extensions.len() == count, "missing skin frame extensions");
    let mut frames = Vec::with_capacity(count);
    let mut palette = None;
    for (i, extension) in extensions.iter().enumerate() {
        let offset = 4 + i * 56;
        let mut local = [0.0; 12];
        for (j, value) in local.iter_mut().enumerate() {
            *value = f32at(structure, offset + j * 4)?;
        }
        let parent = i32at(structure, offset + 48)?;
        ensure!(
            parent >= -1 && parent < count as i32 && parent != i as i32,
            "invalid skin frame parent"
        );
        let anims: Vec<_> = chunks(extension.body)?
            .into_iter()
            .filter(|c| c.tag == 0x11e)
            .collect();
        ensure!(anims.len() <= 1, "duplicate HAnim plugin");
        let mut bone_id = None;
        if let Some(anim) = anims.first() {
            bone_id = Some(i32at(anim.body, 4)?);
            let bones = u32at(anim.body, 8)? as usize;
            ensure!(bones <= 255, "HAnim bone budget");
            if bones > 0 {
                ensure!(
                    palette.is_none() && anim.body.len() == 20 + bones * 12,
                    "invalid HAnim palette"
                );
                let mut entries = vec![None; bones];
                for b in 0..bones {
                    let id = i32at(anim.body, 20 + b * 12)?;
                    let index = u32at(anim.body, 24 + b * 12)? as usize;
                    ensure!(
                        index < bones && entries[index].is_none(),
                        "invalid HAnim palette index"
                    );
                    entries[index] = Some(id);
                }
                palette = Some(entries);
            } else {
                ensure!(anim.body.len() == 12, "invalid HAnim leaf");
            }
        }
        frames.push(Frame {
            local,
            parent: (parent >= 0).then_some(parent as usize),
            bone_id,
        });
    }
    let mut bone_frames = Vec::new();
    for id in palette.context("missing HAnim hierarchy")? {
        let id = id.context("missing HAnim palette entry")?;
        let matches: Vec<_> = frames
            .iter()
            .enumerate()
            .filter(|(_, f)| f.bone_id == Some(id))
            .collect();
        ensure!(matches.len() == 1, "missing or duplicated bone frame {id}");
        bone_frames.push(matches[0].0);
    }
    let list = one(clump, 26)?;
    let geometry_chunks: Vec<_> = chunks(list)?.into_iter().filter(|c| c.tag == 15).collect();
    ensure!(
        geometry_chunks.len() == u32at(one(list, 1)?, 0)? as usize && geometry_chunks.len() <= 128,
        "invalid skin geometry list"
    );
    let mut parts = Vec::new();
    for section in geometry_chunks {
        let geometry = geometry(section.body, &std::collections::HashMap::new())?;
        let plugins = chunks(one(section.body, 3)?)?;
        let skins: Vec<_> = plugins.into_iter().filter(|c| c.tag == 0x116).collect();
        ensure!(skins.len() == 1, "missing skin plugin");
        let skin = decode_skin(skins[0].body, geometry.positions.len())?;
        ensure!(
            skin.inverse_bind.len() == bone_frames.len(),
            "skin and HAnim bone count mismatch"
        );
        parts.push(Part { geometry, skin });
    }
    ensure!(!parts.is_empty(), "empty skinned model");
    let atomics: Vec<_> = chunks(clump)?.into_iter().filter(|c| c.tag == 20).collect();
    ensure!(
        atomics.len() == parts.len() && atomics.len() == u32at(one(clump, 1)?, 0)? as usize,
        "skinned atomics must reference each geometry once"
    );
    let mut parts: Vec<_> = parts.into_iter().map(Some).collect();
    let mut ordered = Vec::new();
    for atomic in atomics {
        let structure = one(atomic.body, 1)?;
        ensure!(
            (u32at(structure, 0)? as usize) < frames.len(),
            "invalid skinned atomic frame"
        );
        let index = u32at(structure, 4)? as usize;
        ordered.push(
            parts
                .get_mut(index)
                .and_then(Option::take)
                .context("invalid or repeated skinned atomic geometry")?,
        );
    }
    let model = Model {
        frames,
        bone_frames,
        parts: ordered,
    };
    model.pose(&model.bind_pose())?; // Validate hierarchy before exposing the asset.
    Ok(model)
}
impl Model {
    pub fn bind_pose(&self) -> Vec<Matrix> {
        self.frames.iter().map(|f| f.local).collect()
    }
    pub fn pose(&self, locals: &[Matrix]) -> Result<Vec<Geometry>> {
        ensure!(
            locals.len() == self.frames.len() && locals.iter().flatten().all(|v| v.is_finite()),
            "invalid skeleton pose"
        );
        ensure!(
            self.frames
                .iter()
                .all(|f| f.parent.is_none_or(|i| i < self.frames.len()))
                && self.bone_frames.iter().all(|i| *i < self.frames.len()),
            "invalid skeleton frame references"
        );
        fn visit(
            model: &Model,
            locals: &[Matrix],
            i: usize,
            states: &mut [u8],
            world: &mut [Matrix],
        ) -> Result<()> {
            ensure!(states[i] != 1, "cyclic skin frame hierarchy");
            if states[i] == 2 {
                return Ok(());
            }
            states[i] = 1;
            world[i] = if let Some(parent) = model.frames[i].parent {
                visit(model, locals, parent, states, world)?;
                compose(&world[parent], &locals[i])
            } else {
                locals[i]
            };
            states[i] = 2;
            Ok(())
        }
        let mut world = vec![[0.0; 12]; self.frames.len()];
        let mut states = vec![0; self.frames.len()];
        for i in 0..self.frames.len() {
            visit(self, locals, i, &mut states, &mut world)?;
        }
        let mut posed = Vec::new();
        for part in &self.parts {
            ensure!(
                part.skin.indices.len() == part.geometry.positions.len()
                    && part.skin.weights.len() == part.geometry.positions.len()
                    && part.skin.inverse_bind.len() == self.bone_frames.len(),
                "invalid pose skin sizes"
            );
            let matrices: Vec<_> = self
                .bone_frames
                .iter()
                .zip(&part.skin.inverse_bind)
                .map(|(frame, bind)| compose(&world[*frame], bind))
                .collect();
            let mut mesh = part.geometry.clone();
            for i in 0..mesh.positions.len() {
                let mut position = [0.0; 3];
                let mut normal = [0.0; 3];
                for j in 0..4 {
                    let weight = part.skin.weights[i][j];
                    if weight == 0.0 {
                        continue;
                    }
                    let matrix = matrices
                        .get(usize::from(part.skin.indices[i][j]))
                        .context("invalid pose bone index")?;
                    let p = transform(matrix, part.geometry.positions[i], true);
                    let n = part
                        .geometry
                        .normals
                        .get(i)
                        .map(|v| transform(matrix, *v, false))
                        .unwrap_or([0.0; 3]);
                    for axis in 0..3 {
                        position[axis] += p[axis] * weight;
                        normal[axis] += n[axis] * weight;
                    }
                }
                ensure!(
                    position.iter().chain(&normal).all(|v| v.is_finite()),
                    "nonfinite skinned vertex"
                );
                mesh.positions[i] = position;
                if !mesh.normals.is_empty() {
                    let length = normal.iter().map(|v| v * v).sum::<f32>().sqrt();
                    if length > 0.0001 {
                        for value in &mut normal {
                            *value /= length;
                        }
                    }
                    mesh.normals[i] = normal;
                }
            }
            posed.push(mesh);
        }
        Ok(posed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn section(tag: u32, body: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for value in [tag, body.len() as u32, 0x1803ffff] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(body);
        bytes
    }
    #[test]
    fn original_free_skinned_dff_round_trip_maps_hanim_to_skin() {
        let room = include_bytes!("../../../../mods/native-room-demo/room.dff");
        let clump = root(room, 16).unwrap();
        let raw_geometry = chunks(one(clump, 26).unwrap())
            .unwrap()
            .into_iter()
            .find(|c| c.tag == 15)
            .unwrap()
            .body;
        let vertices = geometry(raw_geometry, &std::collections::HashMap::new())
            .unwrap()
            .positions
            .len();
        let mut skin = vec![1, 1, 1, 0, 0];
        skin.extend(vec![0; vertices * 4]);
        for _ in 0..vertices {
            for w in [1.0f32, 0.0, 0.0, 0.0] {
                skin.extend(w.to_le_bytes());
            }
        }
        skin.extend(&plugin()[25..]);
        let mut geometry = raw_geometry.to_vec();
        geometry.extend(section(3, &section(0x116, &skin)));
        let mut geometry_list = section(1, &1u32.to_le_bytes());
        geometry_list.extend(section(15, &geometry));
        let mut anim = Vec::new();
        for value in [0x100u32, 0, 1, 0, 36, 0, 0, 0] {
            anim.extend(value.to_le_bytes());
        }
        let mut frames = section(1, one(one(clump, 14).unwrap(), 1).unwrap());
        frames.extend(section(3, &section(0x11e, &anim)));
        let mut body = Vec::new();
        for c in chunks(clump).unwrap() {
            body.extend(match c.tag {
                14 => section(14, &frames),
                26 => section(26, &geometry_list),
                _ => section(c.tag, c.body),
            });
        }
        let model = decode(&section(16, &body)).unwrap();
        assert_eq!(model.bone_frames, vec![0]);
        let posed = model.pose(&model.bind_pose()).unwrap();
        assert_eq!(posed[0].positions, decode_dff(room).unwrap()[0].positions);
        assert!(decode(room).is_err());
    }
    fn identity() -> Matrix {
        [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0]
    }
    fn plugin() -> Vec<u8> {
        let mut bytes = vec![1, 1, 1, 0, 0, 0, 0, 0, 0];
        for weight in [1.0f32, 0.0, 0.0, 0.0] {
            bytes.extend(weight.to_le_bytes());
        }
        for value in [
            1.0f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
        ] {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend([0; 12]);
        bytes
    }
    #[test]
    fn skin_plugin_checks_lengths_weights_indices_and_split_palettes() {
        let bytes = plugin();
        let decoded = decode_skin(&bytes, 1).unwrap();
        assert_eq!(decoded.inverse_bind[0], identity());
        for length in 0..bytes.len() {
            assert!(decode_skin(&bytes[..length], 1).is_err());
        }
        let mut invalid = bytes.clone();
        invalid[5] = 1;
        assert!(decode_skin(&invalid, 1).is_err());
        let mut invalid = bytes.clone();
        invalid[9..13].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(decode_skin(&invalid, 1).is_err());
        let mut invalid = bytes.clone();
        invalid[9..13].copy_from_slice(&0.5f32.to_le_bytes());
        assert!(decode_skin(&invalid, 1).is_err());
        let mut invalid = bytes;
        *invalid.last_mut().unwrap() = 1;
        assert!(decode_skin(&invalid, 1).is_err());
    }
    #[test]
    fn hierarchical_pose_blends_bones_and_validates_cycles() {
        let mut geometry = decode_dff(include_bytes!("../../../../mods/native-room-demo/room.dff"))
            .unwrap()
            .remove(0);
        geometry.positions.fill([1.0, 0.0, 0.0]);
        let vertices = geometry.positions.len();
        let mut child = identity();
        child[9] = 2.0;
        let mut inverse = identity();
        inverse[9] = -2.0;
        let mut model = Model {
            frames: vec![
                Frame {
                    local: identity(),
                    parent: None,
                    bone_id: Some(0),
                },
                Frame {
                    local: child,
                    parent: Some(0),
                    bone_id: Some(1),
                },
            ],
            bone_frames: vec![0, 1],
            parts: vec![Part {
                geometry,
                skin: Skin {
                    indices: vec![[0, 1, 0, 0]; vertices],
                    weights: vec![[0.25, 0.75, 0.0, 0.0]; vertices],
                    inverse_bind: vec![identity(), inverse],
                },
            }],
        };
        assert_eq!(
            model.pose(&model.bind_pose()).unwrap()[0].positions[0],
            [1.0, 0.0, 0.0]
        );
        let mut pose = model.bind_pose();
        pose[0][10] = 3.0;
        pose[1][9] = 4.0;
        assert_eq!(model.pose(&pose).unwrap()[0].positions[0], [2.5, 3.0, 0.0]);
        pose[0][0] = f32::NAN;
        assert!(model.pose(&pose).is_err());
        model.frames[0].parent = Some(1);
        assert!(model.pose(&model.bind_pose()).is_err());
        model.frames[0].parent = Some(99);
        assert!(model.pose(&model.bind_pose()).is_err());
    }
}
