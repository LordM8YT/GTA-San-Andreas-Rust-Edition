//! Built-in Build Mode assets: simple original shapes generated in code, so a
//! creator can block out a world on a clean machine with no downloads and no
//! game files. Referenced from layouts as `builtin:<name>`.
//!
//! Every model is in glTF space (Y up, -Z north, metres) with its origin at
//! the centre of its base, so placing one on the ground needs no offset.
use crate::gltf_model::Model;
use crate::{Batch, Vertex};
use glam::Vec3;
use sa_assets::Texture;
use std::collections::HashMap;

/// Texture key shared by every built-in shape (plain white, vertex coloured).
pub const KEY: &str = "builtin:white";

/// A built-in asset: reference name, display name and catalog category.
pub struct Builtin {
    pub name: &'static str,
    pub label: &'static str,
    pub category: &'static str,
}
/// Every built-in asset, in catalog order.
pub const ALL: &[Builtin] = &[
    Builtin {
        name: "ground",
        label: "Ground plate 32 m",
        category: "Ground",
    },
    Builtin {
        name: "grass",
        label: "Grass plate 32 m",
        category: "Ground",
    },
    Builtin {
        name: "terrain",
        label: "Terrain 64 m (sculpt)",
        category: "Ground",
    },
    Builtin {
        name: "road",
        label: "Road 8 x 32 m",
        category: "Roads",
    },
    Builtin {
        name: "spline-road",
        label: "Road along points",
        category: "Roads",
    },
    Builtin {
        name: "floor",
        label: "Floor tile 4 m",
        category: "Buildings",
    },
    Builtin {
        name: "wall",
        label: "Wall 4 x 3 m",
        category: "Buildings",
    },
    Builtin {
        name: "pillar",
        label: "Pillar 3 m",
        category: "Buildings",
    },
    Builtin {
        name: "platform",
        label: "Platform 6 x 6 m",
        category: "Buildings",
    },
    Builtin {
        name: "stairs",
        label: "Stairs 2 m up",
        category: "Buildings",
    },
    Builtin {
        name: "house",
        label: "Block house",
        category: "Buildings",
    },
    Builtin {
        name: "ramp",
        label: "Ramp 1.5 m",
        category: "Infrastructure",
    },
    Builtin {
        name: "lamp-post",
        label: "Lamp post",
        category: "Infrastructure",
    },
    Builtin {
        name: "cube",
        label: "Cube 1 m",
        category: "Props",
    },
    Builtin {
        name: "crate",
        label: "Wooden crate",
        category: "Props",
    },
    Builtin {
        name: "bench",
        label: "Bench",
        category: "Furniture",
    },
    Builtin {
        name: "tree",
        label: "Block tree",
        category: "Nature",
    },
    Builtin {
        name: "rock",
        label: "Rock",
        category: "Nature",
    },
    // Gameplay: placing one also attaches its behaviour (see
    // `world_logic::preset`). Markers are only drawn in the editor.
    Builtin {
        name: "marker-spawn",
        label: "Spawn point",
        category: GAMEPLAY,
    },
    Builtin {
        name: "door",
        label: "Door (opens with E)",
        category: GAMEPLAY,
    },
    Builtin {
        name: "door-teleport",
        label: "Door to another place",
        category: GAMEPLAY,
    },
    Builtin {
        name: "marker-teleport",
        label: "Teleport pad",
        category: GAMEPLAY,
    },
    Builtin {
        name: "marker-target",
        label: "Destination point",
        category: GAMEPLAY,
    },
    Builtin {
        name: "marker-trigger",
        label: "Trigger zone",
        category: GAMEPLAY,
    },
    Builtin {
        name: "marker-interact",
        label: "Interaction point",
        category: GAMEPLAY,
    },
    Builtin {
        name: "marker-checkpoint",
        label: "Checkpoint",
        category: GAMEPLAY,
    },
];

/// Catalog category of gameplay assets.
pub const GAMEPLAY: &str = "Gameplay";

/// The ground a placed Terrain starts with: flat, 64 m, 2 m squares.
pub fn preset_terrain() -> crate::world_layout::Terrain {
    crate::world_layout::Terrain::flat(64.0, 32)
}

