//! Native menus over the live world. Artwork is geometry; the title font is OFL.
use crate::{
    settings::Settings,
    streaming::{DESTINATIONS, INTERIORS},
};
use egui::{Color32, FontFamily, FontId, Pos2, Rect, RichText, Sense, Vec2};

const GOLD: Color32 = Color32::from_rgb(223, 183, 120);
const WHITE: Color32 = Color32::from_rgb(239, 233, 221);
const MUTED: Color32 = Color32::from_rgb(162, 169, 162);
const RADAR_WORLD_MIN: f32 = -3000.0;
const RADAR_WORLD_MAX: f32 = 3000.0;
const RADAR_TILE_SIZE: f32 = (RADAR_WORLD_MAX - RADAR_WORLD_MIN) / 12.0;

#[derive(Clone, Copy)]
pub struct RadarTile {
    pub index: usize,
    pub texture: egui::TextureId,
}

fn radar_offset(dx: f32, dy: f32, yaw: f32, scale: f32) -> Vec2 {
    let (sin_yaw, cos_yaw) = yaw.sin_cos();
    Vec2::new(
        (dx * cos_yaw - dy * sin_yaw) * scale,
        -(dx * sin_yaw + dy * cos_yaw) * scale,
    )
}

fn clip_radar_polygon(subject: Vec<(Pos2, Pos2)>, clip: &[Pos2]) -> Vec<(Pos2, Pos2)> {
    let mut output = subject;
    for edge_index in 0..clip.len() {
        let a = clip[edge_index];
        let b = clip[(edge_index + 1) % clip.len()];
        let edge = b - a;
        let input = std::mem::take(&mut output);
        if input.is_empty() {
            break;
        }
        let mut previous = *input.last().unwrap();
        let mut previous_distance = edge.x * (previous.0.y - a.y) - edge.y * (previous.0.x - a.x);
        for current in input {
            let current_distance = edge.x * (current.0.y - a.y) - edge.y * (current.0.x - a.x);
            if (current_distance >= 0.0) != (previous_distance >= 0.0) {
                let t = previous_distance / (previous_distance - current_distance);
                output.push((
                    previous.0 + (current.0 - previous.0) * t,
                    previous.1 + (current.1 - previous.1) * t,
                ));
            }
            if current_distance >= 0.0 {
                output.push(current);
            }
            previous = current;
            previous_distance = current_distance;
        }
    }
    output
}

fn add_radar_tile(
    painter: &egui::Painter,
    tile: RadarTile,
    rect: Rect,
    center: [f32; 2],
    yaw: f32,
    scale: f32,
    circle: &[Pos2],
) {
    let row = tile.index / 12;
    let column = tile.index % 12;
    let world_x = RADAR_WORLD_MIN + column as f32 * RADAR_TILE_SIZE;
    let world_y = RADAR_WORLD_MAX - row as f32 * RADAR_TILE_SIZE;
    let world_corners = [
        (world_x, world_y),
        (world_x + RADAR_TILE_SIZE, world_y),
        (world_x + RADAR_TILE_SIZE, world_y - RADAR_TILE_SIZE),
        (world_x, world_y - RADAR_TILE_SIZE),
    ];
    let uvs = [
        Pos2::new(0.0, 0.0),
        Pos2::new(1.0, 0.0),
        Pos2::new(1.0, 1.0),
        Pos2::new(0.0, 1.0),
    ];
    let subject = world_corners
        .into_iter()
        .zip(uvs)
        .map(|((x, y), uv)| {
            let offset = radar_offset(x - center[0], y - center[1], yaw, scale);
            (rect.center() + offset, uv)
        })
        .collect();
    let polygon = clip_radar_polygon(subject, circle);
    if polygon.len() < 3 {
        return;
    }
    let mut mesh = egui::Mesh::with_texture(tile.texture);
    for (position, uv) in polygon {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: position,
            uv,
            color: Color32::WHITE,
        });
    }
    for index in 1..mesh.vertices.len() - 1 {
        mesh.indices.extend([0, index as u32, index as u32 + 1]);
    }
    painter.add(egui::Shape::mesh(mesh));
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Page {
    Main,
    Pause,
    Map,
    Settings,
    Controls,
    Mods,
    Wardrobe,
    Interiors,
    Cars,
    Peds,
    Commands,
    Network,
    Quit,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Action {
    Play,
    Teleport(usize),
    Interior(usize),
    Quit,
    Main,
    Clothing(usize, bool),
    Car(usize),
    Ped(usize),
    SpawnPed(usize),
    ClearPeds,
    Host,
    Join,
    Browse,
    Disconnect,
}
#[derive(Clone, Copy, Debug, PartialEq, Default)]
enum SettingsTab {
    #[default]
    Display,
    Graphics,
    Gameplay,
    Interface,
    Audio,
}
impl SettingsTab {
    fn name(self) -> &'static str {
        match self {
            Self::Display => "Display",
            Self::Graphics => "Graphics",
            Self::Gameplay => "Gameplay",
            Self::Interface => "Interface",
            Self::Audio => "Audio",
        }
    }
    fn index(self) -> usize {
        match self {
            Self::Display => 0,
            Self::Graphics => 1,
            Self::Gameplay => 2,
            Self::Interface => 3,
            Self::Audio => 4,
        }
    }
}
const SETTINGS_TABS: [SettingsTab; 5] = [
    SettingsTab::Display,
    SettingsTab::Graphics,
    SettingsTab::Gameplay,
    SettingsTab::Interface,
    SettingsTab::Audio,
];
pub struct Menu {
    pub page: Option<Page>,
    pub settings: Settings,
    pub mods: Vec<String>,
    pub clothes: Vec<(String, bool)>,
    pub cars: Vec<String>,
    pub peds: Vec<String>,
    pub command: String,
    pub player_name: String,
    pub host_address: String,
    pub join_address: String,
    pub relay_mode: bool,
    pub relay_address: String,
    pub public_session: bool,
    pub join_code: String,
    pub session_code: String,
    pub server_list: Vec<sa_net::relay::Listing>,
    pub browser_status: String,
    pub network_status: String,
    pub ride_status: String,
    pub network_players: Vec<String>,
    pub network_active: bool,
    pub network_ready: bool,
    pub has_played: bool,
    pub has_checkpoint: bool,
    selected: usize,
    pub message: String,
    settings_tab: SettingsTab,
    settings_scroll_to_selection: bool,
    #[cfg(test)]
    settings_scroll_offset: f32,
    #[cfg(test)]
    settings_controls: Vec<(Rect, Rect)>,
    pub graphics_device: String,
    pub graphics_resolution: String,
}

