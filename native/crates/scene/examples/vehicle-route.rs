//! Read-only diagnostic: locate original road materials near Grove Street.
use anyhow::{Context, Result};
use glam::Vec3;
fn main() -> Result<()> {
    let game = std::path::PathBuf::from(
        std::env::args()
            .nth(1)
            .context("Pass original game directory")?,
    );
    let scene = sa_scene::WorldLoader::open(&game)?.load([2500., -1670.], [2500., -1670.], 400.)?;
    for x in [-30., 0., 30.] {
        for z in [-20., 0., 20., 40., 60., 80., 100.] {
            let p = Vec3::new(x, 0., z);
            let mut hits = Vec::new();
            for batch in &scene.batches {
                if !batch.key.to_ascii_lowercase().contains("road") {
                    continue;
                }
                for triangle in batch.vertices.as_chunks::<3>().0 {
                    let a = Vec3::from_array(triangle[0].position);
                    let b = Vec3::from_array(triangle[1].position);
                    let c = Vec3::from_array(triangle[2].position);
                    let ab = b - a;
                    let ac = c - a;
                    let ap = p - a;
                    let det = ab.x * ac.z - ab.z * ac.x;
                    if det.abs() < 0.01 {
                        continue;
                    }
                    let u = (ap.x * ac.z - ap.z * ac.x) / det;
                    let v = (ab.x * ap.z - ab.z * ap.x) / det;
                    let y = a.y + u * ab.y + v * ac.y;
                    if u >= 0. && v >= 0. && u + v <= 1. && y < 30. {
                        hits.push((y, batch.key.as_str()));
                    }
                }
            }
            hits.sort_by(|a, b| a.0.total_cmp(&b.0));
            if let Some((height, key)) = hits.last() {
                println!("Road sample ({x},{z}) y={height:.2}: {key}");
            }
        }
    }
    Ok(())
}
