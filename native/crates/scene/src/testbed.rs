//! SARE Testbed: a small original world built from code, so the engine can
//! start, walk and be tested without any game files. Everything here is
//! generated geometry and flat colours; it contains no third-party assets.
use crate::{collision::CollisionWorld, vehicle, Batch, Scene, Vertex};
use glam::Vec3;
use sa_assets::Texture;
use std::collections::HashMap;

/// Texture key for the testbed's vertex-coloured surfaces.
pub const KEY: &str = "runtime:white";
/// Half the edge length of the square ground, in metres.
pub const HALF_SIZE: f32 = 256.0;
/// Ground height in world coordinates.
pub const GROUND: f32 = 10.0;
const TILE: f32 = 32.0;
/// Where the testbed sits in world coordinates: the runtime's default origin,
/// so runtime (0, height, 0) is the middle of the ground square.
pub const CENTER: [f32; 2] = [2500.0, -1670.0];

/// The testbed's spawn point in world coordinates (x, y, z = height).
pub fn spawn(center: [f32; 2]) -> [f32; 3] {
    [center[0], center[1] - 12.0, GROUND]
}

struct Builder {
    vertices: Vec<Vertex>,
    /// World centre and runtime origin: world (x, y, z) becomes
    /// runtime (x - origin.x, z, origin.y - y), as for streamed SA scenes.
    center: [f32; 2],
    origin: [f32; 2],
}
impl Builder {
    /// A point `east`/`north` metres from the testbed centre, `up` above ground 0.
    fn at(&self, east: f32, north: f32, up: f32) -> Vec3 {
        Vec3::new(
            self.center[0] + east - self.origin[0],
            up,
            self.origin[1] - (self.center[1] + north),
        )
    }
    /// One planar quad (corners in order around it), facing `outward`.
    fn quad(&mut self, corners: [Vec3; 4], outward: Vec3, color: [f32; 3]) {
        let [a, b, c, d] = corners;
        let (b, d) = if (b - a).cross(c - a).dot(outward) < 0.0 {
            (d, b)
        } else {
            (b, d)
        };
        let color = [color[0], color[1], color[2], 1.0];
        for (p, uv) in [
            (a, [0.0, 0.0]),
            (b, [1.0, 0.0]),
            (c, [1.0, 1.0]),
            (a, [0.0, 0.0]),
            (c, [1.0, 1.0]),
            (d, [0.0, 1.0]),
        ] {
            self.vertices.push(Vertex {
                position: p.to_array(),
                uv,
                color,
            });
        }
    }
    /// A solid box from (east, north, up) `min` to `max`, testbed-relative.
    fn cuboid(&mut self, min: [f32; 3], max: [f32; 3], color: [f32; 3]) {
        let p = |e: f32, n: f32, u: f32| self.at(e, n, u);
        let ([e0, n0, u0], [e1, n1, u1]) = (min, max);
        let shade = |k: f32| [color[0] * k, color[1] * k, color[2] * k];
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
                0.8,
            ),
            (
                [p(e0, n0, u0), p(e0, n1, u0), p(e0, n1, u1), p(e0, n0, u1)],
                -Vec3::X,
                0.7,
            ),
            // North is -Z in runtime coordinates.
            (
                [p(e0, n1, u0), p(e1, n1, u0), p(e1, n1, u1), p(e0, n1, u1)],
                -Vec3::Z,
                0.75,
            ),
            (
                [p(e0, n0, u0), p(e1, n0, u0), p(e1, n0, u1), p(e0, n0, u1)],
                Vec3::Z,
                0.65,
            ),
        ];
        for (corners, outward, k) in faces {
            self.quad(corners, outward, shade(k));
        }
    }
    /// A ramp rising northwards from `up0` to `up1` over [n0, n1].
    fn ramp(&mut self, e: [f32; 2], n: [f32; 2], up: [f32; 2], color: [f32; 3]) {
        let corners = [
            self.at(e[0], n[0], up[0]),
            self.at(e[1], n[0], up[0]),
            self.at(e[1], n[1], up[1]),
            self.at(e[0], n[1], up[1]),
        ];
        self.quad(corners, Vec3::Y, color);
    }
}

