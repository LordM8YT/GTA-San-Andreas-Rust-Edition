//! Build Mode world layouts: the objects a creator placed in a content pack,
//! saved as `layout.json` next to `content.json`.
//!
//! A layout lists *nodes*. A node with an `asset` is a placed object; a node
//! without one is a group (a folder in the World Outliner). Each node has a
//! stable numeric id, so references survive renames, reordering and undo.
//!
//! Coordinates are the content pack's Blender axes (X = east, Y = north,
//! Z = up, metres), the same as `world.spawn` in `content.json`. Rotation is
//! a Blender "XYZ Euler" in degrees, scale is per Blender axis.
//!
//! The file is versioned ([`VERSION`]). Older files are migrated on load by
//! [`migrate`]; files from a newer SARE are refused instead of being read
//! partly. Fields this version does not know are kept in `extra` and written
//! back unchanged, so nothing a newer tool or a hand edit added is lost.
//! See docs/build-mode.md.
use crate::world_logic::{Component, Kept};
use anyhow::{bail, ensure, Context, Result};
use glam::{EulerRot, Mat3, Mat4, Quat, Vec3};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;

/// Value of the `format` field, so other JSON files are not mistaken for a layout.
pub const FORMAT: &str = "sare-world-layout";
/// Current layout version written by this SARE.
pub const VERSION: u32 = 2;
/// Default file name inside a pack.
pub const FILE: &str = "layout.json";
/// Most nodes one layout may hold.
pub const MAX_NODES: usize = 20_000;
/// Positions further than this from the pack origin are refused.
pub const MAX_COORDINATE: f32 = 20_000.0;

/// Stable node identifier, unique inside one layout and never reused.
pub type NodeId = u64;

/// Position, rotation and scale in Blender axes.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    /// [east, north, up] in metres.
    pub position: [f32; 3],
    /// Blender XYZ Euler angles in degrees.
    #[serde(default)]
    pub rotation: [f32; 3],
    #[serde(default = "one")]
    pub scale: [f32; 3],
}
fn one() -> [f32; 3] {
    [1.0; 3]
}
fn is_true(value: &bool) -> bool {
    *value
}
fn yes() -> bool {
    true
}
impl Default for Transform {
    fn default() -> Self {
        Self {
            position: [0.0; 3],
            rotation: [0.0; 3],
            scale: [1.0; 3],
        }
    }
}
impl Transform {
    pub fn at(position: [f32; 3]) -> Self {
        Self {
            position,
            ..Self::default()
        }
    }
    /// Rotation in Blender axes.
    pub fn rotation_quat(&self) -> Quat {
        let [x, y, z] = self.rotation.map(f32::to_radians);
        // Blender's XYZ Euler applies X first, then Y, then Z (extrinsic).
        Quat::from_euler(EulerRot::ZYX, z, y, x)
    }
    /// Model matrix taking glTF model space (Y up, -Z north) to glTF/pack
    /// space, where the pack origin is Blender (0, 0, 0).
    pub fn matrix(&self) -> Mat4 {
        let blender = Mat4::from_scale_rotation_translation(
            Vec3::from_array(self.scale),
            self.rotation_quat(),
            Vec3::from_array(self.position),
        );
        let to_gltf = Mat4::from_mat3(BLENDER_TO_GLTF);
        to_gltf * blender * to_gltf.transpose()
    }
    /// No move and no turn: what a group with no pivot of its own has.
    pub fn is_unmoved(&self) -> bool {
        self.position == [0.0; 3] && self.rotation == [0.0; 3]
    }
    /// `child`, given in this (a group's) own axes, in the axes this is in.
    /// A group passes on its position and rotation, never its scale.
    pub fn place(&self, child: &Transform) -> Transform {
        if self.is_unmoved() {
            return *child;
        }
        let turn = self.rotation_quat();
        let position = Vec3::from_array(self.position) + turn * Vec3::from_array(child.position);
        let rotation = if self.rotation == [0.0; 3] {
            child.rotation
        } else {
            euler_degrees(turn * child.rotation_quat())
        };
        Transform {
            position: position.to_array(),
            rotation,
            scale: child.scale,
        }
    }
    /// The opposite of [`Transform::place`]: `world` in this group's own
    /// axes.
    pub fn local_of(&self, world: &Transform) -> Transform {
        if self.is_unmoved() {
            return *world;
        }
        let back = self.rotation_quat().inverse();
        let position = back * (Vec3::from_array(world.position) - Vec3::from_array(self.position));
        let rotation = if self.rotation == [0.0; 3] {
            world.rotation
        } else {
            euler_degrees(back * world.rotation_quat())
        };
        Transform {
            position: position.to_array(),
            rotation,
            scale: world.scale,
        }
    }
    pub fn is_valid(&self) -> bool {
        self.position
            .iter()
            .all(|v| v.is_finite() && v.abs() <= MAX_COORDINATE)
            && self.rotation.iter().all(|v| v.is_finite())
            && self
                .scale
                .iter()
                .all(|v| v.is_finite() && v.abs() >= 0.001 && v.abs() <= 1000.0)
    }
}

/// Blender XYZ Euler degrees of a rotation (the inverse of
/// [`Transform::rotation_quat`]), with tiny float noise rounded away so
/// a turn of 90° stays 90°.
pub fn euler_degrees(rotation: Quat) -> [f32; 3] {
    let (z, y, x) = rotation.normalize().to_euler(EulerRot::ZYX);
    [x, y, z].map(|r| {
        let d = r.to_degrees();
        let tidy = (d * 1000.0).round() / 1000.0;
        if tidy == -0.0 {
            0.0
        } else {
            tidy
        }
    })
}

/// Blender (X east, Y north, Z up) to glTF (X east, Y up, -Z north).
pub const BLENDER_TO_GLTF: Mat3 = Mat3::from_cols(Vec3::X, Vec3::new(0.0, 0.0, -1.0), Vec3::Y);

/// A Blender-axes point in glTF/pack space.
pub fn blender_to_gltf(point: [f32; 3]) -> Vec3 {
    BLENDER_TO_GLTF * Vec3::from_array(point)
}
/// A glTF/pack-space point in Blender axes.
pub fn gltf_to_blender(point: Vec3) -> [f32; 3] {
    (BLENDER_TO_GLTF.transpose() * point).to_array()
}

