//! Drive the scene layer over flat ground and dump what the player would see.
//!
//! Same scenario files as `parity/ref_run.py` (the validated Python GTA
//! kernel), but through [`sa_scene::vehicle::Car`]: suspension lines cast
//! against a real triangle mesh, keyboard-shaped inputs, poses out. A
//! difference here is a bug in the scene's contact generation, unit conversion
//! or input plumbing rather than in the vehicle model.
//!
//! Usage: scene-parity <scenario.txt> <out.jsonl> <GTA folder>

use std::fs;
use std::io::Write;
use std::path::Path;

use glam::Vec3;
use sa_scene::collision::CollisionWorld;
use sa_scene::vehicle::Car;
use sa_scene::{Batch, Vertex};

const TILES: i32 = 40;
const TILE: f32 = 64.0;

/// A flat ground plane as a tile grid: one huge triangle would exceed the
/// collision world's per-triangle cell budget and be dropped.
fn ground() -> CollisionWorld {
    let vertex = |x: f32, z: f32| Vertex {
        position: [x, 0.0, z],
        uv: [0.0, 0.0],
        color: [1.0; 4],
    };
    let mut vertices = Vec::new();
    for ix in -TILES..TILES {
        for iz in -TILES..TILES {
            let (x0, z0) = (ix as f32 * TILE, iz as f32 * TILE);
            let (x1, z1) = (x0 + TILE, z0 + TILE);
            vertices.extend([vertex(x0, z0), vertex(x0, z1), vertex(x1, z1)]);
            vertices.extend([vertex(x0, z0), vertex(x1, z1), vertex(x1, z0)]);
        }
    }
    CollisionWorld::from_batches(&[Batch {
        key: "ground".into(),
        vertices,
        alpha: false,
        animated: false,
        uv_animation: None,
    }])
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: scene-parity <scenario.txt> <out.jsonl> <GTA folder>");
        std::process::exit(2);
    }
    let text = fs::read_to_string(&args[1]).expect("scenario file");
    let (mut ticks, mut ground_on) = (0usize, true);
    let mut ctimer = 1.0f32;
    let mut position = Vec3::new(0.0, 0.55, 0.0);
    let mut seed_velocity = Vec3::ZERO;
    let mut schedule: Vec<(f32, f32, f32, f32)> = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.is_empty() {
            continue;
        }
        let n = |i: usize| f.get(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        match f[0] {
            "ticks" => ticks = f.get(1).and_then(|v| v.parse().ok()).unwrap_or(0),
            "ctimer" => ctimer = n(1),
            "ground" => ground_on = f.get(1).map(|v| *v != "0").unwrap_or(true),
            // Kernel-space seed (z up) -> scene space (y up): the same y/z swap
            // the rest of the scene uses.
            "pos" => position = Vec3::new(n(1), n(3), n(2)),
            "move" => seed_velocity = Vec3::new(n(1), n(3), n(2)) * 50.0,
            "c" => schedule.push((n(1), n(2), n(3), n(4))),
            _ => {}
        }
    }
    let _ = ctimer;

    let world = ground();
    let handling = sa_scene::handling::for_model(Path::new(&args[3]), "infernus")
        .expect("handling.cfg row for the Infernus");
    let mut car = Car::new(position, 0.6).with_handling(handling);
    car.set_state(position, 0.0, seed_velocity);

    let mut out = String::new();
    for tick in 0..ticks {
        let (throttle, brake, steer, handbrake) = schedule.get(tick).cloned().unwrap_or_default();
        car.step_tick(&world, throttle - brake, steer, handbrake > 0.5, 1.0);
        if !ground_on {
            // Nothing to disable: with no ground the lines miss on their own.
        }
        let kernel = car.kernel();
        let body = &kernel.body;
        let rows = [body.basis.right, body.basis.forward, body.basis.up];
        let mut basis = String::from("[");
        for (index, v) in rows.iter().enumerate() {
            basis.push_str(&format!(
                "[{:.9},{:.9},{:.9}]{}",
                v.x,
                v.y,
                v.z,
                if index < 2 { "," } else { "" }
            ));
        }
        basis.push(']');
        out.push_str(&format!(
            "{{\"tick\":{},\"position\":[{:.9},{:.9},{:.9}],\"basis\":{},\"move_speed\":[{:.9},{:.9},{:.9}],\"turn_speed\":[{:.9},{:.9},{:.9}],\"gear\":{},\"compressions\":[{:.9},{:.9},{:.9},{:.9}],\"wheel_counts\":[{:.9},{:.9},{:.9},{:.9}],\"front_steer\":{:.9}}}\n",
            tick + 1,
            body.position.x,
            body.position.y,
            body.position.z,
            basis,
            body.move_speed.x,
            body.move_speed.y,
            body.move_speed.z,
            body.turn_speed.x,
            body.turn_speed.y,
            body.turn_speed.z,
            kernel.transmission.current_gear,
            kernel.previous_compressions[0],
            kernel.previous_compressions[1],
            kernel.previous_compressions[2],
            kernel.previous_compressions[3],
            kernel.wheel_counts[0],
            kernel.wheel_counts[1],
            kernel.wheel_counts[2],
            kernel.wheel_counts[3],
            kernel.front_steer_angle,
        ));
    }
    let mut file = fs::File::create(&args[2]).expect("out file");
    file.write_all(out.as_bytes()).expect("write");
    println!(
        "scene {:<9} {ticks} ticks  final kernel pos=({:.2},{:.2},{:.2})",
        Path::new(&args[1])
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?"),
        car.kernel().body.position.x,
        car.kernel().body.position.y,
        car.kernel().body.position.z
    );
}
