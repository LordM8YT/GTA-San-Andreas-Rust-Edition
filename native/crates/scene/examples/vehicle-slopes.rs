//! Read-only suspension audit on planar sloped collision surfaces near Grove Street.
use anyhow::{Context, Result};
use glam::Vec3;
use sa_scene::{vehicle::Car, WorldLoader};
use std::path::PathBuf;

fn main() -> Result<()> {
    let game = PathBuf::from(
        std::env::args()
            .nth(1)
            .context("Pass your San Andreas directory")?,
    );
    let origin = [2500.0, -1670.0];
    let scene = WorldLoader::open(&game)?.load(origin, origin, 400.0)?;
    let world = scene.collision.context("No original collision data")?;
    let mut checked = 0;
    let mut max_error = 0.0_f32;
    let mut max_wheel_error = 0.0_f32;
    for x in (-320..40).step_by(10) {
        for z in (-200..200).step_by(10) {
            let p = Vec3::new(x as f32, 0.0, z as f32);
            let Some(y) = world.ground_below(p, 35.0) else {
                continue;
            };
            let sample = |dx, dz| world.ground_below(p + Vec3::new(dx, 0.0, dz), y + 0.8);
            let (Some(left), Some(right), Some(back), Some(front)) = (
                sample(-1.35, 0.0),
                sample(1.35, 0.0),
                sample(0.0, -1.35),
                sample(0.0, 1.35),
            ) else {
                continue;
            };
            let gx = (right - left) / 2.7;
            let gz = (front - back) / 2.7;
            let grade = (gx * gx + gz * gz).sqrt();
            if !(0.12..0.5).contains(&grade) {
                continue;
            }
            if world
                .ceiling_above(p, y + 0.05)
                .is_some_and(|roof| roof < y + 2.0)
            {
                continue;
            }
            // Reject seams/curbs and mixed floors: this audit requires one road plane.
            if ![-0.72, 0.72].into_iter().all(|dx| {
                [-1.35, 1.35].into_iter().all(|dz| {
                    sample(dx, dz).is_some_and(|h| (h - (y + dx * gx + dz * gz)).abs() < 0.025)
                })
            }) {
                continue;
            }
            for yaw in [0.0, 0.8, -1.7, std::f32::consts::PI] {
                let mut car = Car::new(Vec3::new(p.x, y + 0.6, p.z), 0.6);
                car.yaw = yaw;
                for _ in 0..120 {
                    car.step(&world, 0.0, 0.0, false, 1.0, 1.0 / 120.0);
                }
                let expected = y + 0.6 * (1.0 + grade * grade).sqrt();
                let error = (car.position.y - expected).abs();
                max_error = max_error.max(error);
                anyhow::ensure!(
                    error < 0.08,
                    "Suspension error {error:.3} m at GTA ({:.0}, {:.0}), grade={grade:.2}",
                    origin[0] + p.x,
                    origin[1] - p.z
                );
                let rotation = sa_scene::vehicle::model_rotation(car.yaw, car.pitch, car.roll);
                for x in [-0.72, 0.72] {
                    for z in [-1.35, 1.35] {
                        let wheel = rotation * Vec3::new(x, -car.clearance, z) + car.position;
                        let road = world
                            .ground_below(wheel, wheel.y + 0.5)
                            .context("Rendered wheel has no original road support")?;
                        let gap = (wheel.y - road).abs();
                        max_wheel_error = max_wheel_error.max(gap);
                        anyhow::ensure!(
                            gap < 0.08,
                            "Rendered wheel error {gap:.3} m at GTA ({:.0}, {:.0}), yaw={yaw}",
                            origin[0] + p.x,
                            origin[1] - p.z
                        );
                    }
                }
                checked += 1;
            }
        }
    }
    anyhow::ensure!(checked > 0, "No sloped collision patches found");
    println!("Original Grove Street collision slope audit passed: {checked} orientations, maximum body height error {max_error:.3} m, rendered wheel error {max_wheel_error:.3} m");
    Ok(())
}
