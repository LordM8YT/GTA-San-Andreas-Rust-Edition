//! Native menus over the live world. Artwork is geometry; the title font is OFL.
use crate::{
    settings::Settings,
    streaming::{DESTINATIONS, INTERIORS},
};
use egui::{Color32, FontFamily, FontId, Pos2, Rect, RichText, Sense, Vec2};

const GOLD: Color32 = Color32::from_rgb(223, 183, 120);
const WHITE: Color32 = Color32::from_rgb(239, 233, 221);
const MUTED: Color32 = Color32::from_rgb(162, 169, 162);
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
                                "Laster nabolag…"
                            } else {
                                "Fri utforskning"
                            })
                            .size(20.0)
                            .color(WHITE)
                            .strong(),
                        );
                        ui.label(
                            RichText::new("Esc  Meny    M  Kart    P  Gå / fly")
                                .size(14.0)
                                .color(MUTED),
                        );
                    });
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
            let heading=match page{Page::Main=>"Hele staten. Din vei.",Page::Pause=>"Ta en pause",Page::Map=>"Velg reisemål",Page::Settings=>"Innstillinger",Page::Controls=>"Kontroller",Page::Wardrobe=>"Garderobe",Page::Interiors=>"Interiører",Page::Mods=>"Lokale ressurser",Page::Quit=>"Avslutt freeroam?"};
            painter.text(Pos2::new(left,top+150.0*scale),egui::Align2::LEFT_TOP,heading,FontId::proportional(18.0*scale),MUTED);
            let footer=screen.bottom()-48.0*scale;
            painter.text(Pos2::new(left,footer),egui::Align2::LEFT_CENTER,"Mus / piltaster  Velg     Enter  Åpne     Esc  Tilbake",FontId::proportional(14.0*scale),MUTED);
            painter.text(Pos2::new(screen.right()-40.0*scale,footer),egui::Align2::RIGHT_CENTER,"SA Runtime  •  Freeroam",FontId::proportional(14.0*scale),MUTED);
            if matches!(page,Page::Main|Page::Pause) {
                let labels=if page==Page::Main{["Utforsk San Andreas","Kart og reisemål","Innstillinger","Kontroller","Mods","Garderobe","Interiører","Avslutt"]}else{["Fortsett","Kart og reisemål","Innstillinger","Kontroller","Mods","Garderobe","Interiører","Til hovedmenyen"]};
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
                painter.text(Pos2::new(right,screen.bottom()-189.0*scale),egui::Align2::LEFT_TOP,"Ingen missions. Bare frihet.\nGå, utforsk og bygg videre.",FontId::proportional(17.0*scale),MUTED);
            } else {
                let start=Pos2::new(left,top+210.0*scale);
                let rect=Rect::from_min_max(start,Pos2::new(screen.right()-screen.width()*0.07,footer-36.0*scale));
                let mut child=ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
                egui::ScrollArea::vertical().max_height(rect.height()).show(&mut child,|ui|{
                    match page {
                        Page::Settings=>{
                            ui.set_max_width(700.0*scale);
                            ui.label(RichText::new("Bilde").size(23.0).color(GOLD).strong());
                            ui.add(egui::Slider::new(&mut self.settings.fov,55.0..=110.0).text("Synsfelt").suffix("°"));
                            ui.checkbox(&mut self.settings.fullscreen,"Fullskjerm");
                            ui.checkbox(&mut self.settings.vsync,"VSync");
                            ui.checkbox(&mut self.settings.show_hud,"Vis hjelpetekst i spillet");
                            ui.add_space(12.0);
                            ui.label(RichText::new("Mus og bevegelse").size(23.0).color(GOLD).strong());
                            ui.add(egui::Slider::new(&mut self.settings.sensitivity,0.2..=3.0).text("Musefølsomhet"));
                            ui.checkbox(&mut self.settings.invert_y,"Inverter vertikal mus");
                            ui.add(egui::Slider::new(&mut self.settings.fly_speed,6.0..=60.0).text("Fart i flykamera"));
                            ui.label(RichText::new("Endringer gjelder med én gang og lagres på denne PC-en.").color(MUTED));
                            if ui.button("Gjenopprett standardvalg").clicked(){self.settings=Settings::default();}
                        },
                        Page::Map=>{
                            for (index, key) in [egui::Key::Num1, egui::Key::Num2, egui::Key::Num3, egui::Key::Num4, egui::Key::Num5, egui::Key::Num6, egui::Key::Num7, egui::Key::Num8, egui::Key::Num9].into_iter().enumerate() {
                                if ctx.input(|input| input.key_pressed(key)) {
                                    action = Some(Action::Teleport(index));
                                }
                            }
                            ui.label(RichText::new("Reis til et område. Kartet lastes før du flyttes.").color(MUTED));
                            ui.columns(2,|columns|{
                                for (index,(name,_)) in DESTINATIONS.iter().enumerate(){
                                    if columns[0].add_sized([300.0*scale,34.0*scale],egui::Button::new(format!("{}    {}",index+1,name))).clicked(){action=Some(Action::Teleport(index));}
                                }
                                let (map,_) = columns[1].allocate_exact_size(Vec2::new(350.0*scale,340.0*scale),Sense::hover());
                                let p=columns[1].painter();p.rect_filled(map,0.0,Color32::from_rgb(27,39,43));
                                let project=|xy:[f32;2]|Pos2::new(map.left()+(xy[0]+3000.0)/6000.0*map.width(),map.bottom()-(xy[1]+3000.0)/6000.0*map.height());
                                for points in [vec![[-2700.0,-2100.0],[-2850.0,900.0],[-1500.0,1200.0],[-900.0,300.0],[-1200.0,-2200.0]],vec![[-2700.0,1100.0],[-2200.0,2800.0],[2500.0,2800.0],[2900.0,500.0],[500.0,300.0]],vec![[-1100.0,200.0],[500.0,600.0],[2900.0,400.0],[2800.0,-2700.0],[-400.0,-2800.0]]] {
                                    p.add(egui::Shape::convex_polygon(points.into_iter().map(project).collect(),Color32::from_rgb(61,74,57),egui::Stroke::new(1.0,Color32::from_rgb(105,118,87))));
                                }
                                for (index,(_,xy)) in DESTINATIONS.iter().enumerate(){let point=project(*xy);p.circle_filled(point,8.0*scale,GOLD);p.text(point,egui::Align2::CENTER_CENTER,(index+1).to_string(),FontId::proportional(11.0*scale),Color32::BLACK);}
                                p.circle_stroke(project([coordinates[0],coordinates[1]]),12.0*scale,egui::Stroke::new(2.0,WHITE));
                                columns[1].label(RichText::new("Oversiktskart • hvit ring viser din posisjon").size(13.0).color(MUTED));
                            });
                        },
                        Page::Controls=>{
                            egui::Grid::new("controls").spacing([80.0*scale,17.0*scale]).show(ui,|ui|{
                                for (key,description) in [("W A S D","Beveg deg / kjør"),("Mus","Se rundt"),("Shift","Løp / fly raskere"),("Space","Hopp / brems i bil"),("F6","Garderobe"),("I","Interiører"),("F9","Hent bil og sett deg inn"),("F","Gå inn i / ut av bilen"),("V","Bytt kamera: person / tredje person"),("P","Bytt mellom gåmodus og flykamera"),("Q / E","Fly ned / opp"),("1–9","Reis til kartområder"),("R","Tilbake til Grove Street"),("M","Åpne kartet"),("Esc","Pausemeny / tilbake")]{ui.label(RichText::new(key).color(GOLD).strong());ui.label(description);ui.end_row();}
                            });
                        },
                        Page::Interiors=>{
                            ui.label(RichText::new("Besøk et rom").size(23.0).color(GOLD).strong());
                            ui.label("Rommet lastes før du flyttes. Bruk kartet eller R for å reise ut igjen.");
                            if ctx.input(|i|i.key_pressed(egui::Key::ArrowDown)){self.selected=(self.selected+1)%INTERIORS.len();}
                            if ctx.input(|i|i.key_pressed(egui::Key::ArrowUp)){self.selected=(self.selected+INTERIORS.len()-1)%INTERIORS.len();}
                            for (index,room) in INTERIORS.iter().enumerate() {
                                let text=RichText::new(room.name).color(if self.selected==index{GOLD}else{WHITE});
                                if ui.add_sized([360.0*scale,45.0*scale],egui::Button::new(text)).clicked() || (self.selected==index && ctx.input(|i|i.key_pressed(egui::Key::Enter))) {action=Some(Action::Interior(index));}
                            }
                            ui.add_space(18.0);ui.label(RichText::new("Bil hentes ute. Dørinnganger og flere rom kommer senere.").color(MUTED));
                        },
                        Page::Wardrobe=>{
                            ui.label(RichText::new("Ditt antrekk").size(23.0).color(GOLD).strong());
                            if self.clothes.is_empty() { ui.label("Ingen ekstra plagg er tilgjengelige for denne figuren."); }
                            else {
                                ui.label("Velg hvilke plagg du vil bruke.");
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
                            ui.label(RichText::new("Aktive ressurser").size(23.0).color(GOLD).strong());
                            if self.mods.is_empty(){ui.label("Ingen lokale mods er aktive.");}else{for name in &self.mods{ui.label(format!("•  {name}"));}}
                            ui.add_space(18.0);ui.label("Legg egne ressurser i mods-mappen, og start spillet på nytt.");
                            ui.label(RichText::new("Custom modeller, teksturer, bygg, bil, spillerfigur og skinnede klær støttes.\nVelg plagg i garderoben (F6). Ped-AI er under utvikling.").color(MUTED));
                        },
                        Page::Quit=>{
                            ui.label("Du kan starte freeroam igjen fra start-freeroam.cmd.");
                            ui.horizontal(|ui|{if ui.button("Avslutt spillet").clicked(){action=Some(Action::Quit);}
                                if ui.button("Bli her").clicked(){self.back();}});
                        },_=>{}
                    }
                    ui.add_space(16.0);
                    if ui.button("Tilbake").clicked(){self.back();}
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
            action = menu.draw(ui.ctx(), [2500.0, -1670.0, 14.0], false);
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
