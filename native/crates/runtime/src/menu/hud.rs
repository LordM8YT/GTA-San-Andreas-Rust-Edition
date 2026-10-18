//! A single top-right status group prevents the clock, ammo and session overlapping.
use super::*;

impl Menu {
    pub(super) fn status_hud(&self, ctx: &egui::Context, scale: f32) {
        egui::Area::new("sare-status".into())
            .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-24.0 * scale, 24.0 * scale))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(ui_theme::SURFACE)
                    .corner_radius(egui::CornerRadius::same(4))
                    .inner_margin((12.0 * scale) as i8)
                    .show(ui, |ui| {
                        ui.set_width(220.0 * scale);
                        ui.spacing_mut().item_spacing.y = 5.0;
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{:02}:{:02}", self.clock.0, self.clock.1))
                                    .size(28.0 * scale)
                                    .color(ui_theme::TEXT)
                                    .strong(),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        RichText::new(if self.network_active {
                                            "Online"
                                        } else {
                                            "Free roam"
                                        })
                                        .size(12.0 * scale)
                                        .color(
                                            if self.network_active {
                                                ui_theme::SUCCESS
                                            } else {
                                                MUTED
                                            },
                                        ),
                                    );
                                },
                            );
                        });
                        if let Some((name, loaded, spare, _, reloading)) = &self.weapon_hud {
                            ui.separator();
                            ui.label(RichText::new(name).size(15.0 * scale).color(WHITE));
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{loaded:02}"))
                                        .size(27.0 * scale)
                                        .color(ui_theme::ACCENT)
                                        .strong(),
                                );
                                ui.label(
                                    RichText::new(format!("/ {spare}"))
                                        .size(15.0 * scale)
                                        .color(MUTED),
                                );
                            });
                            if *reloading {
                                ui.label(
                                    RichText::new("Reloading…").size(12.0 * scale).color(GOLD),
                                );
                            }
                        }
                        if self.network_active {
                            ui.separator();
                            ui.label(
                                RichText::new(format!(
                                    "{} / 20 players",
                                    self.network_players.len()
                                ))
                                .size(13.0 * scale)
                                .color(ui_theme::SUCCESS),
                            );
                            ui.label(
                                RichText::new(&self.network_status)
                                    .size(12.0 * scale)
                                    .color(MUTED),
                            );
                            if !self.ride_status.is_empty() {
                                ui.label(
                                    RichText::new(&self.ride_status)
                                        .size(12.0 * scale)
                                        .color(GOLD),
                                );
                            }
                        }
                    });
            });
    }
}
