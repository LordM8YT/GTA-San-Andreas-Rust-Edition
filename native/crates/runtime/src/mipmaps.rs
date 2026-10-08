//! GPU-generated, linear-light, alpha-weighted texture mip chains.
pub(super) struct Generator {
    layout: wgpu::BindGroupLayout,
    pipeline: wgpu::RenderPipeline,
}

pub(super) fn levels(width: u32, height: u32) -> u32 {
    width.max(height).max(1).ilog2() + 1
}

pub(super) fn texture(device: &wgpu::Device, key: &str, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(key),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        // Original road-sign glyphs share an atlas. A whole-atlas mip would
        // blend neighboring characters together without cell-specific padding.
        mip_level_count: if key == "runtime:roadsignfont" {
            1
        } else {
            levels(width, height)
        },
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    })
}

impl Generator {
    pub(super) fn new(device: &wgpu::Device) -> Self {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Mip source"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Mip generator"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Linear-light mip generator"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mipmaps.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Mip downsample"),
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
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
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
        Self { layout, pipeline }
    }

    pub(super) fn generate(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: &wgpu::Texture,
    ) {
        if texture.mip_level_count() == 1 {
            return;
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("Texture mip chain"),
        });
        for mip in 1..texture.mip_level_count() {
            self.encode_level(device, &mut encoder, texture, mip);
        }
        queue.submit([encoder.finish()]);
    }

    /// Encoding one level lets streaming check its frame budget between levels.
    pub(super) fn encode_level(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        mip: u32,
    ) {
        let source = texture.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level: mip - 1,
            mip_level_count: Some(1),
            ..Default::default()
        });
        let target = texture.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level: mip,
            mip_level_count: Some(1),
            ..Default::default()
        });
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Mip source"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&source),
            }],
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("Downsample mip"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
}

/// Readback checks use owned colors, no game data or window.
pub(super) async fn probe(backends: wgpu::Backends) -> anyhow::Result<()> {
    use anyhow::{ensure, Context};
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = instance
        .request_adapter(&Default::default())
        .await
        .context("No GPU adapter")?;
    let info = adapter.get_info();
    let (device, queue) = adapter.request_device(&Default::default()).await?;
    let generator = Generator::new(&device);
    let glyphs = texture(&device, "runtime:roadsignfont", 128, 128);
    ensure!(
        glyphs.mip_level_count() == 1,
        "glyph atlas must not blend characters in whole-atlas mips"
    );
    let mut checker = vec![0; 16];
    for pixel in checker.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
    checker[4..8].copy_from_slice(&[255; 4]);
    checker[12..16].copy_from_slice(&[255; 4]);
    let mut alpha = vec![0; 16];
    alpha[..4].copy_from_slice(&[255, 0, 0, 255]);
    let mut edge = vec![0; 36];
    for pixel in edge.as_chunks_mut::<4>().0 {
        pixel[3] = 255;
    }
    edge[32] = 255;
    for (width, height, data, expected) in [
        (2, 2, checker, [188u8, 188, 188, 255]),
        (2, 2, alpha, [255, 0, 0, 64]),
        (3, 3, edge, [94, 0, 0, 255]),
        (5, 3, [80, 130, 200, 255].repeat(15), [80, 130, 200, 255]),
        (1, 8, [51, 102, 153, 255].repeat(8), [51, 102, 153, 255]),
        (1, 1, vec![17, 33, 65, 255], [17, 33, 65, 255]),
    ] {
        let texture = texture(&device, "Owned mip fixture", width, height);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        generator.generate(&device, &queue, &texture);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Mip readback"),
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: texture.mip_level_count() - 1,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(1),
                },
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        buffer.map_async(wgpu::MapMode::Read, .., move |result| {
            let _ = sender.send(result);
        });
        device.poll(wgpu::PollType::wait_indefinitely())?;
        receiver.recv()??;
        let bytes = buffer.slice(..).get_mapped_range()?;
        for (got, wanted) in bytes[..4].iter().zip(expected) {
            ensure!(
                (i16::from(*got) - i16::from(wanted)).abs() <= 1,
                "Mip {width}x{height}: {:?}, expected {expected:?}",
                &bytes[..4]
            );
        }
        drop(bytes);
        buffer.unmap();
    }
    println!("GPU mipmap probe passed on {:?}: {} (linear sRGB, alpha edges, odd dimensions, skinny and 1x1 tails)",info.backend,info.name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chains_include_skinny_and_non_power_of_two_tails() {
        for (width, height, count) in [(1, 1, 1), (512, 512, 10), (5, 3, 3), (1, 8, 4), (7, 1, 3)] {
            assert_eq!(levels(width, height), count);
            assert_eq!((width >> (count - 1)).max(1), 1);
            assert_eq!((height >> (count - 1)).max(1), 1);
        }
    }
}