/// Where a placed object's model comes from. Written as a short string so
/// the layout stays portable: no absolute paths, ever.
///
/// - `builtin:<name>`: a model generated by SARE itself ([`crate::builtin_assets`]).
/// - `self:<path>`: a `.glb`/`.gltf` inside this pack's own folder.
/// - `pack:<id>:<path>`: a model inside another installed content pack.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AssetRef {
    Builtin(String),
    Own(String),
    Pack { id: String, path: String },
}
impl AssetRef {
    pub fn parse(text: &str) -> Result<Self> {
        let checked_path = |path: &str| -> Result<String> {
            let p = std::path::Path::new(path);
            ensure!(
                !path.is_empty()
                    && !path.contains('\\')
                    && p.components()
                        .all(|c| matches!(c, std::path::Component::Normal(_))),
                "asset path {path:?} must be a relative path with / separators inside the pack"
            );
            Ok(path.to_owned())
        };
        if let Some(name) = text.strip_prefix("builtin:") {
            ensure!(!name.is_empty(), "empty built-in asset name");
            return Ok(Self::Builtin(name.to_owned()));
        }
        if let Some(path) = text.strip_prefix("self:") {
            return Ok(Self::Own(checked_path(path)?));
        }
        if let Some(rest) = text.strip_prefix("pack:") {
            let (id, path) = rest
                .split_once(':')
                .with_context(|| format!("asset {text:?} must be pack:<id>:<path>"))?;
            ensure!(!id.is_empty(), "asset {text:?} has no pack id");
            return Ok(Self::Pack {
                id: id.to_owned(),
                path: checked_path(path)?,
            });
        }
        bail!("unknown asset reference {text:?}: use builtin:, self: or pack:")
    }
}
impl fmt::Display for AssetRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Builtin(name) => write!(f, "builtin:{name}"),
            Self::Own(path) => write!(f, "self:{path}"),
            Self::Pack { id, path } => write!(f, "pack:{id}:{path}"),
        }
    }
}
impl Serialize for AssetRef {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for AssetRef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let text = String::deserialize(d)?;
        Self::parse(&text).map_err(serde::de::Error::custom)
    }
}

/// One entry of the scene tree: a placed object, or a group when `asset` is None.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    /// The group this node sits in, or None at the top level.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<NodeId>,
    /// A group that is one thing (a car, a house): clicking any of its
    /// objects in the world selects the whole group (Alt+click selects
    /// inside). Other groups are folders: a click picks the object.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub model: bool,
    /// A copy of a saved model ("My Models"): the file in the world's
    /// folder it was placed from, e.g. `models/house.model.json` (see
    /// [`crate::saved_model`]). Its contents are ordinary objects.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_model: Option<String>,
    /// The model this object shows. None makes the node a group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset: Option<AssetRef>,
    #[serde(default)]
    pub transform: Transform,
    /// Drawn in the world (editor and game). A hidden object can still collide.
    #[serde(default = "yes")]
    pub visible: bool,
    /// Locked objects cannot be selected in the viewport or moved.
    #[serde(default)]
    pub locked: bool,
    /// Players and vehicles collide with the object's solid meshes.
    #[serde(default = "yes")]
    pub collision: bool,
    /// Gameplay behaviour (spawn point, trigger, interaction, checkpoint);
    /// see [`crate::world_logic`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gameplay: Option<Kept<Component>>,
    /// A light glowing on the object from dusk to dawn (see [`PointLight`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub light: Option<PointLight>,
    /// Sculpted ground (see [`Terrain`]); the object shows it instead of
    /// its asset's model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terrain: Option<Terrain>,
    /// A road drawn along points (see [`Road`]); the object shows it
    /// instead of its asset's model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub road: Option<Road>,
    /// A part built in Studio: a shape with a size, colour and material
    /// (see [`Part`]); the object shows it instead of its asset's model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub part: Option<Part>,
    /// Lua scripts on this object (`script.object` in them); see
    /// `docs/world-scripts.md`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<Script>,
    /// Scripts may show, hide or move the object while the world is played
    /// (it is then drawn and made solid on its own, like a door). Objects
    /// with scripts of their own, or named in a script, are always.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub scriptable: bool,
    /// Stays where it is put. A part that is not anchored falls when the
    /// world is played and lands on what is below it (see
    /// [`crate::physics`]).
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub anchored: bool,
    /// The creator's own named values (a price, a team, a speed), read by
    /// scripts with `object:get`. A group's apply to everything inside it
    /// that has none of the same name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, Attribute>,
    /// Words that sort objects into sets (`Lamp`, `Coin`), found by
    /// scripts with `world.tagged`. A group's tags apply to everything
    /// inside it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Fields from a newer SARE or a hand edit, written back unchanged.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Node {
    pub fn object(
        id: NodeId,
        name: impl Into<String>,
        asset: AssetRef,
        transform: Transform,
    ) -> Self {
        Self {
            id,
            name: name.into(),
            parent: None,
            model: false,
            saved_model: None,
            asset: Some(asset),
            transform,
            visible: true,
            locked: false,
            collision: true,
            gameplay: None,
            light: None,
            terrain: None,
            road: None,
            part: None,
            scripts: Vec::new(),
            scriptable: false,
            anchored: true,
            attributes: BTreeMap::new(),
            tags: Vec::new(),
            extra: Map::new(),
        }
    }
    pub fn group(id: NodeId, name: impl Into<String>) -> Self {
        Self {
            id,
            name: name.into(),
            parent: None,
            model: false,
            saved_model: None,
            asset: None,
            transform: Transform::default(),
            visible: true,
            locked: false,
            collision: true,
            gameplay: None,
            light: None,
            terrain: None,
            road: None,
            part: None,
            scripts: Vec::new(),
            scriptable: false,
            anchored: true,
            attributes: BTreeMap::new(),
            tags: Vec::new(),
            extra: Map::new(),
        }
    }
    pub fn is_group(&self) -> bool {
        self.asset.is_none()
    }
}

/// One of a node's own values: true or false, a number, a text or a
/// vector (three numbers: a position, a size, or a colour from 0 to 1).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Attribute {
    Bool(bool),
    Number(f64),
    Text(String),
    Vector([f64; 3]),
    /// A kind a newer SARE added: kept, not shown to scripts.
    Other(Value),
}
impl Attribute {
    /// Most attributes on one node.
    pub const MAX_PER_NODE: usize = 64;
    /// Most tags on one node.
    pub const MAX_TAGS: usize = 32;
    /// Longest attribute or tag name, bytes.
    pub const MAX_NAME: usize = 64;
    /// Longest text value, bytes.
    pub const MAX_TEXT: usize = 1024;

    pub fn is_valid(&self) -> bool {
        match self {
            Self::Bool(_) | Self::Other(_) => true,
            Self::Number(n) => n.is_finite(),
            Self::Text(t) => t.len() <= Self::MAX_TEXT,
            Self::Vector(v) => v.iter().all(|n| n.is_finite()),
        }
    }
    /// A name for an attribute or tag: not empty, not too long, no line
    /// breaks or control characters, no space at either end.
    pub fn valid_name(name: &str) -> bool {
        !name.is_empty()
            && name.len() <= Self::MAX_NAME
            && name.trim() == name
            && !name.chars().any(char::is_control)
    }
}