/// Build the testbed around world point `center` for a runtime `origin`.
/// The layout: a checkered ground square, a walking ramp up to a platform,
/// stairs, a row of blocks to jump between and a long gentle driving slope.
pub fn build(center: [f32; 2], origin: [f32; 2]) -> Scene {
    let mut b = Builder {
        vertices: Vec::new(),
        center,
        origin,
    };
    let tiles = (HALF_SIZE * 2.0 / TILE) as i32;
    for i in 0..tiles {
        for j in 0..tiles {
            let (e0, n0) = (-HALF_SIZE + i as f32 * TILE, -HALF_SIZE + j as f32 * TILE);
            let color = if (i + j) % 2 == 0 {
                [0.42, 0.45, 0.42]
            } else {
                [0.34, 0.37, 0.35]
            };
            let corners = [
                b.at(e0, n0, GROUND),
                b.at(e0 + TILE, n0, GROUND),
                b.at(e0 + TILE, n0 + TILE, GROUND),
                b.at(e0, n0 + TILE, GROUND),
            ];
            b.quad(corners, Vec3::Y, color);
        }
    }
    let accent = [0.85, 0.55, 0.2];
    // Walking ramp (about 11 degrees) to a 4 m platform.
    b.ramp([-6.0, 6.0], [0.0, 20.0], [GROUND, GROUND + 4.0], accent);
    b.cuboid(
        [-6.0, 20.0, GROUND],
        [6.0, 32.0, GROUND + 4.0],
        [0.55, 0.6, 0.7],
    );
    // Stairs: 20 steps of 0.2 m beside the platform.
    for step in 0..20 {
        let n = 20.0 - step as f32 * 0.5;
        b.cuboid(
            [8.0, n - 0.5, GROUND],
            [11.0, n, GROUND + 0.2 * (step + 1) as f32],
            [0.6, 0.6, 0.55],
        );
    }
    // Blocks to jump between, 1 m high and 1.5 m apart.
    for k in 0..6 {
        let e = -40.0 + k as f32 * 4.5;
        b.cuboid(
            [e, 10.0, GROUND],
            [e + 3.0, 13.0, GROUND + 1.0],
            [0.3, 0.55, 0.75],
        );
    }
    // A long, gentle slope for vehicles: 4 m over 80 m.
    b.ramp(
        [30.0, 42.0],
        [-60.0, 20.0],
        [GROUND, GROUND + 4.0],
        [0.5, 0.5, 0.5],
    );
    b.cuboid(
        [30.0, 20.0, GROUND],
        [42.0, 40.0, GROUND + 4.0],
        [0.5, 0.5, 0.5],
    );

    let mut scene = flat_scene(b.vertices, false);
    scene.collision = Some(CollisionWorld::from_batches(&scene.batches));
    scene
}

/// One vertex-coloured batch on the white texture, without collision.
/// `posed` batches keep their model-space vertices so the runtime can move
/// them every frame, as it does for car and ped models.
fn flat_scene(vertices: Vec<Vertex>, posed: bool) -> Scene {
    let triangles = vertices.len() / 3;
    let batches = vec![Batch {
        key: KEY.into(),
        vertices,
        alpha: false,
        animated: posed,
        uv_animation: None,
        map_door: None,
        mlo_entity: None,
    }];
    let textures = HashMap::from([(
        KEY.to_string(),
        Texture {
            width: 1,
            height: 1,
            rgba: vec![255; 4],
            has_alpha: false,
        },
    )]);
    Scene {
        vehicle_handling: vehicle::Handling::default(),
        water: None,
        batches,
        textures,
        placements: 1,
        triangles,
        animation: None,
        collision: None,
        vehicle_pieces: HashMap::new(),
    }
}

