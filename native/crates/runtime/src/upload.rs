//! Incremental GPU upload: retain the current scene until its replacement is complete.
use crate::GpuBatch;
use anyhow::{Context, Result};
use sa_assets::Texture;
use sa_scene::{Batch, Scene};
use std::{
    collections::HashMap,
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
pub struct Upload {
    pub scene: Scene,
    textures: std::collections::hash_map::IntoIter<String, Texture>,
    batches: std::vec::IntoIter<Batch>,
    images: HashMap<String, wgpu::BindGroup>,
    current_image: Option<ImageUpload>,
    current_buffer: Option<BufferUpload>,
    ready: Vec<GpuBatch>,
    frames: usize,
    peak_ms: f64,
    bytes: usize,
    reused_images: usize,
    reused_bytes: usize,
}
impl Upload {
    pub fn new(mut scene: Scene) -> Self {
        Self {
            textures: std::mem::take(&mut scene.textures).into_iter(),
            batches: std::mem::take(&mut scene.batches).into_iter(),
            scene,
            images: HashMap::new(),
            current_image: None,
            current_buffer: None,
            ready: Vec::new(),
            frames: 0,
            peak_ms: 0.0,
            bytes: 0,
            reused_images: 0,
            reused_bytes: 0,
        }
    }
    /// Texture keys are identities only within an immutable world loader.
    /// Never call this across a session/mod-resource switch: equal names may
    /// carry different pixels. BindGroup clones retain the actual GPU images.
    pub fn reusing(scene: Scene, previous: &[GpuBatch]) -> Self {
        let images = previous
            .iter()
            .filter(|batch| scene.textures.contains_key(&batch.texture_key))
            .map(|batch| (batch.texture_key.clone(), batch.texture.clone()))
            .collect();
        let mut upload = Self::new(scene);
        upload.images = images;
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
                    let texture = crate::mipmaps::texture(device, &key, image.width, image.height);
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
                        mipmaps.encode_level(device, encoder, &image.texture, image.mip);
                        image.mip += 1;
                        continue;
                    }
                    let image = self.current_image.take().unwrap();
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
                    self.images.insert(image.key, group);
                    continue;
                }
                let stride = image.image.width as usize * 4;
                let rows =
                    ((PACKET_BYTES / stride).max(1) as u32).min(image.image.height - image.row);
                let start = image.row as usize * stride;
                let count = rows as usize * stride;
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
                image.row += rows;
                bytes += count;
                continue;
            }
            if self.current_buffer.is_none() {
                if let Some(batch) = self.batches.next() {
                    let size = std::mem::size_of_val(batch.vertices.as_slice()) as u64;
                    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some(&batch.key),
                        size: size.max(4),
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        mapped_at_creation: false,
                    });
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
                    queue.write_buffer(
                        &buffer.buffer,
                        buffer.offset as u64,
                        &raw[buffer.offset..end],
                    );
                }
                if !buffer.batch.animated {
                    let stride = std::mem::size_of::<sa_scene::Vertex>();
                    let first = buffer.offset / stride;
                    let last = end.div_ceil(stride).min(buffer.batch.vertices.len());
                    buffer.bounds.include(&buffer.batch.vertices[first..last]);
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
    pub fn finish(mut self) -> (Vec<GpuBatch>, Scene) {
        self.ready.sort_by_key(|batch| batch.alpha);
        eprintln!("Incremental GPU upload: {} batches / {:.1} MiB over {} frames; peak CPU slice {:.2} ms; reused {} textures / {:.1} MiB", self.ready.len(), self.bytes as f64 / 1048576.0, self.frames, self.peak_ms, self.reused_images, self.reused_bytes as f64 / 1048576.0);
        (self.ready, self.scene)
    }
}
