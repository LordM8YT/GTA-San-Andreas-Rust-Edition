//! Optional, local-only PNG artwork. No reference images are bundled or downloaded.
use eframe::egui::{self, Color32};
use std::{fs::File, io::BufReader, path::Path};

pub fn load(path: &Path) -> anyhow::Result<egui::ColorImage> {
    anyhow::ensure!(
        path.metadata()?.len() <= 24 * 1024 * 1024,
        "Choose a PNG smaller than 24 MiB"
    );
    let mut decoder = png::Decoder::new(BufReader::new(File::open(path)?));
    decoder.set_limits(png::Limits {
        bytes: 64 * 1024 * 1024,
    });
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info()?;
    let info = reader.info();
    anyhow::ensure!(
        info.width > 0
            && info.height > 0
            && u64::from(info.width) * u64::from(info.height) <= 16_000_000,
        "Choose an image up to 16 megapixels"
    );
    let mut bytes = vec![
        0;
        reader
            .output_buffer_size()
            .ok_or_else(|| anyhow::anyhow!("Image is too large"))?
    ];
    let frame = reader.next_frame(&mut bytes)?;
    let channels = frame.color_type.samples();
    let pixels = bytes[..frame.buffer_size()]
        .chunks_exact(channels)
        .map(|p| match frame.color_type {
            png::ColorType::Rgb => Color32::from_rgb(p[0], p[1], p[2]),
            png::ColorType::Rgba => Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]),
            png::ColorType::Grayscale => Color32::from_gray(p[0]),
            png::ColorType::GrayscaleAlpha => {
                Color32::from_rgba_unmultiplied(p[0], p[0], p[0], p[1])
            }
            _ => Color32::TRANSPARENT,
        })
        .collect();
    Ok(egui::ColorImage::new(
        [frame.width as usize, frame.height as usize],
        pixels,
    ))
}

pub fn background(ui: &egui::Ui, rect: egui::Rect, image: Option<&egui::TextureHandle>) {
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 10, Color32::from_rgb(28, 65, 48));
    if let Some(image) = image {
        let aspect = image.size_vec2().x / image.size_vec2().y;
        let target = rect.width() / rect.height();
        let uv = if aspect > target {
            let half = target / aspect * 0.5;
            egui::Rect::from_min_max(egui::pos2(0.5 - half, 0.), egui::pos2(0.5 + half, 1.))
        } else {
            let half = aspect / target * 0.5;
            egui::Rect::from_min_max(egui::pos2(0., 0.5 - half), egui::pos2(1., 0.5 + half))
        };
        painter.add(egui::Shape::mesh(rounded_mesh(
            rect,
            image.id(),
            uv,
            |_| Color32::WHITE,
        )));
    } else {
        // Original abstract hills: quiet depth when the player has not chosen artwork.
        for (offset, color) in [
            (0., Color32::from_rgb(38, 77, 55)),
            (70., Color32::from_rgb(26, 54, 41)),
        ] {
            let points = vec![
                rect.left_bottom(),
                egui::pos2(rect.left(), rect.bottom() - 35. - offset),
                egui::pos2(
                    rect.left() + rect.width() * 0.50,
                    rect.top() + 100. + offset * 0.3,
                ),
                egui::pos2(rect.right(), rect.top() + 55. + offset),
                rect.right_bottom(),
            ];
            painter
                .with_clip_rect(rect.shrink(10.))
                .add(egui::Shape::convex_polygon(
                    points,
                    color,
                    egui::Stroke::NONE,
                ));
        }
    }
    painter.add(egui::Shape::mesh(rounded_mesh(
        rect,
        egui::TextureId::default(),
        egui::Rect::from_min_max(egui::epaint::WHITE_UV, egui::epaint::WHITE_UV),
        |t| Color32::from_rgba_unmultiplied(7, 23, 17, (240. - 150. * t) as u8),
    )));
}

fn rounded_mesh(
    rect: egui::Rect,
    texture_id: egui::TextureId,
    uv: egui::Rect,
    color: impl Fn(f32) -> Color32,
) -> egui::Mesh {
    let mut mesh = egui::Mesh::with_texture(texture_id);
    let mut vertex = |pos: egui::Pos2| {
        let t = (pos.x - rect.left()) / rect.width();
        let y = (pos.y - rect.top()) / rect.height();
        mesh.vertices.push(egui::epaint::Vertex {
            pos,
            uv: egui::pos2(egui::lerp(uv.x_range(), t), egui::lerp(uv.y_range(), y)),
            color: color(t),
        });
    };
    vertex(rect.center());
    for (center, start) in [
        (rect.right_top() + egui::vec2(-10., 10.), -90.),
        (rect.right_bottom() + egui::vec2(-10., -10.), 0.),
        (rect.left_bottom() + egui::vec2(10., -10.), 90.),
        (rect.left_top() + egui::vec2(10., 10.), 180.),
    ] {
        for step in 0..=8 {
            let a = (start + step as f32 * 90. / 8.).to_radians();
            vertex(center + egui::vec2(a.cos(), a.sin()) * 10.);
        }
    }
    let count = mesh.vertices.len() as u32;
    for i in 1..count {
        mesh.add_triangle(0, i, if i + 1 == count { 1 } else { i + 1 });
    }
    mesh
}

#[cfg(test)]
mod tests {
    #[test]
    fn local_png_preserves_alpha_and_rejects_invalid_or_oversized_inputs() {
        let path = std::env::temp_dir().join(format!(
            "sare-artwork-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        {
            let file = std::fs::File::create(&path).unwrap();
            let mut encoder = png::Encoder::new(file, 2, 1);
            encoder.set_color(png::ColorType::GrayscaleAlpha);
            encoder.set_depth(png::BitDepth::Eight);
            encoder
                .write_header()
                .unwrap()
                .write_image_data(&[120, 255, 240, 128])
                .unwrap();
        }
        let image = super::load(&path).unwrap();
        assert_eq!(image.size, [2, 1]);
        assert_eq!(image.pixels[0], eframe::egui::Color32::from_gray(120));
        assert_eq!(image.pixels[1].a(), 128);
        std::fs::write(&path, b"not an image").unwrap();
        assert!(super::load(&path).is_err());
        std::fs::File::create(&path)
            .unwrap()
            .set_len(24 * 1024 * 1024 + 1)
            .unwrap();
        assert!(super::load(&path)
            .unwrap_err()
            .to_string()
            .contains("24 MiB"));
        std::fs::remove_file(&path).unwrap();
    }
}
