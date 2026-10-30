//! In-game pause menu chrome: a title and player
//! header, a row of tabs with the selected tab in white, a coloured stripe,
//! white-highlighted list rows and instructional key hints at the bottom.
use super::{Action, Menu, Page, DESTINATIONS, MUTED};
use egui::{Color32, FontFamily, FontId, Pos2, Rect, Sense, Vec2};

/// The shared SARE accent keeps launcher and game navigation consistent.
const STRIPE: Color32 = crate::ui_theme::ACCENT;
const PANEL: Color32 = crate::ui_theme::SURFACE;
const TEXT: Color32 = crate::ui_theme::TEXT;

pub(super) const TABS: [(Page, &str); 10] = [
    (Page::Map, "Map"),
    (Page::Pause, "Game"),
    (Page::Network, "Online"),
    (Page::Cars, "Vehicles"),
    (Page::Peds, "Player"),
    (Page::Wardrobe, "Wardrobe"),
    (Page::Interiors, "Interiors"),
    (Page::Settings, "Settings"),
    (Page::Controls, "Controls"),
    (Page::Mods, "Resources"),
];
pub(super) const PAUSE_ITEMS: [&str; 6] = [
    "Resume",
    "Map",
    "Settings",
    "Controls",
    "Multiplayer",
    "Main menu",
];

fn menu_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("menu".into()))
}

