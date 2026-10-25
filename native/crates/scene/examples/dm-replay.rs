//! Replay a recorded MTA deathmatch run through the car kernel, on the map's
//! own collision, and compare the simulated path with the recording.
//!
//! `cargo run --release -p sa-scene --example dm-replay -- \
//!    ../../browser-three/dist/maps/dm-skynet-vol6-profeta.json profeta_slip.tas`
//!
//! The recording is 50 Hz and stores, per frame: position, Euler degrees,
//! velocity and angular velocity in GTA's own units (metres and metres per
//! CTimer step), the health and model, and the keys held. That is exactly what
//! the kernel consumes, so the replay feeds the recorded keys and integrates
//! one 50 Hz step at a time; nothing from the recording is written back into
//! the simulation.
use glam::Vec3;
use sa_scene::collision::CollisionWorld;
use sa_scene::vehicle::Car;
use sa_scene::{Batch, Vertex};
use std::fs;

/// GTA is Z-up with +Y forward; the simulation is Y-up with +Z forward. The
/// swap of the last two axes is its own inverse, so the same function maps
/// both ways.
fn gta_to_scene(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], p[1])
}

/// MTA stores `R = Rz(rz) * Ry(ry) * Rx(rx)` in degrees; columns are the basis.
fn euler_basis(rx: f32, ry: f32, rz: f32) -> [Vec3; 3] {
    let (rx, ry, rz) = (
        rx.to_radians(),
        ry.to_radians(),
        rz.to_radians(),
    );
    let (cx, sx) = (rx.cos(), rx.sin());
    let (cy, sy) = (ry.cos(), ry.sin());
    let (cz, sz) = (rz.cos(), rz.sin());
    let mx = [[1.0, 0.0, 0.0], [0.0, cx, -sx], [0.0, sx, cx]];
    let my = [[cy, 0.0, sy], [0.0, 1.0, 0.0], [-sy, 0.0, cy]];
    let mz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]];
    let mul = |a: [[f32; 3]; 3], b: [[f32; 3]; 3]| {
        let mut out = [[0.0f32; 3]; 3];
        for i in 0..3 {
            for j in 0..3 {
                out[i][j] = (0..3).map(|k| a[i][k] * b[k][j]).sum();
            }
        }
        out
    };
    let m = mul(mz, mul(my, mx));
    [
        Vec3::new(m[0][0], m[1][0], m[2][0]),
        Vec3::new(m[0][1], m[1][1], m[2][1]),
        Vec3::new(m[0][2], m[1][2], m[2][2]),
    ]
}

struct MapScene {
    collision: CollisionWorld,
    triangles: usize,
}

