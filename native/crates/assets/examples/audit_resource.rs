//! Validate converter output with the same decoders used by the game.
use anyhow::{ensure, Context, Result};
use std::{env, fs};
fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    let path = args
        .first()
        .context("usage: audit_resource model.dff [textures.txd] [--skin]")?;
    let bytes = fs::read(path)?;
    if args.iter().any(|arg| arg == "--col") {
        let models = sa_assets::col::decode_col(&bytes)?;
        ensure!(!models.is_empty(), "no collision models");
        for model in models {
            println!(
                "{{\"min\":{:?},\"max\":{:?},\"triangles\":{}}}",
                model.min,
                model.max,
                model.triangles.len()
            );
            if args.iter().any(|arg| arg == "--points") {
                println!("{:?}", model.triangles);
            }
        }
        return Ok(());
    }
    let geometries = if args.iter().any(|a| a == "--skin") {
        let model = sa_assets::skin::decode(&bytes)?;
        model.pose(&model.bind_pose())?
    } else {
        sa_assets::decode_vehicle_dff(&bytes)?
    };
    let dictionary = args
        .get(1)
        .filter(|a| !a.starts_with("--"))
        .map(fs::read)
        .transpose()?;
    for geometry in &geometries {
        ensure!(
            geometry.positions.iter().flatten().all(|v| v.is_finite()),
            "invalid posed vertex"
        );
        for material in &geometry.materials {
            if let Some(name) = &material.texture {
                sa_assets::decode_txd(dictionary.as_deref().context("missing TXD")?, name)?;
            }
        }
    }
    println!("Native decoder accepted {} geometries", geometries.len());
    Ok(())
}
