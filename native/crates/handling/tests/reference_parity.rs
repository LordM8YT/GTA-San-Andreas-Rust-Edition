//! Parity with the validated reference kernel.
//!
//! `parity/golden.txt` holds state sampled from the reference Python port of
//! GTA's automobile model (infernus-physics, which is itself checked against
//! native game telemetry) for a set of fixed scenarios. This test drives the
//! same scenarios through this crate and asserts that the two agree.
//!
//! What it protects: the gear table, the drive-acceleration inertia pair, the
//! springs and dampers, tyre friction and every integration step, over 18
//! seconds of acceleration, braking, cornering, a handbrake turn and free fall.
//! A change that alters the model in any of those paths fails here instead of
//! looking subtly wrong in a game hours later.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use sa_handling::contact::{plane_contacts, WheelContact, WheelContactSet};
use sa_handling::handling::Handling;
use sa_handling::math::Basis;
use sa_handling::vehicle::{Controls, Vehicle};
use sa_handling::Vec3;

/// Position tolerance in metres. The two implementations do the same f32
/// arithmetic in a different order, so they agree to rounding only; measured
/// worst cases are 0.9 mm over 18 s of acceleration and 5.2 mm over 12 s of
/// steady cornering, which is why the bound is a centimetre.
const POSITION_TOLERANCE: f32 = 1.0e-2;
const VELOCITY_TOLERANCE: f32 = 5.0e-3;

fn parity_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../parity")
}

struct Scenario {
    ticks: usize,
    ground: bool,
    position: Vec3,
    basis: Basis,
    move_speed: Vec3,
    controls: Vec<Controls>,
}

fn load_scenario(name: &str) -> Scenario {
    let path = parity_dir()
        .join("scenarios")
        .join(format!("{name}.scenario.txt"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let mut scenario = Scenario {
        ticks: 0,
        ground: true,
        position: Vec3::new(0.0, 0.0, 0.55),
        basis: Basis::default(),
        move_speed: Vec3::new(0.0, 0.0, 0.0),
        controls: Vec::new(),
    };
    for line in text.lines() {
        let f: Vec<&str> = line.split_whitespace().collect();
        if f.is_empty() {
            continue;
        }
        let n = |i: usize| f.get(i).and_then(|v| v.parse::<f32>().ok()).unwrap_or(0.0);
        match f[0] {
            "ticks" => scenario.ticks = f.get(1).and_then(|v| v.parse().ok()).unwrap_or(0),
            "ground" => scenario.ground = f.get(1).map(|v| *v != "0").unwrap_or(true),
            "pos" => scenario.position = Vec3::new(n(1), n(2), n(3)),
            "basis" => {
                let m = [[n(1), n(2), n(3)], [n(4), n(5), n(6)], [n(7), n(8), n(9)]];
                scenario.basis = Basis::new(
                    Vec3::new(m[0][0], m[0][1], m[0][2]),
                    Vec3::new(m[1][0], m[1][1], m[1][2]),
                    Vec3::new(m[2][0], m[2][1], m[2][2]),
                );
            }
            "move" => scenario.move_speed = Vec3::new(n(1), n(2), n(3)),
            "c" => scenario.controls.push(Controls {
                accelerate: n(1),
                brake_reverse: n(2),
                // Scenario files are positive-for-left; the kernel takes GTA's
                // raw axis, positive-for-right.
                steering: -n(3),
                handbrake: n(4),
                steer_forward: 0.0,
                steer_back: 0.0,
                nitro: false,
            }),
            _ => {}
        }
    }
    scenario
}

/// The stock row the goldens were produced with, committed next to them so the
/// test needs neither a game install nor a matching `Handling::default()`.
fn handling() -> Handling {
    let path = parity_dir().join("INFERNUS.txt");
    let row = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    Handling::from_cfg_line(&row).expect("parse the committed INFERNUS row")
}

type Golden = BTreeMap<String, BTreeMap<usize, [f32; 18]>>;

fn load_golden() -> Golden {
    let path = parity_dir().join("golden.txt");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let mut out: Golden = BTreeMap::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        let name = f[0].to_string();
        let tick: usize = f[1].parse().expect("tick");
        let mut values = [0.0f32; 18];
        for (index, value) in f[2..].iter().enumerate() {
            values[index] = value.parse().expect("value");
        }
        out.entry(name).or_default().insert(tick, values);
    }
    out
}

#[test]
fn the_kernel_matches_the_validated_reference() {
    let golden = load_golden();
    assert!(!golden.is_empty(), "no golden rows loaded");
    let handling = handling();
    let mut failures = Vec::new();
    for (name, ticks) in &golden {
        let scenario = load_scenario(name);
        let mut vehicle = Vehicle::new(handling, scenario.position, 1.0);
        vehicle.body.basis = scenario.basis;
        vehicle.body.move_speed = scenario.move_speed;
        vehicle
            .transmission
            .settle_history(scenario.move_speed.length());
        let last = *ticks.keys().max().unwrap();
        for tick in 1..=last.min(scenario.ticks) {
            let controls = scenario.controls.get(tick - 1).cloned().unwrap_or_default();
            let contacts = if scenario.ground {
                plane_contacts(&handling, vehicle.body.position, vehicle.body.basis, 0.0)
            } else {
                let mut set = WheelContactSet::default();
                for index in 0..4 {
                    set.wheels[index] = Some(WheelContact::miss(index));
                }
                set
            };
            vehicle.tick(&controls, &contacts);
            let Some(expected) = ticks.get(&tick) else {
                continue;
            };
            let body = &vehicle.body;
            let position_error =
                (body.position - Vec3::new(expected[0], expected[1], expected[2])).length();
            let velocity_error =
                (body.move_speed - Vec3::new(expected[3], expected[4], expected[5])).length();
            if position_error > POSITION_TOLERANCE || velocity_error > VELOCITY_TOLERANCE {
                failures.push(format!(
                    "{name} tick {tick}: position off {position_error:.5} m, velocity off {velocity_error:.5} raw"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} reference mismatches:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
