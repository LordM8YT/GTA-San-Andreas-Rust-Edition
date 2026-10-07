//! Offline original-texture preview using a locally installed Real-ESRGAN engine.
//! Generated game assets stay local and are not part of the source distribution.
use anyhow::{ensure, Context, Result};
use sa_assets::Texture;
use sa_scene::load_world_at;
use std::{collections::HashMap, fs, path::Path, process::Command};

fn external_path(path: &Path) -> String {
    // NCNN's directory image reader cannot open mixed-separator verbatim paths.
    let text = path.to_string_lossy();
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{unc}");
    }
    text.strip_prefix(r"\\?\").unwrap_or(&text).to_owned()
}

fn write_png(path: &Path, image: &Texture) -> Result<()> {
    let mut encoder = png::Encoder::new(fs::File::create(path)?, image.width, image.height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&image.rgba)?;
    Ok(())
}
fn read_png(path: &Path) -> Result<Texture> {
    let mut decoder = png::Decoder::new(std::io::BufReader::new(fs::File::open(path)?));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let size = reader.output_buffer_size().context("PNG buffer size")?;
    ensure!(size <= 128 * 1024 * 1024, "upscale PNG budget");
    let mut bytes = vec![0; size];
    let info = reader.next_frame(&mut bytes)?;
    let channels = match info.color_type {
        png::ColorType::Rgb => 3,
        png::ColorType::Rgba => 4,
        _ => anyhow::bail!("unsupported engine output"),
    };
    let mut rgba = Vec::with_capacity(info.width as usize * info.height as usize * 4);
    for pixel in bytes[..info.buffer_size()].chunks(channels) {
        rgba.extend(&pixel[..3]);
        rgba.push(255);
    }
    Ok(Texture {
        width: info.width,
        height: info.height,
        rgba,
        has_alpha: false,
    })
}
// Periodic context lets the model see the opposite tile edge, rather than adding
// a different border to each side of a repeating road or wall texture.
fn pad(image: &Texture, margin: u32) -> Texture {
    let (w, h) = (image.width + margin * 2, image.height + margin * 2);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let sx = (i64::from(x) - i64::from(margin)).rem_euclid(i64::from(image.width)) as u32;
            let sy = (i64::from(y) - i64::from(margin)).rem_euclid(i64::from(image.height)) as u32;
            let p = ((sy * image.width + sx) * 4) as usize;
            rgba.extend(&image.rgba[p..p + 4]);
        }
    }
    Texture {
        width: w,
        height: h,
        rgba,
        has_alpha: false,
    }
}
fn crop_scale(
    image: &Texture,
    width: u32,
    height: u32,
    scale: u32,
    margin: u32,
) -> Result<Texture> {
    ensure!(
        image.width == (width + margin * 2) * 4 && image.height == (height + margin * 2) * 4,
        "engine produced wrong dimensions"
    );
    let factor = 4 / scale;
    let mut rgba = Vec::with_capacity((width * height * scale * scale * 4) as usize);
    for y in 0..height * scale {
        for x in 0..width * scale {
            let mut rgb = [0_u32; 3];
            for dy in 0..factor {
                for dx in 0..factor {
                    let p = (((margin * 4 + y * factor + dy) * image.width
                        + margin * 4
                        + x * factor
                        + dx)
                        * 4) as usize;
                    for (c, sum) in rgb.iter_mut().enumerate() {
                        *sum += u32::from(image.rgba[p + c]);
                    }
                }
            }
            rgba.extend(rgb.map(|sum| ((sum + factor * factor / 2) / (factor * factor)) as u8));
            rgba.push(255);
        }
    }
    Ok(Texture {
        width: width * scale,
        height: height * scale,
        rgba,
        has_alpha: false,
    })
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().collect();
    ensure!(
        args.len() >= 4,
        "Usage: upscale-textures GAME OUTPUT ENGINE [COUNT=32] [SCALE=2] [X=2500] [Y=-1670]"
    );
    let game = Path::new(&args[1]);
    let output = Path::new(&args[2]);
    let engine = Path::new(&args[3]).canonicalize()?;
    let count: usize = args.get(4).map_or(Ok(32), |n| n.parse())?;
    let scale: u32 = args.get(5).map_or(Ok(2), |n| n.parse())?;
    let center = [
        args.get(6).map_or(Ok(2500.0_f32), |n| n.parse())?,
        args.get(7).map_or(Ok(-1670.0_f32), |n| n.parse())?,
    ];
    ensure!(
        (1..=128).contains(&count) && matches!(scale, 2 | 4),
        "use 1–128 textures and 2x or 4x"
    );
    ensure!(
        !output.join("mod.json").exists(),
        "output resource already exists; choose another folder to preserve your previous preview"
    );
    let scene = load_world_at(game, center, 400.0)?;
    let mut scores = HashMap::<String, f32>::new();
    for batch in &scene.batches {
        let key = &batch.key;
        if key.starts_with("runtime:") || key.starts_with("lod") || batch.uv_animation.is_some() {
            continue;
        }
        let texture = &scene.textures[key];
        if texture.has_alpha
            || !(64..=512).contains(&texture.width)
            || !(64..=512).contains(&texture.height)
        {
            continue;
        }
        let name = key.split_once(':').unwrap().1;
        if ![
            "road", "tar", "pave", "grass", "brick", "wall", "concrete", "stone", "ground", "rock",
            "asphalt",
        ]
        .iter()
        .any(|word| name.contains(word))
        {
            continue;
        }
        if ["font", "sign", "neon", "scroll", "logo"]
            .iter()
            .any(|word| name.contains(word))
        {
            continue;
        }
        for triangle in batch.vertices.as_chunks::<3>().0.iter() {
            let [a, b, c] = triangle.map(|v| glam::Vec3::from_array(v.position));
            let p = (a + b + c) / 3.0;
            if p.x * p.x + p.z * p.z > 300.0_f32.powi(2) {
                continue;
            }
            *scores.entry(key.clone()).or_default() +=
                (b - a).cross(c - a).length() / (1.0 + (p.x * p.x + p.z * p.z) / 10000.0);
        }
    }
    let mut selected: Vec<_> = scores.into_iter().collect();
    selected.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    selected.truncate(count);
    ensure!(!selected.is_empty(), "no eligible textures in this region");
    let bytes: usize = selected
        .iter()
        .map(|(key, _)| scene.textures[key].rgba.len() * scale as usize * scale as usize)
        .sum();
    ensure!(
        bytes <= 192 * 1024 * 1024,
        "preview exceeds 192 MiB; lower texture count or scale"
    );
    for sub in ["source", "work/input", "work/output", "textures"] {
        fs::create_dir_all(output.join(sub))?;
    }
    let output = output.canonicalize()?;
    let mut overrides = HashMap::new();
    for (i, (key, _)) in selected.iter().enumerate() {
        let file = format!("{i:03}.png");
        let image = &scene.textures[key];
        write_png(&output.join("source").join(&file), image)?;
        write_png(&output.join("work/input").join(&file), &pad(image, 16))?;
        overrides.insert(key.clone(), format!("textures/{file}"));
        println!(
            "{key}: {}x{} -> {}x{}",
            image.width,
            image.height,
            image.width * scale,
            image.height * scale
        );
    }
    // No shell interpolation and no inference in the game's streaming/render thread.
    let status = Command::new(&engine)
        .current_dir(engine.parent().unwrap())
        .args([
            "-n",
            "realesrgan-x4plus",
            "-s",
            "4",
            "-t",
            "128",
            "-j",
            "1:1:1",
            "-f",
            "png",
            "-i",
        ])
        .arg(external_path(&output.join("work/input")))
        .arg("-o")
        .arg(external_path(&output.join("work/output")))
        .status()?;
    ensure!(
        status.success(),
        "AI engine failed; originals remain unchanged, preview stays inactive"
    );
    for (i, (key, _)) in selected.iter().enumerate() {
        let file = format!("{i:03}.png");
        let source = &scene.textures[key];
        let image = read_png(&output.join("work/output").join(&file))
            .with_context(|| format!("AI engine output absent or invalid: {file}"))?;
        write_png(
            &output.join("textures").join(file),
            &crop_scale(&image, source.width, source.height, scale, 16)?,
        )?;
    }
    let manifest = serde_json::json!({"enabled":true,"name":format!("AI textures {scale}x preview"),"texture_overrides":overrides});
    fs::write(
        output.join("mod.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    println!("Activated {} original texture overrides; {:.1} MiB decoded / {:.1} MiB extra VRAM before sharing", selected.len(),bytes as f64/1048576.0,bytes as f64*(1.0-1.0/f64::from(scale*scale))/1048576.0);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrap_context_and_crop_keep_tile_content_and_dimensions() {
        let image = Texture {
            width: 2,
            height: 1,
            rgba: vec![0, 0, 0, 255, 200, 100, 50, 255],
            has_alpha: false,
        };
        let padded = pad(&image, 1);
        assert_eq!(&padded.rgba[..8], &[200, 100, 50, 255, 0, 0, 0, 255]);
        let mut enlarged = Texture {
            width: 16,
            height: 12,
            rgba: Vec::new(),
            has_alpha: false,
        };
        for y in 0..12 {
            for x in 0..16 {
                let p = ((y / 4 * 4 + x / 4) * 4) as usize;
                enlarged.rgba.extend(&padded.rgba[p..p + 4]);
            }
        }
        let cropped = crop_scale(&enlarged, 2, 1, 2, 1).unwrap();
        assert_eq!((cropped.width, cropped.height), (4, 2));
        assert_eq!(
            &cropped.rgba[..16],
            &[0, 0, 0, 255, 0, 0, 0, 255, 200, 100, 50, 255, 200, 100, 50, 255]
        );
    }
}
