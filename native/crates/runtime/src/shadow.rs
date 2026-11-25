//! Sun shadow map: one orthographic depth pass around the player, sampled by
//! the scene shader with hardware depth comparison.
use glam::{Mat4, Vec3, Vec4Swizzles};

pub const SIZE: u32 = 2048;
/// Half-width of the shadowed square around the player, in metres.
const RADIUS: f32 = 90.0;
const DEPTH: f32 = 1000.0;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub struct Shadows {
    pub view: wgpu::TextureView,
    /// Bind group 2 of the scene pipelines: shadow map and comparison sampler.
    pub group: wgpu::BindGroup,
    pub layout: wgpu::BindGroupLayout,
    pub pipeline: wgpu::RenderPipeline,
}

impl Shadows {
    pub fn new(
        device: &wgpu::Device,
        shader: &wgpu::ShaderModule,
        camera_layout: &wgpu::BindGroupLayout,
        image_layout: &wgpu::BindGroupLayout,
        vertex: wgpu::VertexBufferLayout<'_>,
    ) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("sun shadow map"),
            size: wgpu::Extent3d {
                width: SIZE,
                height: SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sun shadow sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
            ],
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        // The depth pass must not bind the shadow map it writes, so it uses
        // only the camera and image groups.
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("shadow pipeline layout"),
            bind_group_layouts: &[Some(camera_layout), Some(image_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sun shadow"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: shader,
                entry_point: Some("vs_shadow"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex)],
            },
            // Alpha-tested so palm leaves and fences cast shaped shadows.
            fragment: Some(wgpu::FragmentState {
                module: shader,
                entry_point: Some("fs_shadow"),
                compilation_options: Default::default(),
                targets: &[],
            }),
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: wgpu::DepthBiasState {
                    constant: 2,
                    slope_scale: 2.0,
                    clamp: 0.0,
                },
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        Self {
            view,
            group,
            layout,
            pipeline,
        }
    }
}

/// Light view-projection looking along `sun` and centred on `focus`, snapped to whole shadow texels so
/// shadow edges do not crawl as the player moves.
pub fn light_matrix(focus: Vec3, sun: Vec3) -> Mat4 {
    let sun = sun.normalize();
    let view = Mat4::look_at_rh(focus + sun * (DEPTH * 0.5), focus, Vec3::Y);
    let projection = Mat4::orthographic_rh(-RADIUS, RADIUS, -RADIUS, RADIUS, 1.0, DEPTH);
    let texel = 2.0 * RADIUS / SIZE as f32;
    let centre = view * focus.extend(1.0);
    let snap = centre.xy() - (centre.xy() / texel).round() * texel;
    Mat4::from_translation(Vec3::new(-snap.x / RADIUS, -snap.y / RADIUS, 0.0)) * projection * view
}

