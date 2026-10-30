//! The title screen uses a cinematic SARE layout with clear, game-first navigation.
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
        let top = screen.top() + screen.height() * 0.055;
        let footer = screen.bottom() - 34.0 * scale;
        ui_theme::horizon(&painter, screen);

        // Brand lockup: compact metadata first, then the custom SARE wordmark.
        painter.text(
            Pos2::new(left, top),
            egui::Align2::LEFT_TOP,
            "SAN ANDREAS  /  OPEN WORLD",
            FontId::proportional((11.0 * scale).max(10.0)),
            ui_theme::ACCENT,
        );
        painter.text(
            Pos2::new(left, top + 17.0 * scale),
            egui::Align2::LEFT_TOP,
            "SARE",
            FontId::new(76.0 * scale, FontFamily::Name("title".into())),
            WHITE,
        );
        painter.text(
            Pos2::new(left + 4.0 * scale, top + 102.0 * scale),
            egui::Align2::LEFT_TOP,
            "SAN ANDREAS RUST EDITION",
            FontId::proportional((12.0 * scale).max(10.0)),
            MUTED,
        );
        painter.text(
            Pos2::new(left + 4.0 * scale, top + 119.0 * scale),
            egui::Align2::LEFT_TOP,
            "SAME CITY. NEW STORIES.",
            FontId::proportional((10.0 * scale).max(9.0)),
            ui_theme::ACCENT,
        );
        painter.line_segment(
            [
                Pos2::new(left, top + 127.0 * scale),
                Pos2::new(left + 118.0 * scale, top + 127.0 * scale),
            ],
            egui::Stroke::new(2.0 * scale, ui_theme::ACCENT),
        );

        // A compact session indicator replaces a floating, unanchored status label.
        let status = if self.network_active {
            "ONLINE SESSION"
        } else {
            "STORY MODE"
        };
        let status_color = if self.network_active {
            ui_theme::SUCCESS
        } else {
            ui_theme::ACCENT
        };
        painter.text(
            Pos2::new(right, top + 7.0 * scale),
            egui::Align2::RIGHT_TOP,
            status,
            FontId::proportional((12.0 * scale).max(10.0)),
            status_color,
        );
        painter.line_segment(
            [
                Pos2::new(right - 112.0 * scale, top + 28.0 * scale),
                Pos2::new(right, top + 28.0 * scale),
            ],
            egui::Stroke::new(1.0, Color32::from_white_alpha(70)),
        );

        let body_top = top + 151.0 * scale;
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

        // The three primary destinations use the same interaction geometry, with
        // a narrow accent rail and a quiet surface instead of oversized gold blocks.
        for (slot, index) in [0, 1, 10].into_iter().enumerate() {
            let rect = Rect::from_min_size(
                Pos2::new(left, body_top + slot as f32 * 83.0 * scale),
                Vec2::new(width, 72.0 * scale),
            );
            let selected = self.selected == index;
            let response = ui.interact(rect, egui::Id::new(("front-play", index)), Sense::click());
            let active = selected || response.hovered() || response.has_focus();
            let fill = if active {
                Color32::from_rgba_unmultiplied(18, 27, 36, 245)
            } else {
                Color32::from_rgba_unmultiplied(10, 15, 21, 205)
            };
            painter.rect_filled(rect, 2, fill);
            painter.rect_filled(
                Rect::from_min_size(
                    rect.min,
                    Vec2::new(if active { 4.0 } else { 2.0 } * scale, rect.height()),
                ),
                0,
                if active {
                    ui_theme::ACCENT
                } else {
                    Color32::from_rgb(66, 78, 86)
                },
            );
            painter.line_segment(
                [rect.left_bottom(), rect.right_bottom()],
                egui::Stroke::new(1.0, Color32::from_white_alpha(if active { 58 } else { 25 })),
            );
            let title_color = if active { ui_theme::TEXT } else { WHITE };
            painter.text(
                rect.min + Vec2::new(17.0, 12.0) * scale,
                egui::Align2::LEFT_TOP,
                labels[index].to_uppercase(),
                FontId::proportional((17.0 * scale).max(14.0)),
                title_color,
            );
            let detail = match index {
                0 => {
                    if self.has_checkpoint {
                        "Return to your last saved location"
                    } else {
                        "Start your story on Grove Street"
                    }
                }
                1 => "Explore the map and choose a destination",
                _ => "Host a session or join your friends",
            };
            let galley = painter.layout(
                detail.into(),
                FontId::proportional((12.5 * scale).max(11.0)),
                MUTED,
                rect.width() - 45.0 * scale,
            );
            painter.galley(rect.min + Vec2::new(17.0, 42.0) * scale, galley, MUTED);
            painter.text(
                rect.right_center() - Vec2::new(17.0 * scale, 0.0),
                egui::Align2::RIGHT_CENTER,
                "›",
                FontId::proportional(25.0 * scale),
                if active { ui_theme::ACCENT } else { MUTED },
            );
            if response.clicked() || (selected && keyboard_select) {
                self.selected = index;
                match index {
                    0 => action = Some(Action::Play),
                    1 => self.open(Page::Map),
                    _ => self.open(Page::Network),
                }
            }
        }

        // Secondary destinations use a compact two-column grid so the title
        // screen stays balanced without turning into a long settings list.
        let grid_gap = 8.0 * scale;
        let grid_width = (right - secondary_left).min(430.0 * scale);
        let row_height = 48.0 * scale;
        let column_width = (grid_width - grid_gap) * 0.5;
        for (slot, index) in (2..10).enumerate() {
            let rect = Rect::from_min_size(
                Pos2::new(
                    secondary_left + (slot % 2) as f32 * (column_width + grid_gap),
                    body_top + (slot / 2) as f32 * (row_height + grid_gap),
                ),
                Vec2::new(column_width, row_height),
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
                Pos2::new(left, footer - 79.0 * scale),
                egui::Align2::LEFT_BOTTOM,
                "CURRENT AREA",
                FontId::proportional((10.0 * scale).max(9.0)),
                MUTED,
            );
            painter.text(
                Pos2::new(left, footer - 59.0 * scale),
                egui::Align2::LEFT_BOTTOM,
                location.to_uppercase(),
                FontId::new(26.0 * scale, FontFamily::Name("title".into())),
                ui_theme::ACCENT,
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
        ui_theme::key_hint(&mut hints, "ENTER / A", "SELECT");
        ui_theme::key_hint(&mut hints, "ARROWS / D-PAD", "MOVE");
        if screen.width() > 950.0 {
            painter.text(
                Pos2::new(right, footer),
                egui::Align2::RIGHT_CENTER,
                format!("SARE v{}", sa_client::VERSION),
                FontId::proportional((12.0 * scale).max(10.0)),
                MUTED,
            );
        }
        if !self.message.is_empty() {
            let galley = painter.layout(
                self.message.clone(),
                FontId::proportional(13.0 * scale),
                ui_theme::ACCENT,
                right - left,
            );
            painter.galley(
                Pos2::new(left, footer - 40.0 * scale),
                galley,
                ui_theme::ACCENT,
            );
        }
        action
    }
}
