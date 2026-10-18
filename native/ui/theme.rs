//! Shared SARE interface tokens and original San Andreas-inspired artwork.
use egui::{Color32, FontId, Pos2, Rect, RichText, Vec2};

/// The same restrained palette is used by the launcher, menus and in-game HUD.
pub const BACKGROUND: Color32 = Color32::from_rgb(10, 15, 21);
pub const SURFACE: Color32 = Color32::from_rgb(18, 27, 36);
pub const SURFACE_HIGH: Color32 = Color32::from_rgb(28, 40, 51);
pub const TEXT: Color32 = Color32::from_rgb(244, 242, 233);
pub const MUTED: Color32 = Color32::from_rgb(164, 176, 186);
pub const ACCENT: Color32 = Color32::from_rgb(230, 180, 92);
pub const SUCCESS: Color32 = Color32::from_rgb(143, 203, 160);
pub const WARNING: Color32 = Color32::from_rgb(237, 139, 105);

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = egui::Visuals::dark();
    style.wrap_mode = Some(egui::TextWrapMode::Wrap);
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.panel_fill = BACKGROUND;
    style.visuals.window_fill = SURFACE;
    style.visuals.extreme_bg_color = BACKGROUND;
    style.visuals.faint_bg_color = SURFACE;
    style.visuals.selection.bg_fill = ACCENT;
    style.visuals.selection.stroke = egui::Stroke::new(1.0, BACKGROUND);
    style.visuals.hyperlink_color = ACCENT;
    style.visuals.widgets.noninteractive.bg_fill = SURFACE;
    style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, SURFACE_HIGH);
    style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, MUTED);
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.corner_radius = egui::CornerRadius::same(3);
        widget.fg_stroke = egui::Stroke::new(1.0, TEXT);
    }
    style.visuals.widgets.inactive.bg_fill = SURFACE_HIGH;
    style.visuals.widgets.inactive.weak_bg_fill = SURFACE;
    style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, SURFACE_HIGH);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(40, 53, 65);
    style.visuals.widgets.hovered.weak_bg_fill = SURFACE_HIGH;
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(73, 59, 39);
    style.visuals.widgets.active.bg_stroke = egui::Stroke::new(1.5, ACCENT);
    style.spacing.item_spacing = Vec2::new(10.0, 9.0);
    style.spacing.button_padding = Vec2::new(13.0, 8.0);
    style.spacing.interact_size.y = 34.0;
    style.spacing.scroll = egui::style::ScrollStyle::solid();
    for (kind, size) in [
        (egui::TextStyle::Heading, 27.0),
        (egui::TextStyle::Body, 15.5),
        (egui::TextStyle::Button, 15.0),
        (egui::TextStyle::Small, 12.5),
    ] {
        style.text_styles.insert(kind, FontId::proportional(size));
    }
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
}

/// Compact keyboard/controller hints, styled like a small in-game control legend.
pub fn key_hint(ui: &mut egui::Ui, key: &str, label: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        egui::Frame::new()
            .fill(SURFACE_HIGH)
            .stroke(egui::Stroke::new(1.0, Color32::from_rgb(54, 67, 77)))
            .corner_radius(3)
            .inner_margin(egui::Margin::symmetric(6, 3))
            .show(ui, |ui| {
                ui.label(RichText::new(key).size(11.0).color(TEXT).strong());
            });
        ui.label(RichText::new(label).size(12.0).color(MUTED));
    });
}

