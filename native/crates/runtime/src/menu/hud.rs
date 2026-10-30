//! A single top-right status group prevents the clock, ammo and session overlapping.
use super::*;

impl Menu {
    pub(super) fn status_hud(&self, ctx: &egui::Context, scale: f32, fps: f32) {
        egui::Area::new("sare-status".into())
            .anchor(egui::Align2::RIGHT_TOP, Vec2::new(-28.0, 26.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(ui_theme::SURFACE)
                    .corner_radius(3)
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.set_width(210.0 * scale);
                        ui.spacing_mut().item_spacing.y = 5.0;
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(format!("{:02}:{:02}", self.clock.0, self.clock.1))
                                    .size(28.0 * scale)
                                    .color(WHITE)
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
                        if self.settings.show_fps {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("FPS").size(11.0 * scale).color(MUTED));
                                ui.label(
                                    RichText::new(format!("{fps:.0}"))
                                        .size(15.0 * scale)
                                        .color(if fps >= 60.0 { ui_theme::SUCCESS } else { GOLD })
                                        .strong(),
                                );
                            });
                        }
                        if self.wanted.0 > 0 {
                            let blink = ctx.input(|input| input.time * 3.0).fract() < 0.5;
                            wanted_stars(ui, self.wanted.0, self.wanted.1 && blink, scale);
                            if self.wanted.1 {
                                ctx.request_repaint();
                            }
                        }
                        if let Some((name, loaded, spare, _, reloading)) = &self.weapon_hud {
                            ui.separator();
                            ui.label(RichText::new(name).size(15.0 * scale).color(WHITE));
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("{loaded:02}"))
                                        .size(27.0 * scale)
                                        .color(GOLD)
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

    /// "BUSTED" across the screen for a few seconds after an arrest, over a
    /// darkening, the way SA cuts away from the arrest.
    pub(super) fn busted_banner(&mut self, ctx: &egui::Context, scale: f32) {
        let now = ctx.input(|input| input.time);
        if std::mem::take(&mut self.busted) {
            self.busted_at = Some(now);
        }
        let Some(age) = self.busted_at.map(|at| now - at) else {
            return;
        };
        if age > BUSTED_SECONDS {
            self.busted_at = None;
            return;
        }
        let fade = ((BUSTED_SECONDS - age) / 0.8)
            .min(age / 0.3)
            .clamp(0.0, 1.0) as f32;
        let screen = ctx.content_rect();
        ctx.layer_painter(egui::LayerId::new(
            egui::Order::Background,
            "busted-shade".into(),
        ))
        .rect_filled(screen, 0.0, Color32::from_black_alpha((150.0 * fade) as u8));
        egui::Area::new("busted".into())
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .interactable(false)
            .show(ctx, |ui| {
                outlined_text(
                    ui,
                    "BUSTED",
                    FontId::new(76.0 * scale, FontFamily::Name("title".into())),
                    Color32::from_rgba_unmultiplied(90, 150, 230, (255.0 * fade) as u8),
                );
            });
        ctx.request_repaint();
    }
}

/// Most wanted stars (`wanted::MAX_LEVEL`; the menu is also built without
/// the game around it).
const STARS: u8 = 6;
/// Seconds the busted banner stays up.
const BUSTED_SECONDS: f64 = 4.0;

/// A row of six stars, the first `level` lit. Painted as shapes so they do
/// not depend on the font having a star glyph. `dim` blanks the lit ones,
/// for flashing.
fn wanted_stars(ui: &mut egui::Ui, level: u8, dim: bool, scale: f32) {
    let size = 22.0 * scale;
    let (rect, _) =
        ui.allocate_exact_size(Vec2::new(size * f32::from(STARS), size), Sense::hover());
    let mut mesh = egui::Mesh::default();
    for star in 0..STARS {
        let lit = star < level && !dim;
        let color = if lit {
            GOLD
        } else {
            Color32::from_rgba_unmultiplied(255, 255, 255, 40)
        };
        let center = Pos2::new(
            rect.left() + size * (f32::from(star) + 0.5),
            rect.center().y,
        );
        let points: Vec<Pos2> = (0..10)
            .map(|i| {
                let angle = std::f32::consts::PI * (i as f32 / 5.0 - 0.5);
                let radius = if i % 2 == 0 { 0.45 } else { 0.19 } * size;
                center + Vec2::new(angle.cos(), angle.sin()) * radius
            })
            .collect();
        let base = mesh.vertices.len() as u32;
        mesh.colored_vertex(center, color);
        for point in &points {
            mesh.colored_vertex(*point, color);
        }
        for i in 0..10 {
            mesh.add_triangle(base, base + 1 + i, base + 1 + (i + 1) % 10);
        }
    }
    ui.painter().add(mesh);
}
