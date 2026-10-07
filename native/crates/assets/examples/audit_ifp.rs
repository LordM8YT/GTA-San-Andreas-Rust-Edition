//! Read-only clip and pose audit, without exporting game assets.
use anyhow::{Context, Result};
use sa_assets::{ifp, skin, Img};
fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .context("usage: audit_ifp <ped.ifp> <gta3.img> <ped.dff>")?;
    let package = ifp::decode(&std::fs::read(path)?)?;
    let mut archive = Img::open(std::path::Path::new(&args.next().context("missing IMG")?))?;
    let model = skin::decode(&archive.read(&args.next().context("missing ped name")?)?)?;
    let keys: usize = package
        .clips
        .iter()
        .flat_map(|c| &c.tracks)
        .map(|t| t.keys.len())
        .sum();
    for clip in &package.clips {
        for sample in 0..5 {
            let locals = clip.pose(&model, clip.duration * sample as f32 / 5.0, true)?;
            let meshes = model.pose(&locals)?;
            anyhow::ensure!(
                meshes
                    .iter()
                    .flat_map(|g| &g.positions)
                    .flatten()
                    .all(|v| v.is_finite() && v.abs() < 100.0),
                "invalid pose {}",
                clip.name
            );
        }
        if ["walk_player", "idle_stance", "run_player", "sprint_civi"].contains(&clip.name.as_str())
        {
            let bound = clip
                .tracks
                .iter()
                .filter(|t| model.frames.iter().any(|f| f.bone_id == Some(t.bone_id)))
                .count();
            anyhow::ensure!(
                bound == clip.tracks.len(),
                "unbound locomotion track {}",
                clip.name
            );
            println!(
                "{}: {:.3}s, {} tracks",
                clip.name,
                clip.duration,
                clip.tracks.len()
            );
        }
    }
    println!(
        "{}: {} clips, {keys} keys; five finite skinned poses per clip",
        package.name,
        package.clips.len()
    );
    Ok(())
}
