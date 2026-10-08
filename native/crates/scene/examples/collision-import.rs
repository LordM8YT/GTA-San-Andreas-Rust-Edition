//! Verify the owned FiveM map's explicit COL contacts in the real streamed world.
use anyhow::{ensure, Context, Result};
use glam::Vec3;
use sa_scene::{vehicle::Car, WorldLoader};
use std::path::PathBuf;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let game = PathBuf::from(args.next().context("game directory")?);
    let mods = PathBuf::from(
        args.next()
            .context("mods directory with converted collision example")?,
    );
    let origin = [2500.0, -1670.0];
    let mut loader = WorldLoader::open(&game)?;
    loader.enable_mods(&mods)?;
    let scene = loader.load(origin, origin, 400.0)?;
    let world = scene.collision.context("missing collision world")?;
    for (x, z, expected) in [(-10.0, 0.0, 15.3), (1.5, 0.0, 15.3), (-40.0, 0.0, 19.0)] {
        let ground = world
            .ground_below(Vec3::new(x, 0.0, z), expected + 0.1)
            .with_context(|| format!("missing imported floor at {x},{z}, expected {expected}"))?;
        ensure!(
            (ground - expected).abs() < 0.01,
            "imported floor at {x},{z}: got {ground}, expected {expected}"
        );
        ensure!(
            world
                .standing_at(Vec3::new(x, 0.0, z), expected + 0.1)
                .is_some(),
            "no standing clearance at {x},{z}; nearest overhead {:?}",
            world.ceiling_above(Vec3::new(x, ground, z), ground + 0.5)
        );
    }
    let mut car = Car::new(Vec3::new(-40.0, 20.0, 0.0), 0.6);
    for _ in 0..240 {
        car.step(&world, 0.0, 0.0, false, 1.0, 1.0 / 120.0);
    }
    ensure!(
        (car.position.y - (19.0 + 0.6 * 1.04_f32.sqrt())).abs() < 0.08,
        "car did not settle on YBN ramp: {:?}",
        car.position
    );
    let rotation = sa_scene::vehicle::model_rotation(car.yaw, car.pitch, car.roll);
    for x in [-0.72, 0.72] {
        for z in [-1.35, 1.35] {
            let wheel = rotation * Vec3::new(x, -car.clearance, z) + car.position;
            let road = world
                .ground_below(wheel, wheel.y + 0.5)
                .context("no ramp support")?;
            ensure!(
                (wheel.y - road).abs() < 0.05,
                "rendered wheel misses imported ramp"
            );
        }
    }
    println!("Imported COL audit passed: model placement, rotated box, world ramp, standing clearance and four vehicle contacts");
    Ok(())
}
