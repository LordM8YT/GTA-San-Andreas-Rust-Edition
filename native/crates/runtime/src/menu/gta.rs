//! In-game pause menu chrome in the style of GTA V: a title and player
//! header, a row of tabs with the selected tab in white, a coloured stripe,
//! white-highlighted list rows and instructional key hints at the bottom.
use super::{Action, Menu, Page, DESTINATIONS, MUTED};
use egui::{Color32, FontFamily, FontId, Pos2, Rect, Sense, Vec2};

/// Grove Street green, used where GTA V shows the character colour.
const STRIPE: Color32 = Color32::from_rgb(56, 160, 82);
const PANEL: Color32 = Color32::from_black_alpha(175);
const TEXT: Color32 = Color32::from_rgb(245, 245, 245);

pub(super) const TABS: [(Page, &str); 10] = [
    (Page::Map, "MAP"),
    (Page::Pause, "GAME"),
    (Page::Network, "ONLINE"),
    (Page::Cars, "CARS"),
    (Page::Peds, "PLAYER"),
    (Page::Wardrobe, "WARDROBE"),
    (Page::Interiors, "INTERIORS"),
    (Page::Settings, "SETTINGS"),
    (Page::Controls, "CONTROLS"),
    (Page::Mods, "MODS"),
];
pub(super) const PAUSE_ITEMS: [&str; 11] = [
    "Resume",
    "Map & destinations",
    "Settings",
    "Controls",
    "Mods",
    "Wardrobe",
    "Interiors",
    "Cars",
    "Peds",
    "Main menu",
    "Multiplayer",
];

fn menu_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("menu".into()))
}

/// One GTA V list row: white with dark text when selected.
fn row(ui: &mut egui::Ui, rect: Rect, text: &str, selected: bool, id: usize, scale: f32) -> bool {
    let response = ui.interact(rect, egui::Id::new(("gta-row", id)), Sense::click());
    let (fill, color) = if selected {
        (TEXT, Color32::from_rgb(20, 20, 20))
    } else if response.hovered() {
        (Color32::from_black_alpha(215), TEXT)
    } else {
        (PANEL, TEXT)
    };
    ui.painter().rect_filled(rect, 0.0, fill);
    ui.painter().text(
        rect.left_center() + Vec2::new(14.0 * scale, 0.0),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(17.0 * scale),
        color,
    );
    response.clicked()
}

/// A key cap followed by its action, laid out right to left.
fn hint(painter: &egui::Painter, right: f32, y: f32, key: &str, label: &str, scale: f32) -> f32 {
    let font = FontId::proportional(14.0 * scale);
    let label_width = painter
        .layout_no_wrap(label.into(), font.clone(), TEXT)
        .size()
        .x;
    painter.text(Pos2::new(right, y), egui::Align2::RIGHT_CENTER, label, font.clone(), TEXT);
    let key_width = painter
        .layout_no_wrap(key.into(), font.clone(), Color32::BLACK)
        .size()
        .x
        .max(14.0 * scale)
        + 10.0 * scale;
    let key_rect = Rect::from_center_size(
        Pos2::new(right - label_width - 8.0 * scale - key_width / 2.0, y),
        Vec2::new(key_width, 22.0 * scale),
    );
    painter.rect_filled(key_rect, 3.0, TEXT);
    painter.text(key_rect.center(), egui::Align2::CENTER_CENTER, key, font, Color32::BLACK);
    key_rect.left() - 22.0 * scale
}

