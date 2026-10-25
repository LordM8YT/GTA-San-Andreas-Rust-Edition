//! Straight-line measurements for real handling rows.
//!
//! `cargo run --release -p sa-scene --example measure -- "F:/GTA San Andreas"`
use glam::Vec3;
use sa_scene::collision::CollisionWorld;
use sa_scene::vehicle::Car;
use sa_scene::{Batch, Vertex};
use std::path::Path;

fn floor(z0: f32, z1: f32, y: f32) -> Batch {
    Batch {
        key: "floor".into(),
        alpha: false,
        animated: false,
        uv_animation: None,
        // Keep each triangle inside the collision grid's per-triangle cell
        // budget (4096 cells): 40 m wide by 4000 m is 5 x 500 cells.
        vertices: [
            [-20.0, y, z0],
            [20.0, y, z0],
            [20.0, y, z1],
            [-20.0, y, z0],
            [20.0, y, z1],
            [-20.0, y, z1],
        ]
        .iter()
        .map(|p| Vertex {
            position: *p,
            ..Default::default()
        })
        .collect(),
    }
}

fn kmh(metres_per_second: f32) -> f32 {
    metres_per_second * 3.6
}

fn measure(model: &str, folder: &Path) {
    let Some(handling) = sa_scene::handling::for_model(folder, model) else {
        println!("{model:<10} no handling row found");
        return;
    };
    let world = CollisionWorld::from_batches(&[floor(-2000.0, 2000.0, 0.0)]);
    println!(
        "  row: mass {:.0} turn {:.0} accel {:.2} top {:.1} grip {:.2} gears {:.0} drive {:.0}",
        handling.mass,
        handling.turn_mass,
        handling.acceleration,
        handling.top_speed,
        handling.tire_grip,
        handling.gears,
        handling.drive
    );
    let mut car = Car::new(Vec3::Y * 0.6, 0.6).with_handling(handling);
    let dt = 1.0 / 120.0;
    let mut t_50 = None;
    let mut t_100 = None;
    let mut top = 0.0f32;
    let mut gears_seen = 0u8;
    for step in 0..2400 {
        car.step(&world, 1.0, 0.0, false, 1.0, dt);
        if step == 60 {
            let d = car.debug();
            println!(
                "  after 1 s: speed {:.2} m/s gear {} air {} comp {:.2} y {:.2} engine {:.2} force {:.3} drag {:.3} maxv {:.0} mass {:.0} step {:.3}",
                d.speed, d.gear, d.airborne, d.compression[0], d.position.y,
                d.engine, d.force_level, d.drag, d.max_velocity, d.mass, d.step
            );
        }
        let speed = kmh(car.speed);
        gears_seen = gears_seen.max(car.gear);
        if t_50.is_none() && speed >= 50.0 {
            t_50 = Some(step as f32 * dt);
        }
        if t_100.is_none() && speed >= 100.0 {
            t_100 = Some(step as f32 * dt);
        }
        top = top.max(speed);
    }
    let start = car.position;
    let mut distance = 0.0;
    for _ in 0..1200 {
        car.step(&world, -1.0, 0.0, false, 1.0, dt);
        distance = (car.position - start).length();
        if car.speed.abs() < 0.2 {
            break;
        }
    }
    println!(
        "{model:<10} 0-50 {:>5}  0-100 {:>5}  top {:>5.0} km/h  gears {}  brake {:>4.0} m",
        t_50.map_or("-".to_string(), |s| format!("{s:.2}s")),
        t_100.map_or("-".to_string(), |s| format!("{s:.2}s")),
        top,
        gears_seen,
        distance
    );
}

fn main() {
    let folder = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "F:/GTA San Andreas".to_string());
    let folder = Path::new(&folder);
    println!("handling rows from {}", folder.display());
    for model in ["infernus", "taxi", "comet", "admiral", "sentinel", "roadtrain"] {
        measure(model, folder);
    }
}
