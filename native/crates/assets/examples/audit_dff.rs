//! Read-only format coverage audit of a local IMG; prints metadata only.
use anyhow::{Context, Result};
use sa_assets::{decode_dff, Img};
use std::collections::BTreeMap;

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .context("usage: audit_dff <gta3.img> [model.dff ...]")?;
    let requested: Vec<String> = args.collect();
    let mut img = Img::open(std::path::Path::new(&path))?;
    let mut names: Vec<_> = if requested.is_empty() {
        img.names()
            .filter(|n| n.ends_with(".dff"))
            .cloned()
            .collect()
    } else {
        requested
    };
    names.sort();
    let mut success = 0;
    let mut triangles = 0;
    let mut failures = BTreeMap::<String, usize>::new();
    for name in &names {
        match img.read(name).and_then(|bytes| decode_dff(&bytes)) {
            Ok(meshes) => {
                success += 1;
                let count: usize = meshes.iter().map(|g| g.triangles.len()).sum();
                triangles += count;
                if names.len() <= 20 {
                    println!("{name}: {} geometries, {count} triangles", meshes.len());
                }
            }
            Err(error) => {
                if names.len() <= 20 {
                    println!("{name}: {error:#}");
                }
                *failures.entry(format!("{error:#}")).or_default() += 1;
            }
        }
    }
    println!(
        "Decoded {success}/{} DFF models, {triangles} triangles",
        names.len()
    );
    for (error, count) in failures {
        println!("{count} rejected: {error}");
    }
    Ok(())
}
