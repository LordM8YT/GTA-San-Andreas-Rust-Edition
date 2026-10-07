//! Linear HDR scene target followed by spatial scaling and image finishing.
use crate::settings::Settings;
use wgpu::util::DeviceExt;

pub const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub struct PostProcess {
    pub color: wgpu::TextureView,
    pub depth: wgpu::TextureView,
    pub size: [u32; 2],
    display_size: [u32; 2],
    finished: wgpu::TextureView,
    upscaled: wgpu::TextureView,
    scale_group: wgpu::BindGroup,
    sharpen_group: wgpu::BindGroup,
    scale_pipeline: wgpu::RenderPipeline,
    sharpen_pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    group: wgpu::BindGroup,
    sampler: wgpu::Sampler,
    params: wgpu::Buffer,
    pipeline: wgpu::RenderPipeline,
}
impl PostProcess {
    pub fn new(
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        size: [u32; 2],
        display_size: [u32; 2],
    ) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Image finishing layout"),
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
            ],
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Spatial scaling"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let params = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Image finishing controls"),
            contents: bytemuck::cast_slice(&[0.0f32; 12]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Image finishing"),
            source: wgpu::ShaderSource::Wgsl(include_str!("postprocess.wgsl").into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Spatial scaling, FXAA and color grading"),
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
                    format: SCENE_FORMAT,
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
        let fsr_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("AMD FSR 1 EASU / RCAS"),
            source: wgpu::ShaderSource::Wgsl(include_str!("fsr1.wgsl").into()),
        });
        let scale_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Spatial scaling, FXAA and color grading"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &fsr_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &fsr_shader,
                entry_point: Some("fs_easu"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: SCENE_FORMAT,
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
        let sharpen_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Spatial scaling, FXAA and color grading"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &fsr_shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &fsr_shader,
                entry_point: Some("fs_rcas"),
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
        let (color, depth, finished, upscaled) = Self::targets(device, size, display_size);
        let group = Self::bind(device, &layout, &color, &sampler, &params);
        let scale_group = Self::bind(device, &layout, &finished, &sampler, &params);
        let sharpen_group = Self::bind(device, &layout, &upscaled, &sampler, &params);
        Self {
            display_size,
            finished,
            upscaled,
            scale_group,
            sharpen_group,
            scale_pipeline,
            sharpen_pipeline,
            color,
            depth,
            size,
            layout,
            group,
            sampler,
            params,
            pipeline,
        }
    }
    fn targets(
        device: &wgpu::Device,
        size: [u32; 2],
        display: [u32; 2],
    ) -> (
        wgpu::TextureView,
        wgpu::TextureView,
        wgpu::TextureView,
        wgpu::TextureView,
    ) {
        let texture = |format, usage, label, dimensions: [u32; 2]| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d {
                        width: dimensions[0],
                        height: dimensions[1],
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
        (
            texture(
                SCENE_FORMAT,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                "HDR scene",
                size,
            ),
            texture(
                wgpu::TextureFormat::Depth24Plus,
                wgpu::TextureUsages::RENDER_ATTACHMENT,
                "Scene depth",
                size,
            ),
            texture(
                SCENE_FORMAT,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                "Tone mapped scene",
                size,
            ),
            texture(
                SCENE_FORMAT,
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                "FSR EASU output",
                display,
            ),
        )
    }
    fn bind(
        device: &wgpu::Device,
        layout: &wgpu::BindGroupLayout,
        color: &wgpu::TextureView,
        sampler: &wgpu::Sampler,
        params: &wgpu::Buffer,
    ) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Scene finishing inputs"),
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
            ],
        })
    }
    pub fn resize(&mut self, device: &wgpu::Device, size: [u32; 2], display: [u32; 2]) {
        if self.size == size && self.display_size == display {
            return;
        }
        (self.color, self.depth, self.finished, self.upscaled) =
            Self::targets(device, size, display);
        self.scale_group = Self::bind(
            device,
            &self.layout,
            &self.finished,
            &self.sampler,
            &self.params,
        );
        self.sharpen_group = Self::bind(
            device,
            &self.layout,
            &self.upscaled,
            &self.sampler,
            &self.params,
        );
        self.display_size = display;
        self.group = Self::bind(
            device,
            &self.layout,
            &self.color,
            &self.sampler,
            &self.params,
        );
        self.size = size;
    }
    pub fn draw(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        queue: &wgpu::Queue,
        output: &wgpu::TextureView,
        settings: &Settings,
        display: [u32; 2],
    ) {
        let params = [
            1.0 / self.size[0] as f32,
            1.0 / self.size[1] as f32,
            settings.sharpness,
            f32::from(settings.fxaa),
            settings.bloom,
            settings.exposure,
            settings.saturation,
            settings.vignette,
            1.0 / display[0].max(1) as f32,
            1.0 / display[1].max(1) as f32,
            f32::from(settings.fsr1),
            0.0,
        ];
        queue.write_buffer(&self.params, 0, bytemuck::cast_slice(&params));
        for (target, pipeline, group, label) in [
            (
                &self.finished,
                &self.pipeline,
                &self.group,
                "FXAA and color grading",
            ),
            (
                &self.upscaled,
                &self.scale_pipeline,
                &self.scale_group,
                "FSR 1 EASU / spatial scaling",
            ),
            (
                output,
                &self.sharpen_pipeline,
                &self.sharpen_group,
                "FSR 1 RCAS / presentation",
            ),
        ] {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some(label),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: target,
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
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, group, &[]);
            pass.draw(0..3, 0..1);
        }
    }
}