/// Boxes in model space: x right, y up, z forward.
struct Model(Builder);
impl Model {
    fn new() -> Self {
        Self(Builder {
            vertices: Vec::new(),
            center: [0.0, 0.0],
            origin: [0.0, 0.0],
        })
    }
    fn part(&mut self, x: [f32; 2], y: [f32; 2], z: [f32; 2], color: [f32; 3]) -> &mut Self {
        // With a zero centre and origin, Builder::at(e, n, u) is (e, u, -n).
        self.0
            .cuboid([x[0], -z[1], y[0]], [x[1], -z[0], y[1]], color);
        self
    }
}

/// SARE Kart: a placeholder car made of boxes, for driving without game files.
/// Model space matches the runtime's car models: the wheels' contact patch at
/// y = -0.55 and the nose towards -z.
pub fn placeholder_car() -> Scene {
    let mut m = Model::new();
    let body = [0.15, 0.45, 0.85];
    m.part([-0.95, 0.95], [-0.25, 0.35], [-2.0, 2.0], body)
        .part([-0.8, 0.8], [0.35, 0.95], [-0.3, 1.3], [0.2, 0.25, 0.3])
        .part([-0.75, 0.75], [0.35, 0.55], [-1.9, -0.3], body)
        // Headlights on the nose, red lights at the back.
        .part([-0.8, -0.45], [0.0, 0.2], [-2.05, -2.0], [1.0, 0.95, 0.6])
        .part([0.45, 0.8], [0.0, 0.2], [-2.05, -2.0], [1.0, 0.95, 0.6])
        .part([-0.8, -0.45], [0.0, 0.2], [2.0, 2.05], [0.8, 0.1, 0.1])
        .part([0.45, 0.8], [0.0, 0.2], [2.0, 2.05], [0.8, 0.1, 0.1]);
    for x in [[-1.05, -0.8], [0.8, 1.05]] {
        for z in [[-1.65, -0.95], [0.95, 1.65]] {
            m.part(x, [-0.55, 0.15], z, [0.08, 0.08, 0.08]);
        }
    }
    flat_scene(m.0.vertices, true)
}

