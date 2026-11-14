//! Customisable characters for content packs: one glTF file with a male and
//! a female rig, interchangeable hair and clothes, and shared animations
//! (tools/blender/make_rp_characters.py builds the SARE RP Town set). The
//! player's [`Appearance`] picks the body, the styles and their colours.
//! See "Characters" in docs/content-packs.md.
//!
//! File layout: mesh nodes are named `<body>.<slot>.<style>.<part>` with
//! body `M` or `F`, slot `base`, `hair`, `top`, `pants` or `shoes`, and a
//! style number. They hang under the rig's bone nodes (rigid skinning, no
//! skin weights). Materials named `skin`, `hair`, `top`, `pants` and `shoes`
//! take the appearance colours; any other material keeps its own colour.
//! Animations `idle`, `walk`, `run`, `sit` and `fall` move the bone nodes.
//! The character faces glTF +Z with its feet at height 0.
use crate::{gltf_model, Batch, Scene, Vertex};
use anyhow::{ensure, Context, Result};
use glam::{Mat4, Quat, Vec3};
use sa_assets::Texture;
pub use sa_net::appearance::Appearance;
use std::collections::HashMap;
use std::path::Path;

/// Batch and texture key of a posed character.
pub const KEY: &str = "pack:character";
/// Clips the animator plays; files may contain more.
pub const CLIPS: [&str; 5] = ["idle", "walk", "run", "sit", "fall"];
/// Metres covered by one walk and one run cycle at the clip's own pace.
const WALK_STRIDE: f32 = 1.6;
const RUN_STRIDE: f32 = 2.64;
/// Locomotion speeds the walk and run clips were made for, m/s.
const WALK_SPEED: f32 = 1.6;
const RUN_SPEED: f32 = 4.4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Slot {
    Base,
    Hair,
    Top,
    Pants,
    Shoes,
}
impl Slot {
    fn parse(text: &str) -> Option<Self> {
        Some(match text {
            "base" => Self::Base,
            "hair" => Self::Hair,
            "top" => Self::Top,
            "pants" => Self::Pants,
            "shoes" => Self::Shoes,
            _ => return None,
        })
    }
    fn style(self, look: &Appearance) -> u8 {
        match self {
            Self::Base => 0,
            Self::Hair => look.hair,
            Self::Top => look.top,
            Self::Pants => look.pants,
            Self::Shoes => look.shoes,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Paint {
    Fixed([f32; 3]),
    Skin,
    Hair,
    Top,
    Pants,
    Shoes,
}
impl Paint {
    fn color(self, look: &Appearance) -> [f32; 3] {
        let srgb = |c: [u8; 3]| c.map(|v| v as f32 / 255.0);
        match self {
            Self::Fixed(c) => c,
            Self::Skin => srgb(look.skin),
            Self::Hair => srgb(look.hair_color),
            Self::Top => srgb(look.top_color),
            Self::Pants => srgb(look.pants_color),
            Self::Shoes => srgb(look.shoes_color),
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Trs {
    t: Vec3,
    r: Quat,
    s: Vec3,
}
impl Trs {
    fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.s, self.r, self.t)
    }
}

struct Node {
    parent: Option<usize>,
    rest: Trs,
}

struct Piece {
    node: usize,
    body: u8,
    slot: Slot,
    style: u8,
    /// Node-space positions and normals, three per triangle.
    positions: Vec<Vec3>,
    normals: Vec<Vec3>,
    paints: Vec<Paint>,
}

enum Track {
    Rotation(Vec<Quat>),
    Translation(Vec<Vec3>),
    Scale(Vec<Vec3>),
}
struct Channel {
    node: usize,
    times: Vec<f32>,
    track: Track,
}
struct Clip {
    name: String,
    duration: f32,
    channels: Vec<Channel>,
}

/// One clip to blend in: its name, time in seconds and weight.
#[derive(Clone, Debug, PartialEq)]
pub struct Play {
    pub clip: &'static str,
    pub time: f32,
    pub weight: f32,
}

/// A loaded character file.
pub struct CharacterModel {
    nodes: Vec<Node>,
    pieces: Vec<Piece>,
    clips: Vec<Clip>,
    vertex_count: usize,
    /// Styles available per body and slot, to fall back to style 0.
    styles: HashMap<(u8, Slot), Vec<u8>>,
}

fn lerp_index(times: &[f32], t: f32) -> (usize, usize, f32) {
    let last = times.len() - 1;
    if t <= times[0] {
        return (0, 0, 0.0);
    }
    if t >= times[last] {
        return (last, last, 0.0);
    }
    let next = times.partition_point(|&x| x <= t).min(last);
    let prev = next.saturating_sub(1);
    let span = (times[next] - times[prev]).max(1e-6);
    (prev, next, ((t - times[prev]) / span).clamp(0.0, 1.0))
}

impl CharacterModel {
    pub fn load(path: &Path) -> Result<Self> {
        let bytes = gltf_model::read_file(path)?;
        let folder = path.parent().unwrap_or(Path::new("."));
        Self::parse(&bytes, folder)
            .with_context(|| format!("loading characters {}", path.display()))
    }

    pub fn parse(bytes: &[u8], folder: &Path) -> Result<Self> {
        let (gltf, buffers) = gltf_model::open_document(bytes, folder)?;
        let document = &gltf.document;
        ensure!(document.nodes().len() <= 4096, "too many nodes");
        let mut nodes: Vec<Node> = document
            .nodes()
            .map(|n| {
                let (t, r, s) = n.transform().decomposed();
                Node {
                    parent: None,
                    rest: Trs {
                        t: Vec3::from_array(t),
                        r: Quat::from_array(r).normalize(),
                        s: Vec3::from_array(s),
                    },
                }
            })
            .collect();
        for node in document.nodes() {
            for child in node.children() {
                nodes[child.index()].parent = Some(node.index());
            }
        }
        // Parents must come first for one-pass posing; refuse cycles.
        for start in 0..nodes.len() {
            let mut depth = 0;
            let mut at = nodes[start].parent;
            while let Some(p) = at {
                depth += 1;
                ensure!(depth < 64, "the node tree is too deep or has a cycle");
                at = nodes[p].parent;
            }
        }

        let mut pieces = Vec::new();
        let mut styles: HashMap<(u8, Slot), Vec<u8>> = HashMap::new();
        let mut vertex_count = 0;
        for node in document.nodes() {
            let Some(mesh) = node.mesh() else { continue };
            let name = node.name().unwrap_or("");
            let mut parts = name.split('.');
            let body = match parts.next() {
                Some("M") => 0,
                Some("F") => 1,
                _ => continue,
            };
            let Some(slot) = parts.next().and_then(Slot::parse) else {
                continue;
            };
            let Some(style) = parts.next().and_then(|s| s.parse::<u8>().ok()) else {
                continue;
            };
            let mut piece = Piece {
                node: node.index(),
                body,
                slot,
                style,
                positions: Vec::new(),
                normals: Vec::new(),
                paints: Vec::new(),
            };
            for primitive in mesh.primitives() {
                if primitive.mode() != gltf::mesh::Mode::Triangles {
                    continue;
                }
                let reader = primitive.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
                let Some(positions) = reader.read_positions() else {
                    continue;
                };
                let positions: Vec<Vec3> = positions.map(Vec3::from_array).collect();
                let normals: Option<Vec<Vec3>> = reader
                    .read_normals()
                    .map(|n| n.map(Vec3::from_array).collect());
                let indices: Vec<u32> = match reader.read_indices() {
                    Some(i) => i.into_u32().collect(),
                    None => (0..positions.len() as u32).collect(),
                };
                ensure!(
                    indices.iter().all(|&i| (i as usize) < positions.len()),
                    "a mesh index is out of range"
                );
                let material = primitive.material();
                let factor = material.pbr_metallic_roughness().base_color_factor();
                let paint = match material.name().unwrap_or("") {
                    "skin" => Paint::Skin,
                    "hair" => Paint::Hair,
                    "top" => Paint::Top,
                    "pants" => Paint::Pants,
                    "shoes" => Paint::Shoes,
                    _ => Paint::Fixed(std::array::from_fn(|i| {
                        crate::pack_vehicle::linear_to_srgb(factor[i])
                    })),
                };
                for tri in indices.as_chunks::<3>().0 {
                    let [a, b, c] = tri.map(|i| i as usize);
                    let face = (positions[b] - positions[a])
                        .cross(positions[c] - positions[a])
                        .normalize_or_zero();
                    for i in [a, b, c] {
                        piece.positions.push(positions[i]);
                        piece.normals.push(
                            normals
                                .as_ref()
                                .and_then(|n| n.get(i).copied())
                                .unwrap_or(face),
                        );
                        piece.paints.push(paint);
                    }
                }
            }
            vertex_count += piece.positions.len();
            ensure!(
                vertex_count <= 600_000,
                "the characters have too many triangles"
            );
            let list = styles.entry((body, slot)).or_default();
            if !list.contains(&style) {
                list.push(style);
            }
            pieces.push(piece);
        }
        ensure!(
            !pieces.is_empty(),
            "no character meshes (named like M.base.0.head) found"
        );
        for list in styles.values_mut() {
            list.sort_unstable();
        }

        let mut clips = Vec::new();
        for animation in document.animations() {
            let mut clip = Clip {
                name: animation.name().unwrap_or("").to_string(),
                duration: 0.0,
                channels: Vec::new(),
            };
            for channel in animation.channels() {
                let reader = channel.reader(|b| buffers.get(b.index()).map(Vec::as_slice));
                let Some(times) = reader.read_inputs() else {
                    continue;
                };
                let times: Vec<f32> = times.collect();
                let Some(outputs) = reader.read_outputs() else {
                    continue;
                };
                let cubic = channel.sampler().interpolation()
                    == gltf::animation::Interpolation::CubicSpline;
                // Cubic splines store in-tangent, value, out-tangent per key.
                fn values<T: Copy>(all: Vec<T>, cubic: bool) -> Vec<T> {
                    if cubic {
                        all.as_chunks::<3>().0.iter().map(|c| c[1]).collect()
                    } else {
                        all
                    }
                }
                use gltf::animation::util::ReadOutputs;
                let track = match outputs {
                    ReadOutputs::Rotations(r) => Track::Rotation(values(
                        r.into_f32()
                            .map(|q| Quat::from_array(q).normalize())
                            .collect(),
                        cubic,
                    )),
                    ReadOutputs::Translations(t) => {
                        Track::Translation(values(t.map(Vec3::from_array).collect(), cubic))
                    }
                    ReadOutputs::Scales(s) => {
                        Track::Scale(values(s.map(Vec3::from_array).collect(), cubic))
                    }
                    ReadOutputs::MorphTargetWeights(_) => continue,
                };
                let len = match &track {
                    Track::Rotation(v) => v.len(),
                    Track::Translation(v) | Track::Scale(v) => v.len(),
                };
                if times.is_empty() || len != times.len() || times.iter().any(|t| !t.is_finite()) {
                    continue;
                }
                clip.duration = clip.duration.max(*times.last().unwrap_or(&0.0));
                clip.channels.push(Channel {
                    node: channel.target().node().index(),
                    times,
                    track,
                });
            }
            clips.push(clip);
        }
        Ok(Self {
            nodes,
            pieces,
            clips,
            vertex_count,
            styles,
        })
    }

    /// Vertices of every posed character; the count never changes.
    pub fn vertex_count(&self) -> usize {
        self.vertex_count
    }
    pub fn clip_names(&self) -> Vec<&str> {
        self.clips.iter().map(|c| c.name.as_str()).collect()
    }
    pub fn clip_duration(&self, name: &str) -> Option<f32> {
        self.clips
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.duration)
    }
    /// How many styles a slot offers for a body (highest index + 1).
    pub fn style_count(&self, body: u8, slot: Slot) -> usize {
        self.styles
            .get(&(body, slot))
            .and_then(|s| s.last())
            .map_or(0, |&s| s as usize + 1)
    }

    fn local_poses(&self, plays: &[Play]) -> Vec<Trs> {
        let mut out: Vec<Trs> = self.nodes.iter().map(|n| n.rest).collect();
        let total: f32 = plays.iter().map(|p| p.weight.max(0.0)).sum();
        if total <= 1e-4 {
            return out;
        }
        // Weighted sums per node, seeded from the rest pose for clips that
        // do not animate a node.
        let mut t = vec![Vec3::ZERO; out.len()];
        let mut s = vec![Vec3::ZERO; out.len()];
        let mut r = vec![glam::Vec4::ZERO; out.len()];
        for play in plays {
            let w = play.weight.max(0.0) / total;
            if w <= 0.0 {
                continue;
            }
            let mut sample: Vec<Trs> = self.nodes.iter().map(|n| n.rest).collect();
            if let Some(clip) = self.clips.iter().find(|c| c.name == play.clip) {
                let time = if clip.duration > 0.0 {
                    play.time.rem_euclid(clip.duration)
                } else {
                    0.0
                };
                for channel in &clip.channels {
                    let Some(target) = sample.get_mut(channel.node) else {
                        continue;
                    };
                    let (a, b, f) = lerp_index(&channel.times, time);
                    match &channel.track {
                        Track::Rotation(v) => target.r = v[a].slerp(v[b], f),
                        Track::Translation(v) => target.t = v[a].lerp(v[b], f),
                        Track::Scale(v) => target.s = v[a].lerp(v[b], f),
                    }
                }
            }
            for (i, pose) in sample.iter().enumerate() {
                t[i] += pose.t * w;
                s[i] += pose.s * w;
                let q = glam::Vec4::from(pose.r);
                // Keep quaternions in one hemisphere before summing.
                let sign = if r[i].dot(q) < 0.0 { -1.0 } else { 1.0 };
                r[i] += q * (w * sign);
            }
        }
        for (i, pose) in out.iter_mut().enumerate() {
            let q = r[i];
            pose.t = t[i];
            pose.s = s[i];
            pose.r = if q.length_squared() > 1e-8 {
                Quat::from_vec4(q).normalize()
            } else {
                pose.r
            };
        }
        out
    }

    fn world_matrices(&self, plays: &[Play]) -> Vec<Mat4> {
        let local = self.local_poses(plays);
        let mut world: Vec<Option<Mat4>> = vec![None; self.nodes.len()];
        fn resolve(i: usize, nodes: &[Node], local: &[Trs], world: &mut Vec<Option<Mat4>>) -> Mat4 {
            if let Some(m) = world[i] {
                return m;
            }
            let own = local[i].matrix();
            let m = match nodes[i].parent {
                Some(p) => resolve(p, nodes, local, world) * own,
                None => own,
            };
            world[i] = Some(m);
            m
        }
        (0..self.nodes.len())
            .map(|i| resolve(i, &self.nodes, &local, &mut world))
            .collect()
    }

    fn shown(&self, piece: &Piece, look: &Appearance) -> bool {
        if piece.body != look.body.min(1) {
            return false;
        }
        let wanted = piece.slot.style(look);
        let available = self
            .styles
            .get(&(piece.body, piece.slot))
            .is_some_and(|s| s.contains(&wanted));
        piece.style == if available { wanted } else { 0 }
    }

    /// Posed vertices in model space (feet at 0, facing +Z), lit and
    /// coloured, as `[position, uv, rgba]` floats like uploaded batches.
    /// Hidden pieces collapse to a point so the length stays fixed.
    pub fn pose_into(&self, plays: &[Play], look: &Appearance, out: &mut Vec<f32>) {
        out.clear();
        out.reserve(self.vertex_count * 9);
        let world = self.world_matrices(plays);
        let light = Vec3::new(0.35, 1.0, 0.25).normalize();
        for piece in &self.pieces {
            let matrix = world[piece.node];
            if !self.shown(piece, look) {
                let p = matrix.transform_point3(Vec3::ZERO);
                for _ in 0..piece.positions.len() {
                    out.extend_from_slice(&[p.x, p.y, p.z, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
                }
                continue;
            }
            for ((position, normal), paint) in piece
                .positions
                .iter()
                .zip(&piece.normals)
                .zip(&piece.paints)
            {
                let p = matrix.transform_point3(*position);
                let n = matrix.transform_vector3(*normal).normalize_or_zero();
                let shade = 0.62 + 0.38 * n.dot(light).max(0.0);
                let c = paint.color(look);
                out.extend_from_slice(&[
                    p.x,
                    p.y,
                    p.z,
                    0.0,
                    0.0,
                    c[0] * shade,
                    c[1] * shade,
                    c[2] * shade,
                    1.0,
                ]);
            }
        }
    }

    /// A character standing idle in `look`, as an uploadable scene with one
    /// animated batch; the runtime rewrites its vertices every frame.
    pub fn scene(&self, look: &Appearance) -> Scene {
        let mut raw = Vec::new();
        self.pose_into(
            &[Play {
                clip: "idle",
                time: 0.0,
                weight: 1.0,
            }],
            look,
            &mut raw,
        );
        let vertices = raw
            .as_chunks::<9>()
            .0
            .iter()
            .map(|v| Vertex {
                position: [v[0], v[1], v[2]],
                uv: [v[3], v[4]],
                color: [v[5], v[6], v[7], v[8]],
            })
            .collect();
        Scene {
            vehicle_handling: Default::default(),
            water: None,
            triangles: self.vertex_count / 3,
            batches: vec![Batch {
                key: KEY.to_string(),
                vertices,
                alpha: false,
                animated: true,
                uv_animation: None,
                map_door: None,
                mlo_entity: None,
            }],
            textures: HashMap::from([(
                KEY.to_string(),
                Texture {
                    width: 1,
                    height: 1,
                    rgba: vec![255; 4],
                    has_alpha: false,
                },
            )]),
            placements: 1,
            animation: None,
            collision: None,
            vehicle_pieces: HashMap::new(),
        }
    }
}

/// What a character is doing, for the animator.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Motion {
    /// Ground speed, m/s.
    pub speed: f32,
    pub airborne: bool,
    pub seated: bool,
}

/// Blends idle, walk, run, sit and fall from a character's motion, with the
/// walk and run cycles kept in step with the ground speed.
#[derive(Clone, Debug)]
pub struct Animator {
    clock: f32,
    /// Shared walk/run cycle position, 0..1.
    phase: f32,
    weights: [f32; 5],
}
impl Default for Animator {
    fn default() -> Self {
        Self {
            clock: 0.0,
            phase: 0.0,
            weights: [1.0, 0.0, 0.0, 0.0, 0.0],
        }
    }
}
impl Animator {
    pub fn update(&mut self, motion: Motion, dt: f32) {
        let dt = dt.clamp(0.0, 0.25);
        self.clock += dt;
        let speed = motion.speed.abs().min(12.0);
        let run = ((speed - WALK_SPEED) / (RUN_SPEED - WALK_SPEED)).clamp(0.0, 1.0);
        let moving = (speed / 0.5).clamp(0.0, 1.0);
        let target = if motion.seated {
            [0.0, 0.0, 0.0, 1.0, 0.0]
        } else if motion.airborne {
            [0.0, 0.0, 0.0, 0.0, 1.0]
        } else {
            [1.0 - moving, moving * (1.0 - run), moving * run, 0.0, 0.0]
        };
        // Quick blends; seat changes snap so nobody slides into a car.
        let rate = if motion.seated || self.weights[3] > 0.0 {
            1.0
        } else {
            (dt * 10.0).min(1.0)
        };
        for (w, t) in self.weights.iter_mut().zip(target) {
            *w += (t - *w) * rate;
        }
        let stride = WALK_STRIDE + (RUN_STRIDE - WALK_STRIDE) * run;
        // Walk in place a little when nearly stopped, so feet settle.
        self.phase = (self.phase + dt * speed.max(0.3 * moving) / stride).rem_euclid(1.0);
    }
    /// Clips to blend now; `durations` gives each clip's length.
    pub fn plays(&self, model: &CharacterModel) -> Vec<Play> {
        CLIPS
            .iter()
            .zip(self.weights)
            .filter(|(_, w)| *w > 0.01)
            .map(|(&clip, weight)| {
                let duration = model.clip_duration(clip).unwrap_or(1.0);
                let time = match clip {
                    "walk" | "run" => self.phase * duration,
                    _ => self.clock,
                };
                Play { clip, time, weight }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn town() -> CharacterModel {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../content/sare-rp-town/characters.glb");
        CharacterModel::load(&path).unwrap()
    }
    fn bounds(raw: &[f32]) -> (Vec3, Vec3) {
        raw.as_chunks::<9>().0.iter().filter(|v| v[8] > 0.0).fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(lo, hi), v| {
                let p = Vec3::new(v[0], v[1], v[2]);
                (lo.min(p), hi.max(p))
            },
        )
    }
    fn play(clip: &'static str, time: f32) -> Vec<Play> {
        vec![Play {
            clip,
            time,
            weight: 1.0,
        }]
    }
    #[test]
    fn town_characters_have_both_bodies_all_slots_and_clips() {
        let model = town();
        for clip in CLIPS {
            assert!(model.clip_duration(clip).unwrap() > 0.1, "{clip}");
        }
        for body in 0..2 {
            assert!(model.style_count(body, Slot::Hair) >= 6);
            assert!(model.style_count(body, Slot::Top) >= 5);
            assert!(model.style_count(body, Slot::Pants) >= 3);
            assert!(model.style_count(body, Slot::Shoes) >= 3);
        }
        let mut raw = Vec::new();
        model.pose_into(&play("idle", 0.0), &Appearance::default(), &mut raw);
        assert_eq!(raw.len(), model.vertex_count() * 9);
        let (lo, hi) = bounds(&raw);
        // Feet on the ground, about 1.8 m tall, facing +Z (the nose).
        assert!(lo.y.abs() < 0.06, "{lo}");
        assert!((1.6..2.0).contains(&hi.y), "{hi}");
        let female = Appearance {
            body: 1,
            ..Default::default()
        };
        let mut other = Vec::new();
        model.pose_into(&play("idle", 0.0), &female, &mut other);
        assert_eq!(other.len(), raw.len());
        assert!(bounds(&other).1.y < hi.y);
    }
    #[test]
    fn colours_styles_and_clips_change_the_pose() {
        let model = town();
        let base = Appearance::default();
        let mut a = Vec::new();
        let mut b = Vec::new();
        model.pose_into(&play("idle", 0.0), &base, &mut a);
        let red = Appearance {
            top_color: [255, 0, 0],
            ..base
        };
        model.pose_into(&play("idle", 0.0), &red, &mut b);
        assert_ne!(a, b);
        // Out-of-range styles fall back to style 0 instead of vanishing.
        let odd = Appearance { hair: 30, ..base };
        model.pose_into(&play("idle", 0.0), &odd, &mut b);
        let zero = Appearance { hair: 0, ..base };
        model.pose_into(&play("idle", 0.0), &zero, &mut a);
        assert_eq!(a, b);
        // Sitting lowers the hips by about 0.4 m; walking moves the legs.
        model.pose_into(&play("sit", 0.0), &base, &mut b);
        let (_, sit_top) = bounds(&b);
        assert!(sit_top.y < bounds(&a).1.y - 0.3, "{sit_top}");
        model.pose_into(&play("walk", 0.25), &base, &mut a);
        model.pose_into(&play("walk", 0.75), &base, &mut b);
        assert_ne!(a, b);
    }
    #[test]
    fn the_animator_follows_speed_seat_and_air() {
        let model = town();
        let mut anim = Animator::default();
        let clips = |anim: &Animator| -> Vec<&'static str> {
            let mut plays = anim.plays(&model);
            plays.sort_by(|a, b| b.weight.total_cmp(&a.weight));
            plays.into_iter().map(|p| p.clip).collect()
        };
        assert_eq!(clips(&anim)[0], "idle");
        for _ in 0..30 {
            anim.update(
                Motion {
                    speed: 1.6,
                    ..Default::default()
                },
                1.0 / 30.0,
            );
        }
        assert_eq!(clips(&anim)[0], "walk");
        for _ in 0..30 {
            anim.update(
                Motion {
                    speed: 6.0,
                    ..Default::default()
                },
                1.0 / 30.0,
            );
        }
        assert_eq!(clips(&anim)[0], "run");
        anim.update(
            Motion {
                seated: true,
                ..Default::default()
            },
            1.0 / 30.0,
        );
        assert_eq!(clips(&anim), ["sit"]);
        for _ in 0..30 {
            anim.update(
                Motion {
                    airborne: true,
                    ..Default::default()
                },
                1.0 / 30.0,
            );
        }
        assert_eq!(clips(&anim)[0], "fall");
    }
}