/// A light on an object, from dusk to dawn: a glowing corona at the lamp
/// that lights what is around it per pixel (the game's Lamp lighting
/// setting), or paints a soft pool of light on the ground under it when
/// that is off. It casts no shadows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PointLight {
    /// Linear RGB, 0-1.
    pub color: [f32; 3],
    /// Corona size in metres.
    #[serde(default = "light_size")]
    pub size: f32,
    /// Radius of the pool of light on the ground, in metres (0: none); the
    /// light per pixel reaches a little further, plus the lamp's height.
    #[serde(default = "light_radius")]
    pub radius: f32,
    /// Where the lamp is, in the object's own axes (metres, Blender).
    #[serde(default = "light_offset")]
    pub offset: [f32; 3],
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
fn light_size() -> f32 {
    1.0
}
fn light_radius() -> f32 {
    6.0
}
fn light_offset() -> [f32; 3] {
    [0.0, 0.0, 3.0]
}
impl PointLight {
    pub const MAX_SIZE: f32 = 6.0;
    pub const MAX_RADIUS: f32 = 12.0;
    /// A warm street lamp.
    pub fn lamp() -> Self {
        Self {
            color: [1.0, 0.85, 0.6],
            size: light_size(),
            radius: light_radius(),
            offset: light_offset(),
            extra: Map::new(),
        }
    }
    pub fn is_valid(&self) -> bool {
        self.color
            .iter()
            .all(|c| c.is_finite() && (0.0..=1.0).contains(c))
            && self.size.is_finite()
            && (0.1..=Self::MAX_SIZE).contains(&self.size)
            && self.radius.is_finite()
            && (0.0..=Self::MAX_RADIUS).contains(&self.radius)
            && self
                .offset
                .iter()
                .all(|v| v.is_finite() && v.abs() <= 100.0)
    }
}

/// A road along points, drawn on a smooth curve through them (see
/// [`crate::road`] for how it is built).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Road {
    /// Points the road runs through, in the object's own metres (x east,
    /// y north, z up), in order.
    pub points: Vec<[f32; 3]>,
    /// Width in metres.
    #[serde(default = "road_width")]
    pub width: f32,
    /// The road runs on from its last point back to its first, as a loop
    /// (with three points or more).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub closed: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
fn road_width() -> f32 {
    8.0
}
impl Road {
    pub const MAX_POINTS: usize = 256;
    pub const MIN_WIDTH: f32 = 1.0;
    pub const MAX_WIDTH: f32 = 40.0;
    /// Furthest a point may be from the object along X, Y or Z, in the
    /// object's own metres.
    pub const MAX_REACH: f32 = 2000.0;
    /// Shortest road, metres.
    pub const MIN_LENGTH: f32 = 0.5;
    /// A straight road from `from` to `to`.
    pub fn straight(from: [f32; 3], to: [f32; 3], width: f32) -> Self {
        Self {
            points: vec![from, to],
            width,
            closed: false,
            extra: Map::new(),
        }
    }
    pub fn is_valid(&self) -> bool {
        (2..=Self::MAX_POINTS).contains(&self.points.len())
            && (Self::MIN_WIDTH..=Self::MAX_WIDTH).contains(&self.width)
            && self
                .points
                .iter()
                .flatten()
                .all(|v| v.is_finite() && v.abs() <= Self::MAX_REACH)
            // Points all in one place would make a road with no surface.
            && self.length() >= Self::MIN_LENGTH
    }
}

/// A part: a basic shape built in Studio, sized in metres, coloured and
/// given a material (see [`crate::part`] for how it is built). Its size is
/// its own; the transform's scale still applies on top.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    #[serde(default)]
    pub shape: PartShape,
    /// Size in metres along the object's own axes: east, north, up.
    pub size: [f32; 3],
    /// Linear RGB, 0-1.
    #[serde(default = "part_color")]
    pub color: [f32; 3],
    #[serde(default)]
    pub material: PartMaterial,
    /// 0 solid to 1 invisible (still solid to walk on unless Collision is
    /// off).
    #[serde(default, skip_serializing_if = "is_zero")]
    pub transparency: f32,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
fn part_color() -> [f32; 3] {
    [0.64, 0.64, 0.66]
}
fn is_zero(v: &f32) -> bool {
    *v == 0.0
}

/// The shape of a [`Part`]. A name this SARE does not know is kept and
/// drawn as a block.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartShape {
    #[default]
    Block,
    /// A ball (stretched by an uneven size).
    Ball,
    /// Upright, round across its east and north size.
    Cylinder,
    /// A slope rising to the north.
    Wedge,
    /// A slope rising to its north-east corner.
    CornerWedge,
    #[serde(untagged)]
    Other(String),
}
impl PartShape {
    pub const ALL: [Self; 5] = [
        Self::Block,
        Self::Ball,
        Self::Cylinder,
        Self::Wedge,
        Self::CornerWedge,
    ];
    pub fn label(&self) -> &str {
        match self {
            Self::Block => "Block",
            Self::Ball => "Ball",
            Self::Cylinder => "Cylinder",
            Self::Wedge => "Wedge",
            Self::CornerWedge => "Corner wedge",
            Self::Other(name) => name,
        }
    }
}

/// The surface of a [`Part`]: a pattern its colour is painted over. A name
/// this SARE does not know is kept and drawn as Plastic.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PartMaterial {
    #[default]
    Plastic,
    Smooth,
    Concrete,
    Wood,
    Brick,
    Tiles,
    Metal,
    /// See-through even with Transparency 0.
    Glass,
    #[serde(untagged)]
    Other(String),
}
impl PartMaterial {
    pub const ALL: [Self; 8] = [
        Self::Plastic,
        Self::Smooth,
        Self::Concrete,
        Self::Wood,
        Self::Brick,
        Self::Tiles,
        Self::Metal,
        Self::Glass,
    ];
    pub fn label(&self) -> &str {
        match self {
            Self::Plastic => "Plastic",
            Self::Smooth => "Smooth",
            Self::Concrete => "Concrete",
            Self::Wood => "Wood",
            Self::Brick => "Brick",
            Self::Tiles => "Tiles",
            Self::Metal => "Metal",
            Self::Glass => "Glass",
            Self::Other(name) => name,
        }
    }
}

impl Part {
    /// Smallest and largest side, metres.
    pub const MIN_SIZE: f32 = 0.05;
    pub const MAX_SIZE: f32 = 2048.0;
    pub fn new(shape: PartShape, size: [f32; 3]) -> Self {
        Self {
            shape,
            size,
            color: part_color(),
            material: PartMaterial::default(),
            transparency: 0.0,
            extra: Map::new(),
        }
    }
    pub fn is_valid(&self) -> bool {
        self.size
            .iter()
            .all(|s| (Self::MIN_SIZE..=Self::MAX_SIZE).contains(s))
            && self.color.iter().all(|c| (0.0..=1.0).contains(c))
            && (0.0..=1.0).contains(&self.transparency)
    }
}

/// Ground sculpted in Build Mode: a square height grid (see
/// [`crate::terrain`] for the layout of `heights` and the brushes).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Terrain {
    /// Side length in metres.
    pub size: f32,
    /// Squares per side.
    pub cells: u16,
    /// One height per corner, (cells + 1)², rows south to north.
    pub heights: Vec<f32>,
    /// Ground colour, linear RGB 0-1.
    #[serde(default = "ground_color")]
    pub color: [f32; 3],
    /// The material of each square, cells², rows south to north (an index
    /// in [`TERRAIN_MATERIALS`], or [`Terrain::HOLE`]); empty: all ground.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paint: Vec<u8>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// What terrain squares can be painted with: a name and a colour (linear
