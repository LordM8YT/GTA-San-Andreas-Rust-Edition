//! Moving parts in glTF models: node animations (translation, rotation,
//! scale) made in Blender or another 3D tool, such as a windmill's sails,
//! a door swinging or a sign turning. Each animated node and everything
//! under it is a *part*; [`Animation::pose`] gives every part's matrix at a
//! time in a clip, which moves the part's vertices from where they rest.
//!
//! Not read (yet): skin weights (armatures that bend a mesh) and morph
//! targets (shape keys). Cubic-spline keys are played as linear.
use anyhow::{ensure, Context, Result};
use glam::{Mat4, Quat, Vec3};

/// Most keyframes one model may have, over all channels.
pub const MAX_KEYS: usize = 1_000_000;

/// How a channel moves between its keys.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Interpolation {
    Linear,
    Step,
}

#[derive(Clone, Debug)]
enum Values {
    Translation(Vec<Vec3>),
    Rotation(Vec<Quat>),
    Scale(Vec<Vec3>),
}

/// One property of one node over time.
#[derive(Clone, Debug)]
struct Channel {
    node: usize,
    times: Vec<f32>,
    values: Values,
    interpolation: Interpolation,
}

/// One named animation, as the 3D tool exported it.
#[derive(Clone, Debug)]
pub struct Clip {
    pub name: String,
    /// Seconds; the clip loops after this.
    pub duration: f32,
    channels: Vec<Channel>,
}

/// A node's own transform at rest, and its parent.
#[derive(Clone, Copy, Debug)]
struct Rest {
    parent: Option<usize>,
    translation: Vec3,
    rotation: Quat,
    scale: Vec3,
}

/// The animated nodes of a model and their clips.
#[derive(Clone, Debug, Default)]
pub struct Animation {
    pub clips: Vec<Clip>,
    /// Every node of the file at rest, by glTF node index.
    nodes: Vec<Rest>,
    /// The nodes that are parts (each one animated by some clip), in order.
    pub parts: Vec<usize>,
    /// World matrix of each part at rest, inverted.
    rest_inverse: Vec<Mat4>,
}

impl Animation {
    /// Read every clip of `document`. None when it has no node animation.
    pub fn read(document: &gltf::Document, buffers: &[Vec<u8>]) -> Result<Option<Self>> {
        let mut nodes: Vec<Rest> = document
            .nodes()
            .map(|node| {
                let (t, r, s) = node.transform().decomposed();
                Rest {
                    parent: None,
                    translation: Vec3::from_array(t),
                    rotation: Quat::from_array(r).normalize(),
                    scale: Vec3::from_array(s),
                }
            })
            .collect();
        for node in document.nodes() {
            for child in node.children() {
                nodes[child.index()].parent = Some(node.index());
            }
        }
        let mut clips = Vec::new();
        let mut keys = 0;
        for (n, animation) in document.animations().enumerate() {
            let mut channels = Vec::new();
            for channel in animation.channels() {
                let reader = channel.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
                let times: Vec<f32> = reader
                    .read_inputs()
                    .context("animation keys missing")?
                    .collect();
                keys += times.len();
                ensure!(
                    keys <= MAX_KEYS,
                    "the model has more than {MAX_KEYS} animation keys"
                );
                let sampler = channel.sampler();
                let (interpolation, stride) = match sampler.interpolation() {
                    gltf::animation::Interpolation::Linear => (Interpolation::Linear, 1),
                    gltf::animation::Interpolation::Step => (Interpolation::Step, 1),
                    // In-tangent, value, out-tangent per key: keep the values.
                    gltf::animation::Interpolation::CubicSpline => (Interpolation::Linear, 3),
                };
                let pick = |i: usize| stride == 1 || i % 3 == 1;
                use gltf::animation::util::ReadOutputs;
                let values = match reader.read_outputs().context("animation values missing")? {
                    ReadOutputs::Translations(v) => Values::Translation(
                        v.enumerate()
                            .filter(|(i, _)| pick(*i))
                            .map(|(_, v)| Vec3::from_array(v))
                            .collect(),
                    ),
                    ReadOutputs::Scales(v) => Values::Scale(
                        v.enumerate()
                            .filter(|(i, _)| pick(*i))
                            .map(|(_, v)| Vec3::from_array(v))
                            .collect(),
                    ),
                    ReadOutputs::Rotations(v) => Values::Rotation(
                        v.into_f32()
                            .enumerate()
                            .filter(|(i, _)| pick(*i))
                            .map(|(_, v)| Quat::from_array(v).normalize())
                            .collect(),
                    ),
                    // Shape keys: not read.
                    ReadOutputs::MorphTargetWeights(_) => continue,
                };
                let count = match &values {
                    Values::Translation(v) | Values::Scale(v) => v.len(),
                    Values::Rotation(v) => v.len(),
                };
                ensure!(
                    count == times.len() && !times.is_empty(),
                    "an animation channel has {} keys but {count} values",
                    times.len()
                );
                ensure!(
                    times.iter().all(|t| t.is_finite() && *t >= 0.0)
                        && times.windows(2).all(|w| w[0] <= w[1]),
                    "animation key times must be positive and in order"
                );
                channels.push(Channel {
                    node: channel.target().node().index(),
                    times,
                    values,
                    interpolation,
                });
            }
            if channels.is_empty() {
                continue;
            }
            let duration = channels
                .iter()
                .filter_map(|c| c.times.last().copied())
                .fold(0.0f32, f32::max);
            clips.push(Clip {
                name: animation
                    .name()
                    .map_or_else(|| format!("clip {}", n + 1), str::to_owned),
                duration,
                channels,
            });
        }
        if clips.is_empty() {
            return Ok(None);
        }
        let mut parts: Vec<usize> = clips
            .iter()
            .flat_map(|c| c.channels.iter().map(|ch| ch.node))
            .collect();
        parts.sort_unstable();
        parts.dedup();
        let mut animation = Self {
            clips,
            nodes,
            parts,
            rest_inverse: Vec::new(),
        };
        let rest = animation.world(&animation.local_rest());
        animation.rest_inverse = animation.parts.iter().map(|&p| rest[p].inverse()).collect();
        Ok(Some(animation))
    }

