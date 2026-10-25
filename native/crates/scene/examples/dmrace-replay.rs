//! Replay a DMRace per-tick recording through the car kernel and compare.
//!
//! The recording stores, every tick: the eleven inputs, the world position,
//! the orientation as forward and up vectors, the velocity in GTA units per
//! step and the angular velocity in radians per step. That is the whole input
//! to the model plus the whole state to check it against, so this is a real
//! oracle rather than a plausibility test: the kernel is fed the recorded
//! inputs from the recorded first frame, and every later state is its own.
//!
//! `cargo run --release -p sa-scene --example dmrace-replay -- \
//!    <map.json> <recording.csv> [limit]`
use glam::Vec3;
use sa_scene::collision::CollisionWorld;
use sa_scene::vehicle::Car;
use sa_scene::{Batch, Vertex};
use std::fs;

fn gta_to_scene(p: [f32; 3]) -> Vec3 {
    Vec3::new(p[0], p[2], p[1])
}

/// MTA stores `R = Rz(rz) * Ry(ry) * Rx(rx)` in degrees; columns are the basis.
#[allow(dead_code)]
fn euler_basis(rx: f32, ry: f32, rz: f32) -> [Vec3; 3] {
    let (rx, ry, rz) = (rx.to_radians(), ry.to_radians(), rz.to_radians());
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
fn load_map(
    path: &str,
    bounds: Option<(Vec3, Vec3)>,
    recorded_path: &[Vec3],
) -> anyhow::Result<MapScene> {
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
        [0, 1, 2],
        [0, 2, 3],
        [4, 6, 5],
        [4, 7, 6],
        [0, 4, 5],
        [0, 5, 1],
        [1, 5, 6],
        [1, 6, 2],
        [2, 6, 7],
        [2, 7, 3],
        [3, 7, 4],
        [3, 4, 0],
    ];
    // The collision export does not carry the original's per-object "solid"
    // flag, so decorative geometry (the gates over the start line) becomes
    // walls. For a replay the recording is the authority on what was solid:
    // skip any instance the recorded car passes through.
    let corridor = {
        let mut sampled = Vec::new();
        for (i, point) in recorded_path.iter().enumerate() {
            if i % 4 == 0 {
                sampled.push(*point);
            }
        }
        sampled
    };
    let passes_through = |low: Vec3, high: Vec3| {
        let pad = Vec3::splat(1.2);
        let (low, high) = (low - pad, high + pad);
        corridor
            .iter()
            .any(|p| p.cmpge(low).all() && p.cmple(high).all())
    };
    let mut skipped = 0usize;
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
        // Row-major 4x4 with translation in the last column.
        let mut m = [[0.0f32; 4]; 4];
        if let Some(rows) = instance["transform"].as_array() {
            for (i, row) in rows.iter().take(4).enumerate() {
                for (j, value) in row.as_array().into_iter().flatten().take(4).enumerate() {
                    m[i][j] = value.as_f64().unwrap_or(0.0) as f32;
                }
            }
        }
        // World-space bounds of this instance, from the model's own bounds.
        if let Some(local) = collision.get("bounds") {
            let read = |value: &serde_json::Value, i: usize| {
                value
                    .as_array()
                    .and_then(|v| v.get(i))
                    .and_then(|x| x.as_f64())
                    .unwrap_or(0.0) as f32
            };
            let (min, max) = (&local["min"], &local["max"]);
            let mut low = Vec3::splat(f32::INFINITY);
            let mut high = Vec3::splat(f32::NEG_INFINITY);
            for i in 0..8 {
                let corner = [
                    if i & 1 == 0 {
                        read(min, 0)
                    } else {
                        read(max, 0)
                    },
                    if i & 2 == 0 {
                        read(min, 1)
                    } else {
                        read(max, 1)
                    },
                    if i & 4 == 0 {
                        read(min, 2)
                    } else {
                        read(max, 2)
                    },
                ];
                let world = gta_to_scene([
                    m[0][0] * corner[0] + m[0][1] * corner[1] + m[0][2] * corner[2] + m[0][3],
                    m[1][0] * corner[0] + m[1][1] * corner[1] + m[1][2] * corner[2] + m[1][3],
                    m[2][0] * corner[0] + m[2][1] * corner[1] + m[2][2] * corner[2] + m[2][3],
                ]);
                low = low.min(world);
                high = high.max(world);
            }
            if passes_through(low, high) {
                skipped += 1;
                continue;
            }
        }
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
                // A panel thinner than a tyre is a gate or a sign, not ground:
                // in the source map these are decorative and non-solid, and a
                // car cannot rest on nine centimetres of plastic.
                let thinnest = (hi[0] - lo[0]).min(hi[1] - lo[1]).min(hi[2] - lo[2]);
                if thinnest < 0.4 {
                    continue;
                }
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
                    push(
                        &mut vertices,
                        corners[face[0]],
                        corners[face[1]],
                        corners[face[2]],
                    );
                }
            }
        }
        let model_vertices = collision["vertices"]
            .as_array()
            .cloned()
            .unwrap_or_default();
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
                if let (Some(a), Some(b), Some(c)) = (
                    points.get(a).copied(),
                    points.get(b).copied(),
                    points.get(c).copied(),
                ) {
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
    if skipped > 0 {
        println!("skipped {skipped} instances the recorded car passes through");
    }
    Ok(MapScene {
        collision,
        triangles,
    })
}