/// The road a placed "Road along points" starts with: 8 m wide, 48 m
/// long with a gentle bend, ready to drag its points.
pub fn preset_road() -> crate::world_layout::Road {
    crate::world_layout::Road {
        points: vec![[-24.0, 0.0, 0.0], [0.0, 6.0, 0.0], [24.0, 0.0, 0.0]],
        ..crate::world_layout::Road::straight([0.0; 3], [0.0; 3], 8.0)
    }
}

/// The light a built-in comes with when placed (a lamp post glows).
pub fn light_preset(name: &str) -> Option<crate::world_layout::PointLight> {
    (name == "lamp-post").then(|| crate::world_layout::PointLight {
        offset: [0.0, 1.0, 4.25],
        ..crate::world_layout::PointLight::lamp()
    })
}

/// Shown only in Build Mode: gameplay markers are invisible and have no
/// collision when the world is played.
pub fn editor_only(name: &str) -> bool {
    name.starts_with("marker-")
}

pub fn find(name: &str) -> Option<&'static Builtin> {
    ALL.iter().find(|b| b.name == name)
}

#[derive(Default)]
struct Shape {
    vertices: Vec<Vertex>,
}
impl Shape {
    /// A box from `min` to `max` in Blender axes (east, north, up).
    fn cuboid(&mut self, min: [f32; 3], max: [f32; 3], color: [f32; 3]) {
        let p = |e: f32, n: f32, u: f32| Vec3::new(e, u, -n);
        let ([e0, n0, u0], [e1, n1, u1]) = (min, max);
        let faces = [
            (
                [p(e0, n0, u1), p(e1, n0, u1), p(e1, n1, u1), p(e0, n1, u1)],
                Vec3::Y,
                1.0,
            ),
            (
                [p(e0, n0, u0), p(e1, n0, u0), p(e1, n1, u0), p(e0, n1, u0)],
                -Vec3::Y,
                0.5,
            ),
            (
                [p(e1, n0, u0), p(e1, n1, u0), p(e1, n1, u1), p(e1, n0, u1)],
                Vec3::X,
                0.82,
            ),
            (
                [p(e0, n0, u0), p(e0, n1, u0), p(e0, n1, u1), p(e0, n0, u1)],
                -Vec3::X,
                0.7,
            ),
            (
                [p(e0, n1, u0), p(e1, n1, u0), p(e1, n1, u1), p(e0, n1, u1)],
                -Vec3::Z,
                0.76,
            ),
            (
                [p(e0, n0, u0), p(e1, n0, u0), p(e1, n0, u1), p(e0, n0, u1)],
                Vec3::Z,
                0.64,
            ),
        ];
        for (corners, outward, shade) in faces {
            self.quad(corners, outward, color.map(|c| c * shade));
        }
    }
    /// A slope rising northwards, with closed sides and back.
    fn wedge(&mut self, width: f32, length: f32, height: f32, color: [f32; 3]) {
        let p = |e: f32, n: f32, u: f32| Vec3::new(e, u, -n);
        let (w, l) = (width / 2.0, length / 2.0);
        let slope_normal = Vec3::new(0.0, length, height).normalize();
        self.quad(
            [
                p(-w, -l, 0.0),
                p(w, -l, 0.0),
                p(w, l, height),
                p(-w, l, height),
            ],
            slope_normal,
            color,
        );
        self.quad(
            [
                p(-w, l, 0.0),
                p(w, l, 0.0),
                p(w, l, height),
                p(-w, l, height),
            ],
            -Vec3::Z,
            color.map(|c| c * 0.76),
        );
        self.quad(
            [p(-w, -l, 0.0), p(w, -l, 0.0), p(w, l, 0.0), p(-w, l, 0.0)],
            -Vec3::Y,
            color.map(|c| c * 0.5),
        );
        for (e, outward, shade) in [(w, Vec3::X, 0.82), (-w, -Vec3::X, 0.7)] {
            let tri = [p(e, -l, 0.0), p(e, l, 0.0), p(e, l, height)];
            self.triangle(tri, outward, color.map(|c| c * shade));
        }
    }
    fn quad(&mut self, corners: [Vec3; 4], outward: Vec3, color: [f32; 3]) {
        let [a, b, c, d] = corners;
        self.triangle([a, b, c], outward, color);
        self.triangle([a, c, d], outward, color);
    }
    fn triangle(&mut self, corners: [Vec3; 3], outward: Vec3, color: [f32; 3]) {
        let [a, mut b, mut c] = corners;
        if (b - a).cross(c - a).dot(outward) < 0.0 {
            std::mem::swap(&mut b, &mut c);
        }
        let color = [color[0], color[1], color[2], 1.0];
        for (p, uv) in [(a, [0.0, 0.0]), (b, [1.0, 0.0]), (c, [1.0, 1.0])] {
            self.vertices.push(Vertex {
                position: p.to_array(),
                uv,
                color,
            });
        }
    }
}