/// Original static artwork: a dusk sky, distant hills and a restrained city silhouette.
pub fn horizon(painter: &egui::Painter, rect: Rect) {
    let painter = painter.with_clip_rect(rect);
    let mut sky = egui::Mesh::default();
    for (pos, color) in [
        (rect.left_top(), Color32::from_rgb(12, 20, 30)),
        (rect.right_top(), Color32::from_rgb(43, 54, 66)),
        (rect.right_bottom(), Color32::from_rgb(119, 79, 54)),
        (rect.left_bottom(), Color32::from_rgb(24, 32, 40)),
    ] {
        sky.colored_vertex(pos, color);
    }
    sky.add_triangle(0, 1, 2);
    sky.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(sky));
    let at = |x: f32, y: f32| {
        Pos2::new(
            rect.left() + rect.width() * x,
            rect.top() + rect.height() * y,
        )
    };

    // Warm sun and layered silhouettes create depth without costly post-processing.
    let sun = at(0.77, 0.39);
    painter.circle_filled(sun, rect.height() * 0.16, Color32::from_rgb(218, 157, 94));
    painter.circle_filled(
        sun,
        rect.height() * 0.19,
        Color32::from_rgba_unmultiplied(218, 157, 94, 35),
    );
    for (ridge, color) in [
        (0.61, Color32::from_rgb(45, 55, 63)),
        (0.75, Color32::from_rgb(27, 37, 45)),
        (0.84, Color32::from_rgb(16, 24, 31)),
    ] {
        let ridge_points = [
            at(0.0, ridge),
            at(0.22, ridge - 0.08),
            at(0.43, ridge + 0.025),
            at(0.64, ridge - 0.07),
            at(0.82, ridge + 0.015),
            at(1.0, ridge - 0.015),
        ];
        let mut land = egui::Mesh::default();
        for pair in ridge_points.windows(2) {
            let first = land.vertices.len() as u32;
            for point in [
                pair[0],
                pair[1],
                Pos2::new(pair[1].x, rect.bottom()),
                Pos2::new(pair[0].x, rect.bottom()),
            ] {
                land.colored_vertex(point, color);
            }
            land.add_triangle(first, first + 1, first + 2);
            land.add_triangle(first, first + 2, first + 3);
        }
        painter.add(egui::Shape::mesh(land));
    }
    for (x, height, width) in [
        (0.40, 0.13, 0.035),
        (0.45, 0.21, 0.025),
        (0.51, 0.16, 0.05),
        (0.58, 0.25, 0.028),
        (0.63, 0.12, 0.04),
        (0.70, 0.19, 0.032),
        (0.75, 0.14, 0.04),
    ] {
        painter.rect_filled(
            Rect::from_min_max(at(x, 0.88 - height), at(x + width, 1.0)),
            0,
            Color32::from_rgb(13, 20, 26),
        );
    }
    for (x, y, height) in [(0.92, 0.44, 0.48), (0.99, 0.57, 0.35)] {
        let crown = at(x, y);
        let bottom = at(x - 0.025, y + height);
        painter.line_segment(
            [bottom, crown],
            egui::Stroke::new(
                (rect.height() * 0.012).max(2.0),
                Color32::from_rgb(9, 15, 20),
            ),
        );
        for i in 0..7 {
            let angle = -2.8 + i as f32 * 0.47;
            let length = rect.height() * 0.17;
            let tip = crown + Vec2::angled(angle) * length;
            let middle = crown + Vec2::angled(angle + 0.18) * length * 0.55;
            painter.add(egui::Shape::convex_polygon(
                vec![
                    crown,
                    middle,
                    tip,
                    crown + Vec2::angled(angle - 0.2) * length * 0.48,
                ],
                Color32::from_rgb(9, 15, 20),
                egui::Stroke::NONE,
            ));
        }
    }

    // A stronger left-side vignette preserves text contrast and leaves the horizon visible.
    let mut shade = egui::Mesh::default();
    for (pos, color) in [
        (rect.left_top(), Color32::from_black_alpha(195)),
        (rect.right_top(), Color32::from_black_alpha(15)),
        (rect.right_bottom(), Color32::from_black_alpha(15)),
        (rect.left_bottom(), Color32::from_black_alpha(195)),
    ] {
        shade.colored_vertex(pos, color);
    }
    shade.add_triangle(0, 1, 2);
    shade.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(shade));
}