/// The placeholder player: a blocky figure 1.8 m tall, feet at y = 0,
/// facing +z (the visor shows which way).
pub fn placeholder_player() -> Scene {
    let mut m = Model::new();
    m.part(
        [-0.22, -0.03],
        [0.0, 0.85],
        [-0.12, 0.12],
        [0.2, 0.25, 0.45],
    )
    .part([0.03, 0.22], [0.0, 0.85], [-0.12, 0.12], [0.2, 0.25, 0.45])
    .part(
        [-0.27, 0.27],
        [0.85, 1.5],
        [-0.15, 0.15],
        [0.95, 0.55, 0.15],
    )
    .part(
        [-0.37, -0.27],
        [0.9, 1.45],
        [-0.08, 0.08],
        [0.85, 0.7, 0.55],
    )
    .part([0.27, 0.37], [0.9, 1.45], [-0.08, 0.08], [0.85, 0.7, 0.55])
    .part(
        [-0.14, 0.14],
        [1.52, 1.82],
        [-0.14, 0.14],
        [0.85, 0.7, 0.55],
    )
    .part([-0.11, 0.11], [1.62, 1.72], [0.14, 0.17], [0.1, 0.8, 0.9]);
    flat_scene(m.0.vertices, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    const ORIGIN: [f32; 2] = [2500.0, -1670.0];
    fn runtime(b: &Builder, east: f32, north: f32, up: f32) -> Vec3 {
        b.at(east, north, up)
    }
    fn builder() -> Builder {
        Builder {
            vertices: Vec::new(),
            center: CENTER,
            origin: ORIGIN,
        }
    }
    #[test]
    fn testbed_has_ground_platform_stairs_and_a_walkable_spawn() {
        let scene = build(CENTER, ORIGIN);
        let world = scene.collision.as_ref().unwrap();
        assert!(world.triangle_count() > 500);
        assert!(scene.textures.contains_key(KEY));
        let b = builder();
        let ground = |e: f32, n: f32| world.ground_below(runtime(&b, e, n, 100.0), 100.0);
        // Ground everywhere inside the square, nothing outside it.
        for (e, n) in [(-250.0, -250.0), (250.0, 250.0), (100.0, -100.0)] {
            assert_eq!(ground(e, n), Some(GROUND), "{e} {n}");
        }
        assert_eq!(ground(300.0, 0.0), None);
        assert!((ground(0.0, 25.0).unwrap() - (GROUND + 4.0)).abs() < 1e-4);
        assert!((ground(0.0, 10.0).unwrap() - (GROUND + 2.0)).abs() < 1e-3);
        // The spawn point is a valid standing position.
        let [x, y, z] = spawn(CENTER);
        let point = Vec3::new(x - ORIGIN[0], z, ORIGIN[1] - y);
        assert!(world.standing_at(point, z + 2.0).is_some());
    }
    #[test]
    fn a_player_walks_up_the_ramp_and_the_stairs_onto_higher_ground() {
        let scene = build(CENTER, ORIGIN);
        let world = scene.collision.as_ref().unwrap();
        let b = builder();
        for (start, end_north, east) in [((0.0, -5.0), 26.0, 0.0), ((9.5, 22.0), 10.6, 9.5)] {
            let feet = runtime(&b, start.0, start.1, GROUND);
            let mut player = world.standing_at(feet, GROUND + 0.5).unwrap();
            // Runtime -Z is north.
            let direction = if end_north > start.1 {
                -Vec3::Z
            } else {
                Vec3::Z
            };
            for _ in 0..600 {
                player.step(world, direction * 4.0, false, 1.0 / 60.0);
                let north = ORIGIN[1] - player.feet.z - CENTER[1];
                if (end_north > start.1 && north >= end_north)
                    || (end_north < start.1 && north <= end_north)
                {
                    break;
                }
            }
            assert!(player.feet.is_finite());
            assert!(
                player.feet.y > GROUND + 3.0,
                "east {east}: stuck at {:?}",
                player.feet
            );
        }
    }
    #[test]
    fn placeholder_car_and_player_are_sized_like_the_real_thing() {
        let points = |scene: &Scene| -> Vec<[f32; 3]> {
            scene.batches[0]
                .vertices
                .iter()
                .map(|v| v.position)
                .collect()
        };
        let car = placeholder_car();
        let size = vehicle::measure(points(&car));
        assert!((size[0] - 1.05).abs() < 1e-4 && (size[1] - 2.05).abs() < 1e-4);
        let lowest = points(&car).iter().map(|p| p[1]).fold(f32::MAX, f32::min);
        assert!((lowest + 0.55).abs() < 1e-4);
        // The headlights (the most forward points) are towards -z.
        let nose = points(&car).iter().map(|p| p[2]).fold(f32::MAX, f32::min);
        assert!((nose + 2.05).abs() < 1e-4);
        let lights = |z: f32| {
            car.batches[0]
                .vertices
                .iter()
                .find(|v| (v.position[2] - z).abs() < 1e-4)
                .map(|v| v.color)
                .unwrap()
        };
        assert!(lights(-2.05)[1] > 0.9 && lights(2.05)[1] < 0.2);
        let player = placeholder_player();
        let height = points(&player)
            .iter()
            .map(|p| p[1])
            .fold(f32::MIN, f32::max);
        assert!((height - 1.82).abs() < 1e-4);
        assert!(car.textures.contains_key(KEY) && player.collision.is_none());
        assert!(car.batches[0].animated && player.batches[0].animated);
    }
}
