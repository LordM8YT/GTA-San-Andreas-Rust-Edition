//! Headless, procedural regression course for SARE vehicle physics.
//!
//! Run with: cargo run -p sa-scene --example vehicle-test-lab
//!
//! This uses project-generated collision geometry only; it does not load or
//! redistribute any original game assets.
use anyhow::{ensure, Result};
use glam::Vec3;
use sa_scene::{
    collision::CollisionWorld,
    vehicle::{Car, Handling},
    Batch, Vertex,
};

const DT: f32 = 1.0 / 120.0;

fn mesh(points: &[[f32; 3]]) -> Batch {
    Batch {
        key: "vehicle-test-lab".into(),
        alpha: false,
        animated: false,
        uv_animation: None,
        vertices: points
            .iter()
            .map(|p| Vertex {
                position: *p,
                ..Vertex::default()
            })
            .collect(),
    }
}

fn floor(z0: f32, z1: f32, y: f32) -> Batch {
    mesh(&[
        [-100.0, y, z0],
        [100.0, y, z0],
        [100.0, y, z1],
        [-100.0, y, z0],
        [100.0, y, z1],
        [-100.0, y, z1],
    ])
}

fn drive(car: &mut Car, world: &CollisionWorld, throttle: f32, steer: f32, seconds: f32) {
    for _ in 0..(seconds / DT).round() as usize {
        car.step(world, throttle, steer, false, 1.0, DT);
    }
}

fn rigid_car(at: Vec3) -> Car {
    let mut car = Car::new(at, 0.6).with_handling(Handling::default());
    car.set_rigid(true);
    car
}

fn flat_ground_test() -> Result<()> {
    let world = CollisionWorld::from_batches(&[floor(-100.0, 100.0, 0.0)]);
    let mut car = rigid_car(Vec3::Y * 0.7);
    drive(&mut car, &world, 0.0, 0.0, 3.0);
    ensure!(
        (car.position.y - 0.6).abs() < 0.05,
        "flat-ground ride height drifted to {:.3} m",
        car.position.y
    );
    ensure!(
        car.pitch.abs() < 0.01 && car.roll.abs() < 0.01,
        "car did not settle level: pitch={:.3}, roll={:.3}",
        car.pitch,
        car.roll
    );
    ensure!(
        car.velocity().length() < 0.05 && !car.airborne(),
        "car did not settle: velocity={:?}, airborne={}",
        car.velocity(),
        car.airborne()
    );
    println!(
        "PASS flat-ground suspension: y={:.3} m, pitch={:.3}, roll={:.3}",
        car.position.y, car.pitch, car.roll
    );
    Ok(())
}

fn wall_impact_test() -> Result<()> {
    let wall = mesh(&[
        [5.0, -1.0, -60.0],
        [5.0, 5.0, -60.0],
        [5.0, 5.0, 60.0],
        [5.0, -1.0, -60.0],
        [5.0, 5.0, 60.0],
        [5.0, -1.0, 60.0],
    ]);
    let world = CollisionWorld::from_batches(&[floor(-100.0, 100.0, 0.0), wall]);
    let mut car = rigid_car(Vec3::new(0.0, 0.7, -20.0));
    car.yaw = 0.5;
    car.speed = 20.0;
    drive(&mut car, &world, 0.0, 0.0, 2.0);
    ensure!(
        car.position.x < 5.0 && car.velocity().length() > 6.0,
        "wall test failed: position={:?}, velocity={:?}",
        car.position,
        car.velocity()
    );
    ensure!(
        car.velocity().x < 1.0,
        "wall contact did not deflect the car as expected: velocity={:?}",
        car.velocity()
    );
    println!(
        "PASS oblique wall impact: position=({:.2}, {:.2}, {:.2}), speed={:.2} m/s",
        car.position.x,
        car.position.y,
        car.position.z,
        car.velocity().length()
    );
    Ok(())
}

fn ramp_and_landing_test() -> Result<()> {
    let world = CollisionWorld::from_batches(&[
        mesh(&[
            [-20.0, 0.0, -100.0],
            [20.0, 0.0, -100.0],
            [20.0, 0.0, 0.0],
            [-20.0, 0.0, -100.0],
            [20.0, 0.0, 0.0],
            [-20.0, 0.0, 0.0],
        ]),
        mesh(&[
            [-20.0, 0.0, 0.0],
            [20.0, 0.0, 0.0],
            [20.0, 3.0, 12.0],
            [-20.0, 0.0, 0.0],
            [20.0, 3.0, 12.0],
            [-20.0, 3.0, 12.0],
        ]),
        floor(12.0, 112.0, -2.0),
    ]);
    let mut car = rigid_car(Vec3::new(0.0, 0.7, -40.0));
    car.speed = 25.0;
    let mut flew = false;
    for _ in 0..600 {
        car.step(&world, 0.3, 0.0, false, 1.0, DT);
        flew |= car.airborne() && car.position.z > 12.0;
    }
    ensure!(flew, "car never became airborne beyond the ramp");
    let up = car.model_orientation() * Vec3::Y;
    ensure!(
        up.y > 0.95 && !car.airborne(),
        "car failed to land upright: up={:?}, position={:?}, airborne={}",
        up,
        car.position,
        car.airborne()
    );
    println!(
        "PASS ramp and landing: position=({:.2}, {:.2}, {:.2}), vertical speed={:.2} m/s",
        car.position.x,
        car.position.y,
        car.position.z,
        car.velocity().y
    );
    Ok(())
}

fn main() -> Result<()> {
    println!("SARE Vehicle Test Lab — procedural headless checks");
    flat_ground_test()?;
    wall_impact_test()?;
    ramp_and_landing_test()?;
    println!("All Vehicle Test Lab checks passed.");
    Ok(())
}
