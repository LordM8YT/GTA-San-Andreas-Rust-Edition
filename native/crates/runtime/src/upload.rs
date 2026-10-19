//! Incremental GPU upload: retain the current scene until its replacement is complete.
use crate::GpuBatch;
use anyhow::{Context, Result};
use sa_assets::Texture;
use sa_scene::{Batch, Scene};
use std::{
    collections::{hash_map::DefaultHasher, HashMap},
    hash::{Hash, Hasher},
    time::{Duration, Instant},
};

const PACKET_BYTES: usize = 256 * 1024;
const FRAME_BYTES: usize = 4 * 1024 * 1024;
const FRAME_TIME: Duration = Duration::from_millis(2);

struct ImageUpload {
    key: String,
    image: Texture,
    texture: wgpu::Texture,
    row: u32,
    mip: u32,
}
struct BufferUpload {
    batch: Batch,
    buffer: wgpu::Buffer,
    offset: usize,
    bounds: crate::culling::Bounds,
}
struct TextureSet {
    textures: HashMap<String, Texture>,
    aliases: HashMap<String, String>,
    aliases_by_image: HashMap<String, Vec<String>>,
    deduplicated_bytes: usize,
}
pub(super) struct PreparedScene {
    pub(super) scene: Scene,
    textures: TextureSet,
}
pub struct Upload {
    pub scene: Scene,
    textures: std::collections::hash_map::IntoIter<String, Texture>,
    batches: std::vec::IntoIter<Batch>,
    images: HashMap<String, wgpu::BindGroup>,
    image_aliases: HashMap<String, String>,
    aliases_by_image: HashMap<String, Vec<String>>,
    current_image: Option<ImageUpload>,
    current_buffer: Option<BufferUpload>,
    ready: Vec<GpuBatch>,
    frames: usize,
    peak_ms: f64,
    peak_stage: &'static str,
    bytes: usize,
    reused_images: usize,
    reused_bytes: usize,
}
impl Upload {
    pub fn new(scene: Scene) -> Self {
        Self::from_prepared(prepare_scene(scene))
    }
    fn from_prepared(mut prepared: PreparedScene) -> Self {
        let TextureSet {
            textures,
            aliases,
            aliases_by_image,
            ..
        } = prepared.textures;
        Self {
            textures: textures.into_iter(),
            batches: std::mem::take(&mut prepared.scene.batches).into_iter(),
            scene: prepared.scene,
            images: HashMap::new(),
            image_aliases: aliases,
            aliases_by_image,
            current_image: None,
            current_buffer: None,
            ready: Vec::new(),
            frames: 0,
            peak_ms: 0.0,
            peak_stage: "none",
            bytes: 0,
            reused_images: 0,
            reused_bytes: 0,
        }
    }
    /// Texture keys are identities only within an immutable world loader.
    /// Never call this across a session/mod-resource switch: equal names may
    /// carry different pixels. BindGroup clones retain the actual GPU images.
    pub fn reusing(scene: PreparedScene, previous: &[GpuBatch]) -> Self {
        let mut upload = Self::from_prepared(scene);
        for batch in previous {
            if let Some(representative) = upload.image_aliases.get(&batch.texture_key).cloned() {
                upload
                    .images
                    .entry(representative.clone())
                    .or_insert_with(|| batch.texture.clone());
                if let Some(aliases) = upload.aliases_by_image.get(&representative) {
                    for alias in aliases {
                        upload
                            .images
                            .entry(alias.clone())
                            .or_insert_with(|| batch.texture.clone());
                    }
                }
            }
        }
        upload
    }
    /// Returns true only when every image and vertex buffer has been uploaded.
    /// The time limit is soft: an individual driver call cannot be interrupted.
    pub fn advance(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        mipmaps: &crate::mipmaps::Generator,
    ) -> Result<bool> {
        let started = Instant::now();
        let mut bytes = 0;
        let mut done = false;
        let mut mip_encoder = None;
        for _ in 0..256 {
            if bytes >= FRAME_BYTES || started.elapsed() >= FRAME_TIME {
                break;
            }
            if self.current_image.is_none() {
                if let Some((key, image)) = self.textures.next() {
                    if self.images.contains_key(&key) {
                        self.reused_images += 1;
                        self.reused_bytes += image.rgba.len();
                        // Drop CPU pixels gradually under the same frame budget.
                        continue;
                    }
                    let operation_started = Instant::now();
                    let texture = crate::mipmaps::texture(device, &key, image.width, image.height);
                    Self::record_operation(
                        &mut self.peak_ms,
                        &mut self.peak_stage,
                        "texture allocation",
                        operation_started,
                    );
                    self.current_image = Some(ImageUpload {
                        key,
                        image,
                        texture,
                        row: 0,
                        mip: 1,
                    });
                }
            }
            if let Some(image) = &mut self.current_image {
                if image.row == image.image.height {
                    if image.mip < image.texture.mip_level_count() {
                        let encoder = mip_encoder.get_or_insert_with(|| {
                            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                                label: Some("Streamed texture mip levels"),
                            })
                        });
                        let operation_started = Instant::now();
                        mipmaps.encode_level(device, encoder, &image.texture, image.mip);
                        Self::record_operation(
                            &mut self.peak_ms,
                            &mut self.peak_stage,
                            "mip encoding",
                            operation_started,
                        );
                        image.mip += 1;
                        continue;
                    }
                    let image = self.current_image.take().unwrap();
                    let operation_started = Instant::now();
                    let view = image.texture.create_view(&Default::default());
                    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some(&image.key),
                        layout,
                        entries: &[
                            wgpu::BindGroupEntry {
                                binding: 0,
                                resource: wgpu::BindingResource::TextureView(&view),
                            },
                            wgpu::BindGroupEntry {
                                binding: 1,
                                resource: wgpu::BindingResource::Sampler(sampler),
                            },
                        ],
                    });
                    Self::record_operation(
                        &mut self.peak_ms,
                        &mut self.peak_stage,
                        "texture bind group",
                        operation_started,
                    );
                    self.images.insert(image.key.clone(), group.clone());
                    if let Some(aliases) = self.aliases_by_image.remove(&image.key) {
                        for alias in aliases {
                            self.images.insert(alias, group.clone());
                        }
                    }
                    continue;
                }

                let stride = image.image.width as usize * 4;
                let rows =
                    ((PACKET_BYTES / stride).max(1) as u32).min(image.image.height - image.row);
                let start = image.row as usize * stride;
                let count = rows as usize * stride;
                let operation_started = Instant::now();
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &image.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d {
                            x: 0,
                            y: image.row,
                            z: 0,
                        },
                        aspect: wgpu::TextureAspect::All,
                    },
                    &image.image.rgba[start..start + count],
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(stride as u32),
                        rows_per_image: Some(rows),
                    },
                    wgpu::Extent3d {
                        width: image.image.width,
                        height: rows,
                        depth_or_array_layers: 1,
                    },
                );
                Self::record_operation(
                    &mut self.peak_ms,
                    &mut self.peak_stage,
                    "texture write",
                    operation_started,
                );
                image.row += rows;
                bytes += count;
                continue;
            }
            if self.current_buffer.is_none() {
                if let Some(batch) = self.batches.next() {
                    let size = std::mem::size_of_val(batch.vertices.as_slice()) as u64;
                    let operation_started = Instant::now();
                    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(&batch.key),
                        size: size.max(4),
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
                    Self::record_operation(
                        &mut self.peak_ms,
                        &mut self.peak_stage,
                        "vertex buffer allocation",
                        operation_started,
                    );
                    self.current_buffer = Some(BufferUpload {
                        batch,
                        buffer,
                        offset: 0,
                        bounds: crate::culling::Bounds::default(),
                    });
                } else {
                    done = true;
                    break;
                }
            }
            if let Some(buffer) = &mut self.current_buffer {
                let raw: &[u8] = bytemuck::cast_slice(&buffer.batch.vertices);
                let end = (buffer.offset + PACKET_BYTES).min(raw.len());
                if end > buffer.offset {
                    let operation_started = Instant::now();
                    queue.write_buffer(
                        &buffer.buffer,
                        buffer.offset as u64,
                        &raw[buffer.offset..end],
                    );
                    Self::record_operation(
                        &mut self.peak_ms,
                        &mut self.peak_stage,
                        "vertex buffer write",
                        operation_started,
                    );
                }
                if !buffer.batch.animated {
                    let operation_started = Instant::now();
                    let stride = std::mem::size_of::<sa_scene::Vertex>();
                    let first = buffer.offset / stride;
                    let last = end.div_ceil(stride).min(buffer.batch.vertices.len());
                    buffer.bounds.include(&buffer.batch.vertices[first..last]);
                    Self::record_operation(
                        &mut self.peak_ms,
                        &mut self.peak_stage,
                        "vertex bounds",
                        operation_started,
                    );
                }
                bytes += end - buffer.offset;
                buffer.offset = end;
                if end == raw.len() {
                    let buffer = self.current_buffer.take().unwrap();
                    let batch = buffer.batch;
                    self.ready.push(GpuBatch {
                        bounds: if batch.animated {
                            None
                        } else {
                            buffer.bounds.finish()
                        },
                        texture_key: batch.key.clone(),
                        buffer: buffer.buffer,
                        count: batch.vertices.len() as u32,
                        texture: self
                            .images
                            .remove(&batch.key)
                            .context("batch texture missing")?,
                        alpha: batch.alpha,
                        animated: batch.animated,
                        uv_animation: batch.uv_animation.clone(),
                        base: if batch.animated || batch.uv_animation.is_some() {
                            bytemuck::cast_slice(&batch.vertices).to_vec()
                        } else {
                            Vec::new()
                        },
                        piece: None,
                    });
                }
            }
        }
        if let Some(encoder) = mip_encoder {
            queue.submit([encoder.finish()]);
        }
        self.frames += 1;
        self.peak_ms = self.peak_ms.max(started.elapsed().as_secs_f64() * 1000.0);
        self.bytes += bytes;
        Ok(done)
    }
    fn record_operation(
        peak_ms: &mut f64,
        peak_stage: &mut &'static str,
        stage: &'static str,
        started: Instant,
    ) {
        let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
        if elapsed_ms > *peak_ms {
            *peak_ms = elapsed_ms;
            *peak_stage = stage;
        }
    }
    pub fn finish(mut self) -> (Vec<GpuBatch>, Scene) {
        self.ready.sort_by_key(|batch| batch.alpha);
        eprintln!("Incremental GPU upload: {} batches / {:.1} MiB over {} frames; peak CPU slice {:.2} ms ({}); reused {} textures / {:.1} MiB", self.ready.len(), self.bytes as f64 / 1048576.0, self.frames, self.peak_ms, self.peak_stage, self.reused_images, self.reused_bytes as f64 / 1048576.0);
        (self.ready, self.scene)
    }
}