/// RGB). The first is the terrain's own colour ([`Terrain::color`]).
pub const TERRAIN_MATERIALS: [(&str, [f32; 3]); 6] = [
    ("Ground", [0.36, 0.5, 0.27]),
    ("Grass", [0.3, 0.52, 0.2]),
    ("Dirt", [0.42, 0.31, 0.2]),
    ("Sand", [0.76, 0.69, 0.5]),
    ("Rock", [0.44, 0.43, 0.42]),
    ("Snow", [0.9, 0.92, 0.95]),
];
fn ground_color() -> [f32; 3] {
    [0.36, 0.5, 0.27]
}
impl Terrain {
    /// A square cut out: nothing is drawn there and nothing is solid.
    pub const HOLE: u8 = 255;
    /// Highest hill or deepest pit, metres.
    pub const MAX_HEIGHT: f32 = 200.0;
    pub const MAX_CELLS: u16 = 128;
    /// Flat ground `size` metres across in `cells` × `cells` squares.
    pub fn flat(size: f32, cells: u16) -> Self {
        let side = usize::from(cells) + 1;
        Self {
            size,
            cells,
            heights: vec![0.0; side * side],
            color: ground_color(),
            paint: Vec::new(),
            extra: Map::new(),
        }
    }
    pub fn is_valid(&self) -> bool {
        (1..=Self::MAX_CELLS).contains(&self.cells)
            && self.size.is_finite()
            && (4.0..=2048.0).contains(&self.size)
            && self.heights.len() == (usize::from(self.cells) + 1).pow(2)
            && self
                .heights
                .iter()
                .all(|h| h.is_finite() && h.abs() <= Self::MAX_HEIGHT)
            && self
                .color
                .iter()
                .all(|c| c.is_finite() && (0.0..=1.0).contains(c))
            && (self.paint.is_empty()
                || (self.paint.len() == usize::from(self.cells).pow(2)
                    && self
                        .paint
                        .iter()
                        .all(|m| usize::from(*m) < TERRAIN_MATERIALS.len() || *m == Self::HOLE)))
    }
    /// The material of the square at (column, row).
    pub fn material(&self, column: usize, row: usize) -> u8 {
        self.paint
            .get(row * usize::from(self.cells) + column)
            .copied()
            .unwrap_or(0)
    }
    /// The colour material `index` paints (the terrain's own for 0).
    pub fn material_color(&self, index: u8) -> [f32; 3] {
        match index {
            0 => self.color,
            i => TERRAIN_MATERIALS
                .get(usize::from(i))
                .map_or(self.color, |(_, c)| *c),
        }
    }
}

/// World-wide settings stored in the layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldSettings {
    /// Fixed hour of day (0-23) while playing this world, or None to let
    /// the clock run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hour: Option<u8>,
    /// Weather while playing (`clear`, `cloudy`, `rain`, `fog`; see
    /// [`crate::world_logic::Weather`]), or None for the game's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weather: Option<String>,
    /// Haze density multiplier (1 = the time and weather's own), 0.2-10.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fog: Option<f32>,
    /// Sky and haze colour, linear RGB 0-1, instead of the time of day's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sky: Option<[f32; 3]>,
    /// Brightness multiplier of the scene's ambient light, 0.3-1.5.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brightness: Option<f32>,
    /// Lua scripts for the whole world (no `script.object`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<Script>,
    /// The world's colour look over each player's own picture settings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub look: Option<WorldLook>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Default for WorldSettings {
    fn default() -> Self {
        Self {
            hour: Some(12),
            weather: None,
            fog: None,
            sky: None,
            brightness: None,
            scripts: Vec::new(),
            look: None,
            extra: Map::new(),
        }
    }
}

/// A world's colour look: how its picture is graded on every player's
/// screen. Each value works on top of the player's own settings (a player
/// who turned bloom off keeps it off), so a look is cheap on any PC.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct WorldLook {
    /// Brightness multiplier, 0.5-2.
    #[serde(default = "unit")]
    pub exposure: f32,
    /// Contrast around mid grey, 0.5-1.6 (1: as drawn).
    #[serde(default = "unit")]
    pub contrast: f32,
    /// Colour strength, 0 (black and white) to 2.
    #[serde(default = "unit")]
    pub saturation: f32,
    /// Colour temperature, -1 (cold blue) to 1 (warm orange).
    #[serde(default)]
    pub warmth: f32,
    /// Glow around bright light, 0-3 (multiplies the player's bloom).
    #[serde(default = "unit")]
    pub bloom: f32,
    /// Darker screen corners, 0-0.8 (added to the player's own).
    #[serde(default)]
    pub vignette: f32,
}
fn unit() -> f32 {
    1.0
}
impl Default for WorldLook {
    fn default() -> Self {
        Self {
            exposure: 1.0,
            contrast: 1.0,
            saturation: 1.0,
            warmth: 0.0,
            bloom: 1.0,
            vignette: 0.0,
        }
    }
}
impl WorldLook {
    /// Ready-made looks: (name, look). The first is the picture as drawn.
    pub fn presets() -> [(&'static str, WorldLook); 8] {
        let look = |exposure, contrast, saturation, warmth, bloom, vignette| WorldLook {
            exposure,
            contrast,
            saturation,
            warmth,
            bloom,
            vignette,
        };
        [
            ("Classic", WorldLook::default()),
            ("Natural", look(1.0, 1.06, 0.95, 0.06, 0.8, 0.1)),
            ("Vivid", look(1.05, 1.15, 1.35, 0.0, 1.2, 0.1)),
            ("Cinematic", look(0.95, 1.22, 0.85, 0.12, 1.3, 0.32)),
            ("Sunset", look(1.05, 1.1, 1.15, 0.5, 1.6, 0.2)),
            ("Cold night", look(0.9, 1.12, 0.8, -0.45, 1.0, 0.25)),
            ("Dreamy", look(1.1, 0.85, 1.1, 0.15, 2.5, 0.15)),
            ("Noir", look(0.95, 1.38, 0.0, 0.0, 0.8, 0.42)),
        ]
    }
    /// The name of the preset this look is, if any.
    pub fn preset_name(&self) -> Option<&'static str> {
        Self::presets()
            .into_iter()
            .find(|(_, look)| look == self)
            .map(|(name, _)| name)
    }
    pub fn is_valid(&self) -> bool {
        let within = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
        within(self.exposure, 0.5, 2.0)
            && within(self.contrast, 0.5, 1.6)
            && within(self.saturation, 0.0, 2.0)
            && within(self.warmth, -1.0, 1.0)
            && within(self.bloom, 0.0, 3.0)
            && within(self.vignette, 0.0, 0.8)
    }
    /// Red, green and blue multipliers for [`Self::warmth`].
    pub fn tint(&self) -> [f32; 3] {
        let w = self.warmth.clamp(-1.0, 1.0);
        [1.0 + 0.14 * w, 1.0 + 0.02 * w, 1.0 - 0.16 * w]
    }
    pub fn is_javascript(&self) -> bool {
        self.language == Self::JAVASCRIPT
    }
    /// A language this SARE runs (a newer one's are kept, not run).
    pub fn is_known_language(&self) -> bool {
        self.language == Self::LUA || self.is_javascript()
    }
}

