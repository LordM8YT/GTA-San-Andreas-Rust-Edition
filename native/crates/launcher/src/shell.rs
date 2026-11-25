//! SARE Client shell: the persistent sidebar, the compact header and the
//! shared pieces pages are built from (experience cards, empty states).
//! Pure presentation: callers get back what the player chose.
use crate::{
    experience::{Experience, Kind},
    ui_theme::{ACCENT, MUTED, SURFACE, SURFACE_HIGH, TEXT},
    Page,
};
use eframe::egui::{self, Color32, RichText};

pub const SIDEBAR: Color32 = Color32::from_rgb(15, 20, 26);

/// Sidebar groups. Add a page here to give it a place in the navigation.
pub const SECTIONS: &[(&str, &[(Page, &str)])] = &[
    (
        "",
        &[
            (Page::Home, "Home"),
            (Page::Discover, "Discover"),
            (Page::Library, "Library"),
            (Page::Multiplayer, "Servers"),
        ],
    ),
    (
        "SOCIAL",
        &[(Page::Friends, "Friends"), (Page::Messages, "Messages")],
    ),
    (
        "YOU",
        &[(Page::Profile, "Profile"), (Page::Create, "Create")],
    ),
    (
        "SYSTEM",
        &[
            (Page::Resources, "Mods"),
            (Page::Settings, "Settings"),
            (Page::Help, "Help"),
        ],
    ),
];

/// Draw the sidebar and return the page the player picked, if any.
pub fn sidebar(ui: &mut egui::Ui, current: Page) -> Option<Page> {
    let mut chosen = None;
    ui.spacing_mut().item_spacing.y = 2.;
    ui.add_space(4.);
    ui.horizontal(|ui| {
        ui.add_space(10.);
        ui.label(
            RichText::new("SARE")
                .font(egui::FontId::new(
                    26.,
                    egui::FontFamily::Name("title".into()),
                ))
                .color(ACCENT),
        );
        ui.label(RichText::new("CLIENT").size(11.).color(MUTED));
    });
    ui.add_space(10.);
    for (title, pages) in SECTIONS {
        if !title.is_empty() {
            ui.add_space(6.);
            ui.horizontal(|ui| {
                ui.add_space(12.);
                ui.label(RichText::new(*title).size(11.).color(MUTED).strong());
            });
        }
        for (page, label) in pages.iter() {
            if nav_item(ui, *page, label, current == *page).clicked() {
                chosen = Some(*page);
            }
        }
    }
    chosen
}

fn nav_item(ui: &mut egui::Ui, page: Page, label: &str, active: bool) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 34.), egui::Sense::click());
    let focused = response.has_focus();
    let fill = if active {
        SURFACE_HIGH
    } else if response.hovered() || focused {
        SURFACE
    } else {
        Color32::TRANSPARENT
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 6, fill);
    if active {
        painter.rect_filled(
            egui::Rect::from_min_size(rect.left_top() + egui::vec2(0., 8.), egui::vec2(3., 18.)),
            2,
            ACCENT,
        );
    }
    if focused {
        painter.rect_stroke(
            rect,
            6,
            egui::Stroke::new(1., ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    let color = if active { ACCENT } else { MUTED };
    icon(
        painter,
        rect.left_center() + egui::vec2(22., 0.),
        page,
        color,
    );
    painter.text(
        rect.left_center() + egui::vec2(42., 0.),
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(15.),
        if active {
            TEXT
        } else {
            MUTED.gamma_multiply(1.15)
        },
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
    response
}

/// Header actions.
pub enum HeaderAction {
    Search,
    Profile,
    Settings,
}

/// Compact header: global search on the left, the player on the right.
pub fn header(ui: &mut egui::Ui, search: &mut String, player: &str) -> Option<HeaderAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        let search_box = ui.add(
            egui::TextEdit::singleline(search)
                .hint_text("Search experiences")
                .desired_width((ui.available_width() - 180.).clamp(120., 420.))
                .margin(egui::vec2(12., 8.))
                .char_limit(80),
        );
        if search_box.changed()
            || (search_box.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
        {
            action = Some(HeaderAction::Search);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(egui::Button::new(RichText::new("⚙").size(17.)).frame(false))
                .on_hover_text("Settings")
                .clicked()
            {
                action = Some(HeaderAction::Settings);
            }
            let profile = ui
                .add(
                    egui::Button::new(RichText::new(player).size(14.))
                        .wrap_mode(egui::TextWrapMode::Truncate)
                        .frame(false)
                        .min_size(egui::vec2(0., 32.)),
                )
                .on_hover_text("Profile");
            avatar(
                ui.painter(),
                profile.rect.left_center() - egui::vec2(16., 0.),
                13.,
                player,
            );
            ui.add_space(26.);
            if profile.clicked() {
                action = Some(HeaderAction::Profile);
            }
        });
    });
    action
}