impl Menu {
    /// Draws header, tabs and hints; returns the content rectangle.
    pub(super) fn gta_chrome(
        &mut self,
        ui: &mut egui::Ui,
        screen: Rect,
        scale: f32,
        page: Page,
        coordinates: [f32; 3],
    ) -> Rect {
        let ctx = ui.ctx().clone();
        let painter = ui.painter().clone();
        painter.rect_filled(screen, 0.0, Color32::from_black_alpha(120));
        let margin = screen.width() * 0.065;
        let left = screen.left() + margin;
        let right = screen.right() - margin;
        let header = screen.top() + screen.height() * 0.055;

        painter.text(
            Pos2::new(left, header),
            egui::Align2::LEFT_TOP,
            "San Andreas",
            menu_font(46.0 * scale),
            TEXT,
        );
        let location = DESTINATIONS
            .iter()
            .min_by(|a, b| {
                let d = |xy: [f32; 2]| (xy[0] - coordinates[0]).powi(2) + (xy[1] - coordinates[1]).powi(2);
                d(a.1).total_cmp(&d(b.1))
            })
            .map(|d| d.0)
            .unwrap_or("San Andreas");
        painter.text(
            Pos2::new(right, header + 2.0 * scale),
            egui::Align2::RIGHT_TOP,
            &self.player_name,
            menu_font(24.0 * scale),
            TEXT,
        );
        let mode = if self.network_active {
            format!("ONLINE  •  {} players", self.network_players.len())
        } else {
            "FREE ROAM".into()
        };
        painter.text(
            Pos2::new(right, header + 32.0 * scale),
            egui::Align2::RIGHT_TOP,
            format!("{}  •  {mode}", location.to_uppercase()),
            FontId::proportional(14.0 * scale),
            MUTED,
        );

        // Tabs. Q / E switch tabs, as on PC in GTA V.
        let tabs_top = header + 66.0 * scale;
        let tab_height = 34.0 * scale;
        let gap = 3.0 * scale;
        let tab_width = (right - left - gap * (TABS.len() - 1) as f32) / TABS.len() as f32;
        let active = TABS.iter().position(|(p, _)| *p == page);
        if !ctx.egui_wants_keyboard_input() {
            let (previous, next) = ctx.input(|i| (i.key_pressed(egui::Key::Q), i.key_pressed(egui::Key::E)));
            let current = active.unwrap_or(1);
            if previous {
                self.open(TABS[(current + TABS.len() - 1) % TABS.len()].0);
            } else if next {
                self.open(TABS[(current + 1) % TABS.len()].0);
            }
        }
        for (index, (tab, label)) in TABS.iter().enumerate() {
            let rect = Rect::from_min_size(
                Pos2::new(left + index as f32 * (tab_width + gap), tabs_top),
                Vec2::new(tab_width, tab_height),
            );
            let response = ui.interact(rect, egui::Id::new(("gta-tab", index)), Sense::click());
            let selected = active == Some(index);
            let (fill, color) = if selected {
                (TEXT, Color32::from_rgb(20, 20, 20))
            } else if response.hovered() {
                (Color32::from_black_alpha(215), TEXT)
            } else {
                (PANEL, TEXT)
            };
            painter.rect_filled(rect, 0.0, fill);
            painter.text(rect.center(), egui::Align2::CENTER_CENTER, *label, menu_font(18.0 * scale), color);
            if response.clicked() && !selected {
                self.open(*tab);
            }
        }
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(left, tabs_top + tab_height + gap),
                Vec2::new(right - left, 4.0 * scale),
            ),
            0.0,
            STRIPE,
        );

        let footer = screen.bottom() - 36.0 * scale;
        let mut x = right;
        for (key, label) in [("Esc", "Back"), ("Enter", "Select"), ("E", "Next tab"), ("Q", "Previous tab")] {
            x = hint(&painter, x, footer, key, label, scale);
        }
        Rect::from_min_max(
            Pos2::new(left, tabs_top + tab_height + 22.0 * scale),
            Pos2::new(right, footer - 30.0 * scale),
        )
    }

    /// The GAME tab: the pause list with details for the highlighted entry.
    pub(super) fn gta_pause(
        &mut self,
        ui: &mut egui::Ui,
        content: Rect,
        scale: f32,
        coordinates: [f32; 3],
    ) -> Option<Action> {
        let ctx = ui.ctx().clone();
        let mut action = None;
        let count = PAUSE_ITEMS.len();
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            self.selected = (self.selected + 1) % count;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            self.selected = (self.selected + count - 1) % count;
        }
        let list_width = (content.width() * 0.34).max(260.0 * scale);
        let row_height = 36.0 * scale;
        for (index, label) in PAUSE_ITEMS.iter().enumerate() {
            let rect = Rect::from_min_size(
                Pos2::new(content.left(), content.top() + index as f32 * (row_height + 2.0)),
                Vec2::new(list_width, row_height),
            );
            let selected = self.selected == index;
            if row(ui, rect, label, selected, index, scale)
                || (selected && ctx.input(|i| i.key_pressed(egui::Key::Enter)))
            {
                self.selected = index;
                match index {
                    0 => action = Some(Action::Play),
                    1 => self.open(Page::Map),
                    2 => self.open(Page::Settings),
                    3 => self.open(Page::Controls),
                    4 => self.open(Page::Mods),
                    5 => self.open(Page::Wardrobe),
                    6 => self.open(Page::Interiors),
                    7 => self.open(Page::Cars),
                    8 => self.open(Page::Peds),
                    9 => action = Some(Action::Main),
                    _ => self.open(Page::Network),
                }
            }
        }
        let panel = Rect::from_min_max(
            Pos2::new(content.left() + list_width + 14.0 * scale, content.top()),
            Pos2::new(
                content.right(),
                content.top() + count as f32 * (row_height + 2.0) - 2.0,
            ),
        );
        let painter = ui.painter();
        painter.rect_filled(panel, 0.0, PANEL);
        let (title, body) = match self.selected {
            0 => ("Resume", "Return to free roam where you left off."),
            1 => ("Map & destinations", "Travel to nine regions of San Andreas. The area loads before you move."),
            2 => ("Settings", "Display, graphics, gameplay, interface and audio."),
            3 => ("Controls", "Keyboard, mouse and controller bindings."),
            4 => ("Mods", "Local or server resources that are loaded now."),
            5 => ("Wardrobe", "Choose which clothes your character wears."),
            6 => ("Interiors", "Visit the interiors that are available today."),
            7 => ("Cars", "Spawn a vehicle nearby and get in."),
            8 => ("Peds", "Change your player model or spawn a ped."),
            9 => ("Main menu", "Leave free roam for the main menu. Offline progress is saved."),
            _ => ("Multiplayer", "Host, join a server or browse a relay. T opens chat, F8 the console."),
        };
        let inner = panel.shrink(22.0 * scale);
        painter.text(inner.left_top(), egui::Align2::LEFT_TOP, title, menu_font(30.0 * scale), TEXT);
        let galley = painter.layout(
            body.into(),
            FontId::proportional(17.0 * scale),
            Color32::from_rgb(210, 210, 210),
            inner.width(),
        );
        painter.galley(inner.left_top() + Vec2::new(0.0, 44.0 * scale), galley, TEXT);
        let facts = [
            ("Position", format!("{:.0}, {:.0}, {:.0}", coordinates[0], coordinates[1], coordinates[2])),
            ("Session", self.network_status.clone()),
            ("Players", if self.network_active { self.network_players.len().to_string() } else { "Offline".into() }),
        ];
        for (index, (label, value)) in facts.iter().enumerate() {
            let y = inner.bottom() - (facts.len() - index) as f32 * 30.0 * scale;
            painter.line_segment(
                [Pos2::new(inner.left(), y - 4.0 * scale), Pos2::new(inner.right(), y - 4.0 * scale)],
                egui::Stroke::new(1.0, Color32::from_white_alpha(30)),
            );
            painter.text(Pos2::new(inner.left(), y + 10.0 * scale), egui::Align2::LEFT_CENTER, *label, FontId::proportional(15.0 * scale), MUTED);
            painter.text(Pos2::new(inner.right(), y + 10.0 * scale), egui::Align2::RIGHT_CENTER, value, FontId::proportional(15.0 * scale), TEXT);
        }
        action
    }
}