/// A Lua script in a world: on an object, or for the whole world in its
/// [`WorldSettings`]. See `docs/world-scripts.md` for what scripts can do and
/// `sa_lua::world` for how they run.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Script {
    pub name: String,
    /// Lua source text.
    #[serde(default)]
    pub source: String,
    /// Off: kept, but not run.
    #[serde(default = "yes", skip_serializing_if = "Clone::clone")]
    pub enabled: bool,
    /// `lua` (when not written) or `javascript`. Both have the same API.
    #[serde(default = "lua", skip_serializing_if = "is_lua")]
    pub language: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
fn lua() -> String {
    Script::LUA.into()
}
fn is_lua(language: &String) -> bool {
    language == Script::LUA
}
impl Script {
    /// Longest source, bytes.
    pub const MAX_SOURCE: usize = 64 * 1024;
    /// Most scripts in one world.
    pub const MAX_SCRIPTS: usize = 64;
    pub const LUA: &'static str = "lua";
    pub const JAVASCRIPT: &'static str = "javascript";
    pub fn new(name: impl Into<String>, source: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: source.into(),
            enabled: true,
            language: Self::LUA.into(),
            extra: Map::new(),
        }
    }
}

/// A node's flags after its groups (see [`Layout::effective_all`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Effective {
    pub visible: bool,
    pub locked: bool,
    pub collision: bool,
}

/// A whole Build Mode layout.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layout {
    pub format: String,
    pub version: u32,
    /// Next id handed out; ids are never reused, even after deletes.
    pub next_id: NodeId,
    #[serde(default)]
    pub settings: WorldSettings,
    /// Tree order: children follow the order they appear here.
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            format: FORMAT.into(),
            version: VERSION,
            next_id: 1,
            settings: WorldSettings::default(),
            nodes: Vec::new(),
            extra: Map::new(),
        }
    }
}

/// Upgrade an older layout document to [`VERSION`]. Each step takes the
/// document from version `n` to `n + 1`; add a step here whenever the format
/// changes, and keep the old ones so every released file still opens.
pub fn migrate(mut doc: Value) -> Result<Value> {
    let object = doc
        .as_object_mut()
        .context("a layout must be a JSON object")?;
    ensure!(
        object.get("format").and_then(Value::as_str) == Some(FORMAT),
        "not a SARE world layout (format must be {FORMAT:?})"
    );
    let version = object
        .get("version")
        .and_then(Value::as_u64)
        .context("the layout has no version number")?;
    ensure!(
        version <= u64::from(VERSION),
        "this layout was saved by a newer SARE (format version {version}, this SARE reads up to {VERSION}); update SARE to open it"
    );
    ensure!(version >= 1, "layout version {version} does not exist");
    let mut at = version;
    while at < u64::from(VERSION) {
        at = migrate_step(object, at)?;
    }
    object.insert("version".into(), Value::from(VERSION));
    Ok(doc)
}

/// Turn a version `from` document into version `from + 1`.
fn migrate_step(_doc: &mut Map<String, Value>, from: u64) -> Result<u64> {
    match from {
        // 2 gave groups a pivot: version 1 groups had none (their transform
        // was not used), and it reads the same.
        1 => Ok(2),
        // Add one arm per format change, e.g.:
        // 2 => { rename or convert fields in `_doc`; Ok(3) }
        _ => bail!("no migration from layout version {from}"),
    }
}

