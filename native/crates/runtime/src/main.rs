use anyhow::{Context, Result};
use glam::{Mat4, Quat, Vec3};
use sa_scene::{
    collision::{CollisionWorld, Player},
    Scene,
};
use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Instant};
mod capture;
mod menu;
mod settings;
mod streaming;
use streaming::{Streamer, ORIGIN, RADIUS};
use wgpu::util::DeviceExt;
use winit::{
    application::ApplicationHandler,
    event::{DeviceEvent, ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{KeyCode, PhysicalKey},
    window::{CursorGrabMode, Window, WindowId},
};

struct GpuBatch {
    buffer: wgpu::Buffer,
    count: u32,
    texture: wgpu::BindGroup,
    alpha: bool,
    animated: bool,
    base: Vec<f32>,
}
struct State {
    window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    size: winit::dpi::PhysicalSize<u32>,
    depth: wgpu::TextureView,
    camera: wgpu::Buffer,
    camera_group: wgpu::BindGroup,
    opaque: wgpu::RenderPipeline,
    blended: wgpu::RenderPipeline,
    batches: Vec<GpuBatch>,
    position: Vec3,
    yaw: f32,
    pitch: f32,
    keys: HashSet<KeyCode>,
    captured: bool,
    last: Instant,
    animation: Option<sa_script::CutAnimation>,
    started: Instant,
    collision: Option<CollisionWorld>,
    water: Option<std::sync::Arc<sa_scene::water::WaterMap>>,
    player: Option<Player>,
    walking: bool,
    image_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    streamer: Option<Streamer>,
    region: [f32; 2],
    interior: u8,
    interior_destination: Option<usize>,
    room_entry: Option<Vec3>,
    title_updated: Instant,
    frame_count: u32,
    destination: Option<[f32; 2]>,
    capture_next: Option<PathBuf>,
    gui_context: egui::Context,
    gui_input: egui_winit::State,
    gui_renderer: egui_wgpu::Renderer,
    menu: menu::Menu,
    applied_settings: settings::Settings,
    quit_requested: bool,
    car: Option<(sa_scene::vehicle::Car, Vec<GpuBatch>)>,
    driving: bool,
    ped: Option<(sa_scene::ped::Ped, Vec<GpuBatch>)>,
    third_person: bool,
    ped_seconds: f32,
    ped_clip: &'static str,
    ped_yaw: f32,
}
impl State {
    fn install_car(&mut self, scene: Scene) -> Result<()> {
        let clearance = -scene
            .batches
            .iter()
            .flat_map(|b| &b.vertices)
            .map(|v| v.position[1])
            .fold(f32::INFINITY, f32::min);
        let batches = Self::upload_scene(
            &self.device,
            &self.queue,
            &self.image_layout,
            &self.sampler,
            scene,
        )?;
        self.car = Some((sa_scene::vehicle::Car::new(Vec3::ZERO, clearance), batches));
        self.place_car();
        self.driving = false;
        Ok(())
    }
    fn place_car(&mut self) {
        if self.interior != 0 {
            return;
        }
        if let (Some(world), Some((car, _))) = (&self.collision, &mut self.car) {
            let origin = if self.driving {
                car.position + Vec3::Y * (1.6 - car.clearance)
            } else {
                self.position
            };
            if let Some(placed) =
                sa_scene::vehicle::Car::spawn_near(world, origin, self.yaw, car.clearance)
            {
                *car = placed;
                self.driving = true;
                self.keys.clear();
            }
        }
    }
    fn toggle_car(&mut self) {
        if let (Some(world), Some((car, _))) = (&self.collision, &mut self.car) {
            if self.driving {
                if let Some(player) = car.exit_player(world) {
                    self.position = player.eye();
                    self.player = Some(player);
                    self.driving = false;
                    self.walking = true;
                }
                car.speed = 0.0;
            } else if self.position.distance(car.position) < 6.0 {
                self.driving = true;
            }
            self.keys.clear();
        }
    }
    async fn new(window: Arc<Window>, mut scene: Scene, first_model: bool) -> Result<Self> {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor::default())
            .await?;
        let format = surface
            .get_capabilities(&adapter)
            .formats
            .into_iter()
            .find(|f| f.is_srgb())
            .context("no sRGB surface")?;
        let size = window.inner_size();
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("SA scene"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shader.wgsl").into()),
        });
        let camera = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera"),
            size: 64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let camera_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera group"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera.as_entire_binding(),
            }],
        });
        let image_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image layout"),
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("SA layout"),
            bind_group_layouts: &[Some(&camera_layout), Some(&image_layout)],
            immediate_size: 0,
        });
        let vertex = wgpu::VertexBufferLayout {
            array_stride: 36,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &wgpu::vertex_attr_array![0=>Float32x3,1=>Float32x2,2=>Float32x4],
        };
        let make_pipeline = |alpha: bool| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(if alpha { "SA alpha" } else { "SA opaque" }),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    compilation_options: Default::default(),
                    buffers: &[Some(vertex.clone())],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(if alpha { "fs_blended" } else { "fs_main" }),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: if alpha {
                            Some(wgpu::BlendState::ALPHA_BLENDING)
                        } else {
                            None
                        },
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    cull_mode: None,
                    ..Default::default()
                },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: wgpu::TextureFormat::Depth24Plus,
                    depth_write_enabled: Some(!alpha),
                    depth_compare: Some(wgpu::CompareFunction::Less),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        };
        let opaque = make_pipeline(false);
        let blended = make_pipeline(true);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("SA textures"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let animation = scene.animation.take();
        let collision = scene.collision.take();
        let water = scene.water.take();
        let batches = Self::upload_scene(&device, &queue, &image_layout, &sampler, scene)?;
        let depth = Self::make_depth(&device, size);
        let gui_context = egui::Context::default();
        let mut menu = menu::Menu::new(&gui_context, Vec::new());
        if first_model || animation.is_some() {
            menu.page = None;
            menu.settings.show_hud = false;
        }
        let applied_settings = menu.settings.clone();
        let gui_input = egui_winit::State::new(
            gui_context.clone(),
            egui::ViewportId::ROOT,
            window.as_ref(),
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let gui_renderer =
            egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());
        let mut state = Self {
            window,
            instance,
            surface,
            device,
            queue,
            format,
            size,
            depth,
            camera,
            camera_group,
            opaque,
            blended,
            batches,
            position: if let Some(ref cut) = animation {
                if cut.camera_keys.is_some() {
                    Vec3::new(
                        cut.camera[0],
                        cut.camera[2] + cut.height_offset,
                        -cut.camera[1],
                    )
                } else {
                    Vec3::new(cut.camera[0], cut.camera[2], -cut.camera[1]).normalize_or_zero()
                        * 3.0
                }
            } else if first_model {
                Vec3::new(0.0, 2.0, -5.0)
            } else {
                Vec3::new(-10.0, 15.5, -5.0)
            },
            yaw: 0.0,
            pitch: 0.0,
            keys: HashSet::new(),
            captured: false,
            last: Instant::now(),
            animation,
            started: Instant::now(),
            collision,
            water,
            player: None,
            walking: false,
            image_layout,
            sampler,
            streamer: None,
            region: ORIGIN,
            interior: 0,
            interior_destination: None,
            room_entry: None,
            title_updated: Instant::now(),
            frame_count: 0,
            destination: None,
            capture_next: None,
            gui_context,
            gui_input,
            gui_renderer,
            menu,
            applied_settings,
            quit_requested: false,
            car: None,
            driving: false,
            ped: None,
            third_person: true,
            ped_seconds: 0.0,
            ped_clip: "idle_stance",
            ped_yaw: 0.0,
        };
        if !first_model && state.animation.is_none() {
            if let Some(world) = &state.collision {
                let player = Player::spawn(world, state.position);
                state.position = player.eye();
                state.player = Some(player);
                state.walking = true;
            }
        }
        state.configure();
        Ok(state)
    }
    fn upload_scene(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image_layout: &wgpu::BindGroupLayout,
        sampler: &wgpu::Sampler,
        scene: Scene,
    ) -> Result<Vec<GpuBatch>> {
        let mut images = std::collections::HashMap::new();
        for (key, image) in scene.textures {
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some(&key),
                size: wgpu::Extent3d {
                    width: image.width,
                    height: image.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &image.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(image.width * 4),
                    rows_per_image: Some(image.height),
                },
                wgpu::Extent3d {
                    width: image.width,
                    height: image.height,
                    depth_or_array_layers: 1,
                },
            );
            let view = texture.create_view(&Default::default());
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(&key),
                layout: image_layout,
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
            images.insert(key, group);
        }
        let mut batches = Vec::new();
        for batch in scene.batches {
            let mut raw = Vec::with_capacity(batch.vertices.len() * 9);
            for v in &batch.vertices {
                raw.extend(v.position);
                raw.extend(v.uv);
                raw.extend(v.color);
            }
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&batch.key),
                contents: bytemuck::cast_slice(&raw),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            batches.push(GpuBatch {
                buffer,
                count: batch.vertices.len() as u32,
                texture: images.remove(&batch.key).context("batch texture missing")?,
                alpha: batch.alpha,
                animated: batch.animated,
                base: if batch.animated { raw } else { Vec::new() },
            });
        }
        batches.sort_by_key(|b| b.alpha);
        Ok(batches)
    }
    fn stream_world(&mut self) {
        let Some(mut streamer) = self.streamer.take() else {
            return;
        };
        if let Some((region, result)) = streamer.poll() {
            let center = region.center;
            let wanted_interior = self
                .interior_destination
                .map(|index| streaming::INTERIORS[index].id)
                .unwrap_or(0);
            let stale = self.destination.is_some_and(|target| {
                streaming::distance(target, center) >= 1.0 || wanted_interior != region.interior
            });
            let result = result.and_then(|scene| {
                if !stale && self.destination.is_some() && region.interior != 0 {
                    let destination = streaming::INTERIORS[self
                        .interior_destination
                        .context("room destination missing")?];
                    let point = Vec3::new(
                        destination.position[0] - ORIGIN[0],
                        destination.position[2],
                        ORIGIN[1] - destination.position[1],
                    );
                    scene
                        .collision
                        .as_ref()
                        .and_then(|world| world.standing_at(point, point.y + 1.0))
                        .context("interior entrance has no safe standing position")?;
                }
                Ok(scene)
            });
            if !stale {
                match result {
                    Ok(mut scene) => {
                        let collision = scene.collision.take();
                        let water = scene.water.take();
                        match Self::upload_scene(
                            &self.device,
                            &self.queue,
                            &self.image_layout,
                            &self.sampler,
                            scene,
                        ) {
                            Ok(batches) => {
                                self.batches = batches;
                                self.collision = collision;
                                self.water = water;
                                self.region = center;
                                self.interior = region.interior;
                                if self
                                    .destination
                                    .is_some_and(|target| streaming::distance(target, center) < 1.0)
                                {
                                    self.destination = None;
                                    self.room_entry =
                                        self.interior_destination.take().map(|index| {
                                            let p = streaming::INTERIORS[index].position;
                                            Vec3::new(p[0] - ORIGIN[0], p[2], ORIGIN[1] - p[1])
                                        });
                                    self.respawn();
                                }
                                eprintln!(
                                    "Installed neighbourhood at {:.0}, {:.0}",
                                    center[0], center[1]
                                );
                            }
                            Err(error) => {
                                streamer.retry_later();
                                self.menu.message = format!("Kartet kunne ikke vises: {error}");
                                eprintln!("GPU upload failed: {error:#}");
                            }
                        }
                    }
                    Err(error) => {
                        self.menu.message = format!("Kartlasting feilet: {error}");
                        eprintln!("Streaming failed; keeping previous region: {error:#}");
                    }
                }
            }
        }
        let center = [ORIGIN[0] + self.position.x, ORIGIN[1] - self.position.z];
        if let Some(destination) = self.destination {
            streamer.request_region(streaming::Region {
                center: destination,
                interior: self
                    .interior_destination
                    .map(|index| streaming::INTERIORS[index].id)
                    .unwrap_or(0),
            });
        } else if self.interior == 0 && streaming::distance(center, self.region) > 140.0 {
            streamer.request(center);
        }
        self.streamer = Some(streamer);
    }
    fn make_depth(device: &wgpu::Device, size: winit::dpi::PhysicalSize<u32>) -> wgpu::TextureView {
        device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("depth"),
                size: wgpu::Extent3d {
                    width: size.width.max(1),
                    height: size.height.max(1),
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth24Plus,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default())
    }
    fn configure(&self) {
        if self.size.width == 0 || self.size.height == 0 {
            return;
        }
        self.surface.configure(
            &self.device,
            &wgpu::SurfaceConfiguration {
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                format: self.format,
                color_space: wgpu::SurfaceColorSpace::Auto,
                width: self.size.width,
                height: self.size.height,
                present_mode: if self.menu.settings.vsync {
                    wgpu::PresentMode::AutoVsync
                } else {
                    wgpu::PresentMode::AutoNoVsync
                },
                alpha_mode: wgpu::CompositeAlphaMode::Auto,
                view_formats: vec![],
                desired_maximum_frame_latency: 2,
            },
        );
    }
    fn resize(&mut self, size: winit::dpi::PhysicalSize<u32>) {
        self.size = size;
        if size.width > 0 && size.height > 0 {
            self.depth = Self::make_depth(&self.device, size);
            self.configure();
        }
    }
    fn capture(&mut self, on: bool) {
        self.captured = on;
        self.window.set_cursor_visible(!on);
        let mode = if on {
            CursorGrabMode::Locked
        } else {
            CursorGrabMode::None
        };
        if self.window.set_cursor_grab(mode).is_err() && on {
            let _ = self.window.set_cursor_grab(CursorGrabMode::Confined);
        }
    }
    fn update(&mut self) {
        let now = Instant::now();
        let dt = if self.menu.page.is_some() {
            0.0
        } else {
            (now - self.last).as_secs_f32().min(0.05)
        };
        self.last = now;
        self.stream_world();
        if let Some(ref cut) = self.animation {
            let duration = cut
                .camera_keys
                .as_ref()
                .and_then(|keys| keys.last())
                .map(|key| key.seconds)
                .unwrap_or_else(|| cut.keys.last().unwrap().seconds)
                .max(0.01);
            let time = (now - self.started).as_secs_f32() % duration;
            let after = cut.keys.partition_point(|key| key.seconds < time);
            let a = &cut.keys[after.saturating_sub(1)];
            let b = &cut.keys[after.min(cut.keys.len() - 1)];
            let factor = if b.seconds > a.seconds {
                (time - a.seconds) / (b.seconds - a.seconds)
            } else {
                0.0
            };
            let qa = Quat::from_xyzw(a.rotation[0], a.rotation[1], a.rotation[2], a.rotation[3])
                .normalize();
            let qb = Quat::from_xyzw(b.rotation[0], b.rotation[1], b.rotation[2], b.rotation[3])
                .normalize();
            let rotation = qa.slerp(qb, factor);
            let ta = Vec3::from_array(a.translation);
            let tb = Vec3::from_array(b.translation);
            let translation = ta.lerp(tb, factor);
            for batch in &self.batches {
                if !batch.animated {
                    continue;
                }
                let mut raw = batch.base.clone();
                for vertex in raw.as_chunks_mut::<9>().0.iter_mut() {
                    let original = Vec3::new(vertex[0], -vertex[2], vertex[1]);
                    let placed = rotation * original + translation;
                    vertex[0] = placed.x;
                    vertex[1] = placed.z + cut.height_offset;
                    vertex[2] = -placed.y;
                }
                self.queue
                    .write_buffer(&batch.buffer, 0, bytemuck::cast_slice(&raw));
            }
        }
        let previous_position = self.position;
        let forward = Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        );
        let right = Vec3::new(-self.yaw.cos(), 0.0, self.yaw.sin());
        let mut motion = Vec3::ZERO;
        for (key, axis) in [
            (KeyCode::KeyW, forward),
            (KeyCode::KeyS, -forward),
            (KeyCode::KeyD, right),
            (KeyCode::KeyA, -right),
            (KeyCode::KeyE, Vec3::Y),
            (KeyCode::KeyQ, -Vec3::Y),
        ] {
            if self.keys.contains(&key) {
                motion += axis;
            }
        }
        let speed = if self.keys.contains(&KeyCode::ShiftLeft)
            || self.keys.contains(&KeyCode::ShiftRight)
        {
            self.menu.settings.fly_speed * 4.0
        } else {
            self.menu.settings.fly_speed
        };
        if self.driving {
            if let (Some(world), Some((car, _))) = (&self.collision, &mut self.car) {
                let previous = car.position;
                let throttle = f32::from(self.keys.contains(&KeyCode::KeyW))
                    - f32::from(self.keys.contains(&KeyCode::KeyS));
                let steer = f32::from(self.keys.contains(&KeyCode::KeyA))
                    - f32::from(self.keys.contains(&KeyCode::KeyD));
                car.step(
                    world,
                    throttle,
                    steer,
                    self.keys.contains(&KeyCode::Space),
                    dt,
                );
                let center = [ORIGIN[0] + car.position.x, ORIGIN[1] - car.position.z];
                if streaming::distance(center, self.region) > RADIUS - 60.0 {
                    car.position = previous;
                    car.speed = 0.0;
                }
                self.position = world.clip_camera(
                    car.position + Vec3::Y,
                    car.position - car.forward() * 7.0 + Vec3::Y * 3.5,
                );
            }
        } else if self.walking {
            if let (Some(world), Some(player)) = (&self.collision, &mut self.player) {
                let horizontal = Vec3::new(motion.x, 0.0, motion.z).normalize_or_zero();
                let walking_speed = if self.keys.contains(&KeyCode::ShiftLeft)
                    || self.keys.contains(&KeyCode::ShiftRight)
                {
                    9.0
                } else {
                    4.5
                };
                if let Some(water) = &self.water {
                    player.step_in_water(
                        world,
                        water,
                        ORIGIN,
                        horizontal * walking_speed,
                        self.keys.contains(&KeyCode::Space),
                        dt,
                    );
                } else {
                    player.step(
                        world,
                        horizontal * walking_speed,
                        self.keys.contains(&KeyCode::Space),
                        dt,
                    );
                }
                self.position = player.eye();
            }
        } else {
            self.position += motion.normalize_or_zero() * dt * speed;
        }
        if self.walking && !self.driving && self.third_person {
            if let (Some((ped, batches)), Some(player)) = (&self.ped, &self.player) {
                if dt > 0.0 {
                    let delta = self.position - previous_position;
                    let horizontal = Vec3::new(delta.x, 0.0, delta.z);
                    let speed = horizontal.length() / dt;
                    let clip = if speed < 0.1 {
                        "idle_stance"
                    } else if speed > 5.0 {
                        "run_player"
                    } else {
                        "walk_player"
                    };
                    if clip != self.ped_clip {
                        self.ped_clip = clip;
                        self.ped_seconds = 0.0;
                    }
                    self.ped_seconds += dt;
                    if horizontal.length_squared() > 0.00001 {
                        self.ped_yaw = horizontal.x.atan2(horizontal.z);
                    }
                }
                match ped.frame(self.ped_clip, self.ped_seconds) {
                    Ok(posed) => {
                        let rotation = Quat::from_rotation_y(self.ped_yaw + std::f32::consts::PI);
                        for (mesh, batch) in posed.iter().zip(batches) {
                            let mut raw = Vec::with_capacity(mesh.vertices.len() * 9);
                            for vertex in &mesh.vertices {
                                let position =
                                    rotation * Vec3::from_array(vertex.position) + player.feet;
                                raw.extend(position.to_array());
                                raw.extend(vertex.uv);
                                raw.extend(vertex.color);
                            }
                            self.queue
                                .write_buffer(&batch.buffer, 0, bytemuck::cast_slice(&raw));
                        }
                    }
                    Err(error) => {
                        self.menu.message = format!("Player animation failed: {error}");
                    }
                }
            }
        }
        if let Some((car, batches)) = &self.car {
            let rotation = Quat::from_rotation_y(car.yaw + std::f32::consts::PI);
            for batch in batches {
                let mut raw = batch.base.clone();
                for v in raw.as_chunks_mut::<9>().0.iter_mut() {
                    let point = rotation * Vec3::new(v[0], v[1], v[2]) + car.position;
                    v[0] = point.x;
                    v[1] = point.y;
                    v[2] = point.z;
                }
                self.queue
                    .write_buffer(&batch.buffer, 0, bytemuck::cast_slice(&raw));
            }
        }
        if self.streamer.is_some() {
            let center = [ORIGIN[0] + self.position.x, ORIGIN[1] - self.position.z];
            if streaming::distance(center, self.region) > RADIUS - 50.0 {
                self.position = previous_position;
                if let Some(player) = &mut self.player {
                    player.feet.x = previous_position.x;
                    player.feet.z = previous_position.z;
                }
            }
            if self.position.y < -100.0 {
                self.respawn();
            }
            self.frame_count += 1;
            if now.duration_since(self.title_updated).as_secs_f32() >= 1.0 {
                let fps =
                    self.frame_count as f32 / now.duration_since(self.title_updated).as_secs_f32();
                let pending = self.streamer.as_ref().is_some_and(|s| s.pending());
                self.window.set_title(&format!("SA Freeroam | {:.0} FPS | GTA {:.0}, {:.0}, {:.1} | {}{} | WASD Shift Space | P fly | R reset", fps, center[0], center[1], self.position.y, if self.driving { "Drive" } else if self.walking { "Walk" } else { "Fly" }, if pending { " | loading map..." } else { "" }));
                self.frame_count = 0;
                self.title_updated = now;
            }
        }
        let camera_time = self.animation.as_ref().and_then(|cut| {
            let keys = cut.camera_keys.as_ref()?;
            let duration = keys.last().unwrap().seconds.max(0.01);
            let seconds = (now - self.started).as_secs_f32() % duration;
            let after = keys.partition_point(|key| key.seconds < seconds);
            let a = &keys[after.saturating_sub(1)];
            let b = &keys[after.min(keys.len() - 1)];
            let factor = if b.seconds > a.seconds {
                (seconds - a.seconds) / (b.seconds - a.seconds)
            } else {
                0.0
            };
            let convert = |p: [f32; 3]| Vec3::new(p[0], p[2] + cut.height_offset, -p[1]);
            Some((
                convert(a.position).lerp(convert(b.position), factor),
                convert(a.target).lerp(convert(b.target), factor),
                a.fov + (b.fov - a.fov) * factor,
            ))
        });
        if let Some((position, _, _)) = camera_time {
            self.position = position;
        }
        let target = camera_time
            .map(|(_, target, _)| target)
            .or_else(|| {
                self.animation.as_ref().map(|cut| {
                    let original = Vec3::new(cut.target[0], cut.target[2], -cut.target[1]);
                    let seconds = (now - self.started).as_secs_f32()
                        % cut.keys.last().unwrap().seconds.max(0.01);
                    let key = &cut.keys[cut
                        .keys
                        .partition_point(|key| key.seconds < seconds)
                        .saturating_sub(1)];
                    original
                        + Vec3::new(key.translation[0], key.translation[2], -key.translation[1])
                })
            })
            .unwrap_or(self.position + forward);
        let target = if self.driving {
            self.car
                .as_ref()
                .map(|(c, _)| c.position + Vec3::Y)
                .unwrap_or(target)
        } else {
            target
        };
        let target = if self.position.distance_squared(target) < 0.0001 {
            self.position + forward
        } else {
            target
        };
        let (view_position, target) =
            if self.third_person && self.walking && !self.driving && self.ped.is_some() {
                let target = self.position - Vec3::Y * 0.5;
                let desired = target - forward * 4.0 + Vec3::Y * 0.7;
                let camera = self
                    .collision
                    .as_ref()
                    .map_or(desired, |world| world.clip_camera(target, desired));
                (
                    camera,
                    if camera.distance_squared(target) < 0.0001 {
                        camera + forward
                    } else {
                        target
                    },
                )
            } else {
                (self.position, target)
            };
        let view = Mat4::look_at_rh(view_position, target, Vec3::Y);
        let fov = camera_time
            .map(|(_, _, fov)| fov)
            .or_else(|| self.animation.as_ref().map(|cut| cut.fov))
            .unwrap_or(self.menu.settings.fov);
        let projection = Mat4::perspective_rh(
            fov.to_radians(),
            self.size.width.max(1) as f32 / self.size.height.max(1) as f32,
            0.1,
            3500.0,
        );
        self.queue.write_buffer(
            &self.camera,
            0,
            bytemuck::cast_slice(&(projection * view).to_cols_array()),
        );
    }
    fn respawn(&mut self) {
        self.driving = false;
        // Recover on loaded ground rather than teleporting into an unloaded map.
        let eye = Vec3::new(
            self.region[0] - ORIGIN[0],
            500.0,
            ORIGIN[1] - self.region[1],
        );
        if let Some(world) = &self.collision {
            if let Some(entry) = self.room_entry {
                if let Some(player) = world.standing_at(entry, entry.y + 1.0) {
                    self.position = player.eye();
                    self.player = Some(player);
                    self.walking = true;
                }
                return;
            }
            let player = Player::spawn(world, eye);
            self.position = player.eye();
            self.player = Some(player);
        }
    }
    fn apply_menu_action(&mut self, action: Option<menu::Action>) {
        match action {
            Some(menu::Action::Play) => {
                self.menu.has_played = true;
                self.menu.page = None;
                self.keys.clear();
                self.last = Instant::now();
                self.capture(true);
            }
            Some(menu::Action::Teleport(index)) => {
                self.interior_destination = None;
                self.destination = Some(streaming::DESTINATIONS[index].1);
                self.menu.has_played = true;
                self.menu.page = None;
                self.keys.clear();
                self.capture(true);
            }
            Some(menu::Action::Interior(index)) => {
                let target = streaming::INTERIORS[index];
                self.destination = Some([target.position[0], target.position[1]]);
                self.interior_destination = Some(index);
                self.menu.has_played = true;
                self.menu.page = None;
                self.keys.clear();
                self.capture(true);
            }
            Some(menu::Action::Quit) => self.quit_requested = true,
            Some(menu::Action::Clothing(index, enabled)) => {
                if let Some((ped, _)) = &mut self.ped {
                    if let Err(error) = ped.set_clothing(index, enabled) {
                        self.menu.message = error.to_string();
                    }
                    self.menu.clothes = ped.clothing_options();
                }
            }
            Some(menu::Action::Main) => {
                self.menu.open(menu::Page::Main);
                self.capture(false);
                self.keys.clear();
            }
            None => {}
        }
    }
    fn apply_settings(&mut self) {
        self.menu.settings.sanitize();
        if self.menu.settings == self.applied_settings {
            return;
        }
        if self.menu.settings.fullscreen != self.applied_settings.fullscreen {
            self.window
                .set_fullscreen(if self.menu.settings.fullscreen {
                    Some(winit::window::Fullscreen::Borderless(None))
                } else {
                    None
                });
        }
        if self.menu.settings.vsync != self.applied_settings.vsync {
            self.configure();
        }
        if let Err(error) = self.menu.settings.save() {
            self.menu.message = format!("Kunne ikke lagre innstillinger: {error}");
        }
        self.applied_settings = self.menu.settings.clone();
    }
    fn render(&mut self) -> bool {
        if self.size.width == 0 || self.size.height == 0 {
            return false;
        }
        self.update();
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f) => f,
            wgpu::CurrentSurfaceTexture::Suboptimal(f) => {
                drop(f);
                self.configure();
                return false;
            }
            wgpu::CurrentSurfaceTexture::Lost => {
                self.surface = self
                    .instance
                    .create_surface(self.window.clone())
                    .expect("recreate surface");
                self.configure();
                return false;
            }
            wgpu::CurrentSurfaceTexture::Outdated => {
                self.configure();
                return false;
            }
            _ => return false,
        };
        let capture_path = self.capture_next.take();
        let capture_texture = capture_path.as_ref().map(|_| {
            self.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("verification capture"),
                size: wgpu::Extent3d {
                    width: self.size.width,
                    height: self.size.height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            })
        });
        let view = capture_texture
            .as_ref()
            .unwrap_or(&frame.texture)
            .create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let raw_input = self.gui_input.take_egui_input(&self.window);
        let coordinates = [
            ORIGIN[0] + self.position.x,
            ORIGIN[1] - self.position.z,
            self.position.y,
        ];
        let loading = self.streamer.as_ref().is_some_and(|s| s.pending());
        let context = self.gui_context.clone();
        let mut action = None;
        let mut output = context.run_ui(raw_input, |ui| {
            action = self.menu.draw(ui.ctx(), coordinates, loading);
        });
        if self.captured && self.menu.page.is_none() {
            output.platform_output.cursor_icon = egui::CursorIcon::None;
        }
        self.gui_input
            .handle_platform_output(&self.window, output.platform_output);
        for (id, deltas) in output.textures_delta.set.drain() {
            for delta in deltas {
                self.gui_renderer
                    .update_texture(&self.device, &self.queue, id, &delta);
            }
        }
        let paint_jobs = context.tessellate(output.shapes, output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.size.width, self.size.height],
            pixels_per_point: output.pixels_per_point,
        };
        let mut commands = self.gui_renderer.update_buffers(
            &self.device,
            &self.queue,
            &mut encoder,
            &paint_jobs,
            &screen,
        );
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("SA world"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.36,
                            g: 0.55,
                            b: 0.72,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.depth,
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
            pass.set_bind_group(0, &self.camera_group, &[]);
            let mut alpha = false;
            pass.set_pipeline(&self.opaque);
            for b in self
                .batches
                .iter()
                .chain(self.car.iter().flat_map(|(_, b)| b))
                .chain(
                    self.ped
                        .iter()
                        .filter(|_| self.third_person && self.walking && !self.driving)
                        .flat_map(|(_, b)| b),
                )
            {
                if b.alpha != alpha {
                    pass.set_pipeline(if b.alpha { &self.blended } else { &self.opaque });
                    alpha = b.alpha;
                }
                pass.set_bind_group(1, &b.texture, &[]);
                pass.set_vertex_buffer(0, b.buffer.slice(..));
                pass.draw(0..b.count, 0..1);
            }
        }
        {
            let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("freeroam menus"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            self.gui_renderer
                .render(&mut pass.forget_lifetime(), &paint_jobs, &screen);
        }
        commands.push(encoder.finish());
        self.queue.submit(commands);
        for id in output.textures_delta.free.drain() {
            self.gui_renderer.free_texture(&id);
        }
        self.window.pre_present_notify();
        self.queue.present(frame);
        self.apply_menu_action(action);
        self.apply_settings();
        if let (Some(texture), Some(path)) = (capture_texture, capture_path) {
            capture::save(
                &self.device,
                &self.queue,
                &texture,
                self.size.width,
                self.size.height,
                self.format,
                &path,
            )
            .expect("GPU capture");
        }
        true
    }
}
struct App {
    car_scene: Option<Scene>,
    ped: Option<sa_scene::ped::Ped>,
    scene: Option<Scene>,
    state: Option<State>,
    first_model: bool,
    streamer: Option<Streamer>,
    smoke: bool,
    smoke_region: usize,
    smoke_frames: usize,
    smoke_started: Instant,
    capture_dir: Option<PathBuf>,
    failure: Option<String>,
    mod_names: Vec<String>,
    smoke_menus: bool,
    smoke_stream: bool,
    smoke_car: bool,
    smoke_ped: bool,
    smoke_wardrobe: bool,
    smoke_interiors: bool,
    car_start: Vec3,
    smoke_returning: bool,
    smoke_center: [f32; 2],
}
impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.is_some() {
            return;
        }
        let title = if self.scene.as_ref().is_some_and(|s| {
            s.animation
                .as_ref()
                .is_some_and(|a| a.camera_keys.is_some())
        }) {
            "SA Native Runtime - Prolog1 preview"
        } else if self.scene.as_ref().is_some_and(|s| s.animation.is_some()) {
            "SA Native Runtime - Cuttest preview"
        } else if self.first_model {
            "SA Native Runtime - First model"
        } else {
            "SA Freeroam - loading"
        };
        let window = Arc::new(
            event_loop
                .create_window(
                    Window::default_attributes()
                        .with_title(title)
                        .with_visible(!self.smoke)
                        .with_inner_size(winit::dpi::LogicalSize::new(1440, 900)),
                )
                .expect("window"),
        );
        let scene = self.scene.take().expect("loaded scene");
        match pollster::block_on(State::new(window.clone(), scene, self.first_model)) {
            Ok(mut state) => {
                state.streamer = self.streamer.take();
                state.menu.mods = self.mod_names.clone();
                if let Some(ped) = self.ped.take() {
                    match ped.scene().and_then(|scene| {
                        State::upload_scene(
                            &state.device,
                            &state.queue,
                            &state.image_layout,
                            &state.sampler,
                            scene,
                        )
                    }) {
                        Ok(batches) => {
                            state.menu.clothes = ped.clothing_options();
                            state.ped = Some((ped, batches));
                        }
                        Err(error) => eprintln!("Ped upload failed: {error:#}"),
                    }
                }
                if let Some(scene) = self.car_scene.take() {
                    if let Err(error) = state.install_car(scene) {
                        state.menu.message = format!("Bil kunne ikke lastes: {error}");
                    }
                }
                if self.smoke && !self.smoke_menus {
                    state.menu.page = None;
                    state.menu.has_played = true;
                }
                if self.smoke_interiors {
                    state.apply_menu_action(Some(menu::Action::Interior(0)));
                }
                if self.smoke_stream {
                    state.walking = false;
                    state.position.y = 70.0;
                    state.pitch = 0.0;
                    state.yaw = -std::f32::consts::FRAC_PI_2;
                }
                if self.smoke_car {
                    state.place_car();
                    self.car_start = state.car.as_ref().expect("car mesh missing").0.position;
                }
                if !self.smoke && state.menu.settings.fullscreen {
                    state
                        .window
                        .set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
                }
                self.state = Some(state);
            }
            Err(error) => {
                eprintln!("GPU startup failed: {error:#}");
                self.failure = Some(format!("GPU startup failed: {error:#}"));
                event_loop.exit();
                return;
            }
        }
        window.request_redraw();
    }
    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let Some(state) = self.state.as_mut() else {
            return;
        };
        let _ = state.gui_input.on_window_event(&state.window, &event);
        if let WindowEvent::KeyboardInput { event, .. } = &event {
            if event.state == ElementState::Pressed && !event.repeat {
                if event.physical_key == PhysicalKey::Code(KeyCode::Escape)
                    && state.streamer.is_some()
                {
                    if state.menu.page.is_some() {
                        state.menu.back();
                    } else {
                        state.menu.open(menu::Page::Pause);
                    }
                    state.capture(state.menu.page.is_none());
                    state.keys.clear();
                    return;
                }
                if event.physical_key == PhysicalKey::Code(KeyCode::KeyI)
                    && state.streamer.is_some()
                {
                    state.menu.open(menu::Page::Interiors);
                    state.capture(false);
                    state.keys.clear();
                    return;
                }
                if event.physical_key == PhysicalKey::Code(KeyCode::KeyM)
                    && state.streamer.is_some()
                {
                    state.menu.open(menu::Page::Map);
                    state.capture(false);
                    state.keys.clear();
                    return;
                }
            }
        }
        if state.menu.page.is_some()
            && matches!(
                event,
                WindowEvent::KeyboardInput { .. } | WindowEvent::MouseInput { .. }
            )
        {
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => state.resize(size),
            WindowEvent::RedrawRequested => {
                if self.smoke_interiors {
                    state.last = Instant::now() - std::time::Duration::from_secs_f32(1.0 / 60.0);
                    state.keys.clear();
                    if state.destination.is_none() {
                        if self.smoke_frames < 60 {
                            state.keys.insert(KeyCode::KeyW);
                        } else if self.smoke_frames < 100 {
                            state.keys.insert(KeyCode::KeyS);
                        }
                        if self.smoke_frames == 25 {
                            state.keys.insert(KeyCode::Space);
                        }
                        if self.smoke_frames == 115 {
                            if let Some(directory) = &self.capture_dir {
                                state.capture_next = Some(
                                    directory.join(format!("interior-{}.png", self.smoke_region)),
                                );
                            }
                        }
                    }
                }
                if self.smoke_ped {
                    if self.smoke_wardrobe {
                        assert!(
                            !state.menu.clothes.is_empty(),
                            "wardrobe requires a clothing resource"
                        );
                        if self.smoke_frames == 185 {
                            state.apply_menu_action(Some(menu::Action::Clothing(0, false)));
                        }
                        if self.smoke_frames == 245 {
                            state.apply_menu_action(Some(menu::Action::Clothing(0, true)));
                        }
                        if [200, 270].contains(&self.smoke_frames) {
                            if let Some(directory) = &self.capture_dir {
                                state.capture_next = Some(directory.join(format!(
                                    "wardrobe-{}.png",
                                    if self.smoke_frames == 200 {
                                        "off"
                                    } else {
                                        "on"
                                    }
                                )));
                            }
                        }
                    }
                    state.last = Instant::now() - std::time::Duration::from_secs_f32(1.0 / 60.0);
                    state.keys.clear();
                    if self.smoke_frames < 180 {
                        state.keys.insert(KeyCode::KeyW);
                    }
                    if (90..180).contains(&self.smoke_frames) {
                        state.keys.insert(KeyCode::ShiftLeft);
                    }
                    state.third_person = !(210..240).contains(&self.smoke_frames);
                }
                if self.smoke_car {
                    state.keys.insert(KeyCode::KeyW);
                    if self.smoke_frames == 180 {
                        state.keys.insert(KeyCode::Space);
                    }
                    if self.smoke_frames == 240 {
                        state.keys.clear();
                        state.toggle_car();
                    }
                    if self.smoke_frames == 270 {
                        state.toggle_car();
                    }
                }
                if self.smoke_stream {
                    state.keys.insert(KeyCode::KeyW);
                    state.keys.insert(KeyCode::ShiftLeft);
                }
                if self.smoke
                    && !self.smoke_stream
                    && !self.smoke_car
                    && !self.smoke_ped
                    && self.smoke_frames == 3
                    && state.destination.is_none()
                    && state.streamer.as_ref().is_some_and(|s| !s.pending())
                {
                    if let Some(directory) = &self.capture_dir {
                        state.capture_next = Some(directory.join(format!(
                            "{}-{}.png",
                            if self.smoke_menus { "menu" } else { "region" },
                            self.smoke_region + 1
                        )));
                    }
                }
                let previous_position = state.position;
                let rendered = state.render();
                if state.quit_requested {
                    event_loop.exit();
                }
                if self.smoke_ped && rendered {
                    assert!(state.ped.is_some(), "ped mesh missing");
                    assert!(
                        self.smoke_started.elapsed().as_secs() < 180,
                        "ped smoke timed out"
                    );
                    self.smoke_frames += 1;
                    let label = match self.smoke_frames {
                        70 => Some("walk_player"),
                        150 => Some("run_player"),
                        250 => Some("idle_stance"),
                        _ => None,
                    };
                    if let Some(label) = label {
                        assert_eq!(state.ped_clip, label, "ped clip selection failed");
                        if let Some(directory) = &self.capture_dir {
                            state.capture_next = Some(directory.join(format!("ped-{label}.png")));
                        }
                    }
                    if self.smoke_frames >= 302 {
                        let player = state.player.as_ref().expect("player missing");
                        assert!(
                            player.feet.is_finite() && player.grounded && state.third_person,
                            "invalid final ped state"
                        );
                        assert!(
                            player.feet.distance(Vec3::new(-10.0, 12.34, -5.0)) > 10.0,
                            "ped failed to move"
                        );
                        println!("GPU ped smoke passed: walk, run, idle, first/third-person toggle; feet {:?}",player.feet);
                        self.smoke = false;
                        self.smoke_ped = false;
                        event_loop.exit();
                        return;
                    }
                }
                if self.smoke_ped {
                    state.window.request_redraw();
                    return;
                }
                if self.smoke_car && rendered {
                    assert!(
                        self.smoke_started.elapsed().as_secs() < 180,
                        "car smoke timed out"
                    );
                    let (car, _) = state.car.as_ref().expect("car failed to load");
                    assert!(
                        car.position.is_finite() && car.position.y > -90.0,
                        "car fell out of world"
                    );
                    self.smoke_frames += 1;
                    if self.smoke_frames == 300 {
                        assert!(state.driving, "car re-entry failed");
                        if let Some(directory) = &self.capture_dir {
                            state.capture_next = Some(directory.join("car.png"));
                        }
                    }
                    if self.smoke_frames >= 302 {
                        assert!(
                            car.position.distance(self.car_start) > 1.0,
                            "car did not move"
                        );
                        println!("GPU car smoke passed: driving, braking, exit and re-entry; car {:?}, speed {:.1}",car.position,car.speed);
                        self.smoke_car = false;
                        self.smoke = false;
                        event_loop.exit();
                        return;
                    }
                    state.window.request_redraw();
                    return;
                }
                if self.smoke_stream {
                    assert!(
                        self.smoke_started.elapsed().as_secs() < 180,
                        "continuous streaming timed out"
                    );
                    assert!(state.position.is_finite(), "non-finite streaming position");
                    assert!(
                        state.position.distance(previous_position) <= 13.0,
                        "region swap moved the camera"
                    );
                    if streaming::distance(self.smoke_center, state.region) > 1.0 {
                        self.smoke_center = state.region;
                        self.smoke_region += 1;
                        println!(
                            "Continuous streaming installed region {} at {:?}",
                            self.smoke_region, state.region
                        );
                    }
                    if !self.smoke_returning && state.position.x <= -600.0 {
                        self.smoke_returning = true;
                        state.yaw = std::f32::consts::FRAC_PI_2;
                    }
                    if self.smoke_returning && state.position.x >= -10.0 && rendered {
                        assert!(
                            self.smoke_region >= 6,
                            "route did not exercise enough region swaps"
                        );
                        println!("GPU continuous streaming passed: 1.2 km return route, {} region swaps, no teleport", self.smoke_region);
                        event_loop.exit();
                    }
                    state.window.request_redraw();
                    return;
                }
                if self.smoke_interiors {
                    assert!(
                        self.smoke_started.elapsed().as_secs() < 120,
                        "interior smoke timed out"
                    );
                    if rendered
                        && state.destination.is_none()
                        && state.streamer.as_ref().is_some_and(|s| !s.pending())
                    {
                        self.smoke_frames += 1;
                        let expected = if self.smoke_region < streaming::INTERIORS.len() {
                            streaming::INTERIORS[self.smoke_region].id
                        } else {
                            0
                        };
                        assert_eq!(
                            state.interior, expected,
                            "wrong interior dimension installed"
                        );
                        assert_eq!(
                            state.water.is_none(),
                            expected != 0,
                            "incorrect water dimension"
                        );
                        assert!(state.position.is_finite());
                        if expected != 0 {
                            assert!(
                                state.position.y
                                    > streaming::INTERIORS[self.smoke_region].position[2] - 2.0,
                                "interior floor fall"
                            );
                        }
                        if self.smoke_frames >= 120 {
                            assert!(
                                state.player.as_ref().is_some_and(|p| p.grounded),
                                "room walk did not land"
                            );
                            println!("GPU interior route stage {} passed: dimension {}, eye {:?}, {} batches",self.smoke_region,expected,state.position,state.batches.len());
                            self.smoke_region += 1;
                            self.smoke_frames = 0;
                            if self.smoke_region < streaming::INTERIORS.len() {
                                state.apply_menu_action(Some(menu::Action::Interior(
                                    self.smoke_region,
                                )));
                            } else if self.smoke_region == streaming::INTERIORS.len() {
                                state.apply_menu_action(Some(menu::Action::Teleport(0)));
                            } else {
                                println!("GPU interior smoke passed: three rooms, walking/jump and exterior return");
                                event_loop.exit();
                            }
                        }
                    }
                    state.window.request_redraw();
                    return;
                }
                if self.smoke_menus && rendered {
                    self.smoke_frames += 1;
                    if self.smoke_frames >= 4 {
                        println!("GPU rendered menu {:?}", state.menu.page);
                        self.smoke_frames = 0;
                        self.smoke_region += 1;
                        let pages = [
                            menu::Page::Main,
                            menu::Page::Pause,
                            menu::Page::Map,
                            menu::Page::Settings,
                            menu::Page::Controls,
                            menu::Page::Mods,
                            menu::Page::Wardrobe,
                            menu::Page::Interiors,
                            menu::Page::Quit,
                        ];
                        if self.smoke_region >= pages.len() {
                            println!("GPU menu smoke passed: 9 menus");
                            event_loop.exit();
                        } else {
                            state.menu.open(pages[self.smoke_region]);
                        }
                    }
                    state.window.request_redraw();
                    return;
                }
                if self.smoke {
                    if self.smoke_started.elapsed().as_secs() > 180 {
                        panic!("smoke tour timed out");
                    }
                    if rendered
                        && state.destination.is_none()
                        && state.streamer.as_ref().is_some_and(|s| !s.pending())
                    {
                        self.smoke_frames += 1;
                        if self.smoke_frames >= 4 {
                            let (name, _) = streaming::DESTINATIONS[self.smoke_region];
                            assert!(
                                state.position.is_finite() && state.position.y > -90.0,
                                "invalid spawn in {name}"
                            );
                            println!(
                                "GPU smoke rendered {name}, eye {:?}, {} batches",
                                state.position,
                                state.batches.len()
                            );
                            self.smoke_region += 1;
                            self.smoke_frames = 0;
                            if self.smoke_region == streaming::DESTINATIONS.len() {
                                println!("GPU smoke tour passed: 9 regions rendered and streamed");
                                event_loop.exit();
                            } else {
                                state.destination =
                                    Some(streaming::DESTINATIONS[self.smoke_region].1);
                            }
                        }
                    }
                }
                state.window.request_redraw();
            }
            WindowEvent::MouseInput {
                button: MouseButton::Left,
                state: ElementState::Pressed,
                ..
            } => state.capture(true),
            WindowEvent::Focused(false) => {
                state.capture(false);
                state.keys.clear();
                if !self.smoke && state.streamer.is_some() && state.menu.page.is_none() {
                    state.menu.open(menu::Page::Pause);
                }
            }
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    if event.state == ElementState::Pressed {
                        match code {
                            KeyCode::Escape => state.capture(false),
                            KeyCode::F9 if !event.repeat => {
                                state.place_car();
                            }
                            KeyCode::F6 if !event.repeat => {
                                state.menu.open(menu::Page::Wardrobe);
                                state.keys.clear();
                                state.capture(false);
                            }
                            KeyCode::KeyV if !event.repeat => {
                                state.third_person = !state.third_person;
                            }
                            KeyCode::KeyF if !event.repeat => {
                                state.toggle_car();
                            }
                            KeyCode::Digit1
                            | KeyCode::Digit2
                            | KeyCode::Digit3
                            | KeyCode::Digit4
                            | KeyCode::Digit5
                            | KeyCode::Digit6
                            | KeyCode::Digit7
                            | KeyCode::Digit8
                            | KeyCode::Digit9
                                if !event.repeat && state.streamer.is_some() =>
                            {
                                let index = match code {
                                    KeyCode::Digit1 => 0,
                                    KeyCode::Digit2 => 1,
                                    KeyCode::Digit3 => 2,
                                    KeyCode::Digit4 => 3,
                                    KeyCode::Digit5 => 4,
                                    KeyCode::Digit6 => 5,
                                    KeyCode::Digit7 => 6,
                                    KeyCode::Digit8 => 7,
                                    _ => 8,
                                };
                                state.interior_destination = None;
                                state.destination = Some(streaming::DESTINATIONS[index].1);
                                state.keys.clear();
                            }
                            KeyCode::KeyR if !event.repeat => {
                                if state.streamer.is_some() {
                                    state.interior_destination = None;
                                    state.destination = Some(ORIGIN);
                                    state.keys.clear();
                                    return;
                                }
                                state.position = if let Some(cut) = &state.animation {
                                    let origin = Vec3::new(
                                        cut.camera[0],
                                        cut.camera[2] + cut.height_offset,
                                        -cut.camera[1],
                                    );
                                    if cut.camera_keys.is_some() {
                                        origin
                                    } else {
                                        origin.normalize_or_zero() * 3.0
                                    }
                                } else if self.first_model {
                                    Vec3::new(0.0, 2.0, -5.0)
                                } else {
                                    Vec3::new(-10.0, 15.5, -5.0)
                                };
                                state.started = Instant::now();
                                state.yaw = 0.0;
                                state.pitch = 0.0;
                                if state.walking {
                                    if let Some(world) = &state.collision {
                                        let player = Player::spawn(world, state.position);
                                        state.position = player.eye();
                                        state.player = Some(player);
                                    }
                                }
                            }
                            KeyCode::KeyP if !event.repeat => {
                                if state.driving {
                                    state.toggle_car();
                                }
                                if state.walking {
                                    state.walking = false;
                                    state.player = None;
                                } else if let Some(world) = &state.collision {
                                    let player = Player::spawn(world, state.position);
                                    state.position = player.eye();
                                    state.player = Some(player);
                                    state.walking = true;
                                }
                            }
                            _ => {
                                state.keys.insert(code);
                            }
                        }
                    } else {
                        state.keys.remove(&code);
                    }
                }
            }
            _ => {}
        }
    }
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.smoke {
            if let Some(state) = &self.state {
                let id = state.window.id();
                self.window_event(event_loop, id, WindowEvent::RedrawRequested);
            }
        }
    }
    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _id: winit::event::DeviceId,
        event: DeviceEvent,
    ) {
        if let (Some(state), DeviceEvent::MouseMotion { delta }) = (self.state.as_mut(), event) {
            if state.captured {
                state.yaw -= delta.0 as f32 * 0.0025 * state.menu.settings.sensitivity;
                state.pitch = (state.pitch
                    - delta.1 as f32
                        * 0.0025
                        * state.menu.settings.sensitivity
                        * if state.menu.settings.invert_y {
                            -1.0
                        } else {
                            1.0
                        })
                .clamp(-1.5, 1.5);
            }
        }
    }
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let game = args
        .windows(2)
        .find(|w| w[0] == "--game-dir")
        .map(|w| PathBuf::from(&w[1]))
        .unwrap_or_else(|| PathBuf::from(r"E:\GTA San Andreas\Grand Theft Auto San Andreas"));
    if let Some(pair) = args.windows(2).find(|w| w[0] == "--inspect-cutscene") {
        println!("{:#?}", sa_script::inspect_cutscene(&game, &pair[1])?);
        return Ok(());
    }
    if args.iter().any(|a| a == "--probe-interiors") {
        let mut loader = sa_scene::WorldLoader::open(&game)?;
        println!(
            "Indexed {} exterior and {} interior placements",
            loader.placement_count(),
            loader.interior_placement_count()
        );
        for (name, id, position) in [
            ("CJ house", 3, [2496.05, -1692.93, 1013.75]),
            ("Sweet house", 1, [2526.46, -1679.09, 1014.5]),
            ("Madd Dogg mansion", 5, [1263.08, -785.309, 1090.96]),
        ] {
            let started = Instant::now();
            let scene = loader
                .load_interior([position[0], position[1]], ORIGIN, 50.0, id)
                .with_context(|| name)?;
            anyhow::ensure!(scene.water.is_none(), "exterior water leaked into {name}");
            let world = scene
                .collision
                .as_ref()
                .context("interior collision missing")?;
            let point = Vec3::new(
                position[0] - ORIGIN[0],
                position[2],
                ORIGIN[1] - position[1],
            );
            let mut player = world
                .standing_at(point, position[2] + 1.0)
                .with_context(|| format!("{name} entrance has no safe standing position"))?;
            let start = player.feet;
            for frame in 0..600 {
                let direction = match (frame / 150) % 4 {
                    0 => Vec3::X,
                    1 => Vec3::Z,
                    2 => -Vec3::X,
                    _ => -Vec3::Z,
                };
                player.step(world, direction * 4.5, frame == 100, 1.0 / 60.0);
                anyhow::ensure!(
                    player.feet.is_finite() && player.feet.y > start.y - 2.0,
                    "{name} movement fell through the floor"
                );
            }
            anyhow::ensure!(player.grounded, "{name} player did not land");
            println!("{name}: {} placements, {} triangles, {} collision triangles; entrance floor {:.3}; 600 walking/jump steps passed in {:.2}s", scene.placements, scene.triangles, world.triangle_count(), start.y, started.elapsed().as_secs_f32());
        }
        return Ok(());
    }
    if args.iter().any(|a| a == "--probe-world") {
        let mut loader = sa_scene::WorldLoader::open(&game)?;
        println!("Indexed {} exterior placements", loader.placement_count());
        for (name, center) in [
            ("Grove Street", ORIGIN),
            ("Los Santos centre", [1480.0, -1730.0]),
            ("Santa Maria beach", [350.0, -1800.0]),
            ("LS airport", [1700.0, -2450.0]),
            ("Countryside", [200.0, -500.0]),
            ("San Fierro", [-2000.0, 300.0]),
            ("Las Venturas", [2000.0, 1500.0]),
            ("Desert", [-500.0, 1900.0]),
            ("Mount Chiliad", [-2300.0, -1600.0]),
        ] {
            let started = Instant::now();
            let mut scene = loader.load(center, ORIGIN, RADIUS).with_context(|| name)?;
            let collision = scene.collision.as_ref().unwrap();
            let mut grounded = 0;
            for x in -4..=4 {
                for z in -4..=4 {
                    let point = Vec3::new(
                        center[0] - ORIGIN[0] + x as f32 * 50.0,
                        1000.0,
                        ORIGIN[1] - center[1] + z as f32 * 50.0,
                    );
                    if collision.ground_below(point, 1000.0).is_some() {
                        grounded += 1;
                    }
                }
            }
            println!("{name}: {} placements, {} triangles, {} textures, {} collision triangles, ground {grounded}/81, {:.2}s", scene.placements, scene.triangles, scene.textures.len(), collision.triangle_count(), started.elapsed().as_secs_f32());
            for batch in &mut scene.batches {
                batch.alpha = false;
            }
            let all = CollisionWorld::from_batches(&scene.batches);
            let mut total = 0;
            for x in -4..=4 {
                for z in -4..=4 {
                    let point = Vec3::new(
                        center[0] - ORIGIN[0] + x as f32 * 50.0,
                        1000.0,
                        ORIGIN[1] - center[1] + z as f32 * 50.0,
                    );
                    if all.ground_below(point, 1000.0).is_some() {
                        total += 1;
                    }
                }
            }
            println!("Including alpha geometry ground {total}/81");
        }
        return Ok(());
    }
    let first_model = args.iter().any(|a| a == "--first-model");
    let cuttest = args.iter().any(|a| a == "--cuttest");
    let prologue = args.iter().any(|a| a == "--prologue");
    let probe = args.iter().any(|a| a == "--probe");
    let mut loader = if !first_model && !cuttest && !prologue {
        Some(sa_scene::WorldLoader::open(&game)?)
    } else {
        None
    };
    if let Some(loader) = &mut loader {
        if !args.iter().any(|a| a == "--no-mods") {
            let directory = args
                .windows(2)
                .find(|w| w[0] == "--mods-dir")
                .map(|w| PathBuf::from(&w[1]))
                .unwrap_or_else(|| PathBuf::from("mods"));
            loader.enable_mods(&directory)?;
        }
    }
    let scene = if prologue {
        sa_scene::load_prologue(&game)?
    } else if cuttest {
        sa_scene::load_cuttest(&game)?
    } else if first_model {
        sa_scene::load_first_model(&game)?
    } else {
        loader.as_mut().unwrap().load(ORIGIN, ORIGIN, RADIUS)?
    };
    println!(
        "{} placements, {} triangles, {} texture batches",
        scene.placements,
        scene.triangles,
        scene.batches.len()
    );
    if args.iter().any(|a| a == "--probe-water") {
        let coast = loader
            .as_mut()
            .context("water probe requires world loader")?
            .load([100.0, -2100.0], ORIGIN, RADIUS)?;
        let world = coast
            .collision
            .as_ref()
            .context("coastal collision missing")?;
        let water = coast.water.as_ref().context("coastal water missing")?;
        let mut player = Player {
            feet: Vec3::new(100.0 - ORIGIN[0], -3.0, ORIGIN[1] + 2100.0),
            vertical_speed: -5.0,
            grounded: false,
        };
        for _ in 0..600 {
            player.step_in_water(world, water, ORIGIN, Vec3::X * 4.5, false, 1.0 / 60.0);
        }
        let level = water
            .level_at([ORIGIN[0] + player.feet.x, ORIGIN[1] - player.feet.z])
            .context("left coastal water")?;
        anyhow::ensure!(
            (player.feet.y - (level - 1.2)).abs() < 0.01 && !player.grounded,
            "coastal float test failed"
        );
        println!(
            "Original coastal water passed: 600 physics steps, level {level}, eye {:?}",
            player.eye()
        );
        return Ok(());
    }
    if args.iter().any(|a| a == "--probe-walk") {
        let world = scene
            .collision
            .as_ref()
            .context("walking probe requires world collision")?;
        for (name, direction) in [
            ("west", -Vec3::X),
            ("east", Vec3::X),
            ("north", -Vec3::Z),
            ("south", Vec3::Z),
        ] {
            let mut player = Player::spawn(world, Vec3::new(-10.0, 15.5, -5.0));
            let start = player.feet;
            let mut grounded = 0;
            for _ in 0..1200 {
                player.step(world, direction * 4.5, false, 1.0 / 60.0);
                anyhow::ensure!(
                    player.feet.is_finite() && player.feet.y > -100.0,
                    "walking probe fell out of world heading {name}"
                );
                grounded += usize::from(player.grounded);
            }
            println!(
                "Walking {name}: {:.1}m displacement, ground {grounded}/1200 frames, feet {:?}",
                (player.feet - start).length(),
                player.feet
            );
        }
        return Ok(());
    }
    if probe {
        if let Some(animation) = &scene.animation {
            println!(
                "{} animated batches; scene height offset {:.2}",
                scene.batches.iter().filter(|batch| batch.animated).count(),
                animation.height_offset
            );
        }
        if let Some(collision) = &scene.collision {
            println!(
                "{} collision triangles; ground at Grove spawn: {:?}",
                collision.triangle_count(),
                collision.ground_below(Vec3::new(-10.0, 15.5, -5.0), 15.4)
            );
        }
        return Ok(());
    }
    let mod_names = loader
        .as_ref()
        .map(|l| l.mod_names().to_vec())
        .unwrap_or_default();
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App {
        ped: if let Some(world_loader) = loader.as_mut() {
            match world_loader
                .take_custom_player(&game)
                .and_then(|custom| match custom {
                    Some(ped) => Ok(ped),
                    None => sa_scene::ped::Ped::load(&game),
                }) {
                Ok(ped) => Some(ped),
                Err(error) => {
                    eprintln!("Ped unavailable: {error:#}");
                    None
                }
            }
        } else {
            None
        },
        car_scene: if loader.is_some() {
            match loader
                .as_mut()
                .and_then(|l| l.take_custom_car())
                .map(Ok)
                .unwrap_or_else(|| sa_scene::load_car(&game))
            {
                Ok(scene) => Some(scene),
                Err(error) => {
                    eprintln!("Car mesh unavailable: {error:#}");
                    None
                }
            }
        } else {
            None
        },
        scene: Some(scene),
        state: None,
        first_model,
        streamer: loader.map(Streamer::new),
        smoke: args.iter().any(|a| {
            a == "--smoke-tour"
                || a == "--smoke-menus"
                || a == "--smoke-stream"
                || a == "--smoke-car"
                || a == "--smoke-ped"
                || a == "--smoke-wardrobe"
                || a == "--smoke-interiors"
        }),
        smoke_region: 0,
        smoke_frames: 0,
        smoke_started: Instant::now(),
        capture_dir: args
            .windows(2)
            .find(|w| w[0] == "--capture-dir")
            .map(|w| PathBuf::from(&w[1])),
        failure: None,
        mod_names,
        smoke_menus: args.iter().any(|a| a == "--smoke-menus"),
        smoke_stream: args.iter().any(|a| a == "--smoke-stream"),
        smoke_car: args.iter().any(|a| a == "--smoke-car"),
        smoke_ped: args
            .iter()
            .any(|a| a == "--smoke-ped" || a == "--smoke-wardrobe"),
        smoke_wardrobe: args.iter().any(|a| a == "--smoke-wardrobe"),
        smoke_interiors: args.iter().any(|a| a == "--smoke-interiors"),
        car_start: Vec3::ZERO,
        smoke_returning: false,
        smoke_center: ORIGIN,
    };
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.failure {
        anyhow::bail!(error);
    }
    Ok(())
}
