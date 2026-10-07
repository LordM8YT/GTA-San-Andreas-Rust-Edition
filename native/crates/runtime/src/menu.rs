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
}
pub struct Menu {
    pub page: Option<Page>,
    pub settings: Settings,
    pub mods: Vec<String>,
    pub clothes: Vec<(String, bool)>,
    pub has_played: bool,
    selected: usize,
    pub message: String,
}

impl Menu {
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
            settings: Settings::load(),
            mods,
            clothes: Vec::new(),
            has_played: false,
            selected: 0,
            message: String::new(),
        }
    }
    pub fn open(&mut self, page: Page) {
        self.page = Some(page);
        self.selected = 0;
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
            Some(Page::Main | Page::Pause) => 8,
            Some(Page::Map) => DESTINATIONS.len(),
            Some(Page::Interiors) => INTERIORS.len(),
            Some(Page::Wardrobe) => self.clothes.len().max(1),
            Some(Page::Settings) => 12,
            _ => 1,
        };
        if up {
            self.selected = (self.selected + item_count - 1) % item_count;
        }
        if down {
            self.selected = (self.selected + 1) % item_count;
        }
        if left || right {
            match self.page {
                Some(Page::Settings) => match self.selected {
                    0 if left => self.settings.fov -= 1.0,
                    0 => self.settings.fov += 1.0,
                    1 => self.settings.fullscreen = !self.settings.fullscreen,
                    2 => self.settings.vsync = !self.settings.vsync,
                    3 => self.settings.show_hud = !self.settings.show_hud,
                    4 => self.settings.show_minimap = !self.settings.show_minimap,
                    5 => self.settings.show_speedometer = !self.settings.show_speedometer,
                    6 if left => self.settings.minimap_zoom -= 0.1,
                    6 => self.settings.minimap_zoom += 0.1,
                    7 if left => self.settings.sensitivity -= 0.1,
                    7 => self.settings.sensitivity += 0.1,
                    8 => self.settings.invert_y = !self.settings.invert_y,
                    9 if left => self.settings.fly_speed -= 1.0,
                    9 => self.settings.fly_speed += 1.0,
                    10 if left => self.settings.vehicle_handling -= 0.1,
                    10 => self.settings.vehicle_handling += 0.1,
                    11 => self.settings = Settings::default(),
                    _ => {}
                },
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
            if self.page == Some(Page::Settings) && self.selected == 11 {
                self.settings = Settings::default();
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
                        _ if self.page == Some(Page::Main) => self.open(Page::Quit),
                        _ => return Some(Action::Main),
                    }
                }
                Some(Page::Map) if self.selected < DESTINATIONS.len() => {
                    return Some(Action::Teleport(self.selected));
                }
                Some(Page::Interiors) if self.selected < INTERIORS.len() => {
                    return Some(Action::Interior(self.selected));
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
                            RichText::new("Esc  Menu    M  Map    P  Walk / fly")
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
            let heading=match page{Page::Main=>"The whole state. Your way.",Page::Pause=>"Take a breath",Page::Map=>"Choose a destination",Page::Settings=>"Settings",Page::Controls=>"Controls",Page::Wardrobe=>"Wardrobe",Page::Interiors=>"Interiors",Page::Mods=>"Local resources",Page::Quit=>"Leave free roam?"};
            painter.text(Pos2::new(left,top+150.0*scale),egui::Align2::LEFT_TOP,heading,FontId::proportional(18.0*scale),MUTED);
            let footer=screen.bottom()-48.0*scale;
            painter.text(Pos2::new(left,footer),egui::Align2::LEFT_CENTER,"D-pad / arrows  Move     A / Enter  Select     B / Esc  Back",FontId::proportional(14.0*scale),MUTED);
            painter.text(Pos2::new(screen.right()-40.0*scale,footer),egui::Align2::RIGHT_CENTER,"SA Runtime  •  Free Roam",FontId::proportional(14.0*scale),MUTED);
            if matches!(page,Page::Main|Page::Pause) {
                let labels=if page==Page::Main{["Explore San Andreas","Map & destinations","Settings","Controls","Mods","Wardrobe","Interiors","Quit"]}else{["Resume","Map & destinations","Settings","Controls","Mods","Wardrobe","Interiors","Main menu"]};
                if ctx.input(|i|i.key_pressed(egui::Key::ArrowDown)){self.selected=(self.selected+1)%labels.len();}
                if ctx.input(|i|i.key_pressed(egui::Key::ArrowUp)){self.selected=(self.selected+labels.len()-1)%labels.len();}
                for (index,label) in labels.iter().enumerate() {
                    let rect=Rect::from_min_size(Pos2::new(left,top+(217.0+index as f32*57.0)*scale),Vec2::new(360.0*scale,49.0*scale));
                    if Self::nav(ui,rect,label,self.selected==index,index,scale)||(self.selected==index&&ctx.input(|i|i.key_pressed(egui::Key::Enter))) {
                        match index {
                            0=>action=Some(Action::Play),1=>self.open(Page::Map),2=>self.open(Page::Settings),
                            3=>self.open(Page::Controls),4=>self.open(Page::Mods),5=>self.open(Page::Wardrobe),6=>self.open(Page::Interiors),
                            _=>if page==Page::Main{self.open(Page::Quit)}else{action=Some(Action::Main)},
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
                egui::ScrollArea::vertical().max_height(rect.height()).show(&mut child,|ui|{
                    match page {
                        Page::Settings=>{
                            ui.set_max_width(700.0*scale);
                            ui.label(RichText::new("Display").size(23.0).color(GOLD).strong());
                            ui.add(egui::Slider::new(&mut self.settings.fov,55.0..=110.0).text("Field of view").suffix("°"));
                            ui.checkbox(&mut self.settings.fullscreen,"Fullscreen");
                            ui.checkbox(&mut self.settings.vsync,"VSync");
                            ui.checkbox(&mut self.settings.show_hud,"Show HUD");
                            ui.checkbox(&mut self.settings.show_minimap,"Show radar / minimap");
                            ui.checkbox(&mut self.settings.show_speedometer,"Show speedometer");
                            ui.add(egui::Slider::new(&mut self.settings.minimap_zoom,0.5..=2.5).text("Radar zoom"));
                            ui.add_space(12.0);
                            ui.label(RichText::new("Mouse & movement").size(23.0).color(GOLD).strong());
                            ui.add(egui::Slider::new(&mut self.settings.sensitivity,0.2..=3.0).text("Mouse sensitivity"));
                            ui.checkbox(&mut self.settings.invert_y,"Invert vertical look");
                            ui.add(egui::Slider::new(&mut self.settings.fly_speed,6.0..=60.0).text("Free-fly speed"));
                            ui.add(egui::Slider::new(&mut self.settings.vehicle_handling,0.5..=1.5).text("Tire grip"));
                            ui.label(RichText::new("Changes apply immediately and are saved on this PC.").color(MUTED));
                            if ui.button("Restore defaults").clicked() {self.settings=Settings::default();}
                        },
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
                                for (key,description) in [("Left stick","Move / steer"),("Right stick","Look around"),("A","Jump / select"),("X","Run"),("Y","Enter / exit car"),("RT / LT","Accelerate / brake / reverse"),("LB","Handbrake"),("Start / B","Pause / go back"),("Back","Open map"),("W A S D","Move / drive"),("Mouse","Look around"),("Shift","Run / fly faster"),("Space","Jump / handbrake in car"),("F6","Wardrobe"),("I","Interiors"),("F9","Spawn and enter a car"),("F","Enter / exit the car"),("V","Toggle first / third person"),("P","Toggle walk / free-fly"),("Q / E","Fly down / up"),("1–9","Travel to map regions"),("R","Return to Grove Street"),("M","Open map"),("Esc","Pause / go back")]{ui.label(RichText::new(key).color(GOLD).strong());ui.label(description);ui.end_row();}
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
                        Page::Mods=>{
                            ui.label(RichText::new("Detected local resources").size(23.0).color(GOLD).strong());
                            if self.mods.is_empty(){ui.label("No mod resources were detected. Place each resource in its own folder under mods/.");}else{for name in &self.mods{ui.label(format!("•  {name}"));}}
                            ui.add_space(18.0);
                            ui.label("To enable or disable a resource, edit its mod.json and set enabled to true or false, then restart.");
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
        });
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(menu.selected, 7);
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
        menu.selected = 2;
        let old_vsync = menu.settings.vsync;
        menu.controller_input(false, false, false, true, false, false);
        assert_ne!(menu.settings.vsync, old_vsync);
        menu.selected = 0;
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
