use anyhow::{Context, Result};
use glam::{Mat4, Quat, Vec3};
use sa_scene::{
    collision::{CollisionWorld, Player},
    Scene,
};
use std::{collections::HashSet, path::PathBuf, sync::Arc, time::Instant};
mod capture;
mod controller;
mod gameplay_audio;
mod menu;
mod multiplayer;
mod postprocess;
mod session_resources;
mod settings;
mod streaming;
mod upload;
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
    texture_key: String,
    buffer: wgpu::Buffer,
    count: u32,
    texture: wgpu::BindGroup,
    alpha: bool,
    animated: bool,
    uv_animation: Option<Arc<sa_assets::uvanim::UvAnimation>>,
    base: Vec<f32>,
}
struct SpawnedPed {
    model: usize,
    feet: Vec3,
    yaw: f32,
    batches: Vec<GpuBatch>,
}
struct State {
    audio: Option<sa_audio::AudioEngine>,
    frontend_sounds: Option<sa_audio::FrontendSounds>,
    gameplay_audio: gameplay_audio::GameplayAudio,
    window: Arc<Window>,
    instance: wgpu::Instance,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    format: wgpu::TextureFormat,
    size: winit::dpi::PhysicalSize<u32>,
    postprocess: postprocess::PostProcess,
    camera: wgpu::Buffer,
    camera_group: wgpu::BindGroup,
    opaque: wgpu::RenderPipeline,
    blended: wgpu::RenderPipeline,
    batches: Vec<GpuBatch>,
    position: Vec3,
    yaw: f32,
    pitch: f32,
    keys: HashSet<KeyCode>,
    gamepad: controller::Input,
    captured: bool,
    last: Instant,
    next_frame: Instant,
    animation: Option<sa_script::CutAnimation>,
    started: Instant,
    collision: Option<CollisionWorld>,
    water: Option<std::sync::Arc<sa_scene::water::WaterMap>>,
    player: Option<Player>,
    walking: bool,
    image_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    streamer: Option<Streamer>,
    uploading: Option<(streaming::Region, upload::Upload)>,
    retired: Vec<streaming::Retired>,
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
    radar_tiles: Vec<menu::RadarTile>,
    _radar_textures: Vec<egui::TextureHandle>,
    menu: menu::Menu,
    applied_settings: settings::Settings,
    persist_settings: bool,
    quit_requested: bool,
    car: Option<(sa_scene::vehicle::Car, Vec<GpuBatch>)>,
    car_uploads: u64,
    car_uploaded_bytes: u64,
    car_render_pose: Option<sa_net::VehiclePose>,
    driving: bool,
    car_catalog: Vec<Option<(sa_scene::vehicle::Car, Vec<GpuBatch>)>>,
    ped_catalog: Vec<Option<(sa_scene::ped::Ped, Vec<GpuBatch>)>>,
    spawned_peds: Vec<SpawnedPed>,
    npc_seconds: f32,
    network_session: Option<sa_net::Session>,
    network_car_spawned: bool,
    passenger: Option<sa_net::PassengerSeat>,
    passenger_car: Option<sa_net::VehiclePose>,
    ride_request: Option<sa_net::RideRequest>,
    ride_sequence: u32,
    ride_reply: Option<sa_net::RideReply>,
    network_publication: Option<sa_net::relay::Publication>,
    network_browser: Option<multiplayer::BrowserRequest>,
    resource_game_dir: PathBuf,
    resource_local_mods: Option<PathBuf>,
    resource_cache_dir: PathBuf,
    resource_job: Option<session_resources::Job>,
    resource_installing: Option<session_resources::Installing>,
    offline_world: Option<session_resources::World>,
    remote_actors: Vec<multiplayer::RemoteActor>,
    network_revision: u64,
    network_last: Instant,
    network_pose_last: Instant,
    active_car: usize,
    active_ped: usize,
    ped: Option<(sa_scene::ped::Ped, Vec<GpuBatch>)>,
    third_person: bool,
    ped_visible: bool,
    ped_seconds: f32,
    ped_clip: &'static str,
    ped_yaw: f32,
}
impl State {
    fn play_menu_sound(&mut self, kind: sa_audio::MenuSound) {
        if let (Some(audio), Some(sounds)) = (&mut self.audio, &self.frontend_sounds) {
            if let Err(error) = audio.play_effect(sounds.sound(kind)) {
                eprintln!("Menu sound failed: {error:#}");
            }
        }
    }
    fn menu_feedback(&mut self, before: (Option<menu::Page>, usize), back: bool) {
        let after = self.menu.sound_position();
        if before != after {
            self.play_menu_sound(if back {
                sa_audio::MenuSound::Back
            } else if before.0 != after.0 {
                sa_audio::MenuSound::Select
            } else {
                sa_audio::MenuSound::Highlight
            });
        }
    }
    fn install_car(&mut self, scene: Scene) -> Result<()> {
        let handling = scene.vehicle_handling;
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
        self.car = Some((
            sa_scene::vehicle::Car::new(Vec3::ZERO, clearance).with_handling(handling),
            batches,
        ));
        self.car_render_pose = None;
        self.place_car();
        self.driving = false;
        Ok(())
    }
    fn place_car(&mut self) {
        self.leave_passenger();
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
                *car = placed.with_handling(car.handling);
                self.car_render_pose = None;
                self.driving = true;
                self.network_car_spawned = true;
                self.keys.clear();
            }
        }
    }
    fn toggle_car(&mut self) {
        if self.passenger.is_some() {
            self.try_leave_passenger();
            return;
        }
        if self.ride_request.is_some_and(|r| r.owner.is_some()) {
            self.leave_passenger();
            return;
        }
        if !self.driving && self.try_passenger(false) {
            return;
        }
        if let (Some(world), Some((car, _))) = (&self.collision, &mut self.car) {
            if self.driving {
                if let Some(player) = car.exit_player(world) {
                    self.position = player.eye();
                    self.player = Some(player);
                    self.driving = false;
                    self.walking = true;
                }
                car.stop();
            } else if self.position.distance(car.position) < 6.0 {
                self.driving = true;
                self.network_car_spawned = true;
            }
            self.keys.clear();
        }
    }
    async fn new(
        window: Arc<Window>,
        mut scene: Scene,
        first_model: bool,
        radar_tiles: Vec<sa_scene::RadarTile>,
    ) -> Result<Self> {
        let mut startup_settings = settings::Settings::load();
        let args: Vec<_> = std::env::args().collect();
        if let Some(pair) = args.windows(2).find(|w| w[0] == "--renderer") {
            startup_settings.renderer = match pair[1].as_str() {
                "auto" => settings::Renderer::Auto,
                "vulkan" => settings::Renderer::Vulkan,
                "dx12" => settings::Renderer::DirectX12,
                other => anyhow::bail!("Unknown renderer {other}. Use auto, vulkan or dx12. OpenGL is not supported by this build."),
            };
        }
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: startup_settings.renderer.backends(),
            ..wgpu::InstanceDescriptor::new_without_display_handle()
        });
        let surface = instance.create_surface(window.clone())?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                compatible_surface: Some(&surface),
                ..Default::default()
            })
            .await.context("Selected renderer could not create a GPU adapter. Try --renderer auto or --renderer vulkan")?;
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
            size: 96,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
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
                        format: postprocess::SCENE_FORMAT,
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
        let gui_context = egui::Context::default();
        let mut radar_texture_handles = Vec::with_capacity(radar_tiles.len());
        let radar_tiles = radar_tiles
            .into_iter()
            .map(|tile| {
                let texture = gui_context.load_texture(
                    format!("original-radar-{:02}", tile.index),
                    egui::ColorImage::from_rgba_unmultiplied(
                        [tile.texture.width as usize, tile.texture.height as usize],
                        &tile.texture.rgba,
                    ),
                    egui::TextureOptions::LINEAR,
                );
                let radar_tile = menu::RadarTile {
                    index: tile.index,
                    texture: texture.id(),
                };
                radar_texture_handles.push(texture);
                radar_tile
            })
            .collect::<Vec<_>>();
        let mut menu = menu::Menu::new(&gui_context, Vec::new());
        menu.settings = startup_settings;
        let gpu = adapter.get_info();
        menu.graphics_device = format!("{} / {:?}", gpu.name, gpu.backend);
        println!("Renderer: {}", menu.graphics_device);
        let postprocess = postprocess::PostProcess::new(
            &device,
            format,
            menu.settings.render_size(size.width, size.height),
            [size.width.max(1), size.height.max(1)],
        );
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
            audio: None,
            gameplay_audio: gameplay_audio::GameplayAudio::default(),
            frontend_sounds: None,
            window,
            instance,
            surface,
            device,
            queue,
            format,
            size,
            postprocess,
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
            gamepad: controller::Input::default(),
            captured: false,
            last: Instant::now(),
            next_frame: Instant::now(),
            animation,
            started: Instant::now(),
            collision,
            water,
            player: None,
            walking: false,
            image_layout,
            sampler,
            streamer: None,
            uploading: None,
            retired: Vec::new(),
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
            radar_tiles,
            _radar_textures: radar_texture_handles,
            menu,
            applied_settings,
            persist_settings: true,
            quit_requested: false,
            car: None,
            car_uploads: 0,
            car_uploaded_bytes: 0,
            car_render_pose: None,
            driving: false,
            car_catalog: vec![None],
            ped_catalog: vec![None],
            spawned_peds: Vec::new(),
            npc_seconds: 0.0,
            network_session: None,
            network_car_spawned: false,
            passenger: None,
            passenger_car: None,
            ride_request: None,
            ride_sequence: 0,
            ride_reply: None,
            network_publication: None,
            network_browser: None,
            resource_game_dir: PathBuf::new(),
            resource_local_mods: None,
            resource_cache_dir: session_resources::cache_directory(),
            resource_job: None,
            resource_installing: None,
            offline_world: None,
            remote_actors: Vec::new(),
            network_revision: 0,
            network_last: Instant::now(),
            network_pose_last: Instant::now(),
            active_car: 0,
            active_ped: 0,
            ped: None,
            third_person: true,
            ped_visible: true,
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
        let upload_started = Instant::now();
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
            let raw: &[f32] = bytemuck::cast_slice(&batch.vertices);
            let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(&batch.key),
                contents: bytemuck::cast_slice(raw),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            });
            batches.push(GpuBatch {
                texture_key: batch.key.clone(),
                buffer,
                count: batch.vertices.len() as u32,
                texture: images.remove(&batch.key).context("batch texture missing")?,
                alpha: batch.alpha,
                animated: batch.animated,
                uv_animation: batch.uv_animation.clone(),
                base: if batch.animated || batch.uv_animation.is_some() {
                    raw.to_vec()
                } else {
                    Vec::new()
                },
            });
        }
        batches.sort_by_key(|b| b.alpha);
        eprintln!(
            "Synchronous GPU upload: {} batches in {:.2} ms",
            batches.len(),
            upload_started.elapsed().as_secs_f64() * 1000.0
        );
        Ok(batches)
    }
    fn loading(&self) -> bool {
        self.uploading.is_some() || self.streamer.as_ref().is_some_and(|s| s.pending())
    }
    fn stale_region(&self, region: streaming::Region) -> bool {
        let wanted_interior = self
            .interior_destination
            .map(|index| streaming::INTERIORS[index].id)
            .unwrap_or(0);
        self.destination.is_some_and(|target| {
            streaming::distance(target, region.center) >= 1.0 || wanted_interior != region.interior
        })
    }
    fn stream_world(&mut self) {
        let Some(mut streamer) = self.streamer.take() else {
            return;
        };
        while let Some(retired) = self.retired.pop() {
            if let Some(retired) = streamer.retire(retired) {
                self.retired.push(retired);
                break;
            }
        }
        if let Some((region, result)) = streamer.poll() {
            if !self.stale_region(region) {
                match result {
                    Ok(scene) => {
                        let valid_entry = if self.destination.is_some() && region.interior != 0 {
                            self.interior_destination
                                .and_then(|index| {
                                    let destination = streaming::INTERIORS[index];
                                    let point = Vec3::new(
                                        destination.position[0] - ORIGIN[0],
                                        destination.position[2],
                                        ORIGIN[1] - destination.position[1],
                                    );
                                    scene
                                        .collision
                                        .as_ref()
                                        .and_then(|world| world.standing_at(point, point.y + 1.0))
                                })
                                .is_some()
                        } else {
                            true
                        };
                        if valid_entry {
                            // This streamer keeps one immutable resource loader.
                            // Reuse textures only within that loader's world;
                            // session preparation deliberately uses Upload::new.
                            self.uploading =
                                Some((region, upload::Upload::reusing(scene, &self.batches)));
                        } else {
                            self.menu.message =
                                "Interior entrance has no safe standing position".into();
                            streamer.retry_later();
                        }
                    }
                    Err(error) => {
                        self.menu.message = format!("Map loading failed: {error}");
                        eprintln!("Keeping previous region: {error:#}");
                    }
                }
            }
        }
        if let Some((region, mut upload)) = self.uploading.take() {
            if self.stale_region(region) {
                self.retired
                    .extend(streamer.retire(streaming::Retired::Upload(Box::new(upload))));
            } else {
                match upload.advance(&self.device, &self.queue, &self.image_layout, &self.sampler) {
                    Ok(false) => self.uploading = Some((region, upload)),
                    Ok(true) => {
                        let (batches, mut scene) = upload.finish();
                        let old = streaming::Retired::Scene {
                            batches: std::mem::replace(&mut self.batches, batches),
                            collision: std::mem::replace(
                                &mut self.collision,
                                scene.collision.take(),
                            ),
                            water: std::mem::replace(&mut self.water, scene.water.take()),
                        };
                        self.retired.extend(streamer.retire(old));
                        self.region = region.center;
                        self.interior = region.interior;
                        if self
                            .destination
                            .is_some_and(|target| streaming::distance(target, region.center) < 1.0)
                        {
                            self.destination = None;
                            self.room_entry = self.interior_destination.take().map(|index| {
                                let p = streaming::INTERIORS[index].position;
                                Vec3::new(p[0] - ORIGIN[0], p[2], ORIGIN[1] - p[1])
                            });
                            self.respawn();
                        }
                        eprintln!(
                            "Installed neighbourhood at {:.0}, {:.0}",
                            region.center[0], region.center[1]
                        );
                    }
                    Err(error) => {
                        streamer.retry_later();
                        self.menu.message = format!("GPU upload failed: {error}");
                        self.retired
                            .extend(streamer.retire(streaming::Retired::Upload(Box::new(upload))));
                    }
                }
            }
        }
        if self.uploading.is_none() && self.retired.is_empty() {
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
        }
        self.streamer = Some(streamer);
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
            self.postprocess.resize(
                &self.device,
                self.menu.settings.render_size(size.width, size.height),
                [size.width.max(1), size.height.max(1)],
            );
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
            (now - self.last)
                .as_secs_f32()
                .min(if self.driving { 0.25 } else { 0.05 })
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
        // UV tracks only touch their small animated material batches. Positions,
        // static map buffers and collision meshes remain unchanged.
        for batch in &self.batches {
            let Some(track) = &batch.uv_animation else {
                continue;
            };
            let matrix = track.matrix((now - self.started).as_secs_f32());
            let mut raw = batch.base.clone();
            for vertex in raw.as_chunks_mut::<9>().0.iter_mut() {
                let uv = sa_assets::uvanim::UvAnimation::transform(matrix, [vertex[3], vertex[4]]);
                vertex[3..5].copy_from_slice(&uv);
            }
            self.queue
                .write_buffer(&batch.buffer, 0, bytemuck::cast_slice(&raw));
        }
        let previous_position = self.position;
        let forward = Vec3::new(
            self.yaw.sin() * self.pitch.cos(),
            self.pitch.sin(),
            self.yaw.cos() * self.pitch.cos(),
        );
        let right = Vec3::new(-self.yaw.cos(), 0.0, self.yaw.sin());
        let mut motion = Vec3::ZERO;
        let forward_input = (self.gamepad.move_y + f32::from(self.keys.contains(&KeyCode::KeyW))
            - f32::from(self.keys.contains(&KeyCode::KeyS)))
        .clamp(-1.0, 1.0);
        let strafe_input = (self.gamepad.move_x + f32::from(self.keys.contains(&KeyCode::KeyD))
            - f32::from(self.keys.contains(&KeyCode::KeyA)))
        .clamp(-1.0, 1.0);
        motion += (forward * forward_input + right * strafe_input).clamp_length_max(1.0);
        motion.y += f32::from(self.keys.contains(&KeyCode::KeyE))
            - f32::from(self.keys.contains(&KeyCode::KeyQ));
        motion = motion.clamp_length_max(1.0);
        let speed = if self.keys.contains(&KeyCode::ShiftLeft)
            || self.keys.contains(&KeyCode::ShiftRight)
        {
            self.menu.settings.fly_speed * 4.0
        } else {
            self.menu.settings.fly_speed
        };
        let speed = if self.gamepad.sprint {
            speed * 4.0
        } else {
            speed
        };
        if self.passenger.is_some() {
            // The host grants the seat; the interpolated remote car drives its camera.
        } else if self.driving {
            if let (Some(world), Some((car, _))) = (&self.collision, &mut self.car) {
                let previous = car.position;
                let throttle = (f32::from(self.keys.contains(&KeyCode::KeyW))
                    - f32::from(self.keys.contains(&KeyCode::KeyS))
                    + self.gamepad.throttle)
                    .clamp(-1.0, 1.0);
                let steer = controller::car_steering(
                    self.keys.contains(&KeyCode::KeyA),
                    self.keys.contains(&KeyCode::KeyD),
                    self.gamepad.move_x,
                );
                car.step(
                    world,
                    throttle,
                    steer,
                    self.keys.contains(&KeyCode::Space) || self.gamepad.handbrake,
                    self.menu.settings.vehicle_handling,
                    dt,
                );
                let center = [ORIGIN[0] + car.position.x, ORIGIN[1] - car.position.z];
                if streaming::distance(center, self.region) > RADIUS - 60.0 {
                    car.position = previous;
                    car.stop();
                }
                self.position = world.clip_camera(
                    car.position + Vec3::Y,
                    car.position - car.forward() * 7.0 + Vec3::Y * 3.5,
                );
            }
        } else if self.walking {
            if let (Some(world), Some(player)) = (&self.collision, &mut self.player) {
                let horizontal = Vec3::new(motion.x, 0.0, motion.z).clamp_length_max(1.0);
                let walking_speed = if self.gamepad.sprint
                    || self.keys.contains(&KeyCode::ShiftLeft)
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
                        self.keys.contains(&KeyCode::Space) || self.gamepad.jump,
                        dt,
                    );
                } else if self.interior != 0 {
                    player.step_in_room(
                        world,
                        horizontal * walking_speed,
                        self.keys.contains(&KeyCode::Space) || self.gamepad.jump,
                        dt,
                    );
                } else {
                    player.step(
                        world,
                        horizontal * walking_speed,
                        self.keys.contains(&KeyCode::Space) || self.gamepad.jump,
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
        self.npc_seconds += dt;
        for npc in &self.spawned_peds {
            let source = if npc.model == self.active_ped {
                self.ped.as_ref()
            } else {
                self.ped_catalog.get(npc.model).and_then(|p| p.as_ref())
            };
            if let Some((ped, _)) = source {
                if let Ok(posed) = ped.frame("idle_stance", self.npc_seconds) {
                    let rotation = Quat::from_rotation_y(npc.yaw + std::f32::consts::PI);
                    for (mesh, batch) in posed.iter().zip(&npc.batches) {
                        let mut raw = Vec::with_capacity(mesh.vertices.len() * 9);
                        for vertex in &mesh.vertices {
                            raw.extend(
                                (rotation * Vec3::from_array(vertex.position) + npc.feet)
                                    .to_array(),
                            );
                            raw.extend(vertex.uv);
                            raw.extend(vertex.color);
                        }
                        self.queue
                            .write_buffer(&batch.buffer, 0, bytemuck::cast_slice(&raw));
                    }
                }
            }
        }
        if let Some((car, batches)) = &self.car {
            let pose = sa_net::VehiclePose {
                position: car.position.to_array(),
                yaw: car.yaw,
                pitch: car.pitch,
                roll: car.roll,
                ..sa_net::VehiclePose::default()
            };
            if multiplayer::car_changed(self.car_render_pose, pose) {
                self.car_uploads += 1;
                let rotation = sa_scene::vehicle::model_rotation(car.yaw, car.pitch, car.roll);
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
                    self.car_uploaded_bytes += (raw.len() * std::mem::size_of::<f32>()) as u64;
                }
                self.car_render_pose = Some(pose);
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
                let pending = self.loading();
                self.window.set_title(&format!("SA Freeroam | {:.0} FPS | GTA {:.0}, {:.0}, {:.1} | {}{} | WASD Shift Space | P fly | R reset", fps, center[0], center[1], self.position.y, if self.driving { "Drive" } else if self.walking { "Walk" } else { "Fly" }, if pending { " | loading map..." } else { "" }));
                self.frame_count = 0;
                self.title_updated = now;
            }
        }
        self.update_network();
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
        if let Some(audio) = &mut self.audio {
            let orientation = if let Some((_, target, _)) = camera_time {
                let forward = (target - self.position).normalize_or_zero();
                Quat::from_rotation_arc(Vec3::NEG_Z, forward)
            } else {
                Quat::from_rotation_y(self.yaw) * Quat::from_rotation_x(-self.pitch)
            };
            if let Err(error) = audio.set_listener(self.position.to_array(), orientation.to_array())
            {
                eprintln!("Audio listener update failed: {error:#}");
            }
        }
        self.update_gameplay_audio(dt);
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
        let target = if let Some(car) = self.passenger_car.filter(|_| self.passenger.is_some()) {
            Vec3::from_array(car.position) + Vec3::Y
        } else if self.driving {
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
        self.ped_visible =
            view_position.distance_squared(self.position - Vec3::Y * 0.5) >= 1.5f32.powi(2);
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
        self.queue.write_buffer(
            &self.camera,
            64,
            bytemuck::cast_slice(&[
                view_position.x,
                view_position.y,
                view_position.z,
                0.0,
                if self.interior == 0 && self.menu.settings.atmospheric_fog {
                    1.0
                } else {
                    0.0
                },
                0.0,
                0.0,
                0.0,
            ]),
        );
    }
    fn respawn(&mut self) {
        self.leave_passenger();
        self.driving = false;
        self.spawned_peds.clear();
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
    fn spawn_ped(&mut self, index: usize) -> Result<()> {
        anyhow::ensure!(
            self.spawned_peds.len() < 8,
            "At most 8 spawned peds. Remove some first."
        );
        let world = self.collision.as_ref().context("No loaded ground")?;
        let side = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        let origin = if self.driving {
            self.car
                .as_ref()
                .map(|(car, _)| car.position + Vec3::Y * (1.6 - car.clearance))
                .unwrap_or(self.position)
        } else {
            self.player
                .as_ref()
                .map(|p| p.eye())
                .unwrap_or(self.position)
        };
        let forward = Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos());
        let distance = 3.0 + self.spawned_peds.len() as f32 * 0.7;
        let player = [side, -side, forward, -forward]
            .into_iter()
            .find_map(|direction| {
                world
                    .standing_at(origin + direction * distance, origin.y + 0.5)
                    .filter(|p| world.clip_camera(origin, p.eye()).distance(p.eye()) < 0.1)
            })
            .context("No safe standing space nearby; move to an open road")?;
        let source = if index == self.active_ped {
            self.ped.as_ref()
        } else {
            self.ped_catalog.get(index).and_then(|p| p.as_ref())
        }
        .context("Ped model unavailable")?;
        let rotation = Quat::from_rotation_y(self.yaw + std::f32::consts::PI);
        let batches = source
            .1
            .iter()
            .map(|batch| {
                let mut raw = batch.base.clone();
                for vertex in raw.as_chunks_mut::<9>().0.iter_mut() {
                    let point = rotation * Vec3::new(vertex[0], vertex[1], vertex[2]) + player.feet;
                    vertex[..3].copy_from_slice(&point.to_array());
                }
                let buffer = self
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("spawned ped"),
                        contents: bytemuck::cast_slice(&raw),
                        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    });
                GpuBatch {
                    texture_key: batch.texture_key.clone(),
                    buffer,
                    count: batch.count,
                    texture: batch.texture.clone(),
                    alpha: batch.alpha,
                    animated: true,
                    uv_animation: None,
                    base: Vec::new(),
                }
            })
            .collect();
        self.spawned_peds.push(SpawnedPed {
            model: index,
            feet: player.feet,
            yaw: self.yaw,
            batches,
        });
        Ok(())
    }
    fn apply_menu_action(&mut self, action: Option<menu::Action>) {
        if action.is_some() {
            self.play_menu_sound(sa_audio::MenuSound::Select);
        }
        match action {
            Some(menu::Action::Host) => self.network_action(true),
            Some(menu::Action::Join) => self.network_action(false),
            Some(menu::Action::Browse) => self.browse_network(),
            Some(menu::Action::Disconnect) => self.disconnect_network(),
            Some(menu::Action::Play) => {
                if self.menu.network_active && !self.menu.network_ready {
                    self.menu.message =
                        "Wait until the session and its resources are ready.".into();
                    return;
                }
                self.menu.has_played = true;
                self.menu.page = None;
                self.keys.clear();
                self.last = Instant::now();
                self.capture(true);
            }
            Some(menu::Action::Teleport(index)) => {
                self.leave_passenger();
                self.interior_destination = None;
                self.destination = Some(streaming::DESTINATIONS[index].1);
                self.menu.has_played = true;
                self.menu.page = None;
                self.keys.clear();
                self.capture(true);
            }
            Some(menu::Action::Interior(index)) => {
                self.leave_passenger();
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
            Some(menu::Action::Car(index)) => {
                self.leave_passenger();
                if self.interior != 0 {
                    self.menu.message = "Vehicles can be spawned outside.".into();
                    return;
                }
                if index < self.car_catalog.len() {
                    let selected = if index == self.active_car {
                        self.car.as_ref()
                    } else {
                        self.car_catalog[index].as_ref()
                    };
                    let origin = if self.driving {
                        self.car
                            .as_ref()
                            .map(|(c, _)| c.position + Vec3::Y * (1.6 - c.clearance))
                            .unwrap_or(self.position)
                    } else {
                        self.player
                            .as_ref()
                            .map(|p| p.eye())
                            .unwrap_or(self.position)
                    };
                    let placed = selected.and_then(|(c, _)| {
                        self.collision.as_ref().and_then(|world| {
                            sa_scene::vehicle::Car::spawn_near(world, origin, self.yaw, c.clearance)
                                .map(|car| car.with_handling(c.handling))
                        })
                    });
                    let Some(placed) = placed else {
                        self.menu.message =
                            "No clear supported space nearby. Move to an open road.".into();
                        return;
                    };
                    if index != self.active_car {
                        if let Some(next) = self.car_catalog[index].take() {
                            self.car_catalog[self.active_car] = self.car.replace(next);
                            self.active_car = index;
                        }
                    }
                    if let Some((car, _)) = &mut self.car {
                        *car = placed;
                    }
                    self.car_render_pose = None;
                    self.driving = true;
                    if self.driving {
                        self.menu.has_played = true;
                        self.menu.message.clear();
                        self.menu.page = None;
                        self.keys.clear();
                        self.capture(true);
                    } else {
                        self.menu.message =
                            "No clear supported space nearby. Move to an open road.".into();
                    }
                }
            }
            Some(menu::Action::SpawnPed(index)) => match self.spawn_ped(index) {
                Ok(()) => {
                    self.menu.has_played = true;
                    self.menu.message.clear();
                    self.menu.page = None;
                    self.keys.clear();
                    self.capture(true);
                }
                Err(e) => self.menu.message = e.to_string(),
            },
            Some(menu::Action::ClearPeds) => self.spawned_peds.clear(),
            Some(menu::Action::Ped(index)) => {
                if index < self.ped_catalog.len() {
                    if index != self.active_ped {
                        if let Some(next) = self.ped_catalog[index].take() {
                            self.ped_catalog[self.active_ped] = self.ped.replace(next);
                            self.active_ped = index;
                        }
                    }
                    if let Some((ped, _)) = &self.ped {
                        self.menu.clothes = ped.clothing_options();
                    }
                    self.menu.has_played = true;
                    self.menu.message.clear();
                    self.menu.page = None;
                    self.keys.clear();
                    self.capture(true);
                }
            }
            Some(menu::Action::Main) => {
                self.disconnect_network();
                self.menu.open(menu::Page::Main);
                self.capture(false);
                self.keys.clear();
            }
            None => {}
        }
    }
    fn apply_audio_settings(&mut self) {
        if let Some(audio) = &mut self.audio {
            let s = &self.menu.settings;
            if let Err(error) =
                audio.set_mix_levels(s.master_volume, s.music_volume, s.effects_volume)
            {
                self.menu.message = format!("Could not apply audio levels: {error}");
            }
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
        self.postprocess.resize(
            &self.device,
            self.menu
                .settings
                .render_size(self.size.width, self.size.height),
            [self.size.width.max(1), self.size.height.max(1)],
        );
        if self.persist_settings {
            if let Err(error) = self.menu.settings.save() {
                self.menu.message = format!("Kunne ikke lagre innstillinger: {error}");
            }
        }
        self.apply_audio_settings();
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
        let loading = self.loading();
        let context = self.gui_context.clone();
        let mut action = None;
        let menu_before = self.menu.sound_position();
        self.menu.graphics_resolution = format!(
            "{} x {} render / {} x {} display",
            self.postprocess.size[0], self.postprocess.size[1], self.size.width, self.size.height
        );
        let mut output = context.run_ui(raw_input, |ui| {
            action = self.menu.draw(
                ui.ctx(),
                coordinates,
                loading,
                self.car
                    .as_ref()
                    .filter(|_| self.driving)
                    .map(|(car, _)| car.speed),
                self.yaw,
                &self.radar_tiles,
            );
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
                    view: &self.postprocess.color,
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
                    view: &self.postprocess.depth,
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
                .chain(self.spawned_peds.iter().flat_map(|p| &p.batches))
                .chain(
                    self.remote_actors
                        .iter()
                        .filter(|a| a.visible)
                        .flat_map(|a| {
                            a.car
                                .iter()
                                .filter(|_| a.car_visible)
                                .chain(a.ped.iter().filter(|_| a.ped_visible))
                        }),
                )
                .chain(
                    self.ped
                        .iter()
                        .filter(|_| {
                            self.third_person && self.walking && !self.driving && self.ped_visible
                        })
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
        self.postprocess.draw(
            &mut encoder,
            &self.queue,
            &view,
            &self.menu.settings,
            [self.size.width, self.size.height],
        );
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
        if action.is_none() {
            self.menu_feedback(menu_before, false);
        }
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
    smoke_audio: bool,
    frontend_sounds: Option<sa_audio::FrontendSounds>,
    car_scene: Option<Scene>,
    initial_models: (String, String),
    offline_car_handling: Option<sa_scene::vehicle::Handling>,
    car_catalog: Vec<(String, Scene)>,
    ped_catalog: Vec<(String, sa_scene::ped::Ped)>,
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
    game_dir: PathBuf,
    smoke_menus: bool,
    smoke_spawner: bool,
    smoke_graphics: bool,
    smoke_stream: bool,
    smoke_car: bool,
    smoke_idle_car: bool,
    idle_car_start: Option<(u64, u64)>,
    smoke_signs: bool,
    smoke_neon: bool,
    smoke_network: bool,
    network_saw_ped: bool,
    network_saw_walk: bool,
    network_saw_car_motion: bool,
    network_saw_car: bool,
    network_captured: bool,
    network_restored: bool,
    network_exited: bool,
    network_saw_parked: bool,
    network_parked_captured: bool,
    network_far_sent: bool,
    network_saw_parked_alone: bool,
    smoke_appearance: bool,
    smoke_passenger: bool,
    passenger_stage: u8,
    passenger_moved: bool,
    passenger_start: Vec3,
    appearance_stage: u8,
    appearance_saw_ped: bool,
    appearance_saw_car: bool,
    appearance_saw_clothes: bool,
    appearance_captured: bool,
    network_players_seen: usize,
    smoke_ped: bool,
    smoke_wardrobe: bool,
    smoke_interiors: bool,
    car_start: Vec3,
    smoke_returning: bool,
    smoke_center: [f32; 2],
    gilrs: Option<gilrs::Gilrs>,
    gamepad_id: Option<gilrs::GamepadId>,
    previous_gamepad: controller::Input,
    last_gamepad_poll: Instant,
    test_tone: Option<sa_audio::SoundEffect>,
}
impl App {
    fn poll_gamepad(&mut self, event_loop: &ActiveEventLoop) {
        if self.smoke {
            return;
        }
        let Some(gilrs) = self.gilrs.as_mut() else {
            return;
        };
        while let Some(event) = gilrs.next_event() {
            match event.event {
                gilrs::EventType::Connected => {
                    self.gamepad_id = Some(event.id);
                    eprintln!("Gamepad connected: {}", gilrs.gamepad(event.id).name());
                }
                gilrs::EventType::Disconnected if self.gamepad_id == Some(event.id) => {
                    self.gamepad_id = None;
                }
                _ => {}
            }
        }
        if self.gamepad_id.is_none() {
            self.gamepad_id = gilrs.gamepads().next().map(|(id, _)| id);
        }
        let input = self
            .gamepad_id
            .map(|id| controller::read(&gilrs.gamepad(id)))
            .unwrap_or_default();
        let previous = self.previous_gamepad;
        self.previous_gamepad = input;
        let look_dt = self.last_gamepad_poll.elapsed().as_secs_f32().min(0.05);
        self.last_gamepad_poll = Instant::now();
        let Some(state) = self.state.as_mut() else {
            return;
        };
        state.gamepad = input;

        if input.pause && !previous.pause && state.streamer.is_some() {
            if state.menu.page.is_some() {
                state.menu.back();
            } else {
                state.menu.open(menu::Page::Pause);
            }
            state.capture(state.menu.page.is_none());
            state.keys.clear();
        }
        if input.map && !previous.map && state.streamer.is_some() {
            state.menu.open(menu::Page::Map);
            state.capture(false);
            state.keys.clear();
        }
        if state.menu.page.is_some() {
            let back = input.menu_back && !previous.menu_back;
            let menu_before = state.menu.sound_position();
            let action = state.menu.controller_input(
                input.menu_up && !previous.menu_up,
                input.menu_down && !previous.menu_down,
                input.menu_left && !previous.menu_left,
                input.menu_right && !previous.menu_right,
                input.menu_accept && !previous.menu_accept,
                back,
            );
            if action.is_none() {
                state.menu_feedback(menu_before, back);
            }
            if action.is_some() {
                state.apply_menu_action(action);
            } else if back {
                state.capture(state.menu.page.is_none());
                state.keys.clear();
            }
        } else if input.enter_exit && !previous.enter_exit {
            state.toggle_car();
        }
        let look_speed = 2.2 * state.menu.settings.sensitivity;
        if state.captured && state.menu.page.is_none() {
            state.yaw -= input.look_x * look_speed * look_dt;
            state.pitch = (state.pitch
                + input.look_y
                    * look_speed
                    * look_dt
                    * if state.menu.settings.invert_y {
                        -1.0
                    } else {
                        1.0
                    })
            .clamp(-1.5, 1.5);
        }
        if state.quit_requested {
            event_loop.exit();
        }
    }
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
        let sign_camera = (self.smoke_signs || self.smoke_neon).then(|| {
            let text = scene
                .batches
                .iter()
                .find(|b| {
                    if self.smoke_neon {
                        b.key.contains("|uv|7313:")
                    } else {
                        b.key == "runtime:roadsignfont"
                    }
                })
                .expect("original sign text missing");
            let quad = text
                .vertices
                .as_chunks::<6>()
                .0
                .iter()
                .min_by(|a, b| {
                    let distance =
                        |v: &sa_scene::Vertex| v.position[0].powi(2) + v.position[2].powi(2);
                    distance(&a[0]).total_cmp(&distance(&b[0]))
                })
                .expect("sign glyph missing");
            let a = Vec3::from_array(quad[0].position);
            let right = (Vec3::from_array(quad[1].position) - a).normalize();
            let up = (Vec3::from_array(quad[2].position) - Vec3::from_array(quad[1].position))
                .normalize();
            let normal = right.cross(up).normalize();
            let target = if self.smoke_neon {
                text.vertices
                    .iter()
                    .map(|v| Vec3::from_array(v.position))
                    .sum::<Vec3>()
                    / text.vertices.len() as f32
            } else {
                a + right * 1.0 + up * 0.5
            };
            (target + normal * 5.0, -normal)
        });
        let radar_tiles = match sa_scene::load_radar_tiles(&self.game_dir) {
            Ok(tiles) => Some(tiles),
            Err(error) => {
                eprintln!("Original radar unavailable: {error:#}");
                None
            }
        };
        let radar_tiles = radar_tiles.unwrap_or_default();
        match pollster::block_on(State::new(
            window.clone(),
            scene,
            self.first_model,
            radar_tiles,
        )) {
            Ok(mut state) => {
                state.persist_settings = !self.smoke;
                if self.smoke_graphics {
                    state.menu.open_graphics();
                    state.menu.settings.apply_preset(2);
                }
                state.frontend_sounds = self.frontend_sounds.take();
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
                state.menu.cars = vec![self.initial_models.0.clone()];
                state.menu.peds = vec![self.initial_models.1.clone()];
                for (name, scene) in std::mem::take(&mut self.car_catalog) {
                    let previous = state.car.take();
                    match state.install_car(scene) {
                        Ok(()) => {
                            state.car_catalog.push(state.car.take());
                            state.menu.cars.push(name);
                        }
                        Err(e) => eprintln!("Catalog upload failed: {e:#}"),
                    }
                    state.car = previous;
                }
                for (name, ped) in std::mem::take(&mut self.ped_catalog) {
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
                            state.ped_catalog.push(Some((ped, batches)));
                            state.menu.peds.push(name);
                        }
                        Err(e) => eprintln!("Catalog ped upload failed: {e:#}"),
                    }
                }
                state.driving = false;
                self.offline_car_handling = state.car.as_ref().map(|(car, _)| car.handling);
                let launch_args: Vec<_> = std::env::args().collect();
                state.resource_game_dir = self.game_dir.clone();
                state.resource_local_mods = if launch_args.iter().any(|a| a == "--no-mods") {
                    None
                } else {
                    Some(
                        launch_args
                            .windows(2)
                            .find(|a| a[0] == "--mods-dir")
                            .map(|a| PathBuf::from(&a[1]))
                            .unwrap_or_else(|| PathBuf::from("mods")),
                    )
                };
                if let Some(pair) = launch_args.windows(2).find(|a| a[0] == "--cache-dir") {
                    state.resource_cache_dir = PathBuf::from(&pair[1]);
                }
                if let Some(pair) = launch_args.windows(2).find(|a| a[0] == "--name") {
                    state.menu.player_name = pair[1].clone();
                }
                if let Some(pair) = launch_args.windows(2).find(|a| a[0] == "--relay-address") {
                    state.menu.relay_address = pair[1].clone();
                    state.menu.relay_mode = true;
                }
                state.menu.public_session = launch_args.iter().any(|a| a == "--public-session");
                if launch_args.iter().any(|a| a == "--relay-host") {
                    state.menu.relay_mode = true;
                    state.menu.open(menu::Page::Network);
                    state.network_action(true);
                } else if let Some(pair) = launch_args.windows(2).find(|a| a[0] == "--join-code") {
                    state.menu.relay_mode = true;
                    state.menu.join_code = pair[1].clone();
                    state.menu.open(menu::Page::Network);
                    state.network_action(false);
                } else if let Some(pair) = launch_args.windows(2).find(|a| a[0] == "--host") {
                    state.menu.relay_mode = false;
                    state.menu.host_address = pair[1].clone();
                    state.menu.open(menu::Page::Network);
                    state.network_action(true);
                } else if let Some(pair) = launch_args.windows(2).find(|a| a[0] == "--join") {
                    state.menu.relay_mode = false;
                    state.menu.join_address = pair[1].clone();
                    state.menu.open(menu::Page::Network);
                    state.network_action(false);
                }
                if self.smoke && !self.smoke_menus && !self.smoke_graphics {
                    state.menu.page = None;
                    state.menu.has_played = true;
                }
                if self.smoke_network
                    && launch_args
                        .iter()
                        .any(|a| a == "--join" || a == "--join-code")
                {
                    state.position.x += 6.0;
                    if let Some(player) = &mut state.player {
                        player.feet.x += 6.0;
                    }
                }
                if self.smoke_interiors {
                    state.apply_menu_action(Some(menu::Action::Interior(0)));
                }
                if self.smoke_stream {
                    state.walking = false;
                    // Use a fixed travel speed so saved user preferences do not
                    // change how many region transitions the route exercises.
                    state.menu.settings.fly_speed = 6.0;
                    state.position.y = 70.0;
                    state.pitch = 0.0;
                    state.yaw = -std::f32::consts::FRAC_PI_2;
                }
                if let Some((eye, forward)) = sign_camera {
                    // Keep the capture camera on the scene under test. Streaming
                    // transitions are exercised separately by the region tour.
                    state.streamer = None;
                    state.walking = false;
                    state.third_person = false;
                    state.position = eye;
                    state.yaw = forward.x.atan2(forward.z);
                    state.pitch = forward.y.asin();
                    self.smoke_started = Instant::now();
                }
                match sa_audio::AudioEngine::new() {
                    Ok(audio) => {
                        state.audio = Some(audio);
                        state.apply_audio_settings();
                        eprintln!("Audio output initialized (F10 plays a test tone)");
                        match sa_audio::gameplay::GameplaySounds::load(&state.resource_game_dir) {
                            Ok(sounds) => {
                                state.gameplay_audio.sounds = Some(sounds);
                                eprintln!("Original engine loops and footsteps loaded");
                            }
                            Err(error) => eprintln!("Gameplay audio unavailable: {error:#}"),
                        }
                    }
                    Err(error) => {
                        eprintln!("Audio output unavailable; continuing silently: {error:#}")
                    }
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
                match gilrs::Gilrs::new() {
                    Ok(gilrs) => {
                        self.gamepad_id = gilrs.gamepads().next().map(|(id, gamepad)| {
                            eprintln!("Gamepad connected: {}", gamepad.name());
                            id
                        });
                        self.gilrs = Some(gilrs);
                    }
                    Err(error) => eprintln!("Xbox/gamepad input unavailable: {error}"),
                }
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
                if state.menu.page.is_none()
                    && matches!(&event.logical_key,winit::keyboard::Key::Character(text) if text.as_str()=="/")
                {
                    state.menu.command = "/".into();
                    state.menu.open(menu::Page::Commands);
                    state.keys.clear();
                    state.capture(false);
                    return;
                }
                if event.physical_key == PhysicalKey::Code(KeyCode::Escape)
                    && state.streamer.is_some()
                {
                    state.play_menu_sound(sa_audio::MenuSound::Back);
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
                    && state.menu.page.is_none()
                    && state.streamer.is_some()
                {
                    state.menu.open(menu::Page::Interiors);
                    state.capture(false);
                    state.keys.clear();
                    return;
                }
                if event.physical_key == PhysicalKey::Code(KeyCode::KeyM)
                    && state.menu.page.is_none()
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
                if !self.smoke && state.menu.settings.fps_limit > 0 {
                    let now = Instant::now();
                    if now < state.next_frame {
                        event_loop.set_control_flow(ControlFlow::WaitUntil(state.next_frame));
                        return;
                    }
                    state.next_frame = now
                        + std::time::Duration::from_secs_f64(
                            1.0 / state.menu.settings.fps_limit as f64,
                        );
                    event_loop.set_control_flow(ControlFlow::WaitUntil(state.next_frame));
                } else {
                    event_loop.set_control_flow(ControlFlow::Poll);
                }

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
                        self.idle_car_start = Some((state.car_uploads, state.car_uploaded_bytes));
                    }
                    if self.smoke_frames == 270 {
                        if let Some((uploads, bytes)) = self.idle_car_start {
                            let updates = state.car_uploads - uploads;
                            let written = state.car_uploaded_bytes - bytes;
                            println!("GPU parked local car: {updates} mesh updates / {:.2} MiB over 30 frames", written as f64 / 1048576.0);
                            if self.smoke_idle_car {
                                assert_eq!(
                                    (updates, written),
                                    (0, 0),
                                    "parked car meshes were uploaded repeatedly"
                                );
                            }
                        }
                        state.toggle_car();
                    }
                }
                if self.smoke_stream {
                    if self.smoke_returning && state.position.x >= -10.0 {
                        // Finish the last replacement before counting the route's
                        // transitions; the upload now spans multiple frames.
                        state.keys.clear();
                    } else {
                        state.keys.insert(KeyCode::KeyW);
                        state.keys.insert(KeyCode::ShiftLeft);
                    }
                }
                if self.smoke
                    && !self.smoke_stream
                    && !self.smoke_car
                    && !self.smoke_ped
                    && !self.smoke_network
                    && self.smoke_frames == 3
                    && state.destination.is_none()
                    && !state.loading()
                {
                    if let Some(directory) = &self.capture_dir {
                        state.capture_next = Some(directory.join(format!(
                            "{}-{}.png",
                            if self.smoke_menus { "menu" } else { "region" },
                            self.smoke_region + 1
                        )));
                    }
                }
                if self.smoke_network {
                    state.keys.clear();
                    let seconds = self.smoke_started.elapsed().as_secs_f32();
                    if self.smoke_passenger && self.network_players_seen >= 2 {
                        let host = state.menu.player_name == "HostTest";
                        if host {
                            if seconds > 10.0 && seconds < 13.0 || seconds > 15.5 {
                                if let Some((car, _)) = &mut state.car {
                                    car.stop();
                                }
                            }
                            if (13.0..15.0).contains(&seconds) {
                                state.keys.insert(KeyCode::KeyW);
                            }
                            if state.remote_actors.iter().any(|a| a.target.ride.is_some()) {
                                self.passenger_stage = 1;
                                self.passenger_moved |=
                                    state.car.as_ref().is_some_and(|(car, _)| car.speed > 1.0);
                            }
                        } else if seconds > 10.5 && self.passenger_stage == 0 {
                            if state.driving {
                                state.toggle_car();
                            }
                            if let Some(car) =
                                state.remote_actors.iter().find_map(|a| a.target.vehicle)
                            {
                                if let Some(world) = &state.collision {
                                    let mut remote = sa_scene::vehicle::Car::new(
                                        Vec3::from_array(car.position),
                                        0.0,
                                    );
                                    remote.yaw = car.yaw;
                                    if let Some(player) = remote.exit_player(world) {
                                        state.position = player.eye();
                                        state.player = Some(player);
                                        state.walking = true;
                                        assert!(
                                            state.try_passenger(true),
                                            "could not request nearby passenger seat"
                                        );
                                        self.passenger_stage = 1;
                                    }
                                }
                            }
                        }
                        if !host && state.passenger.is_some() {
                            if self.passenger_stage == 1 {
                                self.passenger_start =
                                    Vec3::from_array(state.passenger_car.unwrap().position);
                                self.passenger_stage = 2;
                            }
                            self.passenger_moved |= state.passenger_car.is_some_and(|car| {
                                Vec3::from_array(car.position).distance(self.passenger_start) > 3.0
                            });
                            if seconds > 16.0 {
                                assert!(
                                    self.passenger_moved,
                                    "passenger did not follow the moving car"
                                );
                                state.toggle_car();
                                self.passenger_stage = 3;
                            }
                        }
                    }
                    if self.smoke_appearance && self.network_players_seen >= 2 {
                        let host = state.menu.player_name == "HostTest";
                        if self.appearance_stage == 0 && seconds > 1.5 {
                            assert!(
                                state.menu.clothes.len() >= 2,
                                "appearance smoke needs the native clothing demo"
                            );
                            state.apply_menu_action(Some(menu::Action::Clothing(
                                if host { 0 } else { 1 },
                                false,
                            )));
                            self.appearance_stage = 1;
                        } else if self.appearance_stage == 1 && seconds > 4.5 {
                            let index = state
                                .menu
                                .peds
                                .iter()
                                .rposition(|n| n == if host { "Ballas" } else { "Grove Street 2" })
                                .unwrap();
                            state.apply_menu_action(Some(menu::Action::Ped(index)));
                            self.appearance_stage = 2;
                        } else if self.appearance_stage == 2 && seconds > 6.0 {
                            state.apply_menu_action(Some(menu::Action::Ped(0)));
                            self.appearance_stage = 3;
                        } else if self.appearance_stage == 3 && seconds > 9.0 {
                            let index = state
                                .menu
                                .cars
                                .iter()
                                .rposition(|n| n == if host { "Infernus" } else { "Admiral" })
                                .unwrap();
                            let selected = if index == state.active_car {
                                state.car.as_ref()
                            } else {
                                state.car_catalog[index].as_ref()
                            };
                            let tuning =
                                selected.expect("selected original car missing").0.handling;
                            state.apply_menu_action(Some(menu::Action::Car(index)));
                            assert!(
                                state.driving,
                                "appearance smoke could not place the selected car"
                            );
                            assert_eq!(
                                state.car.as_ref().unwrap().0.handling,
                                tuning,
                                "selecting a car discarded its catalog tuning"
                            );
                            eprintln!(
                                "GPU selected vehicle tuning smoke passed: acceleration={}",
                                tuning.acceleration
                            );
                            self.appearance_stage = 4;
                        }
                    }
                    if self.network_players_seen >= 2
                        && ((3.0..4.0).contains(&seconds) || (8.0..9.0).contains(&seconds))
                    {
                        state.keys.insert(KeyCode::KeyW);
                    }
                }
                let previous_position = state.position;
                let rendered = state.render();
                if self.smoke_neon {
                    if rendered {
                        self.smoke_frames += 1;
                    }
                    if !self.smoke_returning && self.smoke_started.elapsed().as_secs_f32() > 1.2 {
                        assert!(state
                            .batches
                            .iter()
                            .any(|b| b.uv_animation.is_some() && !b.base.is_empty()));
                        if let Some(directory) = &self.capture_dir {
                            state.capture_next = Some(directory.join("neon-animated.png"));
                        }
                        self.smoke_returning = true;
                    } else if self.smoke_returning && state.capture_next.is_none() {
                        println!(
                            "GPU original UV animation rendered across two times at {:?}",
                            state.position
                        );
                        event_loop.exit();
                    }
                    state.window.request_redraw();
                    return;
                }
                if self.smoke_signs {
                    if rendered {
                        self.smoke_frames += 1;
                        if self.smoke_frames >= 4 {
                            println!("GPU original sign text rendered at {:?}", state.position);
                            event_loop.exit();
                        }
                    }
                    state.window.request_redraw();
                    return;
                }
                if state.quit_requested {
                    event_loop.exit();
                }
                if self.smoke_network {
                    if self.network_restored && rendered {
                        if self.smoke_audio {
                            assert!(
                                state.gameplay_audio.engine_starts > 0,
                                "no multiplayer engine emitter started"
                            );
                            assert!(
                                state.gameplay_audio.footsteps > 0,
                                "no multiplayer walking footsteps played"
                            );
                            println!(
                                "Gameplay audio smoke passed: {} engine emitters, {} footsteps",
                                state.gameplay_audio.engine_starts, state.gameplay_audio.footsteps
                            );
                        }
                        println!("GPU multiplayer smoke passed: remote walking ped and moving car rendered, {} players seen; offline resources restored and rendered", self.network_players_seen);
                        event_loop.exit();
                        return;
                    }
                    assert!(
                        self.smoke_started.elapsed().as_secs() < 90,
                        "multiplayer smoke timed out: {}",
                        state.menu.network_status
                    );
                    if rendered {
                        self.smoke_frames += 1;
                        // Start choreography after both clients actually joined;
                        // differing startup/relay delays must not skip walking.
                        if self.network_players_seen < 2 && state.menu.network_players.len() >= 2 {
                            self.smoke_started = Instant::now();
                            eprintln!(
                                "GPU server catalog smoke: cars={:?}, peds={:?}",
                                state.menu.cars, state.menu.peds
                            );
                        }
                        self.network_players_seen = self
                            .network_players_seen
                            .max(state.menu.network_players.len());
                        for actor in state.remote_actors.iter().filter(|a| a.visible) {
                            self.network_saw_parked |= actor.car_visible && !actor.current.driving;
                            self.network_saw_parked_alone |=
                                actor.car_visible && !actor.ped_visible && !actor.current.driving;
                            if self.smoke_appearance {
                                self.appearance_saw_ped |= actor.ped_model != 0;
                                self.appearance_saw_car |=
                                    actor.car_model != 0 && actor.current.driving;
                                self.appearance_saw_clothes |= actor.ped_model == 0
                                    && actor.current.clothes
                                        == if state.menu.player_name == "HostTest" {
                                            1
                                        } else {
                                            2
                                        };
                            }
                            if actor.current.driving {
                                self.network_saw_car = true;
                                self.network_saw_car_motion |= actor.current.speed.abs() > 0.1;
                            } else {
                                self.network_saw_ped = true;
                                self.network_saw_walk |= actor.current.moving;
                            }
                        }
                        let seconds = self.smoke_started.elapsed().as_secs_f32();
                        if self.network_players_seen >= 2
                            && (!self.smoke_appearance
                                || (self.appearance_stage == 1 && self.appearance_saw_clothes))
                            && seconds > 2.5
                            && !self.appearance_captured
                        {
                            if let Some(directory) = &self.capture_dir {
                                state.capture_next =
                                    Some(directory.join(if self.smoke_appearance {
                                        "multiplayer-outfits.png"
                                    } else {
                                        "multiplayer-walking.png"
                                    }));
                            }
                            self.appearance_captured = true;
                        }
                        if (7.0..if self.smoke_passenger { 10.0 } else { 11.0 }).contains(&seconds)
                            && !state.driving
                            && self.network_saw_ped
                        {
                            let tuning =
                                state.car.as_ref().expect("network car missing").0.handling;
                            state.place_car();
                            if state.driving {
                                assert_eq!(
                                    state.car.as_ref().unwrap().0.handling,
                                    tuning,
                                    "respawning discarded server vehicle handling"
                                );
                                eprintln!("GPU vehicle tuning smoke passed: acceleration={} brakes={} grip={}",
                                    tuning.acceleration, tuning.brake_deceleration, tuning.tire_grip);
                            }
                        }
                        if seconds > if self.smoke_passenger { 18.0 } else { 11.0 }
                            && !self.network_exited
                            && state.driving
                        {
                            state.toggle_car();
                            self.network_exited = !state.driving;
                        }
                        if seconds > if self.smoke_passenger { 19.0 } else { 12.0 }
                            && self.network_exited
                            && self.network_saw_parked
                            && !self.network_parked_captured
                        {
                            if let Some(directory) = &self.capture_dir {
                                state.capture_next = Some(directory.join("multiplayer-parked.png"));
                            }
                            self.network_parked_captured = true;
                        }
                        if seconds > if self.smoke_passenger { 20.0 } else { 13.5 }
                            && self.network_exited
                            && !self.network_far_sent
                            && state.menu.player_name == "HostTest"
                        {
                            // Leave the car nearby while its owner crosses the avatar culling range.
                            if let Some(player) = &mut state.player {
                                player.feet.x += 350.0;
                                state.position = player.eye();
                            }
                            self.network_far_sent = true;
                        }
                        if seconds > 10.0
                            && self.network_saw_car
                            && self.network_saw_ped
                            && !self.network_captured
                        {
                            if let Some(directory) = &self.capture_dir {
                                state.capture_next = Some(directory.join("multiplayer-world.png"));
                                self.network_captured = true;
                            }
                        }
                        if self.smoke_passenger && seconds > 14.5 && seconds < 15.0 {
                            if let Some(directory) = &self.capture_dir {
                                if !directory.join("multiplayer-passenger.png").exists() {
                                    state.capture_next =
                                        Some(directory.join("multiplayer-passenger.png"));
                                }
                            }
                        }
                        if self.smoke_passenger
                            && state.menu.player_name == "ClientTest"
                            && self.passenger_stage == 3
                        {
                            self.network_exited = state.passenger.is_none() && state.walking;
                        }
                        if seconds > if self.smoke_passenger { 23.0 } else { 16.0 }
                            && self.network_saw_car
                            && self.network_saw_ped
                            && self.network_saw_walk
                            && self.network_saw_car_motion
                        {
                            assert!(
                                self.network_exited && self.network_saw_parked,
                                "remote car disappeared after exit"
                            );
                            if state.menu.player_name == "ClientTest" {
                                assert!(
                                    self.network_saw_parked_alone,
                                    "parked car visibility still follows its distant owner"
                                );
                            }
                            println!("GPU parked vehicle smoke passed: remote car remains after exit and owner culling is independent");
                            if self.smoke_passenger {
                                assert!(
                                    self.passenger_moved && self.passenger_stage >= 1,
                                    "passenger ride missing"
                                );
                                if state.menu.player_name == "ClientTest" {
                                    assert_eq!(self.passenger_stage, 3);
                                    assert!(
                                        state.passenger.is_none() && state.ride_request.is_none()
                                    );
                                    assert_eq!(
                                        state.ride_reply.unwrap().result,
                                        sa_net::RideResult::Left
                                    );
                                }
                                println!("GPU passenger smoke passed: host reserved seat, passenger followed moving car and exited safely");
                            }
                            if self.smoke_appearance {
                                assert!(
                                    self.appearance_saw_ped
                                        && self.appearance_saw_car
                                        && self.appearance_saw_clothes,
                                    "appearance replication missing: ped={} car={} clothes={}",
                                    self.appearance_saw_ped,
                                    self.appearance_saw_car,
                                    self.appearance_saw_clothes
                                );
                                println!("GPU appearance smoke passed: distinct remote wardrobes, selected ped changes and selected car changes rendered");
                            }
                            state.disconnect_network();
                            assert!(
                                state.offline_world.is_none()
                                    && state.resource_installing.is_none()
                            );
                            assert_eq!(
                                state.menu.mods, self.mod_names,
                                "offline resource list was not restored"
                            );
                            assert_eq!(
                                state.car.as_ref().map(|(car, _)| car.handling),
                                self.offline_car_handling,
                                "server handling leaked into the restored offline car"
                            );
                            self.network_restored = true;
                            if let Some(directory) = &self.capture_dir {
                                state.capture_next = Some(directory.join("offline-restored.png"));
                            }
                        }
                    }
                    state.window.request_redraw();
                    return;
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
                        if self.smoke_audio {
                            assert!(
                                state.gameplay_audio.footsteps > 0,
                                "no footsteps played during walking"
                            );
                            println!(
                                "Footstep audio smoke passed: {} steps",
                                state.gameplay_audio.footsteps
                            );
                        }
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
                        if self.smoke_audio {
                            assert!(
                                state.gameplay_audio.engine_starts >= 2,
                                "engine failed to restart after re-entry"
                            );
                            println!(
                                "Engine audio smoke passed: {} starts",
                                state.gameplay_audio.engine_starts
                            );
                        }
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
                    if self.smoke_returning
                        && state.position.x >= -10.0
                        && rendered
                        && !state.loading()
                    {
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
                    if rendered && state.destination.is_none() && !state.loading() {
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
                if self.smoke_graphics && rendered {
                    self.smoke_frames += 1;
                    if self.smoke_frames >= 4 {
                        assert_eq!(
                            state.postprocess.size,
                            state
                                .menu
                                .settings
                                .render_size(state.size.width, state.size.height)
                        );
                        println!(
                            "GPU graphics mode {} passed: {}%, FSR 1 {}, {:?}",
                            self.smoke_region,
                            state.menu.settings.render_scale,
                            state.menu.settings.fsr1,
                            state.postprocess.size
                        );
                        self.smoke_frames = 0;
                        self.smoke_region += 1;
                        if self.smoke_region >= 7 {
                            println!("GPU graphics smoke passed: 7 modes, live scaling and FSR 1 EASU/RCAS");
                            event_loop.exit();
                            return;
                        }
                        state.menu.settings.apply_preset(2);
                        state.menu.settings.render_scale =
                            [100, 67, 50, 67, 150, 100, 83][self.smoke_region];
                        state.menu.settings.fsr1 = ![3, 5].contains(&self.smoke_region);
                        if self.smoke_region == 5 {
                            state.menu.settings.fxaa = false;
                            state.menu.settings.bloom = 0.0;
                            state.menu.settings.atmospheric_fog = false;
                        }
                    }
                    state.window.request_redraw();
                    return;
                }
                if self.smoke_menus && rendered {
                    self.smoke_frames += 1;
                    let mut frame_limit = 4;
                    if self.smoke_spawner {
                        if state.menu.page == Some(menu::Page::Cars) {
                            frame_limit = state.menu.cars.len() + 1;
                            let index = self.smoke_frames - 1;
                            if index < state.menu.cars.len() {
                                state.apply_menu_action(Some(menu::Action::Car(index)));
                                assert!(state.driving, "catalog car spawn failed");
                                state.menu.open(menu::Page::Cars);
                            }
                        } else if state.menu.page == Some(menu::Page::Peds) {
                            frame_limit = state.menu.peds.len() + 1;
                            let index = self.smoke_frames - 1;
                            if index < state.menu.peds.len() {
                                state.apply_menu_action(Some(menu::Action::Ped(index)));
                                assert_eq!(state.active_ped, index);
                                state.apply_menu_action(Some(menu::Action::SpawnPed(index)));
                                assert_eq!(
                                    state.spawned_peds.len(),
                                    index + 1,
                                    "ped spawn failed: {}",
                                    state.menu.message
                                );
                                state.menu.open(menu::Page::Peds);
                            }
                        }
                    }
                    if self.smoke_frames >= frame_limit {
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
                            menu::Page::Cars,
                            menu::Page::Peds,
                            menu::Page::Commands,
                            menu::Page::Network,
                            menu::Page::Quit,
                        ];
                        if self.smoke_region >= pages.len() {
                            println!("GPU menu smoke passed: 13 menus");
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
                    if rendered && state.destination.is_none() && !state.loading() {
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
                            KeyCode::F5 if !event.repeat => {
                                state.menu.open(menu::Page::Network);
                                state.keys.clear();
                                state.capture(false);
                            }
                            KeyCode::F10 if !event.repeat => {
                                if let (Some(audio), Some(tone)) =
                                    (&mut state.audio, &self.test_tone)
                                {
                                    if let Err(error) = audio.play_effect(tone) {
                                        eprintln!("Audio test tone failed: {error:#}");
                                    }
                                }
                            }
                            KeyCode::Slash if !event.repeat => {
                                state.menu.command = "/".into();
                                state.menu.open(menu::Page::Commands);
                                state.keys.clear();
                                state.capture(false);
                            }
                            KeyCode::F7 if !event.repeat => {
                                state.menu.open(menu::Page::Cars);
                                state.keys.clear();
                                state.capture(false);
                            }
                            KeyCode::F8 if !event.repeat => {
                                state.menu.open(menu::Page::Peds);
                                state.keys.clear();
                                state.capture(false);
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
                            KeyCode::KeyG if !event.repeat => {
                                state.try_passenger(true);
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
                                state.leave_passenger();
                                state.interior_destination = None;
                                state.destination = Some(streaming::DESTINATIONS[index].1);
                                state.keys.clear();
                            }
                            KeyCode::KeyR if !event.repeat => {
                                state.leave_passenger();
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
                                state.leave_passenger();
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
        self.poll_gamepad(event_loop);
        if !self.smoke {
            if let Some(state) = &self.state {
                if Instant::now() >= state.next_frame {
                    state.window.request_redraw();
                }
            }
        }
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
    if args.iter().any(|arg| arg == "--probe-audio") {
        let archive = sa_audio::archive::SfxArchive::open(&game)?;
        let mut count = 0;
        let mut samples = 0usize;
        for bank in 0..archive.bank_count().min(144) {
            let sounds = archive
                .read_bank(bank)
                .with_context(|| format!("SFX bank {bank}"))?;
            count += sounds.len();
            samples += sounds
                .iter()
                .map(|sound| sound.samples.len())
                .sum::<usize>();
        }
        sa_audio::FrontendSounds::load(&game)?;
        sa_audio::gameplay::GameplaySounds::load(&game)?;
        println!("Original SFX probe passed: {} indexed banks; {count} sounds / {samples} PCM samples checked in banks 0..143; three stereo menu cues loaded", archive.bank_count());
        return Ok(());
    }
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
                player.step_in_room(world, direction * 4.5, frame == 100, 1.0 / 60.0);
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
        loader.as_mut().unwrap().load(
            if args.iter().any(|a| a == "--smoke-neon") {
                [2000.0, 2300.0]
            } else {
                ORIGIN
            },
            ORIGIN,
            RADIUS,
        )?
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
        smoke_audio: args.iter().any(|arg| arg == "--smoke-audio"),
        frontend_sounds: match sa_audio::FrontendSounds::load(&game) {
            Ok(sounds) => {
                eprintln!("Original San Andreas frontend sounds loaded");
                Some(sounds)
            }
            Err(error) => {
                eprintln!("Original frontend sounds unavailable: {error:#}");
                None
            }
        },
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
        car_scene: if let Some(world_loader) = loader.as_mut() {
            match world_loader
                .take_custom_car()
                .map(Ok)
                .unwrap_or_else(|| world_loader.load_original_car(&game, "taxi"))
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
        initial_models: loader.as_ref().map(|l| l.model_names()).unwrap_or_default(),
        offline_car_handling: None,
        car_catalog: {
            let mut catalog = loader
                .as_mut()
                .map(|l| l.take_car_catalog())
                .unwrap_or_default();
            if let Some(world_loader) = loader.as_ref() {
                for (name, model) in world_loader.additional_original_car_models() {
                    match world_loader.load_original_car(&game, model) {
                        Ok(scene) => catalog.push((name.into(), scene)),
                        Err(e) => eprintln!("Vehicle {model} unavailable: {e:#}"),
                    }
                }
            }
            catalog
        },
        ped_catalog: {
            let mut catalog = loader
                .as_mut()
                .map(|l| l.take_ped_catalog(&game))
                .transpose()?
                .unwrap_or_default();
            if let Some(world_loader) = loader.as_ref() {
                for (name, model) in world_loader.additional_original_ped_models() {
                    match sa_scene::ped::Ped::load_model(&game, model) {
                        Ok(ped) => catalog.push((name.into(), ped)),
                        Err(e) => eprintln!("Ped {model} unavailable: {e:#}"),
                    }
                }
            }
            catalog
        },
        scene: Some(scene),
        state: None,
        first_model,
        streamer: loader.map(Streamer::new),
        smoke: args.iter().any(|a| {
            a == "--smoke-tour"
                || a == "--smoke-menus"
                || a == "--smoke-spawner"
                || a == "--smoke-graphics"
                || a == "--smoke-stream"
                || a == "--smoke-car"
                || a == "--smoke-signs"
                || a == "--smoke-neon"
                || a == "--smoke-network"
                || a == "--smoke-appearance"
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
        game_dir: game,
        test_tone: sa_audio::SoundEffect::tone(660.0, 0.18, 48_000).ok(),
        smoke_spawner: args.iter().any(|a| a == "--smoke-spawner"),
        smoke_menus: args
            .iter()
            .any(|a| a == "--smoke-menus" || a == "--smoke-spawner"),
        smoke_graphics: args.iter().any(|a| a == "--smoke-graphics"),
        smoke_stream: args.iter().any(|a| a == "--smoke-stream"),
        smoke_car: args.iter().any(|a| a == "--smoke-car"),
        smoke_idle_car: args.iter().any(|a| a == "--smoke-idle-car"),
        idle_car_start: None,
        smoke_signs: args.iter().any(|a| a == "--smoke-signs"),
        smoke_neon: args.iter().any(|a| a == "--smoke-neon"),
        smoke_network: args
            .iter()
            .any(|a| a == "--smoke-network" || a == "--smoke-appearance"),
        network_saw_ped: false,
        network_saw_walk: false,
        network_saw_car_motion: false,
        network_saw_car: false,
        network_captured: false,
        network_restored: false,
        network_exited: false,
        network_saw_parked: false,
        network_parked_captured: false,
        network_far_sent: false,
        network_saw_parked_alone: false,
        smoke_appearance: args.iter().any(|a| a == "--smoke-appearance"),
        smoke_passenger: args.iter().any(|a| a == "--smoke-passenger"),
        passenger_stage: 0,
        passenger_moved: false,
        passenger_start: Vec3::ZERO,
        appearance_stage: 0,
        appearance_saw_ped: false,
        appearance_saw_car: false,
        appearance_saw_clothes: false,
        appearance_captured: false,
        network_players_seen: 0,
        smoke_ped: args
            .iter()
            .any(|a| a == "--smoke-ped" || a == "--smoke-wardrobe"),
        smoke_wardrobe: args.iter().any(|a| a == "--smoke-wardrobe"),
        smoke_interiors: args.iter().any(|a| a == "--smoke-interiors"),
        car_start: Vec3::ZERO,
        smoke_returning: false,
        smoke_center: ORIGIN,
        gilrs: None,
        gamepad_id: None,
        previous_gamepad: controller::Input::default(),
        last_gamepad_poll: Instant::now(),
    };
    event_loop.run_app(&mut app)?;
    if let Some(error) = app.failure {
        anyhow::bail!(error);
    }
    Ok(())
}
