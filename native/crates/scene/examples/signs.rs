//! Read-only original asset audit. No game textures or text are redistributed.
use anyhow::{Context, Result};
use sa_assets::{decode_dff, decode_txd, Img};
use std::{fs, path::PathBuf};
fn main() -> Result<()> {
    let game = PathBuf::from(
        std::env::args()
            .nth(1)
            .context("Pass the SA installation directory")?,
    );
    let mut archive = Img::open(&sa_assets::game_path::resolve(&game, "models/gta3.img")?)?;
    let names: Vec<_> = archive
        .names()
        .filter(|n| n.ends_with(".dff"))
        .cloned()
        .collect();
    let mut signs = 0;
    let mut models = 0;
    let mut unsupported = 0;
    for name in names {
        match decode_dff(&archive.read(&name)?) {
            Ok(geometry) => {
                let count = geometry.iter().map(|g| g.road_signs.len()).sum::<usize>();
                signs += count;
                models += usize::from(count > 0);
            }
            Err(e) => {
                unsupported += 1;
                eprintln!("Unsupported original model {name}: {e:#}");
                if e.to_string().contains("roadsign") || e.to_string().contains("2DFX") {
                    anyhow::bail!("{name}: {e:#}");
                }
            }
        }
    }
    println!(
        "Decoded {signs} sign effects in {models} models; {unsupported} other unsupported DFFs"
    );
    anyhow::ensure!(signs > 400, "Original signs were lost during decoding");
    let atlas = decode_txd(
        &fs::read(sa_assets::game_path::resolve(&game, "models/particle.txd")?)?,
        "roadsignfont",
    )?;
    println!(
        "Font {}x{}, alpha {}, blended {}",
        atlas.width,
        atlas.height,
        atlas.has_alpha,
        atlas.needs_blending()
    );
    let output = PathBuf::from("target/sign-font.png");
    let mut encoder = png::Encoder::new(fs::File::create(output)?, atlas.width, atlas.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&atlas.rgba)?;
    let scene = sa_scene::load_world(&game, 400.0)?;
    let batch = scene
        .batches
        .iter()
        .find(|b| b.key == "runtime:roadsignfont")
        .context("No Grove sign text")?;
    println!(
        "Grove region: {} glyphs, atlas present {}",
        batch.vertices.len() / 6,
        scene.textures.contains_key(&batch.key)
    );
    anyhow::ensure!(unsupported == 0, "Original model decoding regressed");
    let vegas = sa_scene::load_world_at(&game, [2000.0, 2300.0], 400.0)?;
    let animated: Vec<_> = vegas
        .batches
        .iter()
        .filter(|b| b.uv_animation.is_some())
        .collect();
    println!("Las Venturas: {} animated material batches", animated.len());
    for batch in animated {
        let track = batch.uv_animation.as_ref().unwrap();
        anyhow::ensure!(
            vegas.textures.contains_key(&batch.key),
            "Animated texture missing"
        );
        let a = track.matrix(0.0);
        let b = track.matrix(0.3);
        anyhow::ensure!(a != b, "Original animation did not advance");
        println!(
            "{}: {} vertices, UV at .3s {:?}",
            batch.key,
            batch.vertices.len(),
            b
        );
    }
    Ok(())
}
