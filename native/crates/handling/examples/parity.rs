//! Replay a parity scenario through the Rust kernel and dump per-tick state.
//!
//! The scenario file is written by `parity/ref_run.py`, which runs the same
//! scenario through the validated Python kernel. Both sides see the same
//! handling row, the same seed pose and the same control schedule, and both
//! build wheel contacts from an infinite ground plane with the identical rule,
//! so any difference in the two dumps is a porting difference in the vehicle
//! model itself.
//!
//! Usage: parity <scenario.txt> <out.jsonl> <handling.cfg>

use std::fs;
use std::io::Write;

use sa_handling::contact::{WheelContact, WheelContactSet};
use sa_handling::handling::Handling;
use sa_handling::math::{Basis, Vec3};
use sa_handling::physical::Body;
use sa_handling::vehicle::{Controls, Vehicle};

/// Four suspension lines against the plane z = 0, in native wheel order.
///
/// Mirrors `ground_contacts` in `parity/ref_run.py`: the line runs from the
/// wheel mount to the line end along the body's own down axis, parameterised
/// by `lambda` with `z(lambda) = 0`; `lambda` is the line fraction.
fn ground_contacts(handling: &Handling, body: &Body, ground: bool) -> WheelContactSet {
    let half = handling.wheel_track / 2.0;
    let definitions = [
        (-half, handling.wheel_front_y),
        (-half, handling.wheel_rear_y),
        (half, handling.wheel_front_y),
        (half, handling.wheel_rear_y),
    ];
    let mut wheels: [Option<WheelContact>; 4] = Default::default();
    for (index, (x, y)) in definitions.iter().enumerate() {
        let (start, end) = handling.suspension_line(*x, *y);
        let world_start = body.position + body.basis.transform_local(start);
        let world_end = body.position + body.basis.transform_local(end);
        if !ground {
            let mut miss = WheelContact::miss(index);
            miss.surface_adhesive_limit = Some(4.5);
            wheels[index] = Some(miss);
            continue;
        }
        let drop = world_start.z - world_end.z;
        if drop <= 1.0e-6 {
            let mut miss = WheelContact::miss(index);
            miss.point = body
                .basis
                .transform_world_to_local(world_end - body.position);
            miss.surface_adhesive_limit = Some(4.5);
            wheels[index] = Some(miss);
            continue;
        }
        let fraction = world_start.z / drop;
        if !(0.0..=1.0).contains(&fraction) {
            let mut miss = WheelContact::miss(index);
            miss.point = body
                .basis
                .transform_world_to_local(world_end - body.position);
            miss.surface_adhesive_limit = Some(4.5);
            wheels[index] = Some(miss);
            continue;
        }
        let hit = world_start + (world_end - world_start) * fraction;
        let local = body.basis.transform_world_to_local(hit - body.position);
        let mut contact = WheelContact::hit(index, local, Vec3::new(0.0, 0.0, 1.0), fraction);
        contact.surface_adhesive_limit = Some(4.5);
        wheels[index] = Some(contact);
    }
    WheelContactSet { wheels }
}

fn rows(basis: Basis) -> String {
    let vectors = [basis.right, basis.forward, basis.up];
    let mut out = String::from("[");
    for (index, v) in vectors.iter().enumerate() {
        out.push_str(&format!(
            "[{:.9},{:.9},{:.9}]{}",
            v.x,
            v.y,
            v.z,
            if index < 2 { "," } else { "" }
        ));
    }
    out.push(']');
    out
}

