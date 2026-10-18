//! The title screen puts the three ways to play over original SARE artwork.
use super::*;

impl Menu {
    pub(super) fn front_menu(
        &mut self,
        ui: &mut egui::Ui,
        screen: Rect,
        scale: f32,
        coordinates: [f32; 3],
        page: Page,
    ) -> Option<Action> {
        let ctx = ui.ctx().clone();
        let painter = ui.painter().clone();
        let inset = screen.width() * 0.065;
        let left = screen.left() + inset;
        let right = screen.right() - inset;
        let top = screen.top() + screen.height() * 0.065;
        let footer = screen.bottom() - 34.0 * scale;
        ui_theme::horizon(&painter, screen);
        painter.text(
            Pos2::new(left, top),
            egui::Align2::LEFT_TOP,
            "SARE",
            FontId::new(82.0 * scale, FontFamily::Name("street".into())),
            WHITE,
        );
        painter.text(
            Pos2::new(left, top + 100.0 * scale),
            egui::Align2::LEFT_TOP,
            "San Andreas Rust Edition",
            FontId::proportional(17.0 * scale),
            MUTED,
        );
        painter.text(
            Pos2::new(right, top + 10.0 * scale),
            egui::Align2::RIGHT_TOP,
            if self.network_active {
                "Multiplayer"
            } else {
                "Free roam"
            },
            FontId::proportional(16.0 * scale),
            GOLD,
        );
        let body_top = top + 154.0 * scale;
        let gap = 24.0 * scale;
        let width = ((right - left - gap) * 0.55).min(610.0 * scale);
        let secondary_left = left + width + gap;
        let mut action = None;
        let labels = [
            if page == Page::Pause {
                "Resume"
            } else if self.has_checkpoint {
                "Continue free roam"
            } else {
                "Explore San Andreas"
            },
            "Map & destinations",
            "Settings",
            "Controls",
            "Resources",
            "Wardrobe",
            "Interiors",
            "Vehicles",
            "Player",
            if page == Page::Main {
                "Quit"
            } else {
                "Main menu"
            },
            "Multiplayer",
        ];
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            self.selected = (self.selected + 1) % labels.len();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            self.selected = (self.selected + labels.len() - 1) % labels.len();
        }
        let keyboard_select =
            !ctx.egui_wants_keyboard_input() && ctx.input(|i| i.key_pressed(egui::Key::Enter));
        // Keep the established action indices so keyboard and controller navigation agree.
        for (slot, index) in [0, 1, 10].into_iter().enumerate() {
            let rect = Rect::from_min_size(
                Pos2::new(left, body_top + slot as f32 * 83.0 * scale),
                Vec2::new(width, 72.0 * scale),
            );
            let selected = self.selected == index;
            let response = ui.interact(rect, egui::Id::new(("front-play", index)), Sense::click());
            let active = selected || response.hovered() || response.has_focus();
            painter.rect_filled(rect, 3, if active { GOLD } else { ui_theme::SURFACE });
            let color = if active { ui_theme::BACKGROUND } else { WHITE };
            painter.text(
                rect.min + Vec2::new(17.0, 13.0) * scale,
                egui::Align2::LEFT_TOP,
                labels[index],
                FontId::proportional(23.0 * scale),
                color,
            );
            let detail = match index {
                0 => {
                    if self.has_checkpoint {
                        "Return to your last saved location"
                    } else {
                        "Start your story on Grove Street"
                    }
                }
                1 => "Find your next corner of the state",
                _ => "Host a session or join your friends",
            };
            let galley = painter.layout(
                detail.into(),
                FontId::proportional((14.0 * scale).max(12.0)),
                if active { ui_theme::BACKGROUND } else { MUTED },
                rect.width() - 34.0 * scale,
            );
            painter.galley(rect.min + Vec2::new(17.0, 43.0) * scale, galley, color);
            if response.clicked() || (selected && keyboard_select) {
                self.selected = index;
                match index {
                    0 => action = Some(Action::Play),
                    1 => self.open(Page::Map),
                    _ => self.open(Page::Network),
                }
            }
        }
        let row_height = 37.0 * scale;
        for (slot, index) in (2..10).enumerate() {
            let rect = Rect::from_min_size(
                Pos2::new(
                    secondary_left,
                    body_top + slot as f32 * (row_height + 2.0 * scale),
                ),
                Vec2::new((right - secondary_left).min(330.0 * scale), row_height),
            );
            if Self::nav(
                ui,
                rect,
                labels[index],
                self.selected == index,
                index,
                scale,
            ) || (self.selected == index && keyboard_select)
            {
                self.selected = index;
                match index {
                    2 => self.open(Page::Settings),
                    3 => self.open(Page::Controls),
                    4 => self.open(Page::Mods),
                    5 => self.open(Page::Wardrobe),
                    6 => self.open(Page::Interiors),
                    7 => self.open(Page::Cars),
                    8 => self.open(Page::Peds),
                    _ => {
                        if page == Page::Main {
                            self.open(Page::Quit)
                        } else {
                            action = Some(Action::Main)
                        }
                    }
                }
            }
        }
        let location = self.zone.as_deref().unwrap_or_else(|| {
            DESTINATIONS
                .iter()
                .min_by(|a, b| {
                    let distance = |xy: [f32; 2]| {
                        (xy[0] - coordinates[0]).powi(2) + (xy[1] - coordinates[1]).powi(2)
                    };
                    distance(a.1).total_cmp(&distance(b.1))
                })
                .map(|d| d.0)
                .unwrap_or("San Andreas")
        });
        if body_top + 345.0 * scale < footer - 58.0 * scale {
            painter.text(
                Pos2::new(left, footer - 68.0 * scale),
                egui::Align2::LEFT_BOTTOM,
                location,
                FontId::new(31.0 * scale, FontFamily::Name("street".into())),
                GOLD,
            );
        }
        let mut hints = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(Rect::from_min_max(
                    Pos2::new(left, footer - 12.0),
                    Pos2::new(right, screen.bottom()),
                ))
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        ui_theme::key_hint(&mut hints, "Enter / A", "Select");
        ui_theme::key_hint(&mut hints, "Arrows / D-pad", "Move");
        if screen.width() > 950.0 {
            painter.text(
                Pos2::new(right, footer),
                egui::Align2::RIGHT_CENTER,
                format!("SARE v{}", sa_client::VERSION),
                FontId::proportional((14.0 * scale).max(12.0)),
                MUTED,
            );
        }
        if !self.message.is_empty() {
            let galley = painter.layout(
                self.message.clone(),
                FontId::proportional(14.0 * scale),
                GOLD,
                right - left,
            );
            painter.galley(Pos2::new(left, footer - 40.0 * scale), galley, GOLD);
        }
        action
    }
}