impl Layout {
    /// Read a layout from JSON text, migrating older versions.
    pub fn from_json(text: &str) -> Result<Self> {
        let doc: Value = serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .context("the layout is not valid JSON")?;
        let layout: Self = serde_json::from_value(migrate(doc)?)
            .context("the layout does not match the SARE world layout format")?;
        layout.validate()?;
        Ok(layout)
    }
    pub fn to_json(&self) -> Result<String> {
        self.validate()?;
        // Version 2 added group pivots. A world without any is written as
        // version 1, so older SARE versions still open it.
        // (Written from the layout itself, not a JSON value, so numbers keep
        // their short form.)
        let version = if self.has_pivots() { VERSION } else { 1 };
        if version == self.version {
            return Ok(serde_json::to_string_pretty(self)? + "\n");
        }
        let mut written = self.clone();
        written.version = version;
        Ok(serde_json::to_string_pretty(&written)? + "\n")
    }
    pub fn read(path: &std::path::Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::from_json(&text).with_context(|| format!("in {}", path.display()))
    }
    /// Write atomically: a temporary file next to `path`, then a rename, so
    /// a crash mid-save never leaves a half-written layout behind.
    pub fn write(&self, path: &std::path::Path) -> Result<()> {
        write_atomic(path, self.to_json()?.as_bytes())
    }
    /// Every script in the world, in order: the world's own first (None),
    /// then each object's (its id).
    pub fn all_scripts(&self) -> Vec<(Option<NodeId>, &Script)> {
        self.settings
            .scripts
            .iter()
            .map(|s| (None, s))
            .chain(
                self.nodes
                    .iter()
                    .flat_map(|n| n.scripts.iter().map(move |s| (Some(n.id), s))),
            )
            .collect()
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(self.format == FORMAT, "not a SARE world layout");
        let s = &self.settings;
        ensure!(
            s.fog
                .is_none_or(|f| f.is_finite() && (0.2..=10.0).contains(&f))
                && s.brightness
                    .is_none_or(|b| b.is_finite() && (0.3..=1.5).contains(&b))
                && s.sky
                    .is_none_or(|c| c.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)))
                && s.look.as_ref().is_none_or(WorldLook::is_valid),
            "the world's fog, sky colour, brightness or look is out of range"
        );
        ensure!(
            self.version == VERSION,
            "layout version {} not migrated",
            self.version
        );
        ensure!(
            self.nodes.len() <= MAX_NODES,
            "a layout may hold at most {MAX_NODES} objects"
        );
        let mut ids = HashSet::new();
        for node in &self.nodes {
            ensure!(node.id != 0, "object ids start at 1");
            ensure!(ids.insert(node.id), "object id {} is used twice", node.id);
            ensure!(
                node.id < self.next_id,
                "object id {} is not below next_id {}",
                node.id,
                self.next_id
            );
            ensure!(
                node.name.len() <= 256,
                "object {} has a very long name",
                node.id
            );
            ensure!(
                node.transform.is_valid(),
                "object {} ({}) has an invalid position, rotation or scale",
                node.id,
                node.name
            );
            ensure!(
                node.road.as_ref().is_none_or(Road::is_valid),
                "object {} ({}) has a road with too few or too many points, a bad width or a point too far away",
                node.id,
                node.name
            );
            ensure!(
                node.part.as_ref().is_none_or(Part::is_valid),
                "object {} ({}) is a part with a size outside {} to {} m, or a colour or transparency outside 0 to 1",
                node.id,
                node.name,
                Part::MIN_SIZE,
                Part::MAX_SIZE
            );
            ensure!(
                node.saved_model.as_deref().is_none_or(|path| {
                    node.is_group() && crate::saved_model::is_saved_model(path)
                        && AssetRef::parse(&format!("self:{path}")).is_ok()
                }),
                "object {} ({}) links to a saved model, but is not a group or the path is not a .model.json file in the world's folder",
                node.id,
                node.name
            );
            ensure!(
                node.attributes.len() <= Attribute::MAX_PER_NODE
                    && node
                        .attributes
                        .iter()
                        .all(|(name, value)| Attribute::valid_name(name) && value.is_valid()),
                "object {} ({}) has more than {} attributes, a bad attribute name, a number that is not finite or a text over {} bytes",
                node.id,
                node.name,
                Attribute::MAX_PER_NODE,
                Attribute::MAX_TEXT
            );
            ensure!(
                node.tags.len() <= Attribute::MAX_TAGS
                    && node.tags.iter().all(|t| Attribute::valid_name(t))
                    && node.tags.iter().collect::<HashSet<_>>().len() == node.tags.len(),
                "object {} ({}) has more than {} tags, a bad tag or the same tag twice",
                node.id,
                node.name,
                Attribute::MAX_TAGS
            );
            ensure!(
                node.terrain.as_ref().is_none_or(Terrain::is_valid),
                "object {} ({}) has terrain with a bad size, grid, heights or paint",
                node.id,
                node.name
            );
            ensure!(
                node.light.as_ref().is_none_or(PointLight::is_valid),
                "object {} ({}) has a light with an invalid colour, size, radius or position",
                node.id,
                node.name
            );
        }
        let scripts = self.all_scripts();
        ensure!(
            scripts.len() <= Script::MAX_SCRIPTS,
            "a world may hold at most {} scripts",
            Script::MAX_SCRIPTS
        );
        for (_, script) in &scripts {
            ensure!(
                script.name.len() <= 256 && script.source.len() <= Script::MAX_SOURCE,
                "script {} is longer than {} KiB or has a very long name",
                script.name,
                Script::MAX_SOURCE / 1024
            );
        }
        let groups: HashMap<NodeId, Option<NodeId>> = self
            .nodes
            .iter()
            .filter(|n| n.is_group())
            .map(|n| (n.id, n.parent))
            .collect();
        for node in &self.nodes {
            let Some(parent) = node.parent else { continue };
            ensure!(
                groups.contains_key(&parent),
                "object {} ({}) is inside {parent}, which is not a group",
                node.id,
                node.name
            );
            // Walk up: a group may not end up inside itself.
            let mut seen = HashSet::from([node.id]);
            let mut at = Some(parent);
            while let Some(id) = at {
                ensure!(
                    seen.insert(id),
                    "groups inside each other form a loop at {id}"
                );
                at = groups.get(&id).copied().flatten();
            }
        }
        Ok(())
    }
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }
    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }
    pub fn index_of(&self, id: NodeId) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }
    /// Hand out a fresh id.
    pub fn allocate_id(&mut self) -> NodeId {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        id
    }
    /// Shown in game: the node and every group above it are visible.
    pub fn effective_visible(&self, id: NodeId) -> bool {
        self.ancestry(id).all(|n| n.visible)
    }
    /// The node's attributes with those of the groups it is in: its own
    /// first, then the nearest group's, and so on.
    pub fn effective_attributes(&self, id: NodeId) -> BTreeMap<String, Attribute> {
        let mut all = BTreeMap::new();
        for node in self.ancestry(id) {
            for (name, value) in &node.attributes {
                all.entry(name.clone()).or_insert_with(|| value.clone());
            }
        }
        all
    }
    /// The node's tags with those of the groups it is in.
    pub fn effective_tags(&self, id: NodeId) -> BTreeSet<String> {
        self.ancestry(id)
            .flat_map(|n| n.tags.iter().cloned())
            .collect()
    }
    /// Locked itself or inside a locked group.
    pub fn effective_locked(&self, id: NodeId) -> bool {
        self.ancestry(id).any(|n| n.locked)
    }
    /// Collides in game: the node and every group above it have collision on.
    pub fn effective_collision(&self, id: NodeId) -> bool {
        self.ancestry(id).all(|n| n.collision)
    }
    /// Visible, locked and collision after groups, for every node at once:
    /// one pass with an id index instead of a search per node.
    pub fn effective_all(&self) -> HashMap<NodeId, Effective> {
        let by_id: HashMap<NodeId, &Node> = self.nodes.iter().map(|n| (n.id, n)).collect();
        self.nodes
            .iter()
            .map(|node| {
                let mut flags = Effective {
                    visible: true,
                    locked: false,
                    collision: true,
                };
                let mut at = Some(node);
                for _ in 0..64 {
                    let Some(step) = at else { break };
                    flags.visible &= step.visible;
                    flags.locked |= step.locked;
                    flags.collision &= step.collision;
                    at = step.parent.and_then(|p| by_id.get(&p).copied());
                }
                (node.id, flags)
            })
            .collect()
    }
    /// The node, then its group, then that group's group, and so on.
    pub fn ancestry(&self, id: NodeId) -> impl Iterator<Item = &Node> + '_ {
        let mut at = self.node(id);
        let mut steps = 0;
        std::iter::from_fn(move || {
            let node = at?;
            steps += 1;
            at = node
                .parent
                .and_then(|p| self.node(p))
                .filter(|_| steps < 64);
            Some(node)
        })
    }
    /// Where node `id` is in the world: its transform placed in every group
    /// above it (groups pass on position and rotation, not scale).
    pub fn world_transform(&self, id: NodeId) -> Transform {
        let mut chain: Vec<&Node> = self.ancestry(id).collect();
        let Some(node) = chain.first() else {
            return Transform::default();
        };
        let mut world = node.transform;
        for group in chain.drain(..).skip(1) {
            world = group.transform.place(&world);
        }
        world
    }
    /// The transform node `id` would need for `world` inside `parent`
    /// (None: the top level).
    pub fn local_in(&self, parent: Option<NodeId>, world: &Transform) -> Transform {
        match parent {
            Some(group) => self.world_transform(group).local_of(world),
            None => *world,
        }
    }
    /// Every node's world transform at once, by id.
    pub fn world_transforms(&self) -> HashMap<NodeId, Transform> {
        let by_id: HashMap<NodeId, &Node> = self.nodes.iter().map(|n| (n.id, n)).collect();
        let moved = self
            .nodes
            .iter()
            .any(|n| n.is_group() && !n.transform.is_unmoved());
        self.nodes
            .iter()
            .map(|node| {
                let mut world = node.transform;
                if moved {
                    let mut at = node.parent.and_then(|p| by_id.get(&p).copied());
                    for _ in 0..64 {
                        let Some(group) = at else { break };
                        world = group.transform.place(&world);
                        at = group.parent.and_then(|p| by_id.get(&p).copied());
                    }
                }
                (node.id, world)
            })
            .collect()
    }
    /// A group has a pivot of its own (it is moved or turned), so the
    /// layout needs format version 2.
    pub fn has_pivots(&self) -> bool {
        self.nodes
            .iter()
            .any(|n| n.is_group() && !n.transform.is_unmoved())
    }
    /// The layout for playing: every object's transform in world space, so
    /// the game, servers and scripts need not know about group pivots.
    pub fn resolved(&self) -> Layout {
        let mut out = self.clone();
        if !self.has_pivots() {
            return out;
        }
        let world = self.world_transforms();
        for node in &mut out.nodes {
            node.transform = world[&node.id];
        }
        out
    }
    /// Every node inside group `id`, at any depth.
    pub fn descendants(&self, id: NodeId) -> Vec<NodeId> {
        let mut out = Vec::new();
        let mut stack = vec![id];
        while let Some(group) = stack.pop() {
            for node in self.nodes.iter().filter(|n| n.parent == Some(group)) {
                if !out.contains(&node.id) {
                    out.push(node.id);
                    stack.push(node.id);
                }
            }
        }
        out
    }
}