fn triple(v: Vec3) -> String {
    format!("[{:.9},{:.9},{:.9}]", v.x, v.y, v.z)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: parity <scenario.txt> <out.jsonl> <handling.cfg>");
        std::process::exit(2);
    }
    let text = fs::read_to_string(&args[1]).expect("scenario file");
    let (mut name, mut ticks) = (String::from("scenario"), 0usize);
    let (mut ctimer, mut ground) = (1.0f32, true);
    let mut position = Vec3::new(0.0, 0.0, 0.55);
    let mut basis = Basis::default();
    let (mut move_speed, mut turn_speed) = (Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, 0.0));
    let mut schedule: Vec<Controls> = Vec::new();
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.is_empty() {
            continue;
        }
        let n = |i: usize| f.get(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        match f[0] {
            "name" => name = f.get(1).unwrap_or(&"scenario").to_string(),
            "ticks" => ticks = f.get(1).and_then(|v| v.parse().ok()).unwrap_or(0),
            "ctimer" => ctimer = n(1),
            "ground" => ground = f.get(1).map(|v| *v != "0").unwrap_or(true),
            "pos" => position = Vec3::new(n(1), n(2), n(3)),
            "basis" => {
                let m = [[n(1), n(2), n(3)], [n(4), n(5), n(6)], [n(7), n(8), n(9)]];
                basis = Basis::new(
                    Vec3::new(m[0][0], m[0][1], m[0][2]),
                    Vec3::new(m[1][0], m[1][1], m[1][2]),
                    Vec3::new(m[2][0], m[2][1], m[2][2]),
                );
            }
            "move" => move_speed = Vec3::new(n(1), n(2), n(3)),
            "turn" => turn_speed = Vec3::new(n(1), n(2), n(3)),
            "c" => schedule.push(Controls {
                accelerate: n(1),
                brake_reverse: n(2),
                // The scenario file is positive-for-left; the kernel takes the
                // raw GTA axis, positive-for-right.
                steering: -n(3),
                handbrake: n(4),
                steer_forward: 0.0,
                steer_back: 0.0,
                nitro: false,
            }),
            _ => {}
        }
    }

    let cfg = fs::read_to_string(&args[3]).expect("handling.cfg");
    let row = cfg
        .lines()
        .find(|line| {
            line.split_whitespace()
                .next()
                .map(|word| word.eq_ignore_ascii_case("INFERNUS"))
                .unwrap_or(false)
        })
        .expect("handling.cfg has no INFERNUS row");
    let handling = Handling::from_cfg_line(row).expect("parse INFERNUS");

    let mut vehicle = Vehicle::new(handling, position, ctimer);
    vehicle.body.basis = basis;
    vehicle.body.move_speed = move_speed;
    vehicle.body.turn_speed = turn_speed;
    vehicle.transmission.settle_history(move_speed.length());

    let mut out = String::new();
    let mut final_state = (0.0f32, 0.0f32, 0usize);
    for tick in 0..ticks {
        let controls = schedule.get(tick).cloned().unwrap_or_default();
        let contacts = ground_contacts(&vehicle.handling, &vehicle.body, ground);
        vehicle.tick(&controls, &contacts);
        let body = &vehicle.body;
        out.push_str(&format!(
            "{{\"tick\":{},\"position\":{},\"basis\":{},\"move_speed\":{},\"turn_speed\":{},\"gear\":{},\"compressions\":[{:.9},{:.9},{:.9},{:.9}],\"wheel_counts\":[{:.9},{:.9},{:.9},{:.9}],\"front_steer\":{:.9}}}\n",
            tick + 1,
            triple(body.position),
            rows(body.basis),
            triple(body.move_speed),
            triple(body.turn_speed),
            vehicle.transmission.current_gear,
            vehicle.previous_compressions[0],
            vehicle.previous_compressions[1],
            vehicle.previous_compressions[2],
            vehicle.previous_compressions[3],
            vehicle.wheel_counts[0],
            vehicle.wheel_counts[1],
            vehicle.wheel_counts[2],
            vehicle.wheel_counts[3],
            vehicle.front_steer_angle,
        ));
        final_state = (
            body.position.y,
            body.move_speed.length() * 50.0 * 3.6,
            vehicle.transmission.current_gear,
        );
    }
    let mut file = fs::File::create(&args[2]).expect("out file");
    file.write_all(out.as_bytes()).expect("write");
    println!(
        "{:9} {:5} ticks  final y={:9.2} v={:6.1} km/h gear={}",
        name, ticks, final_state.0, final_state.1, final_state.2
    );
}