    /// The clip called `name`, or the first one.
    pub fn clip(&self, name: Option<&str>) -> Option<usize> {
        match name {
            Some(name) => self.clips.iter().position(|c| c.name == name),
            None => (!self.clips.is_empty()).then_some(0),
        }
    }

    /// Each part's matrix (same order as [`Animation::parts`]) at `time`
    /// seconds into `clip`, looping: it moves a part's vertices from their
    /// rest place (in the model's space) to where the clip has them.
    pub fn pose(&self, clip: usize, time: f32) -> Vec<Mat4> {
        let mut local = self.local_rest();
        if let Some(clip) = self.clips.get(clip) {
            let time = if clip.duration > 0.0 && time.is_finite() {
                time.rem_euclid(clip.duration)
            } else {
                0.0
            };
            for channel in &clip.channels {
                let Some(node) = local.get_mut(channel.node) else {
                    continue;
                };
                let (i, j, t) = keys(&channel.times, time, channel.interpolation);
                match &channel.values {
                    Values::Translation(v) => node.0 = v[i].lerp(v[j], t),
                    Values::Scale(v) => node.2 = v[i].lerp(v[j], t),
                    Values::Rotation(v) => node.1 = v[i].slerp(v[j], t),
                }
            }
        }
        let world = self.world(&local);
        self.parts
            .iter()
            .zip(&self.rest_inverse)
            .map(|(&p, inverse)| world[p] * *inverse)
            .collect()
    }

    fn local_rest(&self) -> Vec<(Vec3, Quat, Vec3)> {
        self.nodes
            .iter()
            .map(|n| (n.translation, n.rotation, n.scale))
            .collect()
    }

    /// World matrices from local transforms (parents before children not
    /// required: each node walks up its own chain).
    fn world(&self, local: &[(Vec3, Quat, Vec3)]) -> Vec<Mat4> {
        let mut out: Vec<Option<Mat4>> = vec![None; local.len()];
        for start in 0..local.len() {
            let mut chain = vec![start];
            while let Some(parent) = self.nodes[*chain.last().unwrap()].parent {
                if out[parent].is_some() || chain.len() > local.len() {
                    break;
                }
                chain.push(parent);
            }
            for &node in chain.iter().rev() {
                if out[node].is_some() {
                    continue;
                }
                let (t, r, s) = local[node];
                let own = Mat4::from_scale_rotation_translation(s, r, t);
                let parent = self.nodes[node].parent.and_then(|p| out[p]);
                out[node] = Some(parent.map_or(own, |p| p * own));
            }
        }
        out.into_iter()
            .map(|m| m.unwrap_or(Mat4::IDENTITY))
            .collect()
    }
}

/// The keys around `time` and how far between them it is (0..1).
fn keys(times: &[f32], time: f32, interpolation: Interpolation) -> (usize, usize, f32) {
    let last = times.len() - 1;
    if time <= times[0] {
        return (0, 0, 0.0);
    }
    if time >= times[last] {
        return (last, last, 0.0);
    }
    let j = times.partition_point(|&t| t <= time);
    let i = j - 1;
    let span = times[j] - times[i];
    let t = if interpolation == Interpolation::Step || span <= 0.0 {
        0.0
    } else {
        (time - times[i]) / span
    };
    (i, j, t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_found_and_blended() {
        let times = [0.0, 1.0, 3.0];
        assert_eq!(keys(&times, -1.0, Interpolation::Linear), (0, 0, 0.0));
        assert_eq!(keys(&times, 0.5, Interpolation::Linear), (0, 1, 0.5));
        assert_eq!(keys(&times, 2.0, Interpolation::Linear), (1, 2, 0.5));
        assert_eq!(keys(&times, 2.0, Interpolation::Step), (1, 2, 0.0));
        assert_eq!(keys(&times, 9.0, Interpolation::Linear), (2, 2, 0.0));
        assert_eq!(keys(&[0.5], 0.7, Interpolation::Linear), (0, 0, 0.0));
    }
}
