//! Optional temporal anti-aliasing, and the jitter and history that later
//! temporal upscalers (FSR 3, DLSS) need too. Off by default.
use glam::{Mat4, Vec2, Vec3};
use wgpu::util::DeviceExt;

/// Floats in the controls uniform: two matrices and one vec4.
const CONTROLS: usize = 16 + 16 + 4;
/// Weight of the newest frame; the rest comes from the reprojected history.
const CURRENT_WEIGHT: f32 = 0.1;
/// A camera move longer than this in one frame is a cut: teleport, respawn,
/// interior travel or a cutscene shot change.
const CUT_DISTANCE: f32 = 8.0;
const HALTON: usize = 8;

/// Sub-pixel offset for `frame`, in pixels within [-0.5, 0.5]: the Halton (2, 3)
/// sequence that FSR and DLSS integration guides recommend.
pub fn jitter(frame: u32) -> Vec2 {
    let halton = |mut index: u32, base: u32| {
        let mut fraction = 1.0;
        let mut result = 0.0;
        while index > 0 {
            fraction /= base as f32;
            result += fraction * (index % base) as f32;
            index /= base;
        }
        result
    };
    let index = frame % HALTON as u32 + 1;
    Vec2::new(halton(index, 2), halton(index, 3)) - 0.5
}
/// Shifts a projection by a sub-pixel offset of a `size`-pixel target.
pub fn jittered(view_projection: Mat4, offset: Vec2, size: [u32; 2]) -> Mat4 {
    let ndc = Vec2::new(
        offset.x * 2.0 / size[0].max(1) as f32,
        -offset.y * 2.0 / size[1].max(1) as f32,
    );
    Mat4::from_translation(ndc.extend(0.0)) * view_projection
}