/// One list row: white with dark text when selected.
fn row(ui: &mut egui::Ui, rect: Rect, text: &str, selected: bool, id: usize, scale: f32) -> bool {
    let response = ui.interact(rect, egui::Id::new(("gta-row", id)), Sense::click());
    let (fill, color) = if selected {
        (Color32::from_rgb(31, 43, 53), TEXT)
    } else if response.hovered() || response.has_focus() {
        (Color32::from_rgb(35, 47, 58), TEXT)
    } else {
        (PANEL, MUTED)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, fill);
    painter.rect_filled(
        Rect::from_min_size(
            rect.min,
            Vec2::new(if selected { 3.0 } else { 1.0 } * scale, rect.height()),
        ),
        0.0,
        if selected {
            STRIPE
        } else {
            Color32::from_rgb(54, 67, 77)
        },
    );
    painter.text(
        rect.left_center() + Vec2::new(13.0 * scale, 0.0),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(16.0 * scale),
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
    painter.text(
        Pos2::new(right, y),
        egui::Align2::RIGHT_CENTER,
        label,
        font.clone(),
        TEXT,
    );
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
    painter.text(
        key_rect.center(),
        egui::Align2::CENTER_CENTER,
        key,
        font,
        Color32::BLACK,
    );
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
            "SARE",
            FontId::new(46.0 * scale, FontFamily::Name("title".into())),
            TEXT,
        );
        let location = DESTINATIONS
            .iter()
            .min_by(|a, b| {
                let d = |xy: [f32; 2]| {
                    (xy[0] - coordinates[0]).powi(2) + (xy[1] - coordinates[1]).powi(2)
                };
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
            format!("Online  /  {} players", self.network_players.len())
        } else {
            "Free roam".into()
        };
        painter.text(
            Pos2::new(right, header + 32.0 * scale),
            egui::Align2::RIGHT_TOP,
            format!("{location}  /  {mode}"),
            FontId::proportional(14.0 * scale),
            MUTED,
        );

        // Tabs. Q / E switch tabs.
        let tabs_top = header + 66.0 * scale;
        let tab_height = 34.0 * scale;
        let gap = 3.0 * scale;
        let columns = if (right - left) / (TABS.len() as f32) < 85.0 * scale {
            5
        } else {
            TABS.len()
        };
        let tab_rows = TABS.len().div_ceil(columns);
        let tab_width = (right - left - gap * (columns - 1) as f32) / columns as f32;
        let active = TABS.iter().position(|(p, _)| *p == page);
        if !ctx.text_edit_focused() {
            let (previous, next) =
                ctx.input(|i| (i.key_pressed(egui::Key::Q), i.key_pressed(egui::Key::E)));
            let current = active.unwrap_or(1);
            if previous {
                self.open(TABS[(current + TABS.len() - 1) % TABS.len()].0);
            } else if next {
                self.open(TABS[(current + 1) % TABS.len()].0);
            }
        }
        for (index, (tab, label)) in TABS.iter().enumerate() {
            let rect = Rect::from_min_size(
                Pos2::new(
                    left + (index % columns) as f32 * (tab_width + gap),
                    tabs_top + (index / columns) as f32 * (tab_height + gap),
                ),
                Vec2::new(tab_width, tab_height),
            );
            let response = ui.interact(rect, egui::Id::new(("gta-tab", index)), Sense::click());
            let selected = active == Some(index);
            let (fill, color) = if selected {
                (Color32::from_rgb(31, 43, 53), STRIPE)
            } else if response.hovered() || response.has_focus() {
                (Color32::from_rgb(35, 47, 58), TEXT)
            } else {
                (PANEL, MUTED)
            };
            painter.rect_filled(rect, 0.0, fill);
            if selected {
                painter.rect_filled(
                    Rect::from_min_size(
                        Pos2::new(rect.left(), rect.bottom() - 2.0 * scale),
                        Vec2::new(rect.width(), 2.0 * scale),
                    ),
                    0.0,
                    STRIPE,
                );
            }
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                *label,
                menu_font(17.0 * scale),
                color,
            );
            if response.clicked() && !selected {
                self.open(*tab);
            }
        }
        painter.rect_filled(
            Rect::from_min_size(
                Pos2::new(left, tabs_top + (tab_height + gap) * tab_rows as f32),
                Vec2::new(right - left, 4.0 * scale),
            ),
            0.0,
            STRIPE,
        );

        let footer = screen.bottom() - 36.0 * scale;
        let mut x = right;
        for (key, label) in [
            ("Esc", "Back"),
            ("Enter", "Select"),
            ("E", "Next tab"),
            ("Q", "Previous tab"),
        ] {
            x = hint(&painter, x, footer, key, label, scale);
        }
        Rect::from_min_max(
            Pos2::new(
                left,
                tabs_top + (tab_height + gap) * tab_rows as f32 + 18.0 * scale,
            ),
            Pos2::new(right, footer - 30.0 * scale),
        )
    }

    /// The GAME tab: the pause list with details for the highlighted entry.
    pub(super) fn gta_pause(
        &mut self,
        ui: &mut egui::Ui,
        content: Rect,
        scale: f32,
    ) -> Option<Action> {
        let ctx = ui.ctx().clone();
        let mut action = None;
        let count = PAUSE_ITEMS.len();
        self.selected %= count;
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            self.selected = (self.selected + 1) % count;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            self.selected = (self.selected + count - 1) % count;
        }
        let keyboard_select =
            !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Enter));
        let list_width = (content.width() * 0.36).clamp(230.0 * scale, 420.0 * scale);
        let row_height =
            ((content.height() - 2.0 * (count - 1) as f32) / count as f32).min(38.0 * scale);
        for (index, label) in PAUSE_ITEMS.iter().enumerate() {
            let rect = Rect::from_min_size(
                Pos2::new(
                    content.left(),
                    content.top() + index as f32 * (row_height + 2.0),
                ),
                Vec2::new(list_width, row_height),
            );
            let selected = self.selected == index;
            if row(ui, rect, label, selected, index, scale) || (selected && keyboard_select) {
                self.selected = index;
                match index {
                    0 => action = Some(Action::Play),
                    1 => self.open(Page::Map),
                    2 => self.open(Page::Settings),
                    3 => self.open(Page::Controls),
                    4 => self.open(Page::Network),
                    _ => action = Some(Action::Main),
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
        painter.rect_filled(
            Rect::from_min_size(panel.min, Vec2::new(3.0 * scale, panel.height())),
            0.0,
            STRIPE,
        );
        let (title, body) = match self.selected {
            0 => ("Resume", "Return to free roam where you left off."),
            1 => (
                "Map",
                "Open the San Andreas map, set a waypoint and choose a destination.",
            ),
            2 => (
                "Settings",
                "Configure display, graphics, gameplay, interface and audio.",
            ),
            3 => ("Controls", "View keyboard, mouse and controller bindings."),
            4 => (
                "Multiplayer",
                "Host a session, join friends or browse available servers.",
            ),
            _ => (
                "Main menu",
                "Leave free roam and return to the SARE title screen.",
            ),
        };
        let inner = panel.shrink(22.0 * scale);
        painter.text(
            inner.left_top(),
            egui::Align2::LEFT_TOP,
            title,
            menu_font(30.0 * scale),
            STRIPE,
        );
        let galley = painter.layout(
            body.into(),
            FontId::proportional(17.0 * scale),
            Color32::from_rgb(210, 210, 210),
            inner.width(),
        );
        painter.galley(
            inner.left_top() + Vec2::new(0.0, 44.0 * scale),
            galley,
            TEXT,
        );
        let facts = [
            (
                "Location",
                self.zone.clone().unwrap_or_else(|| "San Andreas".into()),
            ),
            (
                "Session",
                if self.network_active {
                    "Multiplayer"
                } else {
                    "Free roam"
                }
                .into(),
            ),
            (
                "Players",
                if self.network_active {
                    self.network_players.len().to_string()
                } else {
                    "Offline".into()
                },
            ),
        ];
        for (index, (label, value)) in facts.iter().enumerate() {
            let y = inner.bottom() - (facts.len() - index) as f32 * 30.0 * scale;
            painter.line_segment(
                [
                    Pos2::new(inner.left(), y - 4.0 * scale),
                    Pos2::new(inner.right(), y - 4.0 * scale),
                ],
                egui::Stroke::new(1.0, Color32::from_white_alpha(30)),
            );
            painter.text(
                Pos2::new(inner.left(), y + 10.0 * scale),
                egui::Align2::LEFT_CENTER,
                *label,
                FontId::proportional(15.0 * scale),
                MUTED,
            );
            painter.text(
                Pos2::new(inner.right(), y + 10.0 * scale),
                egui::Align2::RIGHT_CENTER,
                value,
                FontId::proportional(15.0 * scale),
                TEXT,
            );
        }
        action
    }
}