pub(super) fn prepare_scene(mut scene: Scene) -> PreparedScene {
    let started = Instant::now();
    let textures = deduplicate_textures(std::mem::take(&mut scene.textures));
    eprintln!(
        "Prepared {} unique GPU textures in {:.2} ms; deduplicated {:.1} MiB",
        textures.textures.len(),
        started.elapsed().as_secs_f64() * 1000.0,
        textures.deduplicated_bytes as f64 / 1048576.0
    );
    PreparedScene { scene, textures }
}

fn deduplicate_textures(textures: HashMap<String, Texture>) -> TextureSet {
    let mut unique: HashMap<String, Texture> = HashMap::with_capacity(textures.len());
    let mut fingerprints: HashMap<(u32, u32, bool, bool, u64), Vec<String>> = HashMap::new();
    let mut image_aliases = HashMap::new();
    let mut aliases_by_image: HashMap<String, Vec<String>> = HashMap::new();
    let mut deduplicated_bytes = 0;
    for (key, image) in textures {
        let mut hasher = DefaultHasher::new();
        image.rgba.hash(&mut hasher);
        let signature = (
            image.width,
            image.height,
            image.has_alpha,
            key == "runtime:roadsignfont",
            hasher.finish(),
        );
        let representative = fingerprints.get(&signature).and_then(|keys| {
            keys.iter()
                .find(|candidate| unique[*candidate].rgba == image.rgba)
                .cloned()
        });
        if let Some(representative) = representative {
            deduplicated_bytes += image.rgba.len();
            image_aliases.insert(key.clone(), representative.clone());
            aliases_by_image
                .entry(representative)
                .or_default()
                .push(key);
        } else {
            fingerprints.entry(signature).or_default().push(key.clone());
            image_aliases.insert(key.clone(), key.clone());
            unique.insert(key, image);
        }
    }
    TextureSet {
        textures: unique,
        aliases: image_aliases,
        aliases_by_image,
        deduplicated_bytes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(width: u32, height: u32, has_alpha: bool, rgba: &[u8]) -> Texture {
        Texture {
            width,
            height,
            rgba: rgba.to_vec(),
            has_alpha,
        }
    }

    #[test]
    fn identical_texture_pixels_share_one_upload_and_keep_all_keys() {
        let textures = HashMap::from([
            ("city:wall".into(), image(1, 1, false, &[1, 2, 3, 255])),
            ("suburb:wall".into(), image(1, 1, false, &[1, 2, 3, 255])),
            ("city:road".into(), image(1, 1, false, &[4, 5, 6, 255])),
        ]);
        let deduplicated = deduplicate_textures(textures);
        assert_eq!(deduplicated.textures.len(), 2);
        assert_eq!(deduplicated.aliases.len(), 3);
        assert_eq!(deduplicated.deduplicated_bytes, 4);
        let (alias, representative) = deduplicated
            .aliases
            .iter()
            .find(|(key, representative)| *key != *representative)
            .unwrap();
        assert_eq!(deduplicated.textures[representative].rgba, [1, 2, 3, 255]);
        assert_eq!(deduplicated.aliases_by_image[representative].len(), 1);
        assert!(deduplicated.aliases_by_image[representative].contains(alias));
    }

    #[test]
    fn texture_deduplication_preserves_sampling_and_alpha_semantics() {
        let textures = HashMap::from([
            (
                "runtime:roadsignfont".into(),
                image(1, 1, false, &[1, 2, 3, 255]),
            ),
            ("city:atlas".into(), image(1, 1, false, &[1, 2, 3, 255])),
            (
                "city:transparent".into(),
                image(1, 1, true, &[1, 2, 3, 255]),
            ),
            ("city:different".into(), image(1, 1, false, &[1, 2, 3, 254])),
        ]);
        let deduplicated = deduplicate_textures(textures);
        assert_eq!(deduplicated.textures.len(), 4);
        assert!(deduplicated
            .aliases
            .iter()
            .all(|(key, representative)| key == representative));
        assert_eq!(deduplicated.deduplicated_bytes, 0);
    }
}