/// Round avatar with the player's initial. There are no account pictures yet.
pub fn avatar(painter: &egui::Painter, center: egui::Pos2, radius: f32, name: &str) {
    painter.circle_filled(center, radius, tint(name, 0.55));
    let initial: String = name.chars().next().unwrap_or('P').to_uppercase().collect();
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        initial,
        egui::FontId::proportional(radius * 1.05),
        TEXT,
    );
}

/// A stable, muted colour per name, so covers stay the same between runs.
fn tint(seed: &str, light: f32) -> Color32 {
    let hash = seed.bytes().fold(2166136261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16777619)
    });
    let hue = (hash % 360) as f32 / 360.;
    let rgb = egui::ecolor::Hsva::new(hue, 0.45, light, 1.).to_srgb();
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Original cover art for an experience: San Andreas uses the player's own
/// Home artwork (or the default scene), other worlds a gradient and monogram.
pub fn cover(
    ui: &egui::Ui,
    rect: egui::Rect,
    experience: &Experience,
    hero: Option<&egui::TextureHandle>,
) {
    if experience.kind == Kind::SanAndreas {
        crate::artwork::background(ui, rect, hero);
        return;
    }
    let painter = ui.painter().with_clip_rect(rect);
    let mut mesh = egui::Mesh::default();
    let top = tint(&experience.id, 0.42);
    let bottom = tint(&experience.name, 0.16);
    for (pos, color) in [
        (rect.left_top(), top),
        (rect.right_top(), top.gamma_multiply(0.8)),
        (rect.right_bottom(), bottom),
        (rect.left_bottom(), bottom),
    ] {
        mesh.colored_vertex(pos, color);
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
    let monogram: String = experience
        .name
        .split_whitespace()
        .filter_map(|w| w.chars().next())
        .take(2)
        .collect::<String>()
        .to_uppercase();
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        monogram,
        egui::FontId::new(rect.height() * 0.36, egui::FontFamily::Name("title".into())),
        Color32::from_rgba_unmultiplied(255, 255, 255, 60),
    );
}

/// What a card's buttons asked for.
pub enum CardAction {
    Play,
    Edit,
    Setup,
}

/// Width-filling grid of experience cards.
pub fn card_grid(
    ui: &mut egui::Ui,
    list: &[&Experience],
    hero: Option<&egui::TextureHandle>,
    can_play: impl Fn(&Experience) -> bool,
    needs_setup: impl Fn(&Experience) -> bool,
) -> Option<(usize, CardAction)> {
    let gap = 16.;
    let available = ui.available_width();
    let columns = ((available + gap) / (230. + gap)).floor().max(1.) as usize;
    let width = ((available - gap * (columns - 1) as f32) / columns as f32).floor();
    let mut action = None;
    for (row, chunk) in list.chunks(columns).enumerate() {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for (column, experience) in chunk.iter().enumerate() {
                let index = row * columns + column;
                ui.allocate_ui_with_layout(
                    egui::vec2(width, 0.),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(width);
                        if let Some(chosen) = card(
                            ui,
                            experience,
                            hero,
                            can_play(experience),
                            needs_setup(experience),
                        ) {
                            action = Some((index, chosen));
                        }
                    },
                );
            }
        });
        ui.add_space(gap);
    }
    action
}

