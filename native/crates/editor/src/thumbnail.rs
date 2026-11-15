//! Asset thumbnails for the Asset Browser, drawn on the CPU so they need no
//! GPU, no extra files and work for any glTF a creator drops in.
//!
//! The model is seen from above and to the side (south-east, like a map
//! icon), lit by one fixed light, and fitted to the square.
use glam::{Mat4, Vec2, Vec3};
use sa_scene::gltf_model::Model;

/// Rendered at this many samples per pixel side, then averaged, for smooth
/// edges.
const SUPERSAMPLE: usize = 2;

/// An RGBA image, row by row from the top.
#[derive(Clone, Debug, PartialEq)]
pub struct Thumbnail {
    pub size: usize,
    pub rgba: Vec<u8>,
}

/// Draw `model` into a `size` x `size` thumbnail with a transparent
/// background. A model with nothing to draw gives a fully transparent image.
pub fn render(model: &Model, size: usize) -> Thumbnail {
    let size = size.max(1);
    let big = size * SUPERSAMPLE;
    // Camera looking at the model from the south-east and above (glTF:
    // +X east, -Z north, +Y up).
    let direction = Vec3::new(1.0, 0.9, 1.2).normalize();
    let view = Mat4::look_at_rh(direction, Vec3::ZERO, Vec3::Y);
    let light = Vec3::new(0.4, 1.0, 0.7).normalize();

    // Model triangles in view space, with what each corner needs to shade.
    struct Corner {
        p: Vec3,
        uv: Vec2,
        color: [f32; 4],
    }
    let mut triangles: Vec<([Corner; 3], Option<Image>, f32)> = Vec::new();
    for batch in &model.drawn {
        let texture = model.textures.get(&batch.key).map(|t| Image {
            width: t.width as usize,
            height: t.height as usize,
            rgba: &t.rgba,
        });
        for t in batch.vertices.as_chunks::<3>().0 {
            let world = t.each_ref().map(|v| Vec3::from_array(v.position));
            let normal = (world[1] - world[0]).cross(world[2] - world[0]);
            if normal.length_squared() <= f32::EPSILON {
                continue;
            }
            // Two-sided light: thin props (signs, leaves) look the same from
            // either side.
            let shade = 0.45 + 0.55 * normal.normalize().dot(light).abs();
            let corners = [0, 1, 2].map(|i| Corner {
                p: view.transform_point3(world[i]),
                uv: Vec2::from_array(t[i].uv),
                color: t[i].color,
            });
            triangles.push((corners, texture, shade));
        }
    }

    let mut out = Thumbnail {
        size,
        rgba: vec![0; size * size * 4],
    };
    let points = || triangles.iter().flat_map(|(c, _, _)| c.iter().map(|c| c.p));
    let Some((min, max)) = crate::viewport::bounds(points()) else {
        return out;
    };
    // Fit the projected model in the square, with a small margin.
    let extent = (max - min).truncate().max_element().max(1e-4);
    let scale = big as f32 * 0.88 / extent;
    let centre = (min + max).truncate() * 0.5;
    let to_pixel = |p: Vec3| {
        Vec2::new(
            (p.x - centre.x) * scale + big as f32 * 0.5,
            (centre.y - p.y) * scale + big as f32 * 0.5,
        )
    };

    // Colour and depth (view-space z: larger is nearer).
    let mut color = vec![[0f32; 4]; big * big];
    let mut depth = vec![f32::NEG_INFINITY; big * big];
    for (corners, texture, shade) in &triangles {
        let [a, b, c] = corners.each_ref().map(|c| to_pixel(c.p));
        let area = edge(a, b, c);
        if area.abs() < 1e-6 {
            continue;
        }
        let lo = a.min(b).min(c).floor().max(Vec2::ZERO);
        let hi = a.max(b).max(c).ceil().min(Vec2::splat(big as f32 - 1.0));
        if lo.x > hi.x || lo.y > hi.y {
            continue;
        }
        for y in lo.y as usize..=hi.y as usize {
            for x in lo.x as usize..=hi.x as usize {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let w = [
                    edge(b, c, p) / area,
                    edge(c, a, p) / area,
                    edge(a, b, p) / area,
                ];
                if w.iter().any(|w| *w < 0.0) {
                    continue;
                }
                let z = w[0] * corners[0].p.z + w[1] * corners[1].p.z + w[2] * corners[2].p.z;
                let index = y * big + x;
                if z <= depth[index] {
                    continue;
                }
                let mut rgba = [0f32; 4];
                for (i, corner) in corners.iter().enumerate() {
                    for (channel, value) in rgba.iter_mut().zip(corner.color) {
                        *channel += w[i] * value;
                    }
                }
                if let Some(texture) = texture {
                    let uv = corners[0].uv * w[0] + corners[1].uv * w[1] + corners[2].uv * w[2];
                    let texel = sample(texture, uv);
                    for (channel, value) in rgba.iter_mut().zip(texel) {
                        *channel *= value;
                    }
                }
                // Cut-out transparency (leaves, fences): skip the clear parts.
                if rgba[3] < 0.5 {
                    continue;
                }
                depth[index] = z;
                color[index] = [rgba[0] * shade, rgba[1] * shade, rgba[2] * shade, 1.0];
            }
        }
    }

    // Average each block of samples into one pixel.
    let samples = (SUPERSAMPLE * SUPERSAMPLE) as f32;
    for y in 0..size {
        for x in 0..size {
            let mut sum = [0f32; 4];
            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    let c = color[(y * SUPERSAMPLE + sy) * big + x * SUPERSAMPLE + sx];
                    // Premultiplied, so empty samples do not darken edges.
                    for (s, v) in sum
                        .iter_mut()
                        .zip([c[0] * c[3], c[1] * c[3], c[2] * c[3], c[3]])
                    {
                        *s += v;
                    }
                }
            }
            let alpha = sum[3] / samples;
            let pixel = &mut out.rgba[(y * size + x) * 4..][..4];
            for i in 0..3 {
                let straight = if sum[3] > 0.0 { sum[i] / sum[3] } else { 0.0 };
                pixel[i] = to_byte(straight);
            }
            pixel[3] = to_byte(alpha);
        }
    }
    out
}

/// A texture borrowed from the model.
#[derive(Clone, Copy)]
struct Image<'a> {
    width: usize,
    height: usize,
    rgba: &'a [u8],
}

/// Twice the signed area of the triangle (a, b, p).
fn edge(a: Vec2, b: Vec2, p: Vec2) -> f32 {
    (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x)
}

/// Nearest texel, with the UV wrapped like a repeating texture.
fn sample(texture: &Image, uv: Vec2) -> [f32; 4] {
    let (w, h) = (texture.width, texture.height);
    if w == 0 || h == 0 || texture.rgba.len() < w * h * 4 {
        return [1.0; 4];
    }
    let x = ((uv.x.rem_euclid(1.0) * w as f32) as usize).min(w - 1);
    let y = ((uv.y.rem_euclid(1.0) * h as f32) as usize).min(h - 1);
    let texel = &texture.rgba[(y * w + x) * 4..][..4];
    [texel[0], texel[1], texel[2], texel[3]].map(|v| v as f32 / 255.0)
}

fn to_byte(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}
