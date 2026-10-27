//! Native menus over the live world. Artwork is geometry; the title font is OFL.
use crate::{
    settings::Settings,
    streaming::{DESTINATIONS, INTERIORS},
};
use egui::{Color32, FontFamily, FontId, Pos2, Rect, RichText, Sense, Vec2};

use crate::ui_theme::{self, ACCENT as GOLD, MUTED, TEXT as WHITE};
#[path = "menu/front.rs"]
mod front;
#[path = "menu/hud.rs"]
mod hud;
#[path = "menu/map.rs"]
mod map;
#[path = "menu/pause_tabs.rs"]
mod pause_tabs;
const RADAR_WORLD_MIN: f32 = -3000.0;
const RADAR_WORLD_MAX: f32 = 3000.0;
const RADAR_TILE_SIZE: f32 = (RADAR_WORLD_MAX - RADAR_WORLD_MIN) / 12.0;

#[derive(Clone, Copy)]
pub struct RadarTile {
    pub index: usize,
    pub texture: egui::TextureId,
}

/// A script blip as the radar and map draw it (SA world x, y).
#[derive(Clone, Debug, PartialEq)]
pub struct MapBlip {
    pub position: [f32; 2],
    pub color: Color32,
    pub label: Option<String>,
    pub route: bool,
    pub short_range: bool,
}
/// Script blips and the GPS line, set each frame before `Menu::draw`.
#[derive(Default)]
pub struct MapOverlay {
    pub blips: Vec<MapBlip>,
    /// From the player to the route blip; empty without one.
    pub route: Vec<[f32; 2]>,
    pub route_color: Color32,
}
/// FiveM blip colour indices (`SetBlipColour`); unknown ones are white.
pub fn blip_color(index: i64) -> Color32 {
    match index {
        1 | 6 | 59 => Color32::from_rgb(224, 50, 50),
        2 | 25 | 69 => Color32::from_rgb(114, 204, 114),
        3 | 26 | 38 => Color32::from_rgb(93, 182, 229),
        5 | 46 | 66 => Color32::from_rgb(240, 200, 80),
        7 | 27 | 83 => Color32::from_rgb(171, 104, 220),
        8 | 48 => Color32::from_rgb(240, 120, 190),
        17 | 47 | 81 => Color32::from_rgb(240, 145, 60),
        _ => Color32::from_rgb(240, 240, 240),
    }
}
/// Radar view radius in metres: wider the faster the car goes.
fn radar_range(car_speed: Option<f32>, zoom: f32) -> f32 {
    let metres = match car_speed {
        Some(speed) => 260.0 + speed.abs().min(45.0) * 9.0,
        None => 180.0,
    };
    metres / zoom.max(0.1)
}
/// The parts of a polyline (screen points) inside `area`, cut into short
/// pieces so a line leaving the map stops at its edge.
fn clip_line(points: &[Pos2], area: Rect) -> Vec<[Pos2; 2]> {
    let mut pieces = Vec::new();
    for pair in points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let steps = ((b - a).length() / 6.0).ceil().clamp(1.0, 400.0) as usize;
        for step in 0..steps {
            let from = a + (b - a) * (step as f32 / steps as f32);
            let to = a + (b - a) * ((step + 1) as f32 / steps as f32);
            if area.contains(from) && area.contains(to) {
                pieces.push([from, to]);
            }
        }
    }
    pieces
}
/// `point`, pulled back towards `origin` until it is inside `area`.
/// White HUD text with a black outline, as the original HUD draws it.
fn outlined_text(ui: &mut egui::Ui, text: &str, font: FontId, color: Color32) {
    let galley = ui
        .painter()
        .layout_no_wrap(text.into(), font, Color32::PLACEHOLDER);
    let (rect, _) = ui.allocate_exact_size(galley.size() + Vec2::splat(4.0), Sense::hover());
    let at = rect.min + Vec2::splat(2.0);
    let outline = Color32::from_black_alpha(color.a());
    for offset in [
        [-2.0, 0.0],
        [2.0, 0.0],
        [0.0, -2.0],
        [0.0, 2.0],
        [-1.5, -1.5],
        [1.5, 1.5],
        [-1.5, 1.5],
        [1.5, -1.5],
    ] {
        ui.painter().galley_with_override_text_color(
            at + Vec2::from(offset),
            galley.clone(),
            outline,
        );
    }
    ui.painter()
        .galley_with_override_text_color(at, galley, color);
}
/// Keeps the parts of a polyline inside a circle.
fn clip_line_circle(points: &[Pos2], centre: Pos2, radius: f32) -> Vec<[Pos2; 2]> {
    let mut pieces = Vec::new();
    for pair in points.windows(2) {
        let (a, d) = (pair[0] - centre, pair[1] - pair[0]);
        // Solve |a + t d| = radius for the entry and exit parameters.
        let (qa, qb, qc) = (d.dot(d), 2.0 * a.dot(d), a.dot(a) - radius * radius);
        if qa < 1e-6 {
            continue;
        }
        let disc = qb * qb - 4.0 * qa * qc;
        if disc <= 0.0 {
            continue;
        }
        let root = disc.sqrt();
        let t0 = ((-qb - root) / (2.0 * qa)).max(0.0);
        let t1 = ((-qb + root) / (2.0 * qa)).min(1.0);
        if t0 < t1 {
            pieces.push([pair[0] + d * t0, pair[0] + d * t1]);
        }
    }
    pieces
}
/// The classic circular radar's size in the bottom-left corner.
fn minimap_size(screen: Rect) -> Vec2 {
    Vec2::splat((screen.height() * 0.22).clamp(140.0, 230.0))
}
/// An outlined line: a dark outline under the colour.
fn route_line(painter: &egui::Painter, pieces: &[[Pos2; 2]], color: Color32, width: f32) {
    for (width, color) in [
        (width + 2.5, Color32::from_black_alpha(200)),
        (width, color),
    ] {
        for piece in pieces {
            painter.line_segment(*piece, egui::Stroke::new(width, color));
        }
    }
}
/// A blip: a coloured dot with a dark ring.
fn blip_dot(painter: &egui::Painter, at: Pos2, size: f32, color: Color32) {
    painter.circle_filled(at, size + 1.5, Color32::from_black_alpha(230));
    painter.circle_filled(at, size, color);
}
/// The waypoint: a purple pin.
pub const WAYPOINT: Color32 = Color32::from_rgb(164, 76, 242);
fn waypoint_pin(painter: &egui::Painter, at: Pos2, size: f32) {
    painter.add(egui::Shape::convex_polygon(
        vec![
            at + Vec2::new(0.0, size * 0.2),
            at + Vec2::new(-size * 0.7, -size),
            at + Vec2::new(size * 0.7, -size),
        ],
        WAYPOINT,
        egui::Stroke::new(1.0, Color32::BLACK),
    ));
    blip_dot(
        painter,
        at + Vec2::new(0.0, -size * 1.15),
        size * 0.75,
        WAYPOINT,
    );
}
/// The player's arrow pointing along `direction` (screen space, unit).
fn player_arrow(painter: &egui::Painter, at: Pos2, direction: Vec2, size: f32) {
    let side = Vec2::new(-direction.y, direction.x);
    painter.add(egui::Shape::convex_polygon(
        vec![
            at + direction * size,
            at - direction * size * 0.7 + side * size * 0.75,
            at - direction * size * 0.35,
            at - direction * size * 0.7 - side * size * 0.75,
        ],
        WHITE,
        egui::Stroke::new(1.5, Color32::BLACK),
    ));
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
    /// Game clock shown in the HUD, hours and minutes.
    pub clock: (u32, u32),
    /// Area name at the player, from the original zone data.
    pub zone: Option<String>,
    /// Area last announced, and when (egui time) it changed.
    zone_announced: (Option<String>, f64),
    /// Held weapon: name, rounds loaded, spare rounds, whether a gun is held, reloading.
    pub weapon_hud: Option<(String, u32, u32, bool, bool)>,
    pub ride_status: String,
    pub network_players: Vec<String>,
    pub network_active: bool,
    pub network_ready: bool,
    pub has_played: bool,
    pub has_checkpoint: bool,
    selected: usize,
    list_filter: String,
    pub message: String,
    settings_tab: SettingsTab,
    settings_scroll_to_selection: bool,
    #[cfg(test)]
    settings_scroll_offset: f32,
    #[cfg(test)]
    settings_controls: Vec<(Rect, Rect)>,
    pub graphics_device: String,
    pub graphics_resolution: String,
    pub overlay: MapOverlay,
    /// Eased radar radius in metres.
    radar_range: f32,
    /// The player's map marker; the GPS follows it when no script route is set.
    pub waypoint: Option<[f32; 2]>,
    /// Pause map centre and pixels per metre; None re-centres on the player.
    map_view: Option<([f32; 2], f32)>,
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
                ("Sun lighting", format!("{:.0}%", s.sun_lighting * 100.0), "Shades outdoor surfaces by how they face the sun, on top of the original baked lighting. Zero keeps the original look."),
                ("Sun shadows", on(s.sun_shadows), "Buildings, trees, cars and people cast shadows near you outdoors. Renders the nearby scene a second time; off in the Performance preset."),
                ("Time of day", s.fixed_hour.map_or("Day/night cycle".into(), |hour| format!("{hour:02}:00")), "The cycle runs one game minute per real second. A fixed hour stops the clock."),
                ("Texture filtering", if s.texture_anisotropy == 1 { "Trilinear".into() } else { format!("{}x anisotropic", s.texture_anisotropy) }, "Keeps road and building textures clearer at shallow angles. Mipmaps reduce distant texture shimmer. Requires restart."),
                ("Weather", s.fixed_weather.map_or("Changes by area".into(), |w| crate::settings::WEATHER_NAMES[usize::from(w)].into()), "Weather follows the part of the map you are in and changes every few game hours, using your timecyc.dat. Pick one to keep it."),
                ("Enhanced lighting", on(s.enhanced_lighting), "Rain leaves roads wet, with puddles that reflect cars, lights and buildings. Sun shadows get softer edges. Needs a strong graphics card; off by default."),
                ("Temporal anti-aliasing", on(s.temporal_aa), "Blends several slightly shifted frames for smoother edges and less shimmer on fences, wires and distant buildings. Fast-moving objects can look softer. Off by default."),
            ],
            SettingsTab::Gameplay => vec![
                ("Look sensitivity", format!("{:.1}", s.sensitivity), "Adjusts mouse and right-stick camera movement."),
                ("Invert vertical look", on(s.invert_y), "Reverses the vertical camera direction."),
                ("Free-fly speed", format!("{:.0} m/s", s.fly_speed), "Base movement speed for the free-fly camera."),
                ("Tire grip", format!("{:.1}", s.vehicle_handling), "Higher values give stronger cornering grip. Engine power stays the same."),
                ("Traffic", if s.traffic_cars == 0 { "Off".into() } else { format!("{} cars", s.traffic_cars) }, "Cars driving the city's roads around you. Fewer cars run faster on weak PCs."),
                ("Pedestrians", if s.pedestrians == 0 { "Off".into() } else { format!("{} people", s.pedestrians) }, "People walking the pavements around you. Fewer run faster on weak PCs."),
                ("Car camera recenter", on(s.car_camera_recenter), "Swings the driving camera back behind the car after you stop moving the mouse. Off keeps it where you left it."),
                ("Car physics", if s.rigid_cars { "3D (experimental)".into() } else { "Classic".into() }, "3D drives your car as a full body on four springs: it rolls down hills, slides along walls, tips over and can land on its roof. Classic is the flat model from earlier versions."),
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
            (SettingsTab::Graphics, 11) => s.sun_lighting += delta * 0.05,
            (SettingsTab::Graphics, 12) => s.sun_shadows = !s.sun_shadows,
            (SettingsTab::Graphics, 13) => {
                // Cycle, then each hour; None sits between 23:00 and 00:00.
                s.fixed_hour = match (s.fixed_hour, left) {
                    (None, false) => Some(0),
                    (None, true) => Some(23),
                    (Some(23), false) | (Some(0), true) => None,
                    (Some(hour), false) => Some(hour + 1),
                    (Some(hour), true) => Some(hour - 1),
                };
            }
            (SettingsTab::Graphics, 15) => {
                let count = crate::settings::WEATHER_NAMES.len() as u8;
                s.fixed_weather = match (s.fixed_weather, left) {
                    (None, false) => Some(0),
                    (None, true) => Some(count - 1),
                    (Some(w), false) if w + 1 == count => None,
                    (Some(0), true) => None,
                    (Some(w), false) => Some(w + 1),
                    (Some(w), true) => Some(w - 1),
                };
            }
            (SettingsTab::Graphics, 14) => {
                let values = [1, 2, 4, 8, 16];
                let index = values
                    .iter()
                    .position(|v| *v == s.texture_anisotropy)
                    .unwrap_or(3);
                s.texture_anisotropy =
                    values[(index + if left { values.len() - 1 } else { 1 }) % values.len()];
            }
            (SettingsTab::Graphics, 2) => s.fsr1 = !s.fsr1,
            (SettingsTab::Graphics, 16) => s.enhanced_lighting = !s.enhanced_lighting,
            (SettingsTab::Graphics, 17) => s.temporal_aa = !s.temporal_aa,
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
            (SettingsTab::Gameplay, 5) => {
                s.traffic_cars = if left {
                    s.traffic_cars.saturating_sub(4)
                } else {
                    s.traffic_cars + 4
                }
            }
            (SettingsTab::Gameplay, 6) => {
                s.pedestrians = if left {
                    s.pedestrians.saturating_sub(4)
                } else {
                    s.pedestrians + 4
                }
            }
            (SettingsTab::Gameplay, 7) => s.car_camera_recenter = !s.car_camera_recenter,
            (SettingsTab::Gameplay, 8) => s.rigid_cars = !s.rigid_cars,
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
        let compact = total < 800.0;
        let list_width = if compact {
            total - sidebar - 18.0 * scale
        } else {
            (total - sidebar - 36.0 * scale) * 0.56
        };
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
                if height > 310.0 {
                    ui.add_space(20.0 * scale);
                    ui.label(RichText::new("Left / right: adjust\nUp / down: select\nSelect Category to switch tabs").size(12.0).color(MUTED));
                }
            });
            ui.allocate_ui_with_layout(Vec2::new(list_width, height), egui::Layout::top_down(egui::Align::Min), |ui| {
                let mut scroll = egui::ScrollArea::vertical().id_salt("settings-list").max_height(height).auto_shrink([false, false]);
                if reveal && self.selected == 0 { scroll = scroll.vertical_scroll_offset(0.0); }
                let output = scroll.show(ui, |ui| {
                    for (index, (name, value, _)) in rows.iter().enumerate().skip(1) {
                        let selected = index == self.selected;
                        let (rect, response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 43.0 * scale), Sense::click());
                        if selected && reveal { response.scroll_to_me(Some(egui::Align::Center)); }
                        let fill = if selected { ui_theme::SURFACE_HIGH } else if response.hovered() { ui_theme::SURFACE } else { Color32::from_rgba_unmultiplied(15,21,29,215) };
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
                    if compact {
                        ui.add_space(12.0);
                        ui.collapsing("About this setting", |ui| {
                            let description = &rows[self.selected.min(rows.len()-1)];
                            self.setting_detail(ui, scale, description.0, description.2);
                        });
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
            if !compact {
                ui.allocate_ui_with_layout(Vec2::new(detail_width, height), egui::Layout::top_down(egui::Align::Min), |ui| {
                    egui::ScrollArea::vertical().id_salt("settings-detail").max_height(height).show(ui, |ui| {
                        let description = &rows[self.selected.min(rows.len()-1)];
                        self.setting_detail(ui, scale, description.0, description.2);
                    });
                });
            }
        });
    }
    fn setting_detail(&self, ui: &mut egui::Ui, scale: f32, title: &str, description: &str) {
        ui.label(RichText::new(title).size(23.0 * scale).color(GOLD));
        ui.label(description);
        ui.add_space(20.0 * scale);
        ui.separator();
        ui.label(RichText::new("Your system").strong());
        ui.label(&self.graphics_device);
        ui.label(&self.graphics_resolution);
        ui.add_space(16.0 * scale);
        ui.label(
            RichText::new("Changes are saved automatically. Renderer changes require restart.")
                .size((14.0 * scale).max(12.0))
                .color(MUTED),
        );
        if !self.message.is_empty() {
            ui.add_space(12.0 * scale);
            ui.label(RichText::new(&self.message).color(GOLD));
        }
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
            "title".into(),
            egui::FontData::from_static(include_bytes!("../assets/ArchivoBlack-Regular.ttf"))
                .into(),
        );
        fonts
            .families
            .insert(FontFamily::Name("title".into()), vec!["title".into()]);
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
        ui_theme::apply(ctx);
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
            clock: (12, 0),
            zone: None,
            zone_announced: (None, f64::NEG_INFINITY),
            weapon_hud: None,
            ride_status: String::new(),
            network_players: Vec::new(),
            network_active: false,
            network_ready: false,
            has_played: false,
            has_checkpoint: false,
            selected: 0,
            list_filter: String::new(),
            message: String::new(),
            settings_tab: SettingsTab::Display,
            settings_scroll_to_selection: true,
            #[cfg(test)]
            settings_scroll_offset: 0.0,
            #[cfg(test)]
            settings_controls: Vec::new(),
            graphics_device: String::new(),
            graphics_resolution: String::new(),
            overlay: MapOverlay::default(),
            radar_range: 0.0,
            waypoint: None,
            map_view: None,
        }
    }
    pub fn submit_command(&mut self) {
        match self.command.trim().to_ascii_lowercase().as_str() {
            "/cars" => self.open(Page::Cars),
            "/peds" => self.open(Page::Peds),
            "/mp" => self.open(Page::Network),
            "/mods" => self.open(Page::Mods),
            _ => self.message = "Unknown command. Use /cars, /peds, /mp or /mods.".into(),
        }
    }
    pub fn open(&mut self, page: Page) {
        if page == Page::Map {
            self.map_view = None;
        }
        self.page = Some(page);
        self.selected = 0;
        self.list_filter.clear();
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
            FontId::new(20.0 * scale, FontFamily::Name("menu".into())),
            if active { GOLD } else { WHITE },
        );
        response.clicked()
    }
    /// The radar: rotates with the camera, keeps the player low so more of
    /// the road ahead shows, zooms out with speed, and shows blips (on the
    /// edge when out of range), the waypoint and the GPS line.
    #[allow(clippy::too_many_arguments)]
    fn draw_minimap(
        &mut self,
        ui: &mut egui::Ui,
        size: Vec2,
        coordinates: [f32; 3],
        car_speed: Option<f32>,
        yaw: f32,
        radar_tiles: &[RadarTile],
        dt: f32,
    ) {
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let painter = ui.painter_at(rect.expand(2.0));
        // Round San Andreas-style radar centred on the player.
        let origin = rect.center();
        let radius = rect.width().min(rect.height()) * 0.5 - 3.0;
        let range = radar_range(car_speed, self.settings.minimap_zoom);
        self.radar_range = if self.radar_range > 0.0 {
            self.radar_range + (range - self.radar_range) * (1.0 - (-2.5 * dt).exp())
        } else {
            range
        };
        let map_scale = radius / self.radar_range;
        let center = [coordinates[0], coordinates[1]];
        let to_radar = |xy: [f32; 2]| {
            origin + radar_offset(xy[0] - center[0], xy[1] - center[1], yaw, map_scale)
        };
        painter.circle_filled(origin, radius, Color32::from_rgb(52, 76, 94));
        let circle: Vec<Pos2> = (0..64)
            .map(|index| {
                origin + Vec2::angled(index as f32 * std::f32::consts::TAU / 64.0) * radius
            })
            .collect();
        for tile in radar_tiles {
            add_radar_tile(&painter, *tile, rect, center, yaw, map_scale, &circle);
        }
        let route: Vec<Pos2> = self.overlay.route.iter().map(|xy| to_radar(*xy)).collect();
        route_line(
            &painter,
            &clip_line_circle(&route, origin, radius - 1.0),
            self.overlay.route_color,
            3.5,
        );
        let edge = radius - 7.0;
        let place = |point: Pos2| {
            let offset = point - origin;
            if offset.length() <= edge {
                (point, false)
            } else {
                (origin + offset.normalized() * edge, true)
            }
        };
        for blip in &self.overlay.blips {
            let (at, outside) = place(to_radar(blip.position));
            if outside && blip.short_range {
                continue;
            }
            let size = if blip.route { 6.0 } else { 4.5 };
            blip_dot(
                &painter,
                at,
                if outside { size * 0.8 } else { size },
                blip.color,
            );
        }
        if let Some(waypoint) = self.waypoint {
            let (at, _) = place(to_radar(waypoint));
            waypoint_pin(&painter, at, 8.0);
        }
        player_arrow(&painter, origin, Vec2::new(0.0, -1.0), 8.0);
        painter.circle_stroke(
            origin,
            radius + 1.5,
            egui::Stroke::new(5.0, ui_theme::BACKGROUND),
        );
        painter.circle_stroke(origin, radius + 3.0, egui::Stroke::new(1.0, GOLD));
        let north = origin + radar_offset(0.0, 1.0, yaw, 1.0).normalized() * (radius + 1.5);
        painter.circle_filled(north, 9.0, Color32::BLACK);
        painter.text(
            north,
            egui::Align2::CENTER_CENTER,
            "N",
            FontId::proportional(12.0),
            WHITE,
        );
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
        let scale = (screen.height() / 900.0).clamp(0.8, 1.5);
        if self.page.is_none() {
            if self.settings.show_hud {
                self.status_hud(ctx, scale);
                egui::Area::new("freeroam-hud".into())
                    .fixed_pos(Pos2::new(
                        28.0,
                        screen.bottom()
                            - 96.0
                            - if self.settings.show_minimap {
                                minimap_size(screen).y
                            } else {
                                0.0
                            },
                    ))
                    .interactable(false)
                    .show(ctx, |ui| {
                        if loading {
                            ui.label(
                                RichText::new("Loading neighborhood…")
                                    .size(14.0)
                                    .color(GOLD),
                            );
                        }
                        ui.horizontal(|ui| {
                            ui_theme::key_hint(ui, "Esc", "Menu");
                            ui_theme::key_hint(ui, "M", "Map");
                            if car_speed.is_some() {
                                ui_theme::key_hint(ui, "F", "Leave vehicle");
                            }
                        });
                    });
            }
            if self.settings.show_hud && self.settings.show_minimap {
                egui::Area::new("freeroam-minimap".into())
                    .anchor(egui::Align2::LEFT_BOTTOM, Vec2::new(28.0, -36.0))
                    .interactable(false)
                    .show(ctx, |ui| {
                        let dt = ui.input(|input| input.stable_dt).min(0.1);
                        self.draw_minimap(
                            ui,
                            minimap_size(screen),
                            coordinates,
                            car_speed,
                            yaw,
                            radar_tiles,
                            dt,
                        );
                    });
            }
            if self.settings.show_hud {
                let now = ctx.input(|input| input.time);
                if self.zone != self.zone_announced.0 {
                    self.zone_announced = (self.zone.clone(), now);
                }
                // San Andreas announces a new area in the bottom right, then fades it.
                let age = now - self.zone_announced.1;
                if let (Some(zone), true) = (&self.zone, age < 5.0) {
                    let alpha = (((5.0 - age) / 0.8).min(1.0) * 255.0) as u8;
                    egui::Area::new("freeroam-zone".into())
                        .anchor(
                            egui::Align2::RIGHT_BOTTOM,
                            Vec2::new(
                                -30.0,
                                -if self.settings.show_speedometer && car_speed.is_some() {
                                    132.0 * scale
                                } else {
                                    44.0 * scale
                                },
                            ),
                        )
                        .interactable(false)
                        .show(ctx, |ui| {
                            outlined_text(
                                ui,
                                zone,
                                FontId::new(40.0 * scale, FontFamily::Name("title".into())),
                                Color32::from_rgba_unmultiplied(228, 188, 119, alpha),
                            );
                        });
                    ctx.request_repaint();
                }
                if let (Some(zone), true) = (&self.zone, self.settings.show_minimap) {
                    egui::Area::new("freeroam-zone-label".into())
                        .anchor(egui::Align2::LEFT_BOTTOM, Vec2::new(28.0, -6.0))
                        .interactable(false)
                        .show(ctx, |ui| {
                            egui::Frame::new()
                                .fill(Color32::from_rgba_unmultiplied(15, 21, 29, 210))
                                .inner_margin(egui::Margin::symmetric(8, 2))
                                .show(ui, |ui| {
                                    ui.set_width(minimap_size(screen).x - 16.0);
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            RichText::new(zone).size(15.0 * scale).color(WHITE),
                                        );
                                    });
                                });
                        });
                }
            }
            if let (true, Some((_, _, _, aiming, _))) = (self.settings.show_hud, &self.weapon_hud) {
                if *aiming {
                    let center = screen.center();
                    let painter = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        "freeroam-crosshair".into(),
                    ));
                    let stroke = egui::Stroke::new(2.0 * scale, Color32::WHITE);
                    for (dx, dy) in [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
                        let near = center + Vec2::new(dx, dy) * 5.0 * scale;
                        let far = center + Vec2::new(dx, dy) * 14.0 * scale;
                        painter.line_segment([near, far], stroke);
                    }
                }
            }
            if self.settings.show_hud && self.settings.show_speedometer {
                if let Some(speed) = car_speed {
                    egui::Area::new("freeroam-speedometer".into())
                        .anchor(egui::Align2::RIGHT_BOTTOM, Vec2::new(-28.0, -30.0))
                        .interactable(false)
                        .show(ctx, |ui| {
                            egui::Frame::new()
                                .fill(Color32::from_rgba_unmultiplied(15, 21, 29, 205))
                                .corner_radius(3)
                                .inner_margin(egui::Margin::symmetric(16, 10))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(format!(
                                                "{:03}",
                                                (speed.abs() * 3.6).round() as u32
                                            ))
                                            .size(40.0 * scale)
                                            .color(WHITE)
                                            .strong(),
                                        );
                                        ui.vertical(|ui| {
                                            ui.label(
                                                RichText::new("km/h")
                                                    .size(13.0 * scale)
                                                    .color(GOLD),
                                            );
                                            ui.label(
                                                RichText::new(if speed < -0.1 {
                                                    "Reverse"
                                                } else {
                                                    "Driving"
                                                })
                                                .size(11.0 * scale)
                                                .color(MUTED),
                                            );
                                        });
                                    });
                                });
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
            let footer=screen.bottom()-48.0*scale;
            // In game the pause menu uses tabs; the title screen keeps its own layout.
            let in_game=self.has_played && page!=Page::Main;
            let gta_content=in_game.then(|| self.gta_chrome(ui,screen,scale,page,coordinates));
            if !in_game && !matches!(page, Page::Main | Page::Pause) {
            painter.text(Pos2::new(left,top),egui::Align2::LEFT_TOP,"SARE",FontId::new(52.0*scale,FontFamily::Name("title".into())),WHITE);
            let heading=match page{Page::Main=>"The whole state. Your way.",Page::Pause=>"Take a breath",Page::Map=>"Choose a destination",Page::Settings=>"Settings",Page::Controls=>"Controls",Page::Wardrobe=>"Wardrobe",Page::Interiors=>"Interiors",Page::Mods=>if self.network_active {"Server resources"} else {"Local resources"},Page::Cars=>"Spawn a vehicle",Page::Peds=>"Choose your player",Page::Commands=>"Commands",Page::Network=>"Multiplayer",Page::Quit=>"Leave free roam?"};
            painter.text(Pos2::new(left,top+68.0*scale),egui::Align2::LEFT_TOP,heading,FontId::proportional(26.0*scale),GOLD);
            painter.text(Pos2::new(left,footer),egui::Align2::LEFT_CENTER,"D-pad / arrows  Move     A / Enter  Select     B / Esc  Back",FontId::proportional(14.0*scale),MUTED);
            painter.text(Pos2::new(screen.right()-40.0*scale,footer),egui::Align2::RIGHT_CENTER,"SARE",FontId::proportional(14.0*scale),MUTED);
            }
            if let (Some(content), Page::Pause) = (gta_content, page) {
                if let Some(chosen) = self.gta_pause(ui, content, scale) { action = Some(chosen); }
            } else if let (Some(content), Page::Map) = (gta_content, page) {
                if let Some(chosen) = self.gta_map(ui, content, scale, coordinates, yaw, radar_tiles) { action = Some(chosen); }
            } else if matches!(page,Page::Main|Page::Pause) {
                action = self.front_menu(ui, screen, scale, coordinates, page);
            } else {
                let start=Pos2::new(left,top+116.0*scale);
                let rect=gta_content.unwrap_or_else(|| Rect::from_min_max(start,Pos2::new(screen.right()-screen.width()*0.07,footer-36.0*scale)));
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
                                let route:Vec<Pos2>=self.overlay.route.iter().map(|xy|project(*xy)).collect();
                                if route.len()>=2 {
                                    p.add(egui::Shape::line(route.clone(),egui::Stroke::new(5.0*scale,Color32::from_black_alpha(200))));
                                    p.add(egui::Shape::line(route,egui::Stroke::new(3.0*scale,self.overlay.route_color)));
                                }
                                for blip in &self.overlay.blips {
                                    let point=project(blip.position);
                                    p.circle_filled(point,7.0*scale,Color32::BLACK);p.circle_filled(point,5.5*scale,blip.color);
                                    if let Some(label)=&blip.label { p.text(point+Vec2::new(10.0*scale,0.0),egui::Align2::LEFT_CENTER,label,FontId::proportional(13.0*scale),WHITE); }
                                }
                                p.circle_stroke(project([coordinates[0],coordinates[1]]),12.0*scale,egui::Stroke::new(2.0,WHITE));
                                columns[1].label(RichText::new("Overview map • white ring marks your position").size(13.0).color(MUTED));
                            });
                        },
                        Page::Controls=>{
                            egui::Grid::new("controls").spacing([80.0*scale,17.0*scale]).show(ui,|ui|{
                                for (key,description) in [("Left stick","Move / steer"),("Right stick","Look around"),("A","Jump / select"),("X","Run"),("Y","Enter / exit car"),("RT / LT","Accelerate / brake / reverse / shoot / aim"),("LB","Handbrake"),("Left stick in the air","Tip and turn the car (LB + stick rolls it)"),("RT + LT","Burnout when standing still"),("Start / B","Pause / go back"),("Back","Open map"),("W A S D","Move / drive"),("Mouse","Look around / aim"),("Left click","Shoot"),("Right click","Aim"),("Shift","Run / fly faster"),("Space","Jump / handbrake in car"),("Up / Down arrows in the air","Tip the car's nose down / up"),("Space + A / D in the air","Roll the car"),("W + S","Burnout when standing still"),("F6","Wardrobe"),("I","Interiors"),("F9","Spawn and enter a car"),("F","Enter / exit nearest car or leave passenger seat"),("G","Ride in another player's car"),("V","Toggle first / third person"),("P","Toggle walk / free-fly"),("Q / E","Fly down / up"),("1–9","Travel to map regions"),("R","Return to Grove Street"),("M","Open map"),("Right-click map","Set / remove waypoint"),("Esc","Pause / go back")]{ui.label(RichText::new(key).color(GOLD).strong());ui.label(description);ui.end_row();}
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
                                    ui.label(RichText::new("Over the internet without port forwarding: run the playit.gg program, add a TCP tunnel to local port 7777, press Host session, and give friends the address playit shows.").small());
                                    ui.label("Join address (IP:port, or a playit.gg address)");
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
                                    if server.version!=sa_net::VERSION {ui.label(format!("{}: incompatible protocol {}; client {}. Update to the same build.",server.name,server.version,sa_net::VERSION));}
                                    if ui.add_enabled(!self.network_active && server.players < server.capacity && server.version==sa_net::VERSION, egui::Button::new(format!("Join {} — {} / {}", server.name, server.players, server.capacity))).clicked() {
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
                            ui.label("Players, chosen vehicles and outfits are shared. Cars remain visible after parking; press G to ride with someone. Cars bump into each other, including other players' cars. Traffic is shared with nearby players; spawned NPCs are local.");
                            });
                        }
                        Page::Commands=>{
                            ui.label("/cars - vehicles    /peds - player models    /mp - multiplayer    /mods - resource list");
                            let enter=ctx.input(|i|i.key_pressed(egui::Key::Enter));
                            let response=ui.text_edit_singleline(&mut self.command);
                            if !ctx.memory(|m|m.has_focus(response.id)) {response.request_focus();}
                            if enter || ui.button("Open menu").clicked() {self.submit_command();}
                        },
                        Page::Cars | Page::Peds=>{
                            ui.label(if page==Page::Cars {"Spawn nearby and enter. One vehicle is active at a time."} else {"Use a player model or spawn a nearby ped (S / D-pad right). Spawned peds idle; they have no AI."});
                            if page==Page::Peds && ui.button("Remove spawned peds").clicked(){action=Some(Action::ClearPeds);}
                            let names=if page==Page::Cars {self.cars.clone()}else{self.peds.clone()};
                            // Long lists (every car in the game) get a search box.
                            let mut typing=false;
                            if names.len()>12 {
                                let search=ui.add(egui::TextEdit::singleline(&mut self.list_filter).hint_text("Search").desired_width(440.0*scale));
                                typing=search.has_focus();
                            }
                            let filter=self.list_filter.to_lowercase();
                            let visible:Vec<usize>=names.iter().enumerate().filter(|(_,n)|filter.is_empty()||n.to_lowercase().contains(&filter)).map(|(i,_)|i).collect();
                            let navigation=ctx.input(|i|(i.key_pressed(egui::Key::ArrowDown),i.key_pressed(egui::Key::ArrowUp),i.key_pressed(egui::Key::Enter)));
                            if !visible.is_empty(){
                                let at=visible.iter().position(|&i|i==self.selected);
                                if navigation.0 {self.selected=visible[at.map_or(0,|p|(p+1)%visible.len())];}
                                if navigation.1 {self.selected=visible[at.map_or(visible.len()-1,|p|(p+visible.len()-1)%visible.len())];}
                                if navigation.2 && visible.contains(&self.selected) {action=Some(if page==Page::Cars {Action::Car(self.selected)}else{Action::Ped(self.selected)});}
                            }
                            if page==Page::Peds && !typing && visible.contains(&self.selected) && ctx.input(|i|i.key_pressed(egui::Key::S)){action=Some(Action::SpawnPed(self.selected));}
                            for &index in &visible {
                                let name=&names[index];
                                let text=if index==self.selected {format!("> {name}")}else{name.clone()};
                                ui.horizontal(|ui| {
                                let button=ui.add_sized([440.0*scale,40.0*scale],egui::Button::new(text));
                                if index==self.selected && (navigation.0||navigation.1) {button.scroll_to_me(None);}
                                if button.clicked(){
                                    action=Some(if page==Page::Cars {Action::Car(index)}else{Action::Ped(index)});
                                }
                                if page==Page::Peds && ui.button("Spawn nearby").clicked(){action=Some(Action::SpawnPed(index));}
                                });
                            }
                            if visible.is_empty() && !names.is_empty(){ui.label("Nothing matches the search.");}
                            if names.is_empty(){ui.label("No models are available.");}
                        },
                        Page::Mods=>{
                            ui.label(RichText::new(if self.network_active {"Session resources"} else {"Installed resources"}).size(23.0).color(GOLD).strong());
                            if self.mods.is_empty(){ui.label("No mod resources were found.");}else{for name in &self.mods{ui.label(format!("•  {name}"));}}
                            ui.add_space(18.0);
                            if self.network_active {
                                ui.label("These resources belong to this session. Downloaded files are verified and cached for your next visit. Disconnect to return to your local setup.");
                            } else {
                                ui.label("Place each resource in its own folder under mods/. To enable or disable it, set enabled in resource.json or mod.json, then restart.");
                            }
                            ui.label(RichText::new("Supports documented custom models, textures, buildings, cars, player characters, and skinned clothing. Scripts, DLL plugins, and arbitrary game or FiveM mods are not executed. Choose clothes in the wardrobe (F6).").color(MUTED));
                        },
                        Page::Quit=>{
                            ui.label("You can return to free roam from the SARE launcher or start the runtime directly.");
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
    fn title_actions_and_pause_tabs_stay_inside_small_and_wide_windows() {
        for size in [
            Vec2::new(640.0, 480.0),
            Vec2::new(1280.0, 720.0),
            Vec2::new(2560.0, 1080.0),
        ] {
            let ctx = egui::Context::default();
            let mut menu = Menu::new(&ctx, Vec::new());
            let screen = Rect::from_min_size(Pos2::ZERO, size);
            let draw = |menu: &mut Menu, events: Vec<egui::Event>| {
                let input = egui::RawInput {
                    screen_rect: Some(screen),
                    focused: true,
                    events,
                    ..Default::default()
                };
                let mut output = ctx.run_ui(input, |ui| {
                    menu.draw(ui.ctx(), [0.0; 3], false, None, 0.0, &[]);
                });
                output.textures_delta.clear();
            };
            for _ in 0..3 {
                draw(&mut menu, Vec::new());
            }
            for (id, indices) in [
                ("front-play", vec![0usize, 1, 10]),
                ("nav", (2..10).collect()),
            ] {
                for index in indices {
                    let rect = ctx
                        .read_response(egui::Id::new((id, index)))
                        .expect("missing title action")
                        .rect;
                    assert!(
                        screen.contains_rect(rect),
                        "{id}/{index} is clipped at {size:?}: {rect:?}"
                    );
                }
            }
            // The quit entry was below the bottom edge in the old 480p title layout.
            let quit = ctx
                .read_response(egui::Id::new(("nav", 9usize)))
                .unwrap()
                .rect
                .center();
            for pressed in [true, false] {
                draw(
                    &mut menu,
                    vec![
                        egui::Event::PointerMoved(quit),
                        egui::Event::PointerButton {
                            pos: quit,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                );
            }
            assert_eq!(menu.page, Some(Page::Quit));
            menu.has_played = true;
            menu.open(Page::Pause);
            for _ in 0..3 {
                draw(&mut menu, Vec::new());
            }
            for index in 0..pause_tabs::TABS.len() {
                let rect = ctx
                    .read_response(egui::Id::new(("gta-tab", index)))
                    .unwrap()
                    .rect;
                assert!(screen.contains_rect(rect));
                let scale = (size.y / 900.0).clamp(0.8, 1.5);
                let width = ctx.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap(
                            pause_tabs::TABS[index].1.into(),
                            FontId::new(18.0 * scale, FontFamily::Name("menu".into())),
                            WHITE,
                        )
                        .size()
                        .x
                });
                assert!(width < rect.width(), "tab label is clipped at {size:?}");
            }
            for index in 0..pause_tabs::PAUSE_ITEMS.len() {
                assert!(screen.contains_rect(
                    ctx.read_response(egui::Id::new(("gta-row", index)))
                        .unwrap()
                        .rect
                ));
            }
            menu.open(Page::Settings);
            for _ in 0..3 {
                draw(&mut menu, Vec::new());
            }
            let rows = menu.setting_rows();
            for ((left, right), (_, value, _)) in
                menu.settings_controls.iter().zip(rows.iter().skip(1))
            {
                assert!(screen.contains_rect(*left) && screen.contains_rect(*right));
                let scale = (size.y / 900.0).clamp(0.8, 1.5);
                let width = ctx.fonts_mut(|fonts| {
                    fonts
                        .layout_no_wrap(value.clone(), FontId::proportional(14.0 * scale), WHITE)
                        .size()
                        .x
                });
                assert!(
                    width + 28.0 * scale < right.left() - left.right(),
                    "setting value is hidden by arrow buttons at {size:?}: {value}"
                );
            }
        }
    }
    #[test]
    fn focused_title_action_does_not_also_start_free_roam() {
        for (page, row_id) in [(Page::Main, "nav"), (Page::Pause, "gta-row")] {
            let ctx = egui::Context::default();
            let mut menu = Menu::new(&ctx, Vec::new());
            menu.has_played = page == Page::Pause;
            menu.open(page);
            for _ in 0..3 {
                frame(&ctx, &mut menu, &[]);
            }
            ctx.memory_mut(|m| m.request_focus(egui::Id::new((row_id, 2usize))));
            assert_eq!(frame(&ctx, &mut menu, &[egui::Key::Enter]), None);
            assert_eq!(menu.page, Some(Page::Settings));
            if page == Page::Pause {
                menu.open(Page::Pause);
                frame(&ctx, &mut menu, &[]);
                ctx.memory_mut(|m| m.request_focus(egui::Id::new(("gta-tab", 1usize))));
                frame(&ctx, &mut menu, &[egui::Key::E]);
                assert_eq!(
                    menu.page,
                    Some(Page::Network),
                    "focused tabs must still allow Q/E"
                );
            }
        }
    }
    #[test]
    fn gameplay_status_zone_and_speedometer_do_not_overlap() {
        for size in [Vec2::new(640.0, 480.0), Vec2::new(1280.0, 720.0)] {
            let ctx = egui::Context::default();
            let mut menu = Menu::new(&ctx, Vec::new());
            menu.page = None;
            menu.settings.show_hud = true;
            menu.settings.show_speedometer = true;
            menu.zone = Some("Grove Street".into());
            menu.network_active = true;
            menu.network_players = vec!["Player".into()];
            menu.weapon_hud = Some(("Micro SMG".into(), 32, 180, false, false));
            let screen = Rect::from_min_size(Pos2::ZERO, size);
            for frame in 0..4 {
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        time: Some(frame as f64 / 60.0),
                        ..Default::default()
                    },
                    |ui| {
                        menu.draw(ui.ctx(), [0.0; 3], false, Some(20.0), 0.0, &[]);
                    },
                );
                output.textures_delta.clear();
            }
            let area = |id: &str| {
                ctx.memory(|m| m.area_rect(egui::Id::new(id)))
                    .expect("missing HUD area")
            };
            let zone = area("freeroam-zone");
            let speed = area("freeroam-speedometer");
            let status = area("sare-status");
            assert!(
                !zone.intersects(speed),
                "zone and speed overlap at {size:?}"
            );
            assert!(!status.intersects(zone) && !status.intersects(speed));
            for rect in [zone, speed, status] {
                assert!(
                    screen.contains_rect(rect),
                    "HUD clipped at {size:?}: {rect:?}"
                );
            }
        }
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
    fn frame_events(
        ctx: &egui::Context,
        menu: &mut Menu,
        events: Vec<egui::Event>,
    ) -> Option<Action> {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0))),
            focused: true,
            events,
            ..Default::default()
        };
        let mut action = None;
        let mut output = ctx.run_ui(input, |ui| {
            action = menu.draw(
                ui.ctx(),
                [2500.0, -1670.0, 14.0],
                false,
                Some(20.0),
                0.0,
                &[],
            );
        });
        output.textures_delta.clear();
        action
    }
    #[test]
    fn pause_map_places_and_removes_a_waypoint_and_travels() {
        let ctx = egui::Context::default();
        let mut menu = Menu::new(&ctx, Vec::new());
        menu.has_played = true;
        menu.open(Page::Map);
        frame(&ctx, &mut menu, &[]);
        let (center, zoom) = menu.map_view.unwrap();
        assert_eq!(center, [2500.0, -1670.0]);
        let click = Pos2::new(900.0, 500.0);
        let button = |pressed| egui::Event::PointerButton {
            pos: click,
            button: egui::PointerButton::Secondary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        };
        frame_events(&ctx, &mut menu, vec![egui::Event::PointerMoved(click)]);
        frame_events(&ctx, &mut menu, vec![button(true)]);
        frame_events(&ctx, &mut menu, vec![button(false)]);
        let waypoint = menu.waypoint.expect("right-click places a waypoint");
        assert!(waypoint[0].is_finite() && waypoint[1].is_finite());
        // The same spot again removes it; Backspace removes it too.
        frame_events(&ctx, &mut menu, vec![button(true)]);
        frame_events(&ctx, &mut menu, vec![button(false)]);
        assert_eq!(menu.waypoint, None);
        menu.waypoint = Some([0.0, 0.0]);
        frame(&ctx, &mut menu, &[egui::Key::Backspace]);
        assert_eq!(menu.waypoint, None);
        assert_eq!(
            frame(&ctx, &mut menu, &[egui::Key::Num3]),
            Some(Action::Teleport(2))
        );
        // PageUp zooms in around the centre.
        frame(&ctx, &mut menu, &[egui::Key::PageUp]);
        assert!(menu.map_view.unwrap().1 >= zoom);
        // Back in the game: the radar draws blips, the waypoint and a route.
        menu.waypoint = Some([2600.0, -1600.0]);
        menu.overlay = MapOverlay {
            blips: vec![MapBlip {
                position: [5000.0, 0.0],
                color: blip_color(5),
                label: Some("Far away".into()),
                route: true,
                short_range: false,
            }],
            route: vec![[2500.0, -1670.0], [2600.0, -1670.0], [2600.0, -1600.0]],
            route_color: WAYPOINT,
        };
        frame_events(&ctx, &mut menu, Vec::new());
        menu.page = None;
        frame_events(&ctx, &mut menu, Vec::new());
        assert!(menu.radar_range > 260.0);
    }
    #[test]
    fn radar_helpers_clip_lines_and_pull_blips_to_the_edge() {
        let area = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(100.0, 50.0));
        let centre = Pos2::new(50.0, 50.0);
        let pieces = clip_line_circle(
            &[Pos2::new(-100.0, 50.0), Pos2::new(200.0, 50.0)],
            centre,
            20.0,
        );
        assert_eq!(pieces.len(), 1);
        assert!((pieces[0][0].x - 30.0).abs() < 1e-3 && (pieces[0][1].x - 70.0).abs() < 1e-3);
        assert!(
            clip_line_circle(&[Pos2::new(0.0, 0.0), Pos2::new(10.0, 0.0)], centre, 20.0).is_empty()
        );
        let pieces = clip_line(&[Pos2::new(10.0, 10.0), Pos2::new(300.0, 10.0)], area);
        assert!(!pieces.is_empty());
        assert!(pieces.iter().flatten().all(|p| area.contains(*p)));
        assert!(radar_range(None, 1.0) < radar_range(Some(5.0), 1.0));
        assert!(radar_range(Some(30.0), 1.0) < radar_range(Some(-45.0), 1.0));
        assert_eq!(radar_range(Some(90.0), 2.0), radar_range(Some(45.0), 2.0));
        assert_eq!(blip_color(5), blip_color(46));
        assert_eq!(blip_color(999), blip_color(0));
    }
}
