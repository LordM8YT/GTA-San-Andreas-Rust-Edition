//! Validate converter output with the same decoders used by the game.
use anyhow::{ensure, Context, Result};
use std::{collections::BTreeSet, env, fs};

fn main() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    let path = args
        .first()
        .context("usage: audit_resource model.dff [textures.txd] [--map|--skin|--col] [--materials]")?;
    let bytes = fs::read(path)?;
    let has = |flag: &str| args.iter().any(|arg| arg == flag);

    if has("--col") {
        let models = sa_assets::col::decode_col(&bytes)?;
        ensure!(!models.is_empty(), "no collision models");
        for model in models {
            println!(
                "{{\"name\":{:?},\"min\":{:?},\"max\":{:?},\"triangles\":{}}}",
                model.name,
                model.min,
                model.max,
                model.triangles.len()
            );
            if has("--points") {
                println!("{:?}", model.triangles);
            }
        }
        return Ok(());
    }

    let dictionary = args
        .get(1)
        .filter(|arg| !arg.starts_with("--"))
        .map(fs::read)
        .transpose()?;

    let mut geometry_count = 0usize;
    let mut triangle_count = 0usize;
    let mut texture_names = BTreeSet::new();

    if has("--map") {
        let pieces = sa_assets::decode_map_dff_pieces(&bytes)?;
        ensure!(!pieces.is_empty(), "map DFF contains no renderable atomics");
        for (index, piece) in pieces.iter().enumerate() {
            let geometry = &piece.geometry;
            ensure!(
                geometry.positions.iter().flatten().all(|value| value.is_finite()),
                "invalid vertex in atomic {} ({})",
                index,
                piece.frame_name
            );
            triangle_count += geometry.triangles.len();
            for material in &geometry.materials {
                if let Some(name) = &material.texture {
                    texture_names.insert(name.clone());
                    if has("--materials") {
                        match dictionary.as_deref() {
                            Some(bytes) => {
                                sa_assets::decode_txd(bytes, name)
                                    .with_context(|| format!("atomic {} ({}) texture {}", index, piece.frame_name, name))?;
                                println!("{{\"atomic\":{},\"frame\":{:?},\"texture\":{:?},\"status\":\"ok\"}}",
                                    index, piece.frame_name, name);
                            }
                            None => anyhow::bail!("--materials requires a TXD argument"),
                        }
                    }
                }
            }
            println!(
                "{{\"atomic\":{},\"frame\":{:?},\"triangles\":{},\"pivot\":{:?}}}",
                index,
                piece.frame_name,
                geometry.triangles.len(),
                piece.pivot
            );
            geometry_count += 1;
        }
    } else {
        let geometries = if has("--skin") {
            let model = sa_assets::skin::decode(&bytes)?;
            model.pose(&model.bind_pose())?
        } else {
            sa_assets::decode_vehicle_dff(&bytes)?
        };
        for geometry in &geometries {
            ensure!(
                geometry.positions.iter().flatten().all(|value| value.is_finite()),
                "invalid posed vertex"
            );
            triangle_count += geometry.triangles.len();
            for material in &geometry.materials {
                if let Some(name) = &material.texture {
                    texture_names.insert(name.clone());
                    if has("--materials") {
                        sa_assets::decode_txd(
                            dictionary.as_deref().context("--materials requires a TXD argument")?,
                            name,
                        )?;
                    }
                }
            }
            geometry_count += 1;
        }
    }

    println!(
        "Native decoder accepted {} geometries, {} triangles and {} unique texture names",
        geometry_count,
        triangle_count,
        texture_names.len()
    );
    for name in texture_names {
        println!("texture: {name}");
    }
    Ok(())
}