impl Menu {
    fn setting_rows(&self) -> Vec<(&'static str, String, &'static str)> {
        let on = |value: bool| {
            if value {
                "On".to_string()
            } else {
                "Off".to_string()
            }
        };
        let s = &self.settings;
        let mut rows = vec![(
            "Category",
            self.settings_tab.name().into(),
            "Choose a settings category with left / right.",
        )];
        rows.extend(match self.settings_tab {
            SettingsTab::Display => vec![
                ("Field of view", format!("{:.0} degrees", s.fov), "Wider views show more of the world. Applies to the gameplay camera."),
                ("Window mode", if s.fullscreen { "Borderless fullscreen".into() } else { "Windowed".into() }, "Borderless fullscreen uses your desktop resolution."),
                ("VSync", on(s.vsync), "Synchronize presentation with your display to reduce tearing. No frame generation is used."),
                ("Renderer", s.renderer.name().into(), "Vulkan works on Windows and Linux. DirectX 12 is Windows-only. Requires restart. OpenGL is not supported in this build."),
                ("Frame rate limit", if s.fps_limit == 0 { "Unlimited".into() } else { format!("{} FPS", s.fps_limit) }, "Caps rendered frames to reduce power usage. VSync can impose a lower limit. Frame generation is not used."),
            ],
            SettingsTab::Graphics => vec![
                ("Quality preset", s.preset().into(), "Performance: 67% resolution. Balanced: 83%. Quality: 100%. Ultra: 150% supersampling. Controls are preserved."),
                ("Upscaler", if s.fsr1 { "AMD FSR 1".into() } else { "Linear".into() }, "AMD FSR 1 uses edge-adaptive EASU scaling and RCAS sharpening. At native resolution only RCAS is applied. This is spatial FSR, without frame generation."),
                ("Render resolution", format!("{}%", s.render_scale), "Below 100% uses the selected upscaler. Above 100% supersamples for a cleaner image. HUD and menus stay at display resolution."),
                ("Anti-aliasing", if s.fxaa { "FXAA".into() } else { "Off".into() }, "Smooths high-contrast edges in the current frame. Does not use temporal reconstruction."),
                ("Sharpness", format!("{:.0}%", s.sharpness * 100.0), "Restores fine detail after scaling. Local color bounds reduce bright halos."),
                ("Bloom", format!("{:.0}%", s.bloom * 100.0), "Soft glow around bright areas. Lower values preserve the original lighting."),
                ("Exposure", format!("{:.2}", s.exposure), "Adjusts scene brightness before filmic color mapping. Menus are unaffected."),
                ("Color saturation", format!("{:.0}%", s.saturation * 100.0), "Adjusts world color intensity. 100% preserves the source saturation."),
                ("Vignette", format!("{:.0}%", s.vignette * 100.0), "Subtle shading at the edges of the image. Set to zero for a uniform frame."),
                ("Atmospheric haze", on(s.atmospheric_fog), "Distant outdoor scenery fades into the horizon. Interiors stay clear."),
                ("Texture filtering", if s.texture_anisotropy == 1 { "Trilinear".into() } else { format!("{}x anisotropic", s.texture_anisotropy) }, "Keeps road and building textures clearer at shallow angles. Mipmaps reduce distant texture shimmer. Requires restart."),
            ],
            SettingsTab::Gameplay => vec![
                ("Look sensitivity", format!("{:.1}", s.sensitivity), "Adjusts mouse and right-stick camera movement."),
                ("Invert vertical look", on(s.invert_y), "Reverses the vertical camera direction."),
                ("Free-fly speed", format!("{:.0} m/s", s.fly_speed), "Base movement speed for the free-fly camera."),
                ("Tire grip", format!("{:.1}", s.vehicle_handling), "Higher values give stronger cornering grip. Engine power stays the same."),
            ],
            SettingsTab::Interface => vec![
                ("HUD", on(s.show_hud), "Shows gameplay information over the world."),
                ("Radar / minimap", on(s.show_minimap), "Shows the original San Andreas radar tiles. Requires HUD to be enabled."),
                ("Speedometer", on(s.show_speedometer), "Shows vehicle speed in km/h while driving. Requires HUD to be enabled."),
                ("Radar zoom", format!("{:.1}x", s.minimap_zoom), "Adjusts the area visible around the player."),
            ],
            SettingsTab::Audio => vec![
                ("Master volume", format!("{:.0}%", s.master_volume * 100.0), "Overall output level. Zero mutes all audio."),
                ("Music volume", format!("{:.0}%", s.music_volume * 100.0), "Level for the music bus. Original radio playback is not yet integrated."),
                ("Effects volume", format!("{:.0}%", s.effects_volume * 100.0), "Level for effects and original frontend menu sounds."),
            ],
        });
        rows.push((
            "Restore defaults",
            "Select".into(),
            "Resets all display, graphics, gameplay and interface preferences.",
        ));
        rows
    }
    fn adjust_setting(&mut self, left: bool) {
        let delta = if left { -1.0 } else { 1.0 };
        if self.selected == 0 {
            self.settings_tab = SETTINGS_TABS[(self.settings_tab.index()
                + if left { SETTINGS_TABS.len() - 1 } else { 1 })
                % SETTINGS_TABS.len()];
            self.settings_scroll_to_selection = true;
            return;
        }
        if self.selected == self.setting_rows().len() - 1 {
            self.settings = Settings::default();
            return;
        }
        let s = &mut self.settings;
        match (self.settings_tab, self.selected) {
            (SettingsTab::Display, 1) => s.fov += delta,
            (SettingsTab::Display, 2) => s.fullscreen = !s.fullscreen,
            (SettingsTab::Display, 3) => s.vsync = !s.vsync,
            (SettingsTab::Display, 4) => {
                let renderers = if cfg!(target_os = "windows") {
                    vec![
                        crate::settings::Renderer::Auto,
                        crate::settings::Renderer::Vulkan,
                        crate::settings::Renderer::DirectX12,
                    ]
                } else {
                    vec![
                        crate::settings::Renderer::Auto,
                        crate::settings::Renderer::Vulkan,
                    ]
                };
                let index = renderers.iter().position(|r| *r == s.renderer).unwrap_or(0);
                s.renderer = renderers
                    [(index + if left { renderers.len() - 1 } else { 1 }) % renderers.len()];
            }
            (SettingsTab::Graphics, 1) => {
                let index = match s.preset() {
                    "Performance" => 0,
                    "Balanced" => 1,
                    "Quality" => 2,
                    "Ultra" => 3,
                    _ => 2,
                };
                s.apply_preset((index + if left { 3 } else { 1 }) % 4);
            }
            (SettingsTab::Graphics, 3) => {
                s.render_scale =
                    (s.render_scale as i32 + if left { -5 } else { 5 }).clamp(50, 150) as u32
            }
            (SettingsTab::Graphics, 4) => s.fxaa = !s.fxaa,
            (SettingsTab::Graphics, 5) => s.sharpness += delta * 0.05,
            (SettingsTab::Graphics, 6) => s.bloom += delta * 0.02,
            (SettingsTab::Graphics, 7) => s.exposure += delta * 0.05,
            (SettingsTab::Graphics, 8) => s.saturation += delta * 0.05,
            (SettingsTab::Graphics, 9) => s.vignette += delta * 0.02,
            (SettingsTab::Graphics, 10) => s.atmospheric_fog = !s.atmospheric_fog,
            (SettingsTab::Graphics, 11) => {
                let values = [1, 2, 4, 8, 16];
                let index = values
                    .iter()
                    .position(|v| *v == s.texture_anisotropy)
                    .unwrap_or(3);
                s.texture_anisotropy =
                    values[(index + if left { values.len() - 1 } else { 1 }) % values.len()];
            }
            (SettingsTab::Graphics, 2) => s.fsr1 = !s.fsr1,
            (SettingsTab::Display, 5) => {
                let limits = [0, 30, 60, 90, 120, 144, 165, 240];
                let index = limits.iter().position(|v| *v == s.fps_limit).unwrap_or(0);
                s.fps_limit =
                    limits[(index + if left { limits.len() - 1 } else { 1 }) % limits.len()];
            }
            (SettingsTab::Audio, 1) => s.master_volume += delta * 0.05,
            (SettingsTab::Audio, 2) => s.music_volume += delta * 0.05,
            (SettingsTab::Audio, 3) => s.effects_volume += delta * 0.05,
            (SettingsTab::Gameplay, 1) => s.sensitivity += delta * 0.1,
            (SettingsTab::Gameplay, 2) => s.invert_y = !s.invert_y,
            (SettingsTab::Gameplay, 3) => s.fly_speed += delta,
            (SettingsTab::Gameplay, 4) => s.vehicle_handling += delta * 0.1,
            (SettingsTab::Interface, 1) => s.show_hud = !s.show_hud,
            (SettingsTab::Interface, 2) => s.show_minimap = !s.show_minimap,
            (SettingsTab::Interface, 3) => s.show_speedometer = !s.show_speedometer,
            (SettingsTab::Interface, 4) => s.minimap_zoom += delta * 0.1,
            _ => {}
        }
        self.settings.sanitize();
    }
    fn draw_settings(&mut self, ui: &mut egui::Ui, scale: f32) {
        #[cfg(test)]
        self.settings_controls.clear();
        let rows = self.setting_rows();
        let reveal = std::mem::take(&mut self.settings_scroll_to_selection);
        let total = ui.available_width();
        let height = ui.available_height().max(160.0);
        let sidebar = (155.0 * scale).min(total * 0.2);
        let list_width = (total - sidebar - 36.0 * scale) * 0.56;
        let detail_width = (total - sidebar - list_width - 36.0 * scale).max(100.0);
        ui.spacing_mut().item_spacing = Vec2::new(18.0 * scale, 10.0 * scale);
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(Vec2::new(sidebar, height), egui::Layout::top_down(egui::Align::Min), |ui| {
                for tab in SETTINGS_TABS {
                    let active = self.settings_tab == tab;
                    let text = RichText::new(tab.name()).size(18.0 * scale).color(if active { Color32::BLACK } else { WHITE });
                    if ui.add_sized([sidebar, 40.0 * scale], egui::Button::new(text).selected(active)).clicked() {
                        self.settings_tab = tab;
                        self.selected = 1;
                        self.settings_scroll_to_selection = true;
                    }
                }
                if ui.button("Back").clicked() { self.back(); }
                ui.add_space(20.0 * scale);
                ui.label(RichText::new("Left / right: adjust\nUp / down: select\nSelect Category to switch tabs").size(12.0 * scale).color(MUTED));
            });
            ui.allocate_ui_with_layout(Vec2::new(list_width, height), egui::Layout::top_down(egui::Align::Min), |ui| {
                let mut scroll = egui::ScrollArea::vertical().id_salt("settings-list").max_height(height).auto_shrink([false, false]);
                if reveal && self.selected == 0 { scroll = scroll.vertical_scroll_offset(0.0); }
                let output = scroll.show(ui, |ui| {
                    for (index, (name, value, _)) in rows.iter().enumerate().skip(1) {
                        let selected = index == self.selected;
                        let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 43.0 * scale), Sense::click());
                        if selected && reveal { response.scroll_to_me(Some(egui::Align::Center)); }
                        let fill = if selected { Color32::from_rgb(77,64,44) } else if response.hovered() { Color32::from_rgb(46,46,43) } else { Color32::from_black_alpha(150) };
                        ui.painter().rect_filled(rect, 0.0, fill);
                        ui.painter().text(rect.left_center()+Vec2::new(12.0*scale,0.0), egui::Align2::LEFT_CENTER, name, FontId::proportional(16.0*scale), if selected { GOLD } else { WHITE });
                        let control_width = (list_width * 0.48).min(245.0 * scale);
                        let arrow_width = 28.0 * scale;
                        let left = Rect::from_min_size(Pos2::new(rect.right()-control_width,rect.top()+5.0*scale),Vec2::new(arrow_width,33.0*scale));
                        let right = Rect::from_min_size(Pos2::new(rect.right()-arrow_width-5.0*scale,rect.top()+5.0*scale),Vec2::new(arrow_width,33.0*scale));
                        #[cfg(test)]
                        self.settings_controls.push((left, right));
                        ui.painter().text(Pos2::new((left.right()+right.left())*0.5,rect.center().y), egui::Align2::CENTER_CENTER, value, FontId::proportional(14.0*scale), WHITE);
                        let minus = ui.put(left, egui::Button::new("<")).clicked();
                        let plus = ui.put(right, egui::Button::new(">")).clicked();
                        if response.clicked() || minus || plus { self.selected = index; }
                        if minus || plus { self.adjust_setting(minus); }
                    }
                    if self.settings_tab == SettingsTab::Graphics {
                        ui.add_space(12.0);
                        for name in ["NVIDIA DLSS", "AMD FSR 2 / 3", "Frame generation", "Ray tracing"] {
                            ui.label(RichText::new(format!("{name}: unavailable")).size(13.0*scale).color(MUTED));
                        }
                    }
                });
                #[cfg(test)] { self.settings_scroll_offset = output.state.offset.y; }
                #[cfg(not(test))] { let _ = output; }
            });
            ui.allocate_ui_with_layout(Vec2::new(detail_width, height), egui::Layout::top_down(egui::Align::Min), |ui| {
                let description = &rows[self.selected.min(rows.len()-1)];
                ui.label(RichText::new(description.0).size(23.0*scale).color(GOLD));
                ui.label(description.2);
                ui.add_space(20.0*scale);
                ui.separator();
                ui.label(RichText::new("Your system").strong());
                ui.label(&self.graphics_device);
                ui.label(&self.graphics_resolution);
                ui.add_space(16.0*scale);
                ui.label(RichText::new("Changes are saved automatically. Renderer changes require restart.").size(14.0*scale).color(MUTED));
            });
        });
    }
    pub fn open_graphics(&mut self) {
        self.open(Page::Settings);
        self.settings_tab = SettingsTab::Graphics;
        self.selected = 1;
    }
    pub fn sound_position(&self) -> (Option<Page>, usize) {
        (self.page, self.selected)
    }
    pub fn new(ctx: &egui::Context, mods: Vec<String>) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "street".into(),
            egui::FontData::from_static(include_bytes!("../assets/UnifrakturCook-Bold.ttf")).into(),
        );
        fonts
            .families
            .insert(FontFamily::Name("street".into()), vec!["street".into()]);
        if let Ok(bytes) = std::fs::read("C:/Windows/Fonts/impact.ttf") {
            fonts
                .font_data
                .insert("menu".into(), egui::FontData::from_owned(bytes).into());
            fonts
                .families
                .insert(FontFamily::Name("menu".into()), vec!["menu".into()]);
        } else {
            fonts.families.insert(
                FontFamily::Name("menu".into()),
                fonts.families[&FontFamily::Proportional].clone(),
            );
        }
        ctx.set_fonts(fonts);
        let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
        style.visuals = egui::Visuals::dark();
        style.visuals.override_text_color = Some(WHITE);
        style.visuals.selection.bg_fill = GOLD;
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(120, 93, 47);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(77, 64, 44);
        style.spacing.item_spacing = Vec2::new(18.0, 16.0);
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(17.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, FontId::proportional(17.0));
        ctx.set_style_of(egui::Theme::Dark, style);
        ctx.set_theme(egui::Theme::Dark);
        Self {
            page: Some(Page::Main),
            settings: {
                #[cfg(test)]
                {
                    Settings::default()
                }
                #[cfg(not(test))]
                {
                    Settings::load()
                }
            },
            mods,
            clothes: Vec::new(),
            cars: Vec::new(),
            peds: Vec::new(),
            command: String::new(),
            player_name: "Player".into(),
            host_address: "0.0.0.0:7777".into(),
            join_address: "127.0.0.1:7777".into(),
            relay_mode: false,
            relay_address: std::env::var("SA_RELAY_ADDRESS")
                .unwrap_or_else(|_| "127.0.0.1:7778".into()),
            public_session: false,
            join_code: String::new(),
            session_code: String::new(),
            server_list: Vec::new(),
            browser_status: String::new(),
            network_status: "Offline".into(),
            ride_status: String::new(),
            network_players: Vec::new(),
            network_active: false,
            network_ready: false,
            has_played: false,
            has_checkpoint: false,
            selected: 0,
            message: String::new(),
            settings_tab: SettingsTab::Display,
            settings_scroll_to_selection: true,
            #[cfg(test)]
            settings_scroll_offset: 0.0,
            #[cfg(test)]
            settings_controls: Vec::new(),
            graphics_device: String::new(),
            graphics_resolution: String::new(),
        }
    }
    pub fn submit_command(&mut self) {
        match self.command.trim().to_ascii_lowercase().as_str() {
            "/cars" => self.open(Page::Cars),
            "/peds" => self.open(Page::Peds),
            "/mp" => self.open(Page::Network),
            _ => self.message = "Unknown command. Use /cars, /peds or /mp.".into(),
        }
    }
    pub fn open(&mut self, page: Page) {
        self.page = Some(page);
        self.selected = 0;
        self.settings_scroll_to_selection = true;
    }
    pub fn controller_input(
        &mut self,
        up: bool,
        down: bool,
        left: bool,
        right: bool,
        accept: bool,
        back: bool,
    ) -> Option<Action> {
        let item_count = match self.page {
            Some(Page::Main | Page::Pause) => 11,
            Some(Page::Network) => {
                if self.relay_mode {
                    5
                } else {
                    4
                }
            }
            Some(Page::Cars) => self.cars.len().max(1),
            Some(Page::Peds) => self.peds.len().max(1),
            Some(Page::Map) => DESTINATIONS.len(),
            Some(Page::Interiors) => INTERIORS.len(),
            Some(Page::Wardrobe) => self.clothes.len().max(1),
            Some(Page::Settings) => self.setting_rows().len(),
            _ => 1,
        };
        if (up || down) && self.page == Some(Page::Settings) {
            self.settings_scroll_to_selection = true;
        }
        if up {
            self.selected = (self.selected + item_count - 1) % item_count;
        }
        if down {
            self.selected = (self.selected + 1) % item_count;
        }
        if left || right {
            match self.page {
                Some(Page::Settings) => self.adjust_setting(left),
                Some(Page::Peds) if right && self.selected < self.peds.len() => {
                    return Some(Action::SpawnPed(self.selected))
                }
                Some(Page::Map) => {
                    if left {
                        self.selected =
                            (self.selected + DESTINATIONS.len() - 1) % DESTINATIONS.len();
                    } else {
                        self.selected = (self.selected + 1) % DESTINATIONS.len();
                    }
                }
                _ => {}
            }
            self.settings.sanitize();
        }
        if back {
            self.back();
            return None;
        }
        if accept {
            if self.page == Some(Page::Settings) {
                self.adjust_setting(false);
                return None;
            }
            if let Some(index) = self.clothes.get(self.selected).map(|(_, enabled)| !enabled) {
                if self.page == Some(Page::Wardrobe) {
                    return Some(Action::Clothing(self.selected, index));
                }
            }
            match self.page {
                Some(Page::Main | Page::Pause) => {
                    let index = self.selected;
                    match index {
                        0 => return Some(Action::Play),
                        1 => self.open(Page::Map),
                        2 => self.open(Page::Settings),
                        3 => self.open(Page::Controls),
                        4 => self.open(Page::Mods),
                        5 => self.open(Page::Wardrobe),
                        6 => self.open(Page::Interiors),
                        7 => self.open(Page::Cars),
                        8 => self.open(Page::Peds),
                        9 if self.page == Some(Page::Main) => self.open(Page::Quit),
                        9 => return Some(Action::Main),
                        _ => self.open(Page::Network),
                    }
                }
                Some(Page::Map) if self.selected < DESTINATIONS.len() => {
                    return Some(Action::Teleport(self.selected));
                }
                Some(Page::Interiors) if self.selected < INTERIORS.len() => {
                    return Some(Action::Interior(self.selected));
                }
                Some(Page::Cars) if self.selected < self.cars.len() => {
                    return Some(Action::Car(self.selected))
                }
                Some(Page::Peds) if self.selected < self.peds.len() => {
                    return Some(Action::Ped(self.selected))
                }
                Some(Page::Network) => {
                    return Some(match self.selected {
                        0 => Action::Host,
                        1 => Action::Join,
                        2 => Action::Disconnect,
                        4 => Action::Browse,
                        _ => Action::Play,
                    })
                }
                Some(Page::Quit) => return Some(Action::Quit),
                _ => {}
            }
        }
        None
    }
    pub fn back(&mut self) {
        match self.page {
            Some(Page::Main) => {}
            Some(Page::Pause) => self.page = None,
            _ => self.open(if self.has_played {
                Page::Pause
            } else {
                Page::Main
            }),
        }
    }
    fn nav(
        ui: &mut egui::Ui,
        rect: Rect,
        text: &str,
        selected: bool,
        id: usize,
        scale: f32,
    ) -> bool {
        let response = ui.interact(rect, egui::Id::new(("nav", id)), Sense::click());
        let active = selected || response.hovered() || response.has_focus();
        if active {
            ui.painter()
                .rect_filled(rect, 0.0, Color32::from_black_alpha(85));
            ui.painter().rect_filled(
                Rect::from_min_size(rect.min, Vec2::new(4.0, rect.height())),
                0.0,
                GOLD,
            );
        }
        ui.painter().text(
            rect.min + Vec2::new(20.0 * scale, rect.height() / 2.0),
            egui::Align2::LEFT_CENTER,
            text,
            FontId::new(28.0 * scale, FontFamily::Name("menu".into())),
            if active { GOLD } else { WHITE },
        );
        response.clicked()
    }
    pub fn draw(
        &mut self,
        ctx: &egui::Context,
        coordinates: [f32; 3],
        loading: bool,
        car_speed: Option<f32>,
        yaw: f32,
        radar_tiles: &[RadarTile],
    ) -> Option<Action> {
        let mut action = None;
        let screen = ctx.content_rect();
        let scale = (screen.height() / 900.0).clamp(0.65, 1.5);
        if self.page.is_none() {
            if self.network_active {
                egui::Area::new("multiplayer-status".into())
                    .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-22.0, 22.0))
                    .interactable(false)
                    .show(ctx, |ui| {
                        egui::Frame::new()
                            .fill(Color32::from_black_alpha(190))
                            .inner_margin(8)
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!(
                                        "{} / 20 players  |  F5 Multiplayer",
                                        self.network_players.len()
                                    ))
                                    .color(GOLD),
                                );
                                ui.label(RichText::new(&self.network_status).color(WHITE));
                                ui.label(
                                    RichText::new("G Passenger | F Enter / leave").color(WHITE),
                                );
                                if !self.ride_status.is_empty() {
                                    ui.label(RichText::new(&self.ride_status).color(GOLD));
                                }
                            });
                    });
            }
            if self.settings.show_hud {
                egui::Area::new("freeroam-hud".into())
                    .fixed_pos(Pos2::new(28.0, screen.bottom() - 76.0))
                    .interactable(false)
                    .show(ctx, |ui| {
                        ui.label(
                            RichText::new(if loading {
                                "Loading neighborhood..."
                            } else {
                                "Free Roam"
                            })
                            .size(20.0)
                            .color(WHITE)
                            .strong(),
                        );
                        ui.label(
                            RichText::new("Esc  Menu    M  Map    F7  Cars    F8  Peds")
                                .size(14.0)
                                .color(MUTED),
                        );
                    });
            }
            if self.settings.show_hud && self.settings.show_minimap {
                egui::Area::new("freeroam-minimap".into())
                    .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-28.0, -28.0))
                    .interactable(false)
                    .show(ctx, |ui| {
                        let side = (screen.height() * 0.22).clamp(150.0, 205.0);
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
                        let painter = ui.painter();
                        let radius = side * 0.49;
                        let center = [coordinates[0], coordinates[1]];
                        let map_scale = side / (2400.0 / self.settings.minimap_zoom);
                        painter.rect_filled(rect, 0.0, Color32::from_black_alpha(230));
                        let circle = (0..64)
                            .map(|index| {
                                let angle = index as f32 * std::f32::consts::TAU / 64.0;
                                rect.center() + Vec2::angled(angle) * radius
                            })
                            .collect::<Vec<_>>();
                        for tile in radar_tiles {
                            add_radar_tile(painter, *tile, rect, center, yaw, map_scale, &circle);
                        }
                        painter.circle_stroke(rect.center(), radius, egui::Stroke::new(2.0, GOLD));
                        let nearest = DESTINATIONS
                            .iter()
                            .min_by(|a, b| {
                                let distance = |xy: [f32; 2]| {
                                    (xy[0] - coordinates[0]).powi(2)
                                        + (xy[1] - coordinates[1]).powi(2)
                                };
                                distance(a.1).total_cmp(&distance(b.1))
                            })
                            .map(|destination| destination.1)
                            .unwrap_or([coordinates[0], coordinates[1]]);
                        let map_scale = side / (2400.0 / self.settings.minimap_zoom);
                        for (_, xy) in DESTINATIONS.iter() {
                            let offset = radar_offset(
                                xy[0] - coordinates[0],
                                xy[1] - coordinates[1],
                                yaw,
                                map_scale,
                            );
                            if offset.length() < side * 0.43 {
                                painter.circle_filled(rect.center() + offset, 3.0, MUTED);
                            }
                        }
                        let marker = radar_offset(
                            nearest[0] - coordinates[0],
                            nearest[1] - coordinates[1],
                            yaw,
                            map_scale,
                        );
                        if marker.length() < side * 0.40 {
                            painter.circle_filled(rect.center() + marker, 5.0, GOLD);
                        }
                        painter.circle_stroke(
                            rect.center(),
                            7.0,
                            egui::Stroke::new(2.0, Color32::BLACK),
                        );
                        painter.circle_filled(rect.center(), 5.5, WHITE);
                        painter.text(
                            rect.left_top() + Vec2::splat(12.0),
                            egui::Align2::LEFT_TOP,
                            "RADAR",
                            FontId::proportional(12.0),
                            MUTED,
                        );
                        let north = radar_offset(0.0, 1.0, yaw, side * 0.39);
                        painter.text(
                            rect.center() + north,
                            egui::Align2::CENTER_CENTER,
                            "N",
                            FontId::proportional(12.0),
                            WHITE,
                        );
                        painter.add(egui::Shape::convex_polygon(
                            vec![
                                rect.center() + Vec2::new(0.0, -9.0),
                                rect.center() + Vec2::new(7.0, 6.0),
                                rect.center() + Vec2::new(-7.0, 6.0),
                            ],
                            GOLD,
                            egui::Stroke::new(1.0, Color32::BLACK),
                        ));
                    });
            }
            if self.settings.show_hud && self.settings.show_speedometer {
                if let Some(speed) = car_speed {
                    egui::Area::new("freeroam-speedometer".into())
                        .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-250.0, -30.0))
                        .interactable(false)
                        .show(ctx, |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{:03} KM/H",
                                    (speed.abs() * 3.6).round() as u32
                                ))
                                .size(23.0)
                                .color(GOLD)
                                .strong(),
                            );
                        });
                }
            }
            return None;
        }
        if self.page == Some(Page::Settings) {
            let [up, down, left, right, enter] = ctx.input_mut(|input| {
                [
                    egui::Key::ArrowUp,
                    egui::Key::ArrowDown,
                    egui::Key::ArrowLeft,
                    egui::Key::ArrowRight,
                    egui::Key::Enter,
                ]
                .map(|key| input.consume_key(egui::Modifiers::NONE, key))
            });
            self.controller_input(up, down, left, right, enter, false);
        }
        let page = self.page.unwrap();
        egui::Area::new("freeroam-menu".into()).fixed_pos(screen.min).fade_in(false).show(ctx,|ui| {
            ui.set_min_size(screen.size());
            let painter=ui.painter().clone();
            let mut mesh=egui::Mesh::default();
            for (pos,color) in [(screen.left_top(),Color32::from_black_alpha(252)),(screen.right_top(),Color32::from_black_alpha(170)),(screen.right_bottom(),Color32::from_black_alpha(170)),(screen.left_bottom(),Color32::from_black_alpha(254))] {
                mesh.colored_vertex(pos,color);
            }
            mesh.add_triangle(0,1,2);mesh.add_triangle(0,2,3);painter.add(egui::Shape::mesh(mesh));
            let left=screen.left()+screen.width()*0.065;
            let top=screen.top()+screen.height()*0.12;
            painter.text(Pos2::new(left,top),egui::Align2::LEFT_TOP,"San Andreas",FontId::new(80.0*scale,FontFamily::Name("street".into())),WHITE);
            painter.text(Pos2::new(left+3.0,top+91.0*scale),egui::Align2::LEFT_TOP,"Freeroam",FontId::new(27.0*scale,FontFamily::Name("menu".into())),GOLD);
            let heading=match page{Page::Main=>"The whole state. Your way.",Page::Pause=>"Take a breath",Page::Map=>"Choose a destination",Page::Settings=>"Settings",Page::Controls=>"Controls",Page::Wardrobe=>"Wardrobe",Page::Interiors=>"Interiors",Page::Mods=>"Local resources",Page::Cars=>"Spawn a vehicle",Page::Peds=>"Choose your player",Page::Commands=>"Commands",Page::Network=>"Multiplayer",Page::Quit=>"Leave free roam?"};
            painter.text(Pos2::new(left,top+150.0*scale),egui::Align2::LEFT_TOP,heading,FontId::proportional(18.0*scale),MUTED);
            let footer=screen.bottom()-48.0*scale;
            painter.text(Pos2::new(left,footer),egui::Align2::LEFT_CENTER,"D-pad / arrows  Move     A / Enter  Select     B / Esc  Back",FontId::proportional(14.0*scale),MUTED);
            painter.text(Pos2::new(screen.right()-40.0*scale,footer),egui::Align2::RIGHT_CENTER,"SA Runtime  •  Free Roam",FontId::proportional(14.0*scale),MUTED);
            if matches!(page,Page::Main|Page::Pause) {
                let labels=if page==Page::Main{[if self.has_checkpoint { "Continue free roam" } else { "Explore San Andreas" },"Map & destinations","Settings","Controls","Mods","Wardrobe","Interiors","Cars","Peds","Quit","Multiplayer"]}else{["Resume","Map & destinations","Settings","Controls","Mods","Wardrobe","Interiors","Cars","Peds","Main menu","Multiplayer"]};
                if ctx.input(|i|i.key_pressed(egui::Key::ArrowDown)){self.selected=(self.selected+1)%labels.len();}
                if ctx.input(|i|i.key_pressed(egui::Key::ArrowUp)){self.selected=(self.selected+labels.len()-1)%labels.len();}
                for (index,label) in labels.iter().enumerate() {
                    let rect=Rect::from_min_size(
                        if index==10 {Pos2::new(screen.right()-screen.width()*0.28,top+217.0*scale)}
                        else {Pos2::new(left,top+(217.0+index as f32*45.0)*scale)},
                        Vec2::new(if index==10{screen.width()*0.22}else{360.0*scale},39.0*scale));
                    if Self::nav(ui,rect,label,self.selected==index,index,scale)||(self.selected==index&&ctx.input(|i|i.key_pressed(egui::Key::Enter))) {
                        match index {
                            0=>action=Some(Action::Play),1=>self.open(Page::Map),2=>self.open(Page::Settings),
                            3=>self.open(Page::Controls),4=>self.open(Page::Mods),5=>self.open(Page::Wardrobe),6=>self.open(Page::Interiors),7=>self.open(Page::Cars),8=>self.open(Page::Peds),
                            9=>if page==Page::Main{self.open(Page::Quit)}else{action=Some(Action::Main)},
                            _=>self.open(Page::Network),
                        }
                    }
                }
                let right=screen.right()-screen.width()*0.28;
                let location=DESTINATIONS.iter().min_by(|a,b| {
                    let distance=|xy:[f32;2]|(xy[0]-coordinates[0]).powi(2)+(xy[1]-coordinates[1]).powi(2);
                    distance(a.1).total_cmp(&distance(b.1))
                }).map(|d|d.0).unwrap_or("San Andreas");
                painter.text(Pos2::new(right,screen.bottom()-240.0*scale),egui::Align2::LEFT_TOP,location,FontId::new(32.0*scale,FontFamily::Name("street".into())),WHITE);
                painter.text(Pos2::new(right,screen.bottom()-189.0*scale),egui::Align2::LEFT_TOP,"No missions. Just freedom.\nWalk, explore, and build on.",FontId::proportional(17.0*scale),MUTED);
            } else {
                let start=Pos2::new(left,top+210.0*scale);
                let rect=Rect::from_min_max(start,Pos2::new(screen.right()-screen.width()*0.07,footer-36.0*scale));
                let mut child=ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
                if page == Page::Settings { self.draw_settings(&mut child, scale); } else {
                egui::ScrollArea::vertical().max_height(rect.height()).show(&mut child,|ui|{
                    match page {
                        Page::Settings=>{},
                        Page::Map=>{
                            for (index, key) in [egui::Key::Num1, egui::Key::Num2, egui::Key::Num3, egui::Key::Num4, egui::Key::Num5, egui::Key::Num6, egui::Key::Num7, egui::Key::Num8, egui::Key::Num9].into_iter().enumerate() {
                                if ctx.input(|input| input.key_pressed(key)) {
                                    action = Some(Action::Teleport(index));
                                }
                            }
                            ui.label(RichText::new("Travel to a region. The map loads before you move.").color(MUTED));
                            ui.columns(2,|columns|{
                                for (index,(name,_)) in DESTINATIONS.iter().enumerate(){
                                    if columns[0].add_sized([300.0*scale,34.0*scale],egui::Button::new(format!("{}    {}",index+1,name))).clicked(){action=Some(Action::Teleport(index));}
                                }
                                let (map,_) = columns[1].allocate_exact_size(Vec2::new(350.0*scale,350.0*scale),Sense::hover());
                                let p=columns[1].painter();p.rect_filled(map,0.0,Color32::from_rgb(27,39,43));
                                let project=|xy:[f32;2]|Pos2::new(map.left()+(xy[0]+3000.0)/6000.0*map.width(),map.bottom()-(xy[1]+3000.0)/6000.0*map.height());
                                let clip=[map.left_top(),map.right_top(),map.right_bottom(),map.left_bottom()];
                                for tile in radar_tiles { add_radar_tile(p,*tile,map,[0.0,0.0],0.0,map.width()/6000.0,&clip); }
                                if radar_tiles.is_empty() { p.text(map.center(),egui::Align2::CENTER_CENTER,"Original map unavailable",FontId::proportional(14.0*scale),MUTED); }
                                for (index,(_,xy)) in DESTINATIONS.iter().enumerate(){let point=project(*xy);p.circle_filled(point,8.0*scale,GOLD);p.text(point,egui::Align2::CENTER_CENTER,(index+1).to_string(),FontId::proportional(11.0*scale),Color32::BLACK);}
                                p.circle_stroke(project([coordinates[0],coordinates[1]]),12.0*scale,egui::Stroke::new(2.0,WHITE));
                                columns[1].label(RichText::new("Overview map • white ring marks your position").size(13.0).color(MUTED));
                            });
                        },
                        Page::Controls=>{
                            egui::Grid::new("controls").spacing([80.0*scale,17.0*scale]).show(ui,|ui|{
                                for (key,description) in [("Left stick","Move / steer"),("Right stick","Look around"),("A","Jump / select"),("X","Run"),("Y","Enter / exit car"),("RT / LT","Accelerate / brake / reverse"),("LB","Handbrake"),("Start / B","Pause / go back"),("Back","Open map"),("W A S D","Move / drive"),("Mouse","Look around"),("Shift","Run / fly faster"),("Space","Jump / handbrake in car"),("F6","Wardrobe"),("I","Interiors"),("F9","Spawn and enter a car"),("F","Enter / exit nearest car or leave passenger seat"),("G","Ride in another player's car"),("V","Toggle first / third person"),("P","Toggle walk / free-fly"),("Q / E","Fly down / up"),("1–9","Travel to map regions"),("R","Return to Grove Street"),("M","Open map"),("Esc","Pause / go back")]{ui.label(RichText::new(key).color(GOLD).strong());ui.label(description);ui.end_row();}
                            });
                        },
                        Page::Interiors=>{
                            ui.label(RichText::new("Available interiors").size(23.0).color(GOLD).strong());
                            ui.label("The interior loads before you travel. Use the map or press R to return outside.");
                            if ctx.input(|i|i.key_pressed(egui::Key::ArrowDown)){self.selected=(self.selected+1)%INTERIORS.len();}
                            if ctx.input(|i|i.key_pressed(egui::Key::ArrowUp)){self.selected=(self.selected+INTERIORS.len()-1)%INTERIORS.len();}
                            for (index,room) in INTERIORS.iter().enumerate() {
                                let text=RichText::new(room.name).color(if self.selected==index{GOLD}else{WHITE});
                                if ui.add_sized([360.0*scale,45.0*scale],egui::Button::new(text)).clicked() || (self.selected==index && ctx.input(|i|i.key_pressed(egui::Key::Enter))) {action=Some(Action::Interior(index));}
                            }
                            ui.add_space(18.0);ui.label(RichText::new("The car can be spawned outside. Door entrances and more rooms are coming later.").color(MUTED));
                        },
                        Page::Wardrobe=>{
                            ui.label(RichText::new("Your outfit").size(23.0).color(GOLD).strong());
                            if self.clothes.is_empty() { ui.label("No extra clothing is available for this character."); }
                            else {
                                ui.label("Choose which clothes to wear.");
                                if ctx.input(|i|i.key_pressed(egui::Key::ArrowDown)){self.selected=(self.selected+1)%self.clothes.len();}
                                if ctx.input(|i|i.key_pressed(egui::Key::ArrowUp)){self.selected=(self.selected+self.clothes.len()-1)%self.clothes.len();}
                                if ctx.input(|i|i.key_pressed(egui::Key::Enter)){
                                    let enabled=&mut self.clothes[self.selected].1;*enabled = !*enabled;
                                    action=Some(Action::Clothing(self.selected,*enabled));
                                }
                                for (index,(name,enabled)) in self.clothes.iter_mut().enumerate() {
                                    let text=RichText::new(name.as_str()).color(if index==self.selected{GOLD}else{WHITE});
                                    if ui.checkbox(enabled,text).changed(){self.selected=index;action=Some(Action::Clothing(index,*enabled));}
                                }
                            }
                        },
                        Page::Network=>{
                            ui.label(RichText::new("Play together").size(24.0).color(GOLD));
                            ui.label("Host from the game or join a dedicated server. Up to 20 players.");
                            ui.label(RichText::new(&self.network_status).color(GOLD));
                            ui.add_space(12.0);
                            ui.columns(2, |columns| {
                            let ui = &mut columns[0];
                            ui.label("Your name");
                            ui.add(egui::TextEdit::singleline(&mut self.player_name).char_limit(24));
                            ui.add_enabled_ui(!self.network_active, |ui| {
                                ui.checkbox(&mut self.settings.auto_mod_downloads, "Automatically download required server mods");
                                if ui.checkbox(&mut self.relay_mode, "Use relay / join code").changed() {
                                    self.server_list.clear();
                                    self.browser_status.clear();
                                }
                                if self.relay_mode {
                                    ui.label("Relay address");
                                    if ui.text_edit_singleline(&mut self.relay_address).changed() {
                                        self.server_list.clear();
                                    }
                                    ui.checkbox(&mut self.public_session, "Show my session in the server browser");
                                    ui.label("Join code");
                                    ui.add(egui::TextEdit::singleline(&mut self.join_code).char_limit(12));
                                } else {
                                    ui.label("Host address (0.0.0.0:7777 for LAN)");
                                    ui.text_edit_singleline(&mut self.host_address);
                                    ui.label("Join address (the host's IP and port)");
                                    ui.text_edit_singleline(&mut self.join_address);
                                }
                            });
                            ui.add_space(10.0);
                            if !ctx.egui_wants_keyboard_input() {
                                let count = if self.relay_mode {5} else {4};
                                if ctx.input(|i|i.key_pressed(egui::Key::ArrowDown)){self.selected=(self.selected+1)%count;}
                                if ctx.input(|i|i.key_pressed(egui::Key::ArrowUp)){self.selected=(self.selected+count-1)%count;}
                            }
                            ui.horizontal_wrapped(|ui| {
                            for (index,label,event) in [(0,"Host session",Action::Host),(1,"Join session",Action::Join),(2,"Disconnect",Action::Disconnect),(3,"Enter free roam",Action::Play)] {
                                let enabled=if index==3{self.network_ready}else if index>=2{self.network_active}else{!self.network_active};
                                if ui.add_enabled(enabled,egui::Button::new(label).selected(self.selected==index)).clicked()
                                    || (enabled && self.selected==index && !ctx.egui_wants_keyboard_input() && ctx.input(|i|i.key_pressed(egui::Key::Enter))) {
                                    action=Some(event);
                                }
                            }
                            });
                            let ui = &mut columns[1];
                            if !self.session_code.is_empty() {
                                ui.label(RichText::new(format!("Share join code: {}", self.session_code)).size(22.0).color(GOLD));
                                if ui.button("Copy join code").clicked() {ctx.copy_text(self.session_code.clone());}
                                ui.add_space(12.0);
                            }
                            if self.relay_mode {
                                ui.label(RichText::new("Server browser").size(24.0).color(GOLD));
                                if ui.add(egui::Button::new("Refresh sessions").selected(self.selected==4)).clicked()
                                    || (self.selected==4 && !ctx.egui_wants_keyboard_input() && ctx.input(|i|i.key_pressed(egui::Key::Enter))) {
                                    action=Some(Action::Browse);
                                }
                                ui.label(&self.browser_status);
                                if self.server_list.is_empty() {ui.label("Refresh to find public sessions. For a private session, use your friend's join code.");}
                                for server in &self.server_list {
                                    if ui.add_enabled(!self.network_active && server.players < server.capacity, egui::Button::new(format!("Join {} — {} / {}", server.name, server.players, server.capacity))).clicked() {
                                        self.join_code = server.code.clone();
                                        action = Some(Action::Join);
                                    }
                                }
                            }
                            ui.add_space(12.0);
                            ui.label(format!("Players: {} / 20",self.network_players.len()));
                            for name in &self.network_players{ui.label(name);}
                            ui.add_space(12.0);
                            ui.label(if self.relay_mode {"Both players connect out to the relay. The game host needs no port forwarding. A reachable relay service is required; no public relay is configured by default. Prototype: use a trusted network."} else {"For LAN, join the host's local IP. Over the internet, direct hosting requires TCP port forwarding."});
                            ui.label("Prototype: other players use the Grove Street ped and Taxi. Custom appearance, spawned NPCs and shared vehicle collisions are not synchronized yet.");
                            });
                        }
                        Page::Commands=>{
                            ui.label("/cars - vehicles    /peds - player models    /mp - multiplayer");
                            let enter=ctx.input(|i|i.key_pressed(egui::Key::Enter));
                            let response=ui.text_edit_singleline(&mut self.command);
                            if !ctx.memory(|m|m.has_focus(response.id)) {response.request_focus();}
                            if enter || ui.button("Open menu").clicked() {self.submit_command();}
                        },
                        Page::Cars | Page::Peds=>{
                            ui.label(if page==Page::Cars {"Spawn nearby and enter. One vehicle is active at a time."} else {"Use a player model or spawn a nearby ped (S / D-pad right). Spawned peds idle; they have no AI."});
                            if page==Page::Peds && ui.button("Remove spawned peds").clicked(){action=Some(Action::ClearPeds);}
                            let names=if page==Page::Cars {self.cars.clone()}else{self.peds.clone()};
                            let navigation=ctx.input(|i|(i.key_pressed(egui::Key::ArrowDown),i.key_pressed(egui::Key::ArrowUp),i.key_pressed(egui::Key::Enter)));
                            if !names.is_empty(){
                                if navigation.0 {self.selected=(self.selected+1)%names.len();}
                                if navigation.1 {self.selected=(self.selected+names.len()-1)%names.len();}
                                if navigation.2 {action=Some(if page==Page::Cars {Action::Car(self.selected)}else{Action::Ped(self.selected)});}
                            }
                            if page==Page::Peds && !names.is_empty() && ctx.input(|i|i.key_pressed(egui::Key::S)){action=Some(Action::SpawnPed(self.selected));}
                            for (index,name) in names.iter().enumerate() {
                                let text=if index==self.selected {format!("> {name}")}else{name.clone()};
                                ui.horizontal(|ui| {
                                if ui.add_sized([440.0*scale,40.0*scale],egui::Button::new(text)).clicked(){
                                    action=Some(if page==Page::Cars {Action::Car(index)}else{Action::Ped(index)});
                                }
                                if page==Page::Peds && ui.button("Spawn nearby").clicked(){action=Some(Action::SpawnPed(index));}
                                });
                            }
                            if names.is_empty(){ui.label("No models are available.");}
                        },
                        Page::Mods=>{
                            ui.label(RichText::new("Detected local resources").size(23.0).color(GOLD).strong());
                            if self.mods.is_empty(){ui.label("No mod resources were detected. Place each resource in its own folder under mods/.");}else{for name in &self.mods{ui.label(format!("•  {name}"));}}
                            ui.add_space(18.0);
                            ui.label("To enable or disable a resource, set enabled in its resource.json or mod.json, then restart.");
                            ui.label(RichText::new("Supports documented custom models, textures, buildings, cars, player characters, and skinned clothing. Scripts, DLL plugins, and arbitrary GTA/FiveM mods are not executed. Choose clothes in the wardrobe (F6).").color(MUTED));
                        },
                        Page::Quit=>{
                            ui.label("You can start free roam again from start-freeroam.cmd.");
                            ui.horizontal(|ui|{if ui.button("Quit game").clicked(){action=Some(Action::Quit);}
                                if ui.button("Stay here").clicked(){self.back();}});
                        },_=>{}
                    }
                    ui.add_space(16.0);
                    if ui.button("Back").clicked(){self.back();}
                    if !self.message.is_empty(){ui.label(RichText::new(&self.message).color(GOLD));}
                });
                }
            }
        });
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiplayer_menu_opens_and_controller_can_host_join_disconnect() {
        let ctx = egui::Context::default();
        let mut menu = Menu::new(&ctx, Vec::new());
        menu.controller_input(true, false, false, false, false, false);
        menu.controller_input(false, false, false, false, true, false);
        assert_eq!(menu.page, Some(Page::Network));
        assert_eq!(
            menu.controller_input(false, false, false, false, true, false),
            Some(Action::Host)
        );
        assert_eq!(
            menu.controller_input(false, true, false, false, true, false),
            Some(Action::Join)
        );
        assert_eq!(
            menu.controller_input(false, true, false, false, true, false),
            Some(Action::Disconnect)
        );
        menu.command = "/mp".into();
        menu.submit_command();
        assert_eq!(menu.page, Some(Page::Network));
    }
    #[test]
    fn relay_browser_is_reachable_and_direct_mode_has_no_hidden_browser_action() {
        let ctx = egui::Context::default();
        let mut menu = Menu::new(&ctx, Vec::new());
        menu.open(Page::Network);
        assert_eq!(
            menu.controller_input(true, false, false, false, true, false),
            Some(Action::Play)
        );
        menu.relay_mode = true;
        menu.open(Page::Network);
        assert_eq!(
            menu.controller_input(true, false, false, false, true, false),
            Some(Action::Browse)
        );
    }

    #[test]
    fn radar_coordinates_follow_heading_with_north_up() {
        let north = radar_offset(0.0, 100.0, 0.0, 1.0);
        assert!(north.x.abs() < 0.001);
        assert!((north.y + 100.0).abs() < 0.001);

        let east = radar_offset(100.0, 0.0, 0.0, 1.0);
        assert!((east.x - 100.0).abs() < 0.001);
        assert!(east.y.abs() < 0.001);

        let facing_east = radar_offset(100.0, 0.0, std::f32::consts::FRAC_PI_2, 1.0);
        assert!(facing_east.x.abs() < 0.001);
        assert!((facing_east.y + 100.0).abs() < 0.001);
    }

    #[test]
    fn radar_tile_indices_cover_the_expected_world_grid() {
        assert_eq!(RADAR_WORLD_MIN, -3000.0);
        assert_eq!(RADAR_WORLD_MAX, 3000.0);
        assert_eq!(RADAR_TILE_SIZE, 500.0);
        for index in [0, 11, 12, 143] {
            assert!(index / 12 < 12);
            assert!(index % 12 < 12);
        }
    }

    #[test]
    fn radar_circle_clipping_keeps_map_uvs_inside_the_circle() {
        let circle = (0..64)
            .map(|index| {
                let angle = index as f32 * std::f32::consts::TAU / 64.0;
                Pos2::new(100.0, 100.0) + Vec2::angled(angle) * 50.0
            })
            .collect::<Vec<_>>();
        let square = vec![
            (Pos2::new(40.0, 40.0), Pos2::new(0.0, 0.0)),
            (Pos2::new(160.0, 40.0), Pos2::new(1.0, 0.0)),
            (Pos2::new(160.0, 160.0), Pos2::new(1.0, 1.0)),
            (Pos2::new(40.0, 160.0), Pos2::new(0.0, 1.0)),
        ];
        let clipped = clip_radar_polygon(square, &circle);
        assert!(clipped.len() >= 8);
        for (position, uv) in clipped {
            assert!((position - Pos2::new(100.0, 100.0)).length() <= 50.1);
            assert!((0.0..=1.0).contains(&uv.x));
            assert!((0.0..=1.0).contains(&uv.y));
        }
    }

    fn frame(ctx: &egui::Context, menu: &mut Menu, keys: &[egui::Key]) -> Option<Action> {
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0))),
            focused: true,
            ..Default::default()
        };
        for key in keys {
            input.events.push(egui::Event::Key {
                key: *key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            });
        }
        let mut action = None;
        let mut output = ctx.run_ui(input, |ui| {
            action = menu.draw(ui.ctx(), [2500.0, -1670.0, 14.0], false, None, 0.0, &[]);
        });
        output.textures_delta.clear();
        action
    }
    #[test]
    fn commands_open_catalogs_and_controller_selects_models() {
        let ctx = egui::Context::default();
        let mut menu = Menu::new(&ctx, Vec::new());
        menu.cars = vec!["Taxi".into(), "Infernus".into()];
        menu.peds = vec!["Grove".into()];
        menu.command = " /CARS ".into();
        menu.submit_command();
        assert_eq!(menu.page, Some(Page::Cars));
        menu.controller_input(false, true, false, false, false, false);
        assert_eq!(
            menu.controller_input(false, false, false, false, true, false),
            Some(Action::Car(1))
        );
        menu.command = "/peds".into();
        menu.submit_command();
        assert_eq!(menu.page, Some(Page::Peds));
        assert_eq!(
            menu.controller_input(false, false, false, false, true, false),
            Some(Action::Ped(0))
        );
        assert_eq!(
            menu.controller_input(false, false, false, true, false, false),
            Some(Action::SpawnPed(0))
        );
        menu.command = "/nope".into();
        menu.submit_command();
        assert_eq!(menu.page, Some(Page::Peds));
        assert!(menu.message.contains("Unknown"));
    }
    #[test]
    fn command_enter_and_keyboard_catalog_selection_work() {
        let ctx = egui::Context::default();
        let mut menu = Menu::new(&ctx, Vec::new());
        menu.cars = vec!["Taxi".into(), "Infernus".into()];
        menu.command = "/cars".into();
        menu.open(Page::Commands);
        frame(&ctx, &mut menu, &[]);
        frame(&ctx, &mut menu, &[egui::Key::Enter]);
        assert_eq!(menu.page, Some(Page::Cars));
        frame(&ctx, &mut menu, &[]);
        frame(&ctx, &mut menu, &[egui::Key::ArrowDown]);
        frame(&ctx, &mut menu, &[]);
        assert_eq!(
            frame(&ctx, &mut menu, &[egui::Key::Enter]),
            Some(Action::Car(1))
        );
    }
    #[test]
    fn keyboard_navigation_and_back_preserve_game_state() {
        let ctx = egui::Context::default();
        let mut menu = Menu::new(&ctx, Vec::new());
        frame(&ctx, &mut menu, &[]);
        frame(&ctx, &mut menu, &[egui::Key::ArrowDown]);
        frame(&ctx, &mut menu, &[]);
        frame(&ctx, &mut menu, &[egui::Key::Enter]);
        assert_eq!(menu.page, Some(Page::Map));
        menu.back();
        assert_eq!(menu.page, Some(Page::Main));
        frame(&ctx, &mut menu, &[]);
        assert_eq!(
            frame(&ctx, &mut menu, &[egui::Key::Enter]),
            Some(Action::Play)
        );
        menu.has_played = true;
        menu.open(Page::Pause);
        menu.back();
        assert_eq!(menu.page, None);
        menu.open(Page::Settings);
        menu.back();
        assert_eq!(menu.page, Some(Page::Pause));
    }
    #[test]
    fn controller_navigation_wraps_and_selects_menu_actions() {
        let ctx = egui::Context::default();
        let mut menu = Menu::new(&ctx, Vec::new());
        assert_eq!(menu.selected, 0);
        menu.controller_input(true, false, false, false, false, false);
        assert_eq!(menu.selected, 10);
        menu.controller_input(false, true, false, false, false, false);
        assert_eq!(menu.selected, 0);
        assert_eq!(
            menu.controller_input(false, false, false, false, true, false),
            Some(Action::Play)
        );
        menu.has_played = true;
        menu.open(Page::Pause);
        menu.controller_input(false, true, false, false, false, false);
        assert_eq!(menu.selected, 1);
        assert_eq!(
            menu.controller_input(false, false, false, false, true, false),
            None
        );
        assert_eq!(menu.page, Some(Page::Map));
        assert_eq!(
            menu.controller_input(false, false, false, false, true, false),
            Some(Action::Teleport(0))
        );
        menu.controller_input(false, false, false, false, false, true);
        assert_eq!(menu.page, Some(Page::Pause));
        menu.open(Page::Settings);
        menu.controller_input(false, true, false, false, false, false);
        assert_eq!(menu.selected, 1);
        assert_eq!(
            menu.controller_input(false, false, false, false, true, false),
            None
        );
        assert_eq!(menu.page, Some(Page::Settings));
        menu.selected = 3;
        let old_vsync = menu.settings.vsync;
        menu.controller_input(false, false, false, true, false, false);
        assert_ne!(menu.settings.vsync, old_vsync);
        menu.selected = 1;
        let old_fov = menu.settings.fov;
        menu.controller_input(false, false, false, true, false, false);
        assert_eq!(menu.settings.fov, old_fov + 1.0);
    }
    #[test]
    fn controller_navigation_toggles_selected_clothing() {
        let ctx = egui::Context::default();
        let mut menu = Menu::new(&ctx, Vec::new());
        menu.clothes = vec![("Jacket".into(), true), ("Hat".into(), true)];
        menu.open(Page::Wardrobe);
        assert_eq!(
            menu.controller_input(false, false, false, false, true, false),
            Some(Action::Clothing(0, false))
        );
        menu.controller_input(false, true, false, false, false, false);
        assert_eq!(menu.selected, 1);
        assert_eq!(
            menu.controller_input(false, false, false, false, true, false),
            Some(Action::Clothing(1, false))
        );
    }
    #[test]
    fn graphics_and_audio_settings_are_reachable_with_controller_and_keyboard() {
        let ctx = egui::Context::default();
        let mut menu = Menu::new(&ctx, Vec::new());
        menu.settings = Settings::default();
        menu.open_graphics();
        menu.selected = 2;
        menu.controller_input(false, false, false, true, false, false);
        assert!(!menu.settings.fsr1);
        menu.selected = 3;
        let before = menu.settings.render_scale;
        frame(&ctx, &mut menu, &[]);
        frame(&ctx, &mut menu, &[egui::Key::ArrowLeft]);
        assert_eq!(menu.settings.render_scale, before - 5);
        menu.selected = 0;
        for _ in 0..3 {
            menu.controller_input(false, false, false, true, false, false);
        }
        assert_eq!(menu.settings_tab, SettingsTab::Audio);
        menu.selected = 1;
        menu.controller_input(false, false, true, false, false, false);
        assert_eq!(menu.settings.master_volume, 0.95);
        menu.selected = menu.setting_rows().len() - 1;
        menu.controller_input(false, false, false, false, true, false);
        assert_eq!(menu.settings, Settings::default());
    }
    #[test]
    fn mouse_arrows_adjust_both_directions_and_enter_changes_value_once() {
        let ctx = egui::Context::default();
        let mut menu = Menu::new(&ctx, Vec::new());
        menu.open_graphics();
        for _ in 0..4 {
            frame(&ctx, &mut menu, &[]);
        }
        for (left, expected) in [(true, 95), (false, 105)] {
            let controls = menu.settings_controls[2];
            let pos = if left {
                controls.0.center()
            } else {
                controls.1.center()
            };
            for pressed in [None, Some(true), Some(false)] {
                let mut input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0))),
                    focused: true,
                    ..Default::default()
                };
                input.events.push(egui::Event::PointerMoved(pos));
                if let Some(pressed) = pressed {
                    input.events.push(egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                let mut output = ctx.run_ui(input, |ui| {
                    menu.draw(ui.ctx(), [0.0; 3], false, None, 0.0, &[]);
                });
                output.textures_delta.clear();
            }
            assert_eq!(menu.settings.render_scale, expected);
            assert_eq!(menu.selected, 3);
            if left {
                frame(&ctx, &mut menu, &[egui::Key::Enter]);
                assert_eq!(menu.settings.render_scale, 100, "Enter adjusted twice");
                frame(&ctx, &mut menu, &[]);
            }
        }
    }
    #[test]
    fn mouse_wheel_scroll_does_not_snap_back_to_selected_setting() {
        let ctx = egui::Context::default();
        let mut menu = Menu::new(&ctx, Vec::new());
        menu.open_graphics();
        let mut scroll_after_wheel = 0.0;
        for frame_index in 0..40 {
            let mut input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0))),
                time: Some(frame_index as f64 / 60.0),
                focused: true,
                ..Default::default()
            };
            input
                .events
                .push(egui::Event::PointerMoved(Pos2::new(300.0, 340.0)));
            if frame_index == 3 {
                input.events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    phase: egui::TouchPhase::Move,
                    delta: Vec2::new(0.0, -160.0),
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let mut output = ctx.run_ui(input, |ui| {
                menu.draw(ui.ctx(), [0.0; 3], false, None, 0.0, &[]);
            });
            output.textures_delta.clear();
            if frame_index == 20 {
                scroll_after_wheel = menu.settings_scroll_offset;
            }
        }
        assert!(
            scroll_after_wheel > 30.0,
            "wheel did not scroll the list: {scroll_after_wheel}"
        );
        assert!(
            menu.settings_scroll_offset >= scroll_after_wheel - 1.0,
            "scroll snapped back"
        );
        assert_eq!(menu.selected, 1, "mouse wheel must not change selection");
    }
    #[test]
    fn wardrobe_keyboard_toggles_selected_clothing() {
        let ctx = egui::Context::default();
        let mut menu = Menu::new(&ctx, Vec::new());
        menu.clothes = vec![("Jacket".into(), true), ("Hat".into(), true)];
        menu.open(Page::Wardrobe);
        frame(&ctx, &mut menu, &[]);
        assert_eq!(
            frame(&ctx, &mut menu, &[egui::Key::Enter]),
            Some(Action::Clothing(0, false))
        );
        frame(&ctx, &mut menu, &[]);
        frame(&ctx, &mut menu, &[egui::Key::ArrowDown]);
        frame(&ctx, &mut menu, &[]);
        assert_eq!(
            frame(&ctx, &mut menu, &[egui::Key::Enter]),
            Some(Action::Clothing(1, false))
        );
    }
}