struct Frame {
    throttle: f32,
    brake: f32,
    steer: f32,
    handbrake: bool,
    nitro: bool,
    position: Vec3,
    forward: Vec3,
    up: Vec3,
    velocity: Vec3,
    angular: Vec3,
    /// Non-empty when the recording restarts here (spawn, reset, rewind).
    event: String,
}

fn triple(fields: &[&str], start: usize) -> Vec3 {
    let read = |i: usize| {
        fields
            .get(i)
            .and_then(|v| v.parse::<f32>().ok())
            .unwrap_or(0.0)
    };
    Vec3::new(read(start), read(start + 1), read(start + 2))
}

fn load_recording(path: &str) -> anyhow::Result<(Vec<Frame>, f32, usize)> {
    let text = fs::read_to_string(path)?;
    let mut frames = Vec::new();
    let mut tick_rate = 50.0f32;
    let mut timestep = 1.0f32;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            if let Some(value) = rest.strip_prefix("tick_rate=") {
                tick_rate = value.trim().parse().unwrap_or(50.0);
            }
            continue;
        }
        if line.starts_with("tick,") {
            continue; // header row
        }
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() < 32 {
            continue;
        }
        // tick, timestep, steer, accelerate, brake, steer_updown, handbrake,
        // nitro, look_left, look_right, look_behind, horn, event,
        // x, y, z, rx, ry, rz, fx, fy, fz, ux, uy, uz, vx, vy, vz, wx, wy, wz,
        // nitro_level, nitro_gas
        timestep = fields[1].parse().unwrap_or(1.0);
        let accelerate: f32 = fields[3].parse().unwrap_or(0.0);
        let brake: f32 = fields[4].parse().unwrap_or(0.0);
        let handbrake: f32 = fields[6].parse().unwrap_or(0.0);
        let nitro: f32 = fields[7].parse().unwrap_or(0.0);
        frames.push(Frame {
            throttle: accelerate,
            brake,
            steer: fields[2].parse().unwrap_or(0.0),
            handbrake: handbrake != 0.0,
            nitro: nitro != 0.0,
            position: triple(&fields, 13),
            forward: triple(&fields, 19),
            up: triple(&fields, 22),
            velocity: triple(&fields, 25),
            angular: triple(&fields, 28),
            event: fields.get(12).unwrap_or(&"").to_string(),
        });
    }
    Ok((frames, timestep, tick_rate as usize))
}

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let map_path = args.next().expect("map json");
    let recording = args.next().expect("recording csv");
    let limit: usize = args
        .next()
        .and_then(|v| v.parse().ok())
        .unwrap_or(usize::MAX);
    let steer_sign: f32 = args.next().and_then(|v| v.parse().ok()).unwrap_or(1.0);
    let map = load_map(&map_path, None, &[])?;
    let (frames, timestep, tick_rate) = load_recording(&recording)?;
    println!(
        "map {} triangles | recording {} ticks at {} Hz (timestep {:.4}) = {:.1} s",
        map.triangles,
        frames.len(),
        tick_rate,
        timestep,
        frames.len() as f32 * timestep / 50.0
    );
    // The source map may run a tuned profile: take the recorded top speed as
    // the car's when it exceeds the stock row, or the replay is capped below
    // the recording from the start.
    let recorded_top = frames
        .iter()
        .map(|frame| frame.velocity.length() * 50.0)
        .fold(0.0f32, f32::max);
    let first = &frames[0];
    let mut car = Car::new(first.position, 0.6);
    // The recording drives the model the map expects; take its handling from
    // the row the spawn names when available.
    if let Ok(folder) = std::env::var("GTA_SA_DIR") {
        if let Some(handling) =
            sa_scene::handling::for_model(std::path::Path::new(&folder), "infernus")
        {
            car = car.with_handling(handling);
        }
    }
    car.install_nitro(10);
    car.set_full_state(
        first.position,
        first.forward,
        first.up,
        first.velocity * 50.0,
        first.angular * 50.0,
    );
    let dt = timestep / 50.0;
    // A recording is a sequence of attempts: the player rewinds or respawns,
    // and the state jumps. Replaying across a jump is meaningless, so cut the
    // file at every event and treat each piece as its own run from its own
    // first frame.
    let mut starts: Vec<usize> = vec![0];
    for (index, frame) in frames.iter().enumerate() {
        if index > 0 && !frame.event.trim().is_empty() {
            starts.push(index);
        }
    }
    let mut overall = Vec::new();
    let mut worst = 0.0f32;
    let mut longest = 0usize;
    for (segment, start) in starts.iter().enumerate() {
        let end_index = starts
            .get(segment + 1)
            .copied()
            .unwrap_or(frames.len())
            .min(limit);
        if end_index <= start + 1 {
            continue;
        }
        let seed = &frames[*start];
        let mut car = Car::new(seed.position, 0.6);
        if let Ok(folder) = std::env::var("GTA_SA_DIR") {
            if let Some(mut handling) =
                sa_scene::handling::for_model(std::path::Path::new(&folder), "infernus")
            {
                if recorded_top > handling.top_speed {
                    handling.top_speed = recorded_top;
                }
                car = car.with_handling(handling);
            }
        }
        car.install_nitro(10);
        car.set_full_state(
            seed.position,
            seed.forward,
            seed.up,
            seed.velocity * 50.0,
            seed.angular * 50.0,
        );
        let mut errors = Vec::new();
        let mut velocity_errors = Vec::new();
        let mut first_bad: Option<usize> = None;
        for index in (*start + 1)..end_index {
            let frame = &frames[index - 1];
            car.set_nitro(frame.nitro);
            car.step_tick(
                &map.collision,
                frame.throttle - frame.brake,
                frame.steer * steer_sign,
                frame.handbrake,
                1.0,
            );
            let expected = &frames[index];
            let error = car.position.distance(expected.position);
            if segment == 0 && index <= 8 {
                println!(
                    "   t{:02} sim=({:9.2},{:9.2},{:9.2}) v={:6.1} | rec=({:9.2},{:9.2},{:9.2}) v={:6.1} | err {:.2}",
                    index,
                    car.position.x, car.position.y, car.position.z,
                    car.velocity().length() * 3.6,
                    expected.position.x, expected.position.y, expected.position.z,
                    expected.velocity.length() * 50.0 * 3.6,
                    error
                );
            }
            errors.push(error);
            velocity_errors.push((car.velocity() - expected.velocity * 50.0).length());
            if first_bad.is_none() && error > 2.0 {
                first_bad = Some(index - *start);
            }
        }
        let n = errors.len().max(1) as f32;
        let mean = errors.iter().sum::<f32>() / n;
        let max = errors.iter().cloned().fold(0.0f32, f32::max);
        let final_error = errors.last().copied().unwrap_or(0.0);
        let v_rms = (velocity_errors.iter().map(|e| e * e).sum::<f32>() / n).sqrt();
        overall.extend(errors.iter().copied());
        worst = worst.max(max);
        longest = longest.max(errors.len());
        println!(
            "segment {segment}: {} ticks ({:.1} s) | first>2m at tick {:?} | final {:.1} m | mean {:.1} m | max {:.1} m | v rms {:.1} m/s",
            errors.len(),
            errors.len() as f32 * dt,
            first_bad.map(|t| (t, format!("{:.2}s", t as f32 * dt))),
            final_error,
            mean,
            max,
            v_rms
        );
    }
    let n = overall.len().max(1) as f32;
    let rms = (overall.iter().map(|e| e * e).sum::<f32>() / n).sqrt();
    let mean = overall.iter().sum::<f32>() / n;
    println!(
        "\nall segments: {} ticks, mean {:.2} m, rms {:.2} m, worst max {:.1} m, longest segment {} ticks",
        overall.len(),
        mean,
        rms,
        worst,
        longest
    );
    Ok(())
}