/// Write `bytes` to `path` through a temporary file and a rename.
pub fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let folder = path.parent().unwrap_or(std::path::Path::new("."));
    std::fs::create_dir_all(folder).with_context(|| format!("creating {}", folder.display()))?;
    let name = path
        .file_name()
        .context("the save path has no file name")?
        .to_string_lossy();
    let temporary = folder.join(format!(".{name}.saving"));
    std::fs::write(&temporary, bytes)
        .with_context(|| format!("writing {}", temporary.display()))?;
    std::fs::rename(&temporary, path).with_context(|| format!("saving {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn looks_are_valid_named_and_saved_only_when_set() {
        let presets = WorldLook::presets();
        assert_eq!(presets[0], ("Classic", WorldLook::default()));
        for (name, look) in presets {
            assert!(look.is_valid(), "{name}");
            assert_eq!(look.preset_name(), Some(name));
        }
        let warm = WorldLook {
            warmth: 1.0,
            ..WorldLook::default()
        };
        let [r, _, b] = warm.tint();
        assert!(r > 1.0 && b < 1.0);
        assert_eq!(warm.preset_name(), None);
        let mut layout = Layout::default();
        assert!(!serde_json::to_string(&layout).unwrap().contains("look"));
        layout.settings.look = Some(WorldLook {
            contrast: 9.0,
            ..WorldLook::default()
        });
        assert!(layout.validate().is_err());
    }

    fn sample() -> Layout {
        let mut layout = Layout::default();
        let group = layout.allocate_id();
        layout.nodes.push(Node::group(group, "Props"));
        let id = layout.allocate_id();
        let mut crate_node = Node::object(
            id,
            "Crate",
            AssetRef::Builtin("crate".into()),
            Transform {
                position: [3.0, -4.5, 0.5],
                rotation: [0.0, 0.0, 90.0],
                scale: [2.0, 1.0, 1.0],
            },
        );
        crate_node.parent = Some(group);
        layout.nodes.push(crate_node);
        let id = layout.allocate_id();
        layout.nodes.push(Node::object(
            id,
            "House",
            AssetRef::Pack {
                id: "example-town".into(),
                path: "world.glb".into(),
            },
            Transform::at([10.0, 0.0, 0.0]),
        ));
        layout
    }

    #[test]
    fn layouts_round_trip_through_json() {
        let layout = sample();
        let text = layout.to_json().unwrap();
        assert!(text.contains("\"asset\": \"builtin:crate\""));
        assert!(text.contains("\"pack:example-town:world.glb\""));
        assert_eq!(Layout::from_json(&text).unwrap(), layout);
    }

    #[test]
    fn unknown_fields_survive_a_round_trip() {
        let mut doc: Value = serde_json::to_value(sample()).unwrap();
        doc["future_lighting"] = serde_json::json!({"sun": 3});
        doc["nodes"][1]["script"] = Value::from("on_touch.lua");
        doc["settings"]["fog"] = Value::from(0.4);
        let layout = Layout::from_json(&doc.to_string()).unwrap();
        let again: Value = serde_json::from_str(&layout.to_json().unwrap()).unwrap();
        assert_eq!(again["future_lighting"]["sun"], 3);
        assert_eq!(again["nodes"][1]["script"], "on_touch.lua");
        assert_eq!(again["settings"]["fog"], 0.4);
    }

    #[test]
    fn versions_are_checked_and_migrated() {
        let mut doc: Value = serde_json::to_value(sample()).unwrap();
        doc["version"] = Value::from(VERSION + 1);
        let error = Layout::from_json(&doc.to_string()).unwrap_err();
        assert!(format!("{error:#}").contains("newer SARE"));
        doc["version"] = Value::from(0);
        assert!(Layout::from_json(&doc.to_string()).is_err());
        doc.as_object_mut().unwrap().remove("version");
        assert!(Layout::from_json(&doc.to_string()).is_err());
        doc["version"] = Value::from(VERSION);
        doc["format"] = Value::from("something-else");
        assert!(Layout::from_json(&doc.to_string()).is_err());
        // The current version passes through migrate unchanged.
        let current = serde_json::to_value(sample()).unwrap();
        assert_eq!(migrate(current.clone()).unwrap(), current);
    }

    #[test]
    fn broken_layouts_are_refused_not_half_read() {
        let mut layout = sample();
        layout.nodes[2].id = layout.nodes[1].id;
        assert!(layout.validate().is_err());
        let mut layout = sample();
        layout.nodes[1].parent = Some(layout.nodes[2].id); // not a group
        assert!(layout.validate().is_err());
        let mut layout = sample();
        layout.nodes[0].parent = Some(layout.nodes[0].id); // group in itself
        assert!(layout.validate().is_err());
        let mut layout = sample();
        layout.nodes[1].transform.scale = [0.0, 1.0, 1.0];
        assert!(layout.validate().is_err());
        let mut layout = sample();
        layout.next_id = 2;
        assert!(layout.validate().is_err());
    }

    #[test]
    fn asset_references_stay_portable() {
        for text in [
            "builtin:cube",
            "self:assets/props/bench.glb",
            "pack:town:a/b.glb",
        ] {
            assert_eq!(AssetRef::parse(text).unwrap().to_string(), text);
        }
        for bad in [
            "C:/Users/me/bench.glb",
            "self:../escape.glb",
            "self:/abs.glb",
            "self:a\\b.glb",
            "pack:town",
            "pack::x.glb",
            "builtin:",
        ] {
            assert!(AssetRef::parse(bad).is_err(), "{bad} should be refused");
        }
    }

    #[test]
    fn transforms_follow_blender_axes() {
        // A point 1 m east of the model origin, object turned 90 degrees
        // about up: it ends up 1 m north (glTF -Z).
        let t = Transform {
            position: [10.0, 20.0, 3.0],
            rotation: [0.0, 0.0, 90.0],
            scale: [1.0; 3],
        };
        let p = t.matrix().transform_point3(Vec3::X);
        let expected = blender_to_gltf([10.0, 21.0, 3.0]);
        assert!(p.distance(expected) < 1e-4, "{p} vs {expected}");
        // Scale along Blender Z (up) stretches glTF Y.
        let tall = Transform {
            scale: [1.0, 1.0, 3.0],
            ..Transform::default()
        };
        assert!(
            tall.matrix()
                .transform_point3(Vec3::Y)
                .distance(Vec3::Y * 3.0)
                < 1e-5
        );
        assert_eq!(
            gltf_to_blender(blender_to_gltf([1.0, 2.0, 3.0])),
            [1.0, 2.0, 3.0]
        );
    }

    #[test]
    fn groups_pass_visibility_lock_and_collision_down() {
        let mut layout = sample();
        let (group, child, other) = (layout.nodes[0].id, layout.nodes[1].id, layout.nodes[2].id);
        layout.node_mut(group).unwrap().visible = false;
        layout.node_mut(group).unwrap().locked = true;
        layout.node_mut(group).unwrap().collision = false;
        assert!(!layout.effective_visible(child));
        assert!(layout.effective_locked(child));
        assert!(!layout.effective_collision(child));
        assert!(layout.effective_visible(other));
        assert_eq!(layout.descendants(group), vec![child]);
        let all = layout.effective_all();
        for node in &layout.nodes {
            let id = node.id;
            assert_eq!(
                all[&id],
                Effective {
                    visible: layout.effective_visible(id),
                    locked: layout.effective_locked(id),
                    collision: layout.effective_collision(id),
                }
            );
        }
    }

    #[test]
    fn saves_are_atomic_and_readable() {
        let dir = std::env::temp_dir().join(format!("sare-layout-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(FILE);
        sample().write(&path).unwrap();
        assert_eq!(Layout::read(&path).unwrap(), sample());
        assert!(!dir.join(".layout.json.saving").exists());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
    }

    #[test]
    fn group_pivots_place_their_contents() {
        let mut layout = Layout::default();
        let mut model = Node::group(1, "Car");
        model.transform = Transform {
            position: [10.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 90.0],
            scale: [1.0; 3],
        };
        let mut wheel = Node::object(
            2,
            "Wheel",
            AssetRef::Builtin("cube".into()),
            Transform {
                position: [2.0, 0.0, 0.0],
                rotation: [0.0, 0.0, 10.0],
                scale: [2.0, 1.0, 1.0],
            },
        );
        wheel.parent = Some(1);
        layout.nodes = vec![model, wheel];
        layout.next_id = 3;
        // 2 m east of a pivot turned 90° is 2 m north of it.
        let world = layout.world_transform(2);
        assert!(close(world.position, [10.0, 2.0, 0.0]), "{world:?}");
        assert!(close(world.rotation, [0.0, 0.0, 100.0]), "{world:?}");
        assert_eq!(world.scale, [2.0, 1.0, 1.0], "groups do not scale");
        assert_eq!(layout.world_transforms()[&2], world);
        // And back.
        let local = layout.local_in(Some(1), &world);
        assert!(close(local.position, [2.0, 0.0, 0.0]) && close(local.rotation, [0.0, 0.0, 10.0]));
        // Played: everything in world space.
        let played = layout.resolved();
        assert_eq!(played.node(2).unwrap().transform, world);
        // Pivots need version 2; a world without them stays version 1, so
        // older SARE versions still open it.
        assert!(layout.to_json().unwrap().contains("\"version\": 2"));
        let read = Layout::from_json(&layout.to_json().unwrap()).unwrap();
        assert_eq!(read.world_transform(2), world);
        layout.nodes[0].transform = Transform::default();
        assert!(layout.to_json().unwrap().contains("\"version\": 1"));
        assert_eq!(
            layout.resolved().node(2).unwrap().transform,
            layout.nodes[1].transform
        );
    }

    #[test]
    fn attributes_and_tags_pass_down_from_groups_and_survive_a_round_trip() {
        let mut layout = Layout::default();
        let mut street = Node::group(1, "Street");
        street.tags = vec!["Lamp".into()];
        street
            .attributes
            .insert("Team".into(), Attribute::Text("Blue".into()));
        street
            .attributes
            .insert("Speed".into(), Attribute::Number(2.0));
        let mut lamp = Node::object(
            2,
            "Lamp",
            AssetRef::Builtin("cube".into()),
            Transform::default(),
        );
        lamp.parent = Some(1);
        lamp.tags = vec!["Light".into()];
        lamp.attributes
            .insert("Speed".into(), Attribute::Number(7.5));
        lamp.attributes.insert("On".into(), Attribute::Bool(true));
        lamp.attributes
            .insert("Tint".into(), Attribute::Vector([1.0, 0.5, 0.0]));
        layout.nodes = vec![street, lamp];
        layout.next_id = 3;
        let all = layout.effective_attributes(2);
        assert_eq!(all["Speed"], Attribute::Number(7.5), "its own come first");
        assert_eq!(all["Team"], Attribute::Text("Blue".into()));
        assert_eq!(
            layout.effective_tags(2).into_iter().collect::<Vec<_>>(),
            ["Lamp", "Light"]
        );
        let text = layout.to_json().unwrap();
        assert!(text.contains("\"Tint\": [") && text.contains("\"tags\": ["));
        assert_eq!(Layout::from_json(&text).unwrap(), layout);
        // A kind from a newer SARE is kept.
        let newer = text.replace("\"On\": true", "\"On\": {\"curve\": [1, 2]}");
        let read = Layout::from_json(&newer).unwrap();
        assert!(matches!(
            read.nodes[1].attributes["On"],
            Attribute::Other(_)
        ));
        assert!(read.to_json().unwrap().contains("curve"));
        // Bad names and repeated tags are refused.
        layout.nodes[1].tags.push("Light".into());
        assert!(layout.validate().is_err());
        layout.nodes[1].tags.pop();
        layout.nodes[1]
            .attributes
            .insert(" Padded".into(), Attribute::Bool(false));
        assert!(layout.validate().is_err());
    }

    #[test]
    fn scripts_say_their_language_and_lua_stays_unwritten() {
        let mut layout = Layout::default();
        let mut js = Script::new("Door", "print('hi');");
        js.language = Script::JAVASCRIPT.into();
        layout.settings.scripts = vec![Script::new("Old", "print('hi')"), js];
        let text = layout.to_json().unwrap();
        assert_eq!(
            text.matches("\"language\"").count(),
            1,
            "only JavaScript says so"
        );
        let read = Layout::from_json(&text).unwrap();
        assert!(!read.settings.scripts[0].is_javascript());
        assert!(read.settings.scripts[1].is_javascript());
        let sources = crate::world_logic::script_sources(&read);
        assert_eq!(
            sources.iter().map(|s| s.javascript).collect::<Vec<_>>(),
            [false, true]
        );
        // A language from a newer SARE is kept but not run.
        let newer = text.replace("\"javascript\"", "\"python\"");
        let read = Layout::from_json(&newer).unwrap();
        assert_eq!(read.settings.scripts[1].language, "python");
        assert_eq!(crate::world_logic::script_sources(&read).len(), 1);
    }
}
