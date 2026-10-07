//! Read-only skeleton audit; no original files are exported.
use anyhow::{Context, Result};
use sa_assets::{skin, Img};
fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .context("usage: audit_skin <gta3.img> <ped.dff>...")?;
    let mut img = Img::open(std::path::Path::new(&path))?;
    let mut checked = 0;
    let mut absent = 0;
    for name in args {
        if !img.has(&name) {
            eprintln!("Absent archive model: {name}");
            absent += 1;
            continue;
        }
        let bytes = img.read(&name).with_context(|| name.clone())?;
        let rigid = sa_assets::decode_dff(&bytes)?;
        let model = skin::decode(&bytes).with_context(|| name.clone())?;
        let posed = model.pose(&model.bind_pose())?;
        anyhow::ensure!(rigid.len() == posed.len(), "atomic count mismatch");
        let error = rigid
            .iter()
            .zip(&posed)
            .flat_map(|(a, b)| a.positions.iter().zip(&b.positions))
            .map(|(a, b)| {
                a.iter()
                    .zip(b)
                    .map(|(x, y)| (x - y) * (x - y))
                    .sum::<f32>()
                    .sqrt()
            })
            .fold(0.0f32, f32::max);
        println!(
            "{name}: {} frames, {} bones, {} parts, max bind-pose error {error:.6}",
            model.frames.len(),
            model.bone_frames.len(),
            model.parts.len()
        );
        anyhow::ensure!(error < 0.01, "bind pose differs from original vertices");
        checked += 1;
    }
    println!("Verified {checked} skinned models; {absent} requested names absent from archive");
    Ok(())
}