/// Keep only the map around the recorded path: a DM map is hundreds of
/// thousands of triangles and the replay never leaves one stretch of it.
fn load_map(path: &str, bounds: Option<(Vec3, Vec3)>) -> anyhow::Result<MapScene> {
    let text = fs::read_to_string(path)?;
    let map: serde_json::Value = serde_json::from_str(&text)?;
    let empty = serde_json::Map::new();
    let models = map["models"].as_object().unwrap_or(&empty);
    let mut vertices: Vec<Vertex> = Vec::new();
    let push = |vertices: &mut Vec<Vertex>, a: Vec3, b: Vec3, c: Vec3| {
        for p in [a, b, c] {
            vertices.push(Vertex {
                position: p.to_array(),
                ..Default::default()
            });
        }
    };
    // Box faces as index triples into the 8 corners.
    const BOX_FACES: [[usize; 3]; 12] = [
        [0, 1, 2], [0, 2, 3], [4, 6, 5], [4, 7, 6],
        [0, 4, 5], [0, 5, 1], [1, 5, 6], [1, 6, 2],
        [2, 6, 7], [2, 7, 3], [3, 7, 4], [3, 4, 0],
    ];
    let instances = map["instances"].as_array().cloned().unwrap_or_default();
    for instance in &instances {
        if instance["collisions"].as_bool() == Some(false) {
            continue;
        }
        let Some(model) = instance["model"].as_i64().map(|id| id.to_string()) else {
            continue;
        };
        let Some(collision) = models.get(&model).map(|m| &m["collision"]) else {
            continue;
        };
        if let Some((low, high)) = bounds {
            let okay = instance["position"]
                .as_array()
                .and_then(|p| {
                    let x = p.first()?.as_f64()? as f32;
                    let y = p.get(1)?.as_f64()? as f32;
                    let z = p.get(2)?.as_f64()? as f32;
                    let p = gta_to_scene([x, y, z]);
                    Some(p.cmpge(low).all() && p.cmple(high).all())
                })
                .unwrap_or(false);
            if !okay {
                continue;
            }
        }
        if collision.is_null() {
            continue;
        }
        // Row-major 4x4 with translation in the last column.
        let mut m = [[0.0f32; 4]; 4];
        if let Some(rows) = instance["transform"].as_array() {
            for (i, row) in rows.iter().take(4).enumerate() {
                for (j, value) in row.as_array().into_iter().flatten().take(4).enumerate() {
                    m[i][j] = value.as_f64().unwrap_or(0.0) as f32;
                }
            }
        }
        let place = |v: [f32; 3]| {
            let x = m[0][0] * v[0] + m[0][1] * v[1] + m[0][2] * v[2] + m[0][3];
            let y = m[1][0] * v[0] + m[1][1] * v[1] + m[1][2] * v[2] + m[1][3];
            let z = m[2][0] * v[0] + m[2][1] * v[1] + m[2][2] * v[2] + m[2][3];
            gta_to_scene([x, y, z])
        };
        if let Some(boxes) = collision["boxes"].as_array() {
            for b in boxes {
                let (min, max) = (
                    b["min"].as_array().cloned().unwrap_or_default(),
                    b["max"].as_array().cloned().unwrap_or_default(),
                );
                if min.len() < 3 || max.len() < 3 {
                    continue;
                }
                let f = |v: &Vec<serde_json::Value>| {
                    [
                        v[0].as_f64().unwrap_or(0.0) as f32,
                        v[1].as_f64().unwrap_or(0.0) as f32,
                        v[2].as_f64().unwrap_or(0.0) as f32,
                    ]
                };
                let (lo, hi) = (f(&min), f(&max));
                let corners: [Vec3; 8] = [
                    place([lo[0], lo[1], lo[2]]),
                    place([hi[0], lo[1], lo[2]]),
                    place([hi[0], hi[1], lo[2]]),
                    place([lo[0], hi[1], lo[2]]),
                    place([lo[0], lo[1], hi[2]]),
                    place([hi[0], lo[1], hi[2]]),
                    place([hi[0], hi[1], hi[2]]),
                    place([lo[0], hi[1], hi[2]]),
                ];
                for face in BOX_FACES {
                    push(&mut vertices, corners[face[0]], corners[face[1]], corners[face[2]]);
                }
            }
        }
        let model_vertices = collision["vertices"].as_array().cloned().unwrap_or_default();
        let points: Vec<Vec3> = model_vertices
            .iter()
            .filter_map(|v| v.as_array())
            .map(|v| {
                let read = |i: usize| v.get(i).and_then(|x| x.as_f64()).unwrap_or(0.0) as f32;
                place([read(0), read(1), read(2)])
            })
            .collect();
        if let Some(faces) = collision["faces"].as_array() {
            for face in faces {
                let read = |key: &str| face[key].as_u64().unwrap_or(0) as usize;
                let (a, b, c) = (read("a"), read("b"), read("c"));
                if let (Some(a), Some(b), Some(c)) =
                    (points.get(a).copied(), points.get(b).copied(), points.get(c).copied())
                {
                    push(&mut vertices, a, b, c);
                }
            }
        }
    }
    let triangles = vertices.len() / 3;
    let collision = CollisionWorld::from_batches(&[Batch {
        key: "map".into(),
        alpha: false,
        animated: false,
        uv_animation: None,
        vertices,
    }]);
    Ok(MapScene {
        collision,
        triangles,
    })
}

struct Frame {
    position: Vec3,
    rotation_deg: [f32; 3],
    velocity: Vec3,
    keys: Vec<String>,
}