const CONCRETE: [f32; 3] = [0.62, 0.62, 0.6];
const WOOD: [f32; 3] = [0.55, 0.38, 0.22];
const DARK: [f32; 3] = [0.22, 0.22, 0.24];

/// Build a built-in model, or None for an unknown name.
pub fn model(name: &str) -> Option<Model> {
    if name == "terrain" {
        // A placed terrain shows its own heights (`Node::terrain`); this is
        // the picture and the stand-in.
        return Some(crate::terrain::model(&preset_terrain()));
    }
    if name == "spline-road" {
        // Likewise: a placed road shows its own points (`Node::road`).
        return Some(crate::road::model(&preset_road()));
    }
    let mut s = Shape::default();
    match name {
        "ground" => s.cuboid([-16.0, -16.0, -0.3], [16.0, 16.0, 0.0], [0.5, 0.5, 0.48]),
        "grass" => s.cuboid([-16.0, -16.0, -0.3], [16.0, 16.0, 0.0], [0.33, 0.5, 0.24]),
        "road" => {
            s.cuboid([-4.0, -16.0, -0.3], [4.0, 16.0, 0.02], [0.2, 0.2, 0.21]);
            for i in 0..8 {
                let n = -15.0 + i as f32 * 4.0;
                s.cuboid([-0.1, n, 0.02], [0.1, n + 2.0, 0.03], [0.9, 0.85, 0.5]);
            }
        }
        "floor" => s.cuboid([-2.0, -2.0, 0.0], [2.0, 2.0, 0.1], CONCRETE),
        "wall" => s.cuboid([-2.0, -0.125, 0.0], [2.0, 0.125, 3.0], [0.78, 0.74, 0.66]),
        "pillar" => s.cuboid([-0.25, -0.25, 0.0], [0.25, 0.25, 3.0], CONCRETE),
        "platform" => s.cuboid([-3.0, -3.0, 0.0], [3.0, 3.0, 1.0], CONCRETE),
        "stairs" => {
            for step in 0..8 {
                let n = -2.0 + step as f32 * 0.5;
                let top = 0.25 * (step + 1) as f32;
                s.cuboid([-1.0, n, 0.0], [1.0, n + 0.5, top], CONCRETE);
            }
        }
        "house" => {
            s.cuboid([-4.0, -3.0, 0.0], [4.0, 3.0, 3.2], [0.85, 0.78, 0.62]);
            s.cuboid([-4.3, -3.3, 3.2], [4.3, 3.3, 3.5], [0.55, 0.25, 0.2]);
            s.cuboid([-0.6, -3.05, 0.0], [0.6, -2.95, 2.2], WOOD);
            for e in [-2.6, 2.6] {
                s.cuboid(
                    [e - 0.6, -3.05, 1.2],
                    [e + 0.6, -2.95, 2.2],
                    [0.45, 0.6, 0.75],
                );
            }
        }
        "ramp" => s.wedge(4.0, 6.0, 1.5, CONCRETE),
        "lamp-post" => {
            s.cuboid([-0.08, -0.08, 0.0], [0.08, 0.08, 4.5], DARK);
            s.cuboid([-0.08, -0.08, 4.4], [0.08, 1.0, 4.55], DARK);
            s.cuboid([-0.2, 0.8, 4.2], [0.2, 1.2, 4.4], [1.0, 0.95, 0.75]);
        }
        "cube" => s.cuboid([-0.5, -0.5, 0.0], [0.5, 0.5, 1.0], [0.8, 0.8, 0.8]),
        "crate" => {
            s.cuboid([-0.5, -0.5, 0.0], [0.5, 0.5, 1.0], WOOD);
            s.cuboid(
                [-0.52, -0.52, 0.0],
                [0.52, 0.52, 0.1],
                WOOD.map(|c| c * 0.7),
            );
            s.cuboid(
                [-0.52, -0.52, 0.9],
                [0.52, 0.52, 1.0],
                WOOD.map(|c| c * 0.7),
            );
        }
        "bench" => {
            s.cuboid([-0.9, -0.22, 0.42], [0.9, 0.22, 0.48], WOOD);
            s.cuboid([-0.9, 0.18, 0.48], [0.9, 0.24, 0.9], WOOD);
            for e in [-0.8, 0.7] {
                s.cuboid([e, -0.2, 0.0], [e + 0.1, 0.2, 0.42], DARK);
            }
        }
        "tree" => {
            s.cuboid([-0.2, -0.2, 0.0], [0.2, 0.2, 2.5], [0.4, 0.27, 0.15]);
            s.cuboid([-1.4, -1.4, 2.2], [1.4, 1.4, 4.2], [0.2, 0.45, 0.18]);
            s.cuboid([-0.9, -0.9, 4.2], [0.9, 0.9, 5.2], [0.24, 0.52, 0.2]);
        }
        "rock" => {
            s.cuboid([-0.9, -0.7, 0.0], [0.9, 0.7, 0.8], [0.5, 0.48, 0.45]);
            s.cuboid([-0.5, -0.4, 0.8], [0.6, 0.5, 1.2], [0.55, 0.53, 0.5]);
        }
        "door" | "door-teleport" => {
            let panel = if name == "door" {
                WOOD
            } else {
                [0.25, 0.32, 0.55]
            };
            s.cuboid([-0.6, -0.06, 0.0], [0.6, 0.06, 2.2], panel);
            s.cuboid([0.38, -0.12, 1.0], [0.48, 0.12, 1.08], [0.85, 0.75, 0.4]);
        }
        marker if editor_only(marker) => {
            let color = match marker {
                "marker-spawn" => [0.3, 0.85, 0.4],
                "marker-teleport" => [0.7, 0.4, 0.95],
                "marker-target" => [0.35, 0.6, 1.0],
                "marker-trigger" => [1.0, 0.6, 0.2],
                "marker-interact" => [0.3, 0.85, 0.9],
                "marker-checkpoint" => [1.0, 0.85, 0.2],
                _ => return None,
            };
            s.cuboid([-0.5, -0.5, 0.0], [0.5, 0.5, 0.05], color);
            s.cuboid([-0.05, -0.05, 0.05], [0.05, 0.05, 1.6], DARK);
            s.cuboid([0.05, -0.02, 1.2], [0.5, 0.02, 1.6], color);
            if matches!(marker, "marker-spawn" | "marker-target") {
                // The way the player will face: +Y.
                s.cuboid([-0.12, 0.0, 0.05], [0.12, 0.9, 0.12], color);
            }
        }
        _ => return None,
    }
    let triangles = s.vertices.len() / 3;
    let batch = |vertices: Vec<Vertex>| Batch {
        key: KEY.into(),
        vertices,
        alpha: false,
        animated: false,
        uv_animation: None,
        map_door: None,
        mlo_entity: None,
    };
    // Markers are pictures for the editor, never solid.
    let collision = if editor_only(name) {
        Vec::new()
    } else {
        vec![batch(s.vertices.clone())]
    };
    Some(Model {
        collision,
        drawn: vec![batch(s.vertices)],
        textures: HashMap::from([(KEY.into(), white())]),
        triangles,
    })
}

pub fn white() -> Texture {
    Texture {
        width: 1,
        height: 1,
        rgba: vec![255; 4],
        has_alpha: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_builds_and_sits_on_its_origin() {
        for builtin in ALL {
            let model = model(builtin.name).unwrap_or_else(|| panic!("{}", builtin.name));
            assert!(model.triangles > 0, "{}", builtin.name);
            // Gameplay markers are never solid; everything else is.
            assert_eq!(
                model.collision.is_empty(),
                editor_only(builtin.name),
                "{}",
                builtin.name
            );
            let lowest = model.drawn[0]
                .vertices
                .iter()
                .map(|v| v.position[1])
                .fold(f32::MAX, f32::min);
            // Ground pieces sink below 0 so their top is flush with it.
            assert!(
                lowest <= 0.0 && lowest > -0.5,
                "{} lowest {lowest}",
                builtin.name
            );
        }
        assert!(model("nope").is_none());
        let names: std::collections::HashSet<_> = ALL.iter().map(|b| b.name).collect();
        assert_eq!(names.len(), ALL.len());
    }
}