fn card(
    ui: &mut egui::Ui,
    experience: &Experience,
    hero: Option<&egui::TextureHandle>,
    can_play: bool,
    needs_setup: bool,
) -> Option<CardAction> {
    let width = ui.available_width();
    let mut action = None;
    let frame = egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(8)
        .show(ui, |ui| {
            ui.set_width(width);
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(width, width * 9. / 16.), egui::Sense::hover());
            cover(ui, rect, experience, hero);
            egui::Frame::new()
                .inner_margin(egui::Margin::symmetric(12, 10))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.;
                    ui.add(
                        egui::Label::new(RichText::new(&experience.name).size(16.).strong())
                            .truncate(),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(&experience.creator).size(12.5).color(MUTED),
                        )
                        .truncate(),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(&experience.description)
                                .size(13.)
                                .color(MUTED),
                        )
                        .truncate(),
                    )
                    .on_hover_text(&experience.description);
                    ui.add_space(6.);
                    ui.horizontal(|ui| {
                        if needs_setup {
                            if ui
                                .button("Set up")
                                .on_hover_text(
                                    "Choose your own San Andreas installation in Settings",
                                )
                                .clicked()
                            {
                                action = Some(CardAction::Setup);
                            }
                        } else if ui
                            .add_enabled(can_play, crate::primary("Play"))
                            .on_disabled_hover_text("Close the running game first")
                            .clicked()
                        {
                            action = Some(CardAction::Play);
                        }
                        if experience.editable
                            && ui
                                .add_enabled(can_play, egui::Button::new("Edit"))
                                .on_hover_text("Open in Build Mode")
                                .clicked()
                        {
                            action = Some(CardAction::Edit);
                        }
                        if let Some(version) = &experience.version {
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add(
                                        egui::Label::new(
                                            RichText::new(format!("v{version}"))
                                                .size(12.)
                                                .color(MUTED),
                                        )
                                        .extend(),
                                    );
                                },
                            );
                        }
                    });
                });
        });
    if frame.response.hovered() {
        ui.painter().rect_stroke(
            frame.response.rect,
            8,
            egui::Stroke::new(1., ACCENT.gamma_multiply(0.6)),
            egui::StrokeKind::Inside,
        );
    }
    action
}

/// A page heading with an optional line under it.
pub fn title(ui: &mut egui::Ui, text: &str, subtitle: &str) {
    ui.label(RichText::new(text).size(30.).strong().color(TEXT));
    if !subtitle.is_empty() {
        ui.label(RichText::new(subtitle).color(MUTED));
    }
    ui.add_space(14.);
}

/// Section heading inside a page.
pub fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.);
    ui.label(RichText::new(text).size(20.).strong().color(TEXT));
    ui.add_space(6.);
}

/// An honest empty state: what is missing, why, and what works today.
/// Returns true when the optional button was clicked.
pub fn empty_state(
    ui: &mut egui::Ui,
    page: Page,
    heading: &str,
    body: &str,
    button: Option<&str>,
) -> bool {
    let mut clicked = false;
    egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(10)
        .inner_margin(egui::Margin::symmetric(28, 28))
        .show(ui, |ui| {
            ui.set_width(ui.available_width().min(640.));
            let (rect, _) = ui.allocate_exact_size(egui::vec2(44., 44.), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 22., SURFACE_HIGH);
            icon(ui.painter(), rect.center(), page, ACCENT);
            ui.add_space(6.);
            ui.label(RichText::new(heading).size(19.).strong());
            ui.label(RichText::new(body).color(MUTED));
            if let Some(label) = button {
                ui.add_space(8.);
                clicked = ui.button(label).clicked();
            }
        });
    clicked
}

