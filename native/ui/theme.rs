//! Shared native interface palette and quiet, static San Andreas-inspired artwork.
use egui::{Color32, FontId, Pos2, Rect, RichText, Vec2};

pub const BACKGROUND: Color32 = Color32::from_rgb(15, 21, 29);
pub const SURFACE: Color32 = Color32::from_rgb(24, 33, 44);
pub const SURFACE_HIGH: Color32 = Color32::from_rgb(35, 46, 59);
pub const TEXT: Color32 = Color32::from_rgb(245, 241, 230);
pub const MUTED: Color32 = Color32::from_rgb(169, 181, 193);
pub const ACCENT: Color32 = Color32::from_rgb(228, 188, 119);
pub const SUCCESS: Color32 = Color32::from_rgb(143, 194, 165);

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
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(52, 66, 79);
    style.visuals.widgets.hovered.weak_bg_fill = SURFACE_HIGH;
    style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(74, 67, 49);
    style.visuals.widgets.active.bg_stroke = egui::Stroke::new(2.0, ACCENT);
    style.spacing.item_spacing = Vec2::new(12.0, 12.0);
    style.spacing.button_padding = Vec2::new(14.0, 9.0);
    style.spacing.interact_size.y = 34.0;
    style.spacing.scroll = egui::style::ScrollStyle::solid();
    for (kind, size) in [
        (egui::TextStyle::Heading, 28.0),
        (egui::TextStyle::Body, 16.0),
        (egui::TextStyle::Button, 16.0),
        (egui::TextStyle::Small, 13.0),
    ] {
        style.text_styles.insert(kind, FontId::proportional(size));
    }
    ctx.set_theme(egui::Theme::Dark);
    ctx.set_style_of(egui::Theme::Dark, style);
}

pub fn key_hint(ui: &mut egui::Ui, key: &str, label: &str) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        egui::Frame::new()
            .fill(SURFACE_HIGH)
            .corner_radius(2)
            .inner_margin(egui::Margin::symmetric(5, 2))
            .show(ui, |ui| {
                ui.label(RichText::new(key).size(11.0).color(TEXT).strong());
            });
        ui.label(RichText::new(label).size(12.0).color(MUTED));
    });
}

/// Original vector artwork. No game assets, image downloads or animated effects.
pub fn horizon(painter: &egui::Painter, rect: Rect) {
    let painter = painter.with_clip_rect(rect);
    let mut sky = egui::Mesh::default();
    for (pos, color) in [
        (rect.left_top(), BACKGROUND),
        (rect.right_top(), Color32::from_rgb(51, 66, 83)),
        (rect.right_bottom(), Color32::from_rgb(102, 81, 66)),
        (rect.left_bottom(), SURFACE),
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
    let sun = at(0.76, 0.4);
    painter.circle_filled(sun, rect.height() * 0.19, Color32::from_rgb(191, 150, 104));
    for (ridge, color) in [
        (0.63, Color32::from_rgb(41, 52, 62)),
        (0.79, Color32::from_rgb(23, 33, 43)),
    ] {
        let ridge_points = [
            at(0.0, ridge),
            at(0.35, ridge - 0.15),
            at(0.6, ridge + 0.03),
            at(0.87, ridge - 0.11),
            at(1.0, ridge),
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
        (0.48, 0.16, 0.045),
        (0.55, 0.25, 0.03),
        (0.6, 0.13, 0.06),
        (0.69, 0.2, 0.035),
        (0.75, 0.12, 0.05),
    ] {
        painter.rect_filled(
            Rect::from_min_max(at(x, 0.88 - height), at(x + width, 1.0)),
            0,
            BACKGROUND,
        );
    }
    for (x, y, height) in [(0.9, 0.43, 0.55), (0.99, 0.59, 0.4)] {
        let crown = at(x, y);
        let bottom = at(x - 0.025, y + height);
        painter.line_segment(
            [bottom, crown],
            egui::Stroke::new((rect.height() * 0.012).max(2.0), BACKGROUND),
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
                BACKGROUND,
                egui::Stroke::NONE,
            ));
        }
    }
    // Readable text on the left, with the scene left visible on the right.
    let mut shade = egui::Mesh::default();
    for (pos, color) in [
        (rect.left_top(), Color32::from_black_alpha(155)),
        (rect.right_top(), Color32::TRANSPARENT),
        (rect.right_bottom(), Color32::TRANSPARENT),
        (rect.left_bottom(), Color32::from_black_alpha(155)),
    ] {
        shade.colored_vertex(pos, color);
    }
    shade.add_triangle(0, 1, 2);
    shade.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(shade));
}