pub struct Temporal {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    /// Ping-pong history; `frame % 2` is written while the other is read.
    pub history: [wgpu::TextureView; 2],
    groups: [wgpu::BindGroup; 2],
    pub frame: u32,
    previous: Option<(Mat4, Vec3)>,
}
impl Temporal {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        size: [u32; 2],
    ) -> Self {
        let texture = |binding, sample_type| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type,
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let float = wgpu::TextureSampleType::Float { filterable: true };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Temporal anti-aliasing layout"),
            entries: &[
                texture(0, float),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture(3, wgpu::TextureSampleType::Depth),
                texture(4, float),
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Temporal anti-aliasing"),
            source: wgpu::ShaderSource::Wgsl(include_str!("temporal.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Temporal anti-aliasing"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Temporal history"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Temporal anti-aliasing controls"),
            contents: bytemuck::cast_slice(&[0.0f32; CONTROLS]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let history = Self::targets(device, format, size);
        let groups = Self::bind(device, &layout, &sampler, &params, color, depth, &history);
        Self {
            layout,
            pipeline,
            sampler,
            params,
            history,
            groups,
            frame: 0,
            previous: None,
        }
    }
    fn targets(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: [u32; 2],
    ) -> [wgpu::TextureView; 2] {
        std::array::from_fn(|_| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("Temporal history"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    // Copies let tests and captures read the resolved frame.
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING
                        | wgpu::TextureUsages::COPY_SRC,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        })
    }
    fn bind(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        params: &wgpu::Buffer,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        history: &[wgpu::TextureView; 2],
    ) -> [wgpu::BindGroup; 2] {
        // Group `i` writes history[i], so it reads the other one.
        std::array::from_fn(|i| {
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("Temporal anti-aliasing inputs"),
                layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(color),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(&history[1 - i]),
                    },
                ],
            })
        })
    }
    /// New scene targets: the history no longer matches and starts over.
    pub fn resize(
        &mut self,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        color: &wgpu::TextureView,
        depth: &wgpu::TextureView,
        size: [u32; 2],
    ) {
        self.history = Self::targets(device, format, size);
        self.groups = Self::bind(
            device,
            &self.layout,
            &self.sampler,
            &self.params,
            color,
            depth,
            &self.history,
        );
        self.previous = None;
    }
    /// The offset to render the coming frame with.
    pub fn jitter(&self) -> Vec2 {
        jitter(self.frame)
    }
    /// Forgets the history, for frames drawn without temporal anti-aliasing.
    pub fn reset(&mut self) {
        self.previous = None;
    }
    /// Resolves the frame drawn with `jittered` into the next history image and
    /// returns its index in `history`. `view_projection` is the same camera
    /// without jitter, which the following frame reprojects against.
    pub fn resolve(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        size: [u32; 2],
        jittered: Mat4,
        view_projection: Mat4,
        eye: Vec3,
    ) -> usize {
        let previous = self
            .previous
            .filter(|(_, last_eye)| last_eye.distance(eye) < CUT_DISTANCE);
        let mut params = jittered.inverse().to_cols_array().to_vec();
        params.extend(
            previous
                .map_or(view_projection, |(last, _)| last)
                .to_cols_array(),
        );
        params.extend([
            1.0 / size[0].max(1) as f32,
            1.0 / size[1].max(1) as f32,
            if previous.is_some() { 0.0 } else { 1.0 },
            CURRENT_WEIGHT,
        ]);
        debug_assert_eq!(params.len(), CONTROLS);
        queue.write_buffer(&self.params, 0, bytemuck::cast_slice(&params));
        let target = (self.frame % 2) as usize;
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Temporal anti-aliasing"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.history[target],
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.groups[target], &[]);
        pass.draw(0..3, 0..1);
        self.previous = Some((view_projection, eye));
        self.frame = self.frame.wrapping_add(1);
        target
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// Runs the real shader: a still camera over a still image must reproduce
    /// that image exactly, for geometry and for the sky (depth zero), with no
    /// drift or NaN from reprojection.
    async fn still_frames() -> anyhow::Result<Vec<[f32; 4]>> {
        use anyhow::Context;
        const SIZE: [u32; 2] = [32, 8];
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN | wgpu::Backends::DX12 | wgpu::Backends::METAL,
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let adapter = instance
            .request_adapter(&Default::default())
            .await
            .context("No GPU adapter")?;
        let (device, queue) = adapter.request_device(&Default::default()).await?;
        let target = |format, usage| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: None,
                    size: wgpu::Extent3d {
                        width: SIZE[0],
                        height: SIZE[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let sampled = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let format = crate::postprocess::SCENE_FORMAT;
        let color = target(format, sampled);
        let depth = target(wgpu::TextureFormat::Depth32Float, sampled);
        let mut temporal = Temporal::new(&device, format, &color, &depth, SIZE);
        let camera = Mat4::perspective_rh(1.2, 4.0, 3500.0, 0.1)
            * Mat4::look_at_rh(Vec3::ZERO, Vec3::NEG_Z, Vec3::Y);
        let mut results = Vec::new();
        // Half the frames see geometry at mid depth, half see only sky.
        for (frame, (value, depth_value)) in [
            (0.25, 0.5),
            (0.25, 0.5),
            (0.25, 0.5),
            (4.0, 0.0),
            (4.0, 0.0),
        ]
        .into_iter()
        .enumerate()
        {
            let mut encoder = device.create_command_encoder(&Default::default());
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &color,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: value,
                            g: value,
                            b: value,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(depth_value),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let jitter = temporal.jitter();
            let eye = if frame < 3 {
                Vec3::ZERO
            } else {
                Vec3::X * 100.0
            };
            let written = temporal.resolve(
                &mut encoder,
                &queue,
                SIZE,
                jittered(camera, jitter, SIZE),
                camera,
                eye,
            );
            let readback = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: (SIZE[0] * SIZE[1] * 8) as u64,
                usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                mapped_at_creation: false,
            });
            encoder.copy_texture_to_buffer(
                temporal.history[written].texture().as_image_copy(),
                wgpu::TexelCopyBufferInfo {
                    buffer: &readback,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(SIZE[0] * 8),
                        rows_per_image: None,
                    },
                },
                wgpu::Extent3d {
                    width: SIZE[0],
                    height: SIZE[1],
                    depth_or_array_layers: 1,
                },
            );
            queue.submit([encoder.finish()]);
            readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            device.poll(wgpu::PollType::wait_indefinitely())?;
            let bytes = readback.slice(..).get_mapped_range()?.to_vec();
            let half = |i: usize| f16_to_f32(u16::from_le_bytes([bytes[i], bytes[i + 1]]));
            let mut low = [f32::INFINITY; 4];
            let mut high = [f32::NEG_INFINITY; 4];
            for pixel in 0..(SIZE[0] * SIZE[1]) as usize {
                let v = half(pixel * 8);
                low[0] = low[0].min(v);
                high[0] = high[0].max(v);
            }
            results.push([low[0], high[0], value as f32, 0.0]);
        }
        Ok(results)
    }
    fn f16_to_f32(bits: u16) -> f32 {
        let sign = if bits & 0x8000 != 0 { -1.0 } else { 1.0 };
        let exponent = i32::from((bits >> 10) & 0x1f);
        let mantissa = f32::from(bits & 0x3ff);
        sign * match exponent {
            0 => mantissa / 1024.0 * 2f32.powi(-14),
            31 => f32::INFINITY,
            _ => (1.0 + mantissa / 1024.0) * 2f32.powi(exponent - 15),
        }
    }
    #[test]
    fn still_image_survives_the_real_shader() {
        let results = match pollster::block_on(still_frames()) {
            Ok(results) => results,
            Err(error) => {
                eprintln!("skipping: {error}");
                return;
            }
        };
        for (frame, [low, high, expected, _]) in results.into_iter().enumerate() {
            assert!(
                (low - expected).abs() < 0.01 && (high - expected).abs() < 0.01,
                "frame {frame}: {low}..{high}, expected {expected}"
            );
        }
    }
    #[test]
    fn jitter_covers_the_pixel_without_bias() {
        let offsets: Vec<_> = (0..HALTON as u32).map(jitter).collect();
        assert!(offsets.iter().all(|o| o.x.abs() <= 0.5 && o.y.abs() <= 0.5));
        let mean = offsets.iter().copied().sum::<Vec2>() / HALTON as f32;
        assert!(mean.length() < 0.07, "{mean:?}");
        assert_eq!(jitter(0), jitter(HALTON as u32));
    }
    #[test]
    fn jittered_projection_moves_points_by_the_offset_in_pixels() {
        let projection = Mat4::perspective_rh(1.2, 2.0, 3500.0, 0.1)
            * Mat4::look_at_rh(Vec3::ZERO, Vec3::NEG_Z, Vec3::Y);
        let size = [200, 100];
        let point = Vec3::new(1.0, 0.5, -10.0);
        let pixel = |m: Mat4| {
            let ndc = m.project_point3(point);
            Vec2::new((ndc.x + 1.0) * 0.5 * 200.0, (1.0 - ndc.y) * 0.5 * 100.0)
        };
        let moved = pixel(jittered(projection, Vec2::new(0.25, -0.5), size)) - pixel(projection);
        assert!((moved - Vec2::new(0.25, -0.5)).length() < 1e-3, "{moved:?}");
    }
}