/// Original outline icons, drawn so no icon font or image is needed.
pub fn icon(painter: &egui::Painter, center: egui::Pos2, page: Page, color: Color32) {
    let stroke = egui::Stroke::new(1.5, color);
    let p = |x, y| center + egui::vec2(x, y);
    let line = |points: Vec<egui::Pos2>| {
        painter.add(egui::Shape::line(points, stroke));
    };
    match page {
        Page::Home => {
            line(vec![p(-8., 0.), p(0., -7.), p(8., 0.)]);
            line(vec![p(-6., -1.), p(-6., 7.), p(6., 7.), p(6., -1.)]);
        }
        Page::Discover => {
            painter.circle_stroke(center, 8., stroke);
            painter.add(egui::Shape::convex_polygon(
                vec![p(4., -4.), p(1.5, 1.5), p(-4., 4.), p(-1.5, -1.5)],
                color,
                egui::Stroke::NONE,
            ));
        }
        Page::Library => {
            for x in [-6., -1.5] {
                painter.rect_stroke(
                    egui::Rect::from_min_size(p(x, -7.), egui::vec2(3.5, 14.)),
                    1,
                    stroke,
                    egui::StrokeKind::Inside,
                );
            }
            line(vec![
                p(3., -6.),
                p(6.5, -7.),
                p(8., 6.5),
                p(4.5, 7.5),
                p(3., -6.),
            ]);
        }
        Page::Multiplayer => {
            for y in [-6., 2.] {
                painter.rect_stroke(
                    egui::Rect::from_min_size(p(-7., y), egui::vec2(14., 5.)),
                    1,
                    stroke,
                    egui::StrokeKind::Inside,
                );
                painter.circle_filled(p(4., y + 2.5), 1., color);
            }
        }
        Page::Friends => {
            painter.circle_stroke(p(-3., -3.), 3., stroke);
            line(vec![
                p(-8., 7.),
                p(-7., 3.),
                p(-3., 1.5),
                p(1., 3.),
                p(2., 7.),
            ]);
            painter.circle_stroke(p(4.5, -4.), 2.5, stroke);
            line(vec![p(4., 1.), p(7., 2.), p(8., 6.)]);
        }
        Page::Messages => {
            line(vec![
                p(-8., -6.),
                p(8., -6.),
                p(8., 4.),
                p(-2., 4.),
                p(-6., 8.),
                p(-6., 4.),
                p(-8., 4.),
                p(-8., -6.),
            ]);
        }
        Page::Profile => {
            painter.circle_stroke(p(0., -3.), 3.5, stroke);
            line(vec![
                p(-6., 8.),
                p(-5., 3.),
                p(0., 1.),
                p(5., 3.),
                p(6., 8.),
            ]);
        }
        Page::Create => {
            painter.rect_stroke(
                egui::Rect::from_center_size(center, egui::vec2(16., 16.)),
                3,
                stroke,
                egui::StrokeKind::Inside,
            );
            painter.line_segment([p(0., -4.), p(0., 4.)], stroke);
            painter.line_segment([p(-4., 0.), p(4., 0.)], stroke);
        }
        Page::Resources => {
            painter.add(egui::Shape::closed_line(
                vec![
                    p(-7., -4.),
                    p(0., -8.),
                    p(7., -4.),
                    p(7., 5.),
                    p(0., 9.),
                    p(-7., 5.),
                ],
                stroke,
            ));
            painter.line_segment([p(-7., -4.), p(0., 0.)], stroke);
            painter.line_segment([p(7., -4.), p(0., 0.)], stroke);
            painter.line_segment([p(0., 0.), p(0., 9.)], stroke);
        }
        Page::Settings => {
            for (x, y) in [(-5., -3.), (0., 4.), (5., -1.)] {
                painter.line_segment([p(x, -8.), p(x, 8.)], stroke);
                painter.circle_filled(p(x, y), 2.5, color);
            }
        }
        Page::Help => {
            painter.circle_stroke(center, 8., stroke);
            painter.line_segment([p(0., -1.), p(0., 5.)], stroke);
            painter.circle_filled(p(0., -4.), 1., color);
        }
    }
}