/// Renders an owned scene (a ground square under a floating roof) through the
/// real shadow and scene shaders and returns the ground brightness in the
/// roof's shadow and in open sun. Needs no game data or window.
pub(super) async fn probe(backends: wgpu::Backends) -> anyhow::Result<(f32, f32)> {
    use anyhow::Context;
    use wgpu::util::DeviceExt;
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = instance
        .request_adapter(&Default::default())
        .await
        .context("No GPU adapter")?;
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("SA scene"),
        source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
    });
    let uniform = |binding, visibility| wgpu::BindGroupLayoutEntry {
        binding,
        visibility,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    };
    let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[uniform(0, wgpu::ShaderStages::VERTEX_FRAGMENT)],
    });
    let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
        ],
    });
    let vertex = wgpu::VertexBufferLayout {
        array_stride: 36,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Float32x4],
    };
    let shadows = Shadows::new(
        &device,
        &shader,
        &camera_layout,
        &image_layout,
        vertex.clone(),
    );
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[
            Some(&camera_layout),
            Some(&image_layout),
            Some(&shadows.layout),
        ],
        immediate_size: 0,
    });
    const TARGET: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
    let scene = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: None,
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vs_main"),
            compilation_options: Default::default(),
            buffers: &[Some(vertex)],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fs_main"),
            compilation_options: Default::default(),
            targets: &[Some(TARGET.into())],
        }),
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth24Plus,
            depth_write_enabled: Some(true),
            depth_compare: Some(wgpu::CompareFunction::Less),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    let quad = |y: f32, half: f32| -> Vec<f32> {
        [
            [-1.0, -1.0],
            [1.0, -1.0],
            [1.0, 1.0],
            [-1.0, -1.0],
            [1.0, 1.0],
            [-1.0, 1.0],
        ]
        .iter()
        .flat_map(|[x, z]| [x * half, y, z * half, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0])
        .collect()
    };
    let mut vertices = quad(0.0, 40.0);
    vertices.extend(quad(10.0, 5.0));
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&vertices),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let white = device.create_texture_with_data(
        &queue,
        &wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        Default::default(),
        &[255; 4],
    );
    let white_view = white.create_view(&Default::default());
    let sampler = device.create_sampler(&Default::default());
    let image = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &image_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&white_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    // Top-down orthographic camera over the ground square.
    let eye = Vec3::new(0.0, 60.0, 0.0);
    let camera_matrix = Mat4::orthographic_rh(-40.0, 40.0, -40.0, 40.0, 1.0, 100.0)
        * Mat4::look_at_rh(eye, Vec3::ZERO, Vec3::NEG_Z);
    let mut uniforms = camera_matrix.to_cols_array().to_vec();
    let sun = Vec3::new(-0.42, 0.80, 0.43).normalize();
    uniforms.extend([eye.x, eye.y, eye.z, 0.0, 0.0, 1.0, 1.0, 0.0]);
    uniforms.extend(light_matrix(Vec3::ZERO, sun).to_cols_array());
    uniforms.extend([sun.x, sun.y, sun.z, 1.0]);
    uniforms.extend([1.0, 0.95, 0.86, 0.0, 1.0, 1.0, 1.0, 0.0, 0.5, 0.6, 0.7, 0.0]);
    // No object lights.
    uniforms.resize(uniforms.len() + 4 + crate::MAX_POINT_LIGHTS * 8, 0.0);
    let camera = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&uniforms),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let camera_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &camera_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: camera.as_entire_binding(),
        }],
    });
    const PIXELS: u32 = 64;
    let target = |format, usage| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: PIXELS,
                height: PIXELS,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    let color = target(
        TARGET,
        wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
    );
    let depth = target(
        wgpu::TextureFormat::Depth24Plus,
        wgpu::TextureUsages::RENDER_ATTACHMENT,
    );
    let color_view = color.create_view(&Default::default());
    let depth_view = depth.create_view(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &shadows.view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&shadows.pipeline);
        pass.set_bind_group(0, &camera_group, &[]);
        pass.set_bind_group(1, &image, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..12, 0..1);
    }
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color_view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&scene);
        pass.set_bind_group(0, &camera_group, &[]);
        pass.set_bind_group(1, &image, &[]);
        pass.set_bind_group(2, &shadows.group, &[]);
        pass.set_vertex_buffer(0, buffer.slice(..));
        pass.draw(0..12, 0..1);
    }
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (PIXELS * PIXELS * 4) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        color.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(PIXELS * 4),
                rows_per_image: None,
            },
        },
        color.size(),
    );
    queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely())?;
    let data = readback.slice(..).get_mapped_range()?.to_vec();
    let brightness = |world: Vec3| {
        let clip = camera_matrix.project_point3(world);
        let x = ((clip.x * 0.5 + 0.5) * PIXELS as f32) as usize;
        let y = ((0.5 - clip.y * 0.5) * PIXELS as f32) as usize;
        data[(y * PIXELS as usize + x) * 4] as f32 / 255.0
    };
    // The roof spans x, z in [-5, 5] at 10 m; the sun pushes its shadow to
    // roughly x in [0, 10], z in [-10, 0].
    Ok((
        brightness(Vec3::new(8.0, 0.0, -8.0)),
        brightness(Vec3::new(-20.0, 0.0, 20.0)),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    const SUN: Vec3 = Vec3::new(-0.42, 0.80, 0.43);
    #[test]
    fn focus_lands_near_the_centre_of_the_map() {
        let focus = Vec3::new(2495.3, 13.7, -1668.9);
        let clip = light_matrix(focus, SUN) * focus.extend(1.0);
        assert!(clip.x.abs() < 2.0 / SIZE as f32 * 1.01);
        assert!(clip.y.abs() < 2.0 / SIZE as f32 * 1.01);
        assert!((0.0..1.0).contains(&clip.z));
        let edge =
            light_matrix(focus, SUN) * (focus + Vec3::new(RADIUS * 0.9, 0.0, 0.0)).extend(1.0);
        assert!(edge.x.abs() <= 1.0 && edge.y.abs() <= 1.0);
    }
    #[test]
    fn roof_shadows_the_ground() {
        let result = pollster::block_on(probe(
            wgpu::Backends::VULKAN | wgpu::Backends::DX12 | wgpu::Backends::METAL,
        ));
        let Ok((shadowed, sunny)) = result else {
            eprintln!("skipping: {}", result.unwrap_err());
            return;
        };
        eprintln!("shadowed {shadowed:.2}, sunny {sunny:.2}");
        assert!(sunny > 0.5, "open ground should be lit: {sunny}");
        assert!(
            shadowed < sunny * 0.75,
            "shadowed {shadowed} vs sunny {sunny}"
        );
    }
}