fn load_tas(path: &str) -> anyhow::Result<Vec<Frame>> {
    let text = fs::read_to_string(path)?;
    let mut frames = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line == "+run" {
            continue;
        }
        if line == "-run" {
            break;
        }
        let fields: Vec<&str> = line.split('|').collect();
        if fields.len() < 9 {
            continue;
        }
        let triple = |text: &str| -> [f32; 3] {
            let mut out = [0.0f32; 3];
            for (i, part) in text.split(',').take(3).enumerate() {
                out[i] = part.parse().unwrap_or(0.0);
            }
            out
        };
        let position = triple(fields[1]);
        let rotation = triple(fields[2]);
        let velocity = triple(fields[3]);
        let keys = fields[8]
            .split(',')
            .map(|k| k.trim().to_ascii_lowercase())
            .filter(|k| !k.is_empty())
            .collect();
        frames.push(Frame {
            position: gta_to_scene(position),
            rotation_deg: rotation,
            velocity: gta_to_scene(velocity),
            keys,
        });
    }
    Ok(frames)
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let map_path = args.next().unwrap_or_else(|| {
        "../../browser-three/dist/maps/dm-skynet-vol6-profeta.json".into()
    });
    let tas_path = args.next().unwrap_or_else(|| "profeta_slip.tas".into());
    let limit: usize = args
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(usize::MAX);
    let frames = load_tas(&tas_path)?;
    let bounds = {
        let mut low = Vec3::splat(f32::INFINITY);
        let mut high = Vec3::splat(f32::NEG_INFINITY);
        for frame in &frames {
            low = low.min(frame.position);
            high = high.max(frame.position);
        }
        let margin = Vec3::splat(80.0);
        Some((low - margin, high + margin))
    };
    let map = load_map(&map_path, bounds)?;
    println!(
        "map {} triangles, {} recorded frames ({:.1} s)",
        map.triangles,
        frames.len(),
        frames.len() as f32 * 0.02
    );
    let first = &frames[0];
    let basis = euler_basis(first.rotation_deg[0], first.rotation_deg[1], first.rotation_deg[2]);
    let forward = gta_to_scene(basis[1].to_array());
    let yaw = forward.x.atan2(forward.z);
    let mut car = Car::new(first.position, 0.6);
    // Optional fourth argument: the game folder, so the recording's model
    // (the Infernus, 411) drives with its own handling.cfg row.
    if let Some(folder) = args.next() {
        if let Some(handling) = sa_scene::handling::for_model(std::path::Path::new(&folder), "infernus") {
            println!("handling: infernus row from {folder}");
            car = car.with_handling(handling);
        } else {
            println!("handling: could not read the infernus row from {folder}");
        }
    }
    // DM race cars run a nitro bottle; the recordings hold `vehicle_fire`
    // through the speed sections, which is what makes them so fast.
    car.install_nitro(10);
    // The map's own handling is not in the collision export; the recorded car
    // clearly runs a faster one than stock (it is a speed map). Take the
    // recorded top speed as the profile's, or the replay is capped below the
    // recording and the car coasts once it is over its own limiter.
    let recorded_top = frames
        .iter()
        .map(|frame| frame.velocity.length() * 50.0)
        .fold(0.0f32, f32::max);
    let mut handling = car.handling;
    if recorded_top > handling.top_speed {
        println!(
            "handling: recorded top {:.0} m/s exceeds the row's {:.0}, using the recorded one",
            recorded_top, handling.top_speed
        );
        handling.top_speed = recorded_top;
        car = car.with_handling(handling);
    }
    car.set_state(first.position, yaw, first.velocity * 50.0);
    let ground = map
        .collision
        .segment_hit(first.position + Vec3::Y * 5.0, -Vec3::Y, 40.0);
    println!(
        "start ground: {:?} | start speed {:.1} km/h",
        ground.map(|hit| (hit.point.y, hit.normal.y)),
        first.velocity.length() * 50.0 * 3.6
    );
    let dt = 0.02;
    let mut errors = Vec::new();
    let mut max_speed = 0.0f32;
    let mut airborne = 0usize;
    let count = frames.len().min(limit);
    for index in 1..count {
        let held = &frames[index - 1].keys;
        let throttle = if held.iter().any(|k| k == "accelerate") {
            1.0
        } else if held.iter().any(|k| k == "brake_reverse") {
            -1.0
        } else {
            0.0
        };
        let steer = if held.iter().any(|k| k == "vehicle_left") {
            1.0
        } else if held.iter().any(|k| k == "vehicle_right") {
            -1.0
        } else {
            0.0
        };
        let handbrake = held.iter().any(|k| k == "handbrake");
        car.set_nitro(held.iter().any(|k| k == "vehicle_fire"));
        car.step(&map.collision, throttle, steer, handbrake, 1.0, dt);
        let error = car.position.distance(frames[index].position);
        errors.push(error);
        max_speed = max_speed.max(car.speed * 3.6);
        if car.airborne() {
            airborne += 1;
        }
        if index <= 10 {
            let d = car.debug();
            println!(
                "  f{index:03} v={:6.1} km/h gear={} gas={:.2} brake={:.2} drive={:.6} comp={:.2} counts={:?} air={}",
                car.speed * 3.6, d.gear, d.gas_pedal, d.brake_pedal, d.drive_acceleration,
                d.compression[0], d.wheel_counts, d.airborne
            );
        }
        if index % 250 == 0 {
            println!(
                "t={:5.1}s  sim=({:8.1},{:8.1},{:8.1})  rec=({:8.1},{:8.1},{:8.1})  err={:6.2} m  v={:5.1} km/h  gear {}",
                index as f32 * dt,
                car.position.x, car.position.y, car.position.z,
                frames[index].position.x, frames[index].position.y, frames[index].position.z,
                error, car.speed * 3.6, car.gear
            );
        }
    }
    let n = errors.len().max(1) as f32;
    let mean = errors.iter().sum::<f32>() / n;
    let rms = (errors.iter().map(|e| e * e).sum::<f32>() / n).sqrt();
    let max = errors.iter().cloned().fold(0.0f32, f32::max);
    let final_error = errors.last().copied().unwrap_or(0.0);
    println!(
        "\nframes {} | final error {:.1} m | mean {:.1} m | rms {:.1} m | max {:.1} m | top speed {:.0} km/h | airborne {:.1}%",
        errors.len(),
        final_error,
        mean,
        rms,
        max,
        max_speed,
        100.0 * airborne as f32 / n
    );
    Ok(())
}
