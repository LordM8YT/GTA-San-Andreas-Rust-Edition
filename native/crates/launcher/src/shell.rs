//! SARE Client shell: fonts, colours, the persistent sidebar, the compact
//! header and the shared pieces pages are built from (experience cards,
//! empty states). Pure presentation: callers get back what the player chose.
//!
//! Type is Inter (SIL OFL, `assets/fonts`), icons are Phosphor (MIT, via
//! `egui-phosphor`). Only the SARE wordmark keeps Archivo Black.
use crate::{
    experience::{Experience, Kind},
    Page,
};
use eframe::egui::{self, Color32, FontFamily, FontId, RichText};
use egui_phosphor::{fill, regular as icons};
use palette::*;
use std::collections::HashMap;

/// Launcher colours: neutral greys with one accent, so images carry the colour.
pub mod palette {
    use eframe::egui::Color32;
    pub const BACKGROUND: Color32 = Color32::from_rgb(14, 16, 20);
    pub const SIDEBAR: Color32 = Color32::from_rgb(18, 20, 25);
    pub const SURFACE: Color32 = Color32::from_rgb(24, 27, 33);
    pub const SURFACE_HIGH: Color32 = Color32::from_rgb(34, 38, 46);
    pub const HOVER: Color32 = Color32::from_rgb(43, 48, 58);
    pub const BORDER: Color32 = Color32::from_rgb(40, 44, 52);
    pub const TEXT: Color32 = Color32::from_rgb(236, 238, 242);
    pub const MUTED: Color32 = Color32::from_rgb(142, 148, 158);
    /// SARE gold, used sparingly: the primary action and the active page.
    pub const ACCENT: Color32 = Color32::from_rgb(232, 182, 72);
}

const SEMIBOLD: &str = "inter-semibold";
const DISPLAY: &str = "inter-display";
const ICONS: &str = "phosphor";
const ICONS_FILL: &str = "phosphor-fill";

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(SEMIBOLD.into()))
}
pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(DISPLAY.into()))
}

/// Register Inter, the wordmark font and Phosphor icons.
pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    for (name, bytes) in [
        (
            "inter",
            &include_bytes!("../assets/fonts/Inter-Regular.ttf")[..],
        ),
        (
            SEMIBOLD,
            &include_bytes!("../assets/fonts/Inter-SemiBold.ttf")[..],
        ),
        (
            DISPLAY,
            &include_bytes!("../assets/fonts/InterDisplay-Bold.ttf")[..],
        ),
        (
            "title",
            &include_bytes!("../../runtime/assets/ArchivoBlack-Regular.ttf")[..],
        ),
    ] {
        fonts
            .font_data
            .insert(name.into(), egui::FontData::from_static(bytes).into());
    }
    let proportional = fonts.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "inter".into());
    let fallback = proportional.clone();
    for name in [SEMIBOLD, DISPLAY, "title"] {
        let mut family = vec![name.to_owned()];
        family.extend(fallback.iter().cloned());
        fonts.families.insert(FontFamily::Name(name.into()), family);
    }
    // Icon families hold only Phosphor: Inter has glyphs of its own in the
    // private-use range Phosphor uses, so icons are never mixed into text.
    for (name, variant) in [
        (ICONS, egui_phosphor::Variant::Regular),
        (ICONS_FILL, egui_phosphor::Variant::Fill),
    ] {
        fonts.font_data.insert(
            name.into(),
            egui::FontData::from_static(variant.font_bytes()).into(),
        );
        fonts
            .families
            .insert(FontFamily::Name(name.into()), vec![name.to_owned()]);
    }
    ctx.set_fonts(fonts);
}

/// Client look on top of the shared SARE theme: flat, neutral surfaces,
/// no outlines on buttons, gold only where it means something.
pub fn apply_style(ctx: &egui::Context) {
    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    let v = &mut style.visuals;
    v.override_text_color = Some(TEXT);
    v.panel_fill = BACKGROUND;
    v.window_fill = SURFACE;
    v.extreme_bg_color = Color32::from_rgb(10, 11, 14);
    v.faint_bg_color = SURFACE;
    v.code_bg_color = SURFACE_HIGH;
    v.selection.bg_fill = ACCENT.gamma_multiply(0.45);
    v.selection.stroke = egui::Stroke::new(1.0, ACCENT);
    v.hyperlink_color = ACCENT;
    v.window_corner_radius = egui::CornerRadius::same(10);
    v.widgets.noninteractive.bg_fill = SURFACE;
    v.widgets.noninteractive.weak_bg_fill = SURFACE;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, MUTED);
    for (widget, fill) in [
        (&mut v.widgets.inactive, SURFACE_HIGH),
        (&mut v.widgets.hovered, HOVER),
        (&mut v.widgets.active, Color32::from_rgb(52, 58, 70)),
        (&mut v.widgets.open, SURFACE_HIGH),
    ] {
        widget.bg_fill = fill;
        widget.weak_bg_fill = fill;
        widget.bg_stroke = egui::Stroke::NONE;
        widget.fg_stroke = egui::Stroke::new(1.0, TEXT);
        widget.corner_radius = egui::CornerRadius::same(6);
        widget.expansion = 0.0;
    }
    // Text fields keep a quiet border so they read as inputs.
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, HOVER);
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0, ACCENT.gamma_multiply(0.7));
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(14.0, 7.0);
    style.spacing.interact_size.y = 32.0;
    for (kind, font) in [
        (egui::TextStyle::Heading, display(24.0)),
        (egui::TextStyle::Body, FontId::proportional(14.5)),
        (egui::TextStyle::Button, semibold(14.0)),
        (egui::TextStyle::Small, FontId::proportional(12.0)),
        (egui::TextStyle::Monospace, FontId::monospace(13.0)),
    ] {
        style.text_styles.insert(kind, font);
    }
    ctx.set_style_of(egui::Theme::Dark, style);
}

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
        "Social",
        &[(Page::Friends, "Friends"), (Page::Messages, "Messages")],
    ),
    (
        "You",
        &[(Page::Profile, "Profile"), (Page::Create, "Create")],
    ),
    (
        "System",
        &[
            (Page::Resources, "Mods"),
            (Page::Settings, "Settings"),
            (Page::Help, "Help"),
        ],
    ),
];

/// Phosphor icon for a page: outline normally, filled when active.
pub fn page_icon(page: Page, active: bool) -> &'static str {
    match (page, active) {
        (Page::Home, false) => icons::HOUSE,
        (Page::Home, true) => fill::HOUSE,
        (Page::Discover, false) => icons::COMPASS,
        (Page::Discover, true) => fill::COMPASS,
        (Page::Library, false) => icons::SQUARES_FOUR,
        (Page::Library, true) => fill::SQUARES_FOUR,
        (Page::Multiplayer, false) => icons::HARD_DRIVES,
        (Page::Multiplayer, true) => fill::HARD_DRIVES,
        (Page::Friends, false) => icons::USERS,
        (Page::Friends, true) => fill::USERS,
        (Page::Messages, false) => icons::CHAT_CIRCLE,
        (Page::Messages, true) => fill::CHAT_CIRCLE,
        (Page::Profile, false) => icons::USER_CIRCLE,
        (Page::Profile, true) => fill::USER_CIRCLE,
        (Page::Create, false) => icons::PLUS_SQUARE,
        (Page::Create, true) => fill::PLUS_SQUARE,
        (Page::Resources, false) => icons::PUZZLE_PIECE,
        (Page::Resources, true) => fill::PUZZLE_PIECE,
        (Page::Settings, false) => icons::GEAR_SIX,
        (Page::Settings, true) => fill::GEAR_SIX,
        (Page::Help, false) => icons::QUESTION,
        (Page::Help, true) => fill::QUESTION,
        (Page::Details, active) => page_icon(Page::Discover, active),
    }
}

pub fn icon_font(size: f32, filled: bool) -> FontId {
    FontId::new(
        size,
        FontFamily::Name(if filled { ICONS_FILL } else { ICONS }.into()),
    )
}

/// An icon to put in front of a button or label.
pub fn icon(glyph: &str, color: Color32) -> RichText {
    RichText::new(glyph)
        .font(icon_font(15., false))
        .color(color)
}

/// Draw the sidebar and return the page the player picked, if any.
pub fn sidebar(ui: &mut egui::Ui, current: Page) -> Option<Page> {
    let mut chosen = None;
    ui.spacing_mut().item_spacing.y = 2.;
    ui.add_space(6.);
    ui.horizontal(|ui| {
        ui.add_space(10.);
        ui.label(
            RichText::new("SARE")
                .font(FontId::new(24., FontFamily::Name("title".into())))
                .color(TEXT),
        );
    });
    ui.add_space(14.);
    for (group, (_, pages)) in SECTIONS.iter().enumerate() {
        if group > 0 {
            ui.add_space(8.);
            let y = ui.cursor().top();
            let x = ui.max_rect().x_range();
            ui.painter().hline(
                (x.min + 12.)..=(x.max - 12.),
                y,
                egui::Stroke::new(1., BORDER),
            );
            ui.add_space(9.);
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
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 36.), egui::Sense::click());
    let hovered = response.hovered() || response.has_focus();
    let fill = if active {
        SURFACE_HIGH
    } else if hovered {
        SURFACE
    } else {
        Color32::TRANSPARENT
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 8, fill);
    if response.has_focus() {
        painter.rect_stroke(
            rect,
            8,
            egui::Stroke::new(1., ACCENT.gamma_multiply(0.6)),
            egui::StrokeKind::Inside,
        );
    }
    let color = if active || hovered { TEXT } else { MUTED };
    painter.text(
        rect.left_center() + egui::vec2(14., 0.),
        egui::Align2::LEFT_CENTER,
        page_icon(page, active),
        icon_font(19., active),
        if active { ACCENT } else { color },
    );
    painter.text(
        rect.left_center() + egui::vec2(44., 0.),
        egui::Align2::LEFT_CENTER,
        label,
        if active {
            semibold(14.5)
        } else {
            FontId::proportional(14.5)
        },
        color,
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
                .hint_text("Search worlds and games")
                .desired_width((ui.available_width() - 180.).clamp(120., 440.))
                .margin(egui::Margin {
                    left: 36,
                    right: 12,
                    top: 8,
                    bottom: 8,
                })
                .char_limit(80),
        );
        ui.painter().text(
            search_box.rect.left_center() + egui::vec2(13., 0.),
            egui::Align2::LEFT_CENTER,
            icons::MAGNIFYING_GLASS,
            icon_font(16., false),
            MUTED,
        );
        if search_box.changed()
            || (search_box.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)))
        {
            action = Some(HeaderAction::Search);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(
                    egui::Button::new(RichText::new(icons::GEAR_SIX).font(icon_font(19., false)))
                        .frame(false),
                )
                .on_hover_text("Settings")
                .clicked()
            {
                action = Some(HeaderAction::Settings);
            }
            ui.add_space(4.);
            let profile = ui
                .add(
                    egui::Button::new(RichText::new(player).font(semibold(14.)))
                        .wrap_mode(egui::TextWrapMode::Truncate)
                        .frame(false)
                        .min_size(egui::vec2(0., 32.)),
                )
                .on_hover_text("Profile");
            avatar(
                ui.painter(),
                profile.rect.left_center() - egui::vec2(19., 0.),
                14.,
                player,
            );
            ui.add_space(36.);
            if profile.clicked() {
                action = Some(HeaderAction::Profile);
            }
        });
    });
    action
}

/// Round avatar with the player's initial. There are no account pictures yet.
pub fn avatar(painter: &egui::Painter, center: egui::Pos2, radius: f32, name: &str) {
    painter.circle_filled(center, radius, tint(name, 0.62, 0.35));
    let initial: String = name.chars().next().unwrap_or('P').to_uppercase().collect();
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        initial,
        semibold(radius),
        Color32::WHITE,
    );
}

/// A stable colour per name, so covers stay the same between runs.
fn tint(seed: &str, value: f32, saturation: f32) -> Color32 {
    let hash = seed.bytes().fold(2166136261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16777619)
    });
    let hue = (hash % 360) as f32 / 360.;
    let rgb = egui::ecolor::Hsva::new(hue, saturation, value, 1.).to_srgb();
    Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

/// Cover images, loaded the first time a card is drawn and kept until the
/// worlds are scanned again. A pack can ship `cover.png` (16:9 works best);
/// without one it gets a generated gradient.
#[derive(Default)]
pub struct Covers {
    textures: HashMap<String, (egui::TextureHandle, bool)>,
    fade: Option<egui::TextureHandle>,
}

impl Covers {
    pub fn clear(&mut self) {
        self.textures.clear();
    }
    /// Dark fade laid over banners: strong at the left and bottom.
    fn fade(&mut self, ctx: &egui::Context) -> egui::TextureHandle {
        self.fade
            .get_or_insert_with(|| {
                let (w, h) = (64usize, 32usize);
                let pixels = (0..w * h)
                    .map(|i| {
                        let (x, y) = ((i % w) as f32 / w as f32, (i / w) as f32 / h as f32);
                        let left = (1. - x / 0.7).clamp(0., 1.).powf(1.3) * 0.82;
                        let bottom = ((y - 0.35) / 0.65).clamp(0., 1.).powf(1.5) * 0.75;
                        let alpha = (left + bottom - left * bottom).max(0.12);
                        Color32::from_rgba_unmultiplied(8, 9, 12, (alpha * 255.) as u8)
                    })
                    .collect();
                ctx.load_texture(
                    "banner-fade",
                    egui::ColorImage::new([w, h], pixels),
                    egui::TextureOptions::LINEAR,
                )
            })
            .clone()
    }
    /// The cover and whether it is the pack's own image.
    fn get(&mut self, ctx: &egui::Context, experience: &Experience) -> (egui::TextureHandle, bool) {
        if let Some(found) = self.textures.get(&experience.id) {
            return found.clone();
        }
        let own = experience
            .pack_folder()
            .map(|folder| folder.join("cover.png"))
            .filter(|path| path.is_file())
            .and_then(|path| crate::artwork::load(&path).ok());
        let is_own = own.is_some();
        let texture = ctx.load_texture(
            format!("cover-{}", experience.id),
            own.unwrap_or_else(|| gradient(experience)),
            egui::TextureOptions::LINEAR,
        );
        self.textures
            .insert(experience.id.clone(), (texture.clone(), is_own));
        (texture, is_own)
    }
}

/// A soft two-colour cover with a light glow in one corner.
fn gradient(experience: &Experience) -> egui::ColorImage {
    let (a, b) = match experience.kind {
        Kind::SanAndreas => (
            Color32::from_rgb(214, 128, 70),
            Color32::from_rgb(38, 30, 58),
        ),
        Kind::Testbed => (
            Color32::from_rgb(70, 150, 170),
            Color32::from_rgb(24, 40, 56),
        ),
        Kind::Pack(_) => (
            tint(&experience.id, 0.78, 0.55),
            tint(&experience.name, 0.22, 0.6),
        ),
    };
    let (w, h) = (128usize, 72usize);
    let mix = |x: Color32, y: Color32, t: f32| {
        let l = |p: u8, q: u8| (p as f32 + (q as f32 - p as f32) * t) as u8;
        Color32::from_rgb(l(x.r(), y.r()), l(x.g(), y.g()), l(x.b(), y.b()))
    };
    let pixels = (0..w * h)
        .map(|i| {
            let (x, y) = ((i % w) as f32 / w as f32, (i / w) as f32 / h as f32);
            let t = (x * 0.35 + y * 0.65).clamp(0., 1.);
            let glow = (1. - ((x - 0.85).powi(2) + (y - 0.1).powi(2)).sqrt() * 1.6).max(0.);
            mix(mix(a, b, t), Color32::WHITE, glow * 0.18)
        })
        .collect();
    egui::ColorImage::new([w, h], pixels)
}

fn kind_icon(kind: &Kind) -> &'static str {
    match kind {
        Kind::SanAndreas => icons::MAP_TRIFOLD,
        Kind::Testbed => icons::CUBE,
        Kind::Pack(_) => icons::BUILDINGS,
    }
}

/// Paint a cover into `rect` with rounded corners, cropped to fill it.
/// San Andreas uses the player's own Home artwork when one is chosen.
pub fn cover(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    experience: &Experience,
    covers: &mut Covers,
    hero: Option<&egui::TextureHandle>,
) {
    if experience.kind == Kind::SanAndreas && hero.is_none() {
        // Original artwork; nothing from the game is shown before launch.
        crate::artwork::background(ui, rect, None);
        return;
    }
    let (texture, own) = match hero.filter(|_| experience.kind == Kind::SanAndreas) {
        Some(hero) => (hero.clone(), true),
        None => covers.get(ui.ctx(), experience),
    };
    let size = texture.size_vec2();
    let (aspect, target) = (size.x / size.y, rect.width() / rect.height());
    let uv = if aspect > target {
        let half = target / aspect * 0.5;
        egui::Rect::from_min_max(egui::pos2(0.5 - half, 0.), egui::pos2(0.5 + half, 1.))
    } else {
        let half = aspect / target * 0.5;
        egui::Rect::from_min_max(egui::pos2(0., 0.5 - half), egui::pos2(1., 0.5 + half))
    };
    egui::Image::from_texture(&texture)
        .uv(uv)
        .corner_radius(10)
        .paint_at(ui, rect);
    // Small tiles without art get their kind's icon; banners stay clean.
    if !own && rect.height() < 200. {
        ui.painter().text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            kind_icon(&experience.kind),
            icon_font(rect.height() * 0.38, false),
            Color32::from_white_alpha(70),
        );
    }
}

/// Columns that fit tiles of at least `min` points with `gap` between them.
fn columns(available: f32, min: f32, gap: f32) -> (usize, f32) {
    let columns = ((available + gap) / (min + gap)).floor().max(1.) as usize;
    let width = ((available - gap * (columns - 1) as f32) / columns as f32).floor();
    (columns, width)
}

const GAP: f32 = 16.;
const TILE: f32 = 210.;

/// Width-filling grid of tiles. Returns the index of the tile clicked.
pub fn card_grid(
    ui: &mut egui::Ui,
    list: &[&Experience],
    covers: &mut Covers,
    hero: Option<&egui::TextureHandle>,
) -> Option<usize> {
    let (columns, width) = columns(ui.available_width(), TILE, GAP);
    let mut clicked = None;
    for (row, chunk) in list.chunks(columns).enumerate() {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for (column, experience) in chunk.iter().enumerate() {
                if tile(ui, width, experience, covers, hero) {
                    clicked = Some(row * columns + column);
                }
            }
        });
        ui.add_space(GAP);
    }
    clicked
}

/// What the Create template grid asked for.
pub enum TemplatePick {
    Blank,
    Open(usize),
}

/// Templates grid with a blank world first, like a new-project picker.
pub fn template_grid(
    ui: &mut egui::Ui,
    list: &[&Experience],
    covers: &mut Covers,
) -> Option<TemplatePick> {
    let (columns, width) = columns(ui.available_width(), TILE, GAP);
    let mut pick = None;
    let total = list.len() + 1;
    for row in 0..total.div_ceil(columns) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for slot in row * columns..(row * columns + columns).min(total) {
                if slot == 0 {
                    if blank_tile(ui, width) {
                        pick = Some(TemplatePick::Blank);
                    }
                } else if tile(ui, width, list[slot - 1], covers, None) {
                    pick = Some(TemplatePick::Open(slot - 1));
                }
            }
        });
        ui.add_space(GAP);
    }
    pick
}

fn blank_tile(ui: &mut egui::Ui, width: f32) -> bool {
    let image = egui::vec2(width, (width * 9. / 16.).round());
    let (rect, response) =
        ui.allocate_exact_size(image + egui::vec2(0., 46.), egui::Sense::click());
    let cover_rect = egui::Rect::from_min_size(rect.min, image);
    let hot = response.hovered() || response.has_focus();
    ui.painter()
        .rect_filled(cover_rect, 10, if hot { SURFACE_HIGH } else { SURFACE });
    ui.painter().rect_stroke(
        cover_rect,
        10,
        egui::Stroke::new(
            if hot { 2. } else { 1. },
            if hot {
                Color32::from_white_alpha(200)
            } else {
                BORDER
            },
        ),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        cover_rect.center(),
        egui::Align2::CENTER_CENTER,
        icons::PLUS,
        icon_font(cover_rect.height() * 0.3, false),
        TEXT,
    );
    ui.painter().text(
        egui::pos2(rect.left(), cover_rect.bottom() + 8.),
        egui::Align2::LEFT_TOP,
        "Blank world",
        semibold(14.5),
        TEXT,
    );
    ui.painter().text(
        egui::pos2(rect.left(), cover_rect.bottom() + 28.),
        egui::Align2::LEFT_TOP,
        "Flat ground to build on",
        FontId::proportional(12.5),
        MUTED,
    );
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Blank world"));
    response.clicked()
}

/// What a row header or tile asked for.
pub enum RowAction {
    Open(usize),
    SeeAll,
}

/// One row of tiles under a heading, like a shelf. Shows what fits and a
/// "See all" link when there is more.
pub fn shelf(
    ui: &mut egui::Ui,
    heading: &str,
    list: &[&Experience],
    covers: &mut Covers,
    hero: Option<&egui::TextureHandle>,
) -> Option<RowAction> {
    let (columns, width) = columns(ui.available_width(), TILE, GAP);
    let mut action = None;
    ui.add_space(14.);
    ui.horizontal(|ui| {
        ui.label(RichText::new(heading).font(semibold(18.)).color(TEXT));
        if list.len() > columns {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("See all").font(semibold(13.5)).color(MUTED),
                        )
                        .frame(false),
                    )
                    .clicked()
                {
                    action = Some(RowAction::SeeAll);
                }
            });
        }
    });
    ui.add_space(4.);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        for (index, experience) in list.iter().take(columns).enumerate() {
            if tile(ui, width, experience, covers, hero) {
                action = Some(RowAction::Open(index));
            }
        }
    });
    action
}

/// One tile: the cover carries it, the name and creator sit underneath.
/// The whole tile is one button that opens the experience page.
fn tile(
    ui: &mut egui::Ui,
    width: f32,
    experience: &Experience,
    covers: &mut Covers,
    hero: Option<&egui::TextureHandle>,
) -> bool {
    let image = egui::vec2(width, (width * 9. / 16.).round());
    let (rect, response) =
        ui.allocate_exact_size(image + egui::vec2(0., 46.), egui::Sense::click());
    let cover_rect = egui::Rect::from_min_size(rect.min, image);
    cover(ui, cover_rect, experience, covers, hero);
    let hot = response.hovered() || response.has_focus();
    if hot {
        ui.painter()
            .rect_filled(cover_rect, 10, Color32::from_white_alpha(14));
        ui.painter().rect_stroke(
            cover_rect,
            10,
            egui::Stroke::new(2., Color32::from_white_alpha(200)),
            egui::StrokeKind::Inside,
        );
    }
    let painter = ui.painter().with_clip_rect(rect);
    let name = painter.layout_no_wrap(experience.name.clone(), semibold(14.5), TEXT);
    painter.galley(
        egui::pos2(rect.left(), cover_rect.bottom() + 8.),
        truncate(
            &painter,
            &experience.name,
            semibold(14.5),
            TEXT,
            width,
            name,
        ),
        TEXT,
    );
    let meta = match &experience.version {
        Some(version) => format!("{} · v{version}", experience.creator),
        None => experience.creator.clone(),
    };
    let meta_font = FontId::proportional(12.5);
    let galley = painter.layout_no_wrap(meta.clone(), meta_font.clone(), MUTED);
    painter.galley(
        egui::pos2(rect.left(), cover_rect.bottom() + 28.),
        truncate(&painter, &meta, meta_font, MUTED, width, galley),
        MUTED,
    );
    let response = response.on_hover_text(&experience.description);
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &experience.name)
    });
    response.clicked()
}

/// Cut a single line with an ellipsis when it is wider than `width`.
fn truncate(
    painter: &egui::Painter,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
    full: std::sync::Arc<egui::Galley>,
) -> std::sync::Arc<egui::Galley> {
    if full.size().x <= width {
        return full;
    }
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    painter.layout_job(job)
}

/// Large banner for one experience, with the text on a dark fade.
/// Returns true when the banner itself (not a button) was clicked.
pub fn banner(
    ui: &mut egui::Ui,
    rect: egui::Rect,
    experience: &Experience,
    covers: &mut Covers,
    hero: Option<&egui::TextureHandle>,
) -> egui::Response {
    let response = ui.interact(
        rect,
        ui.id().with(("banner", &experience.id)),
        egui::Sense::click(),
    );
    cover(ui, rect, experience, covers, hero);
    // Fade from the left and the bottom so white text always reads. A small
    // texture keeps the banner's rounded corners.
    let fade = covers.fade(ui.ctx());
    egui::Image::from_texture(&fade)
        .corner_radius(10)
        .paint_at(ui, rect);
    response
}

/// The big gold action.
pub fn play_button(label: &str) -> egui::Button<'_> {
    egui::Button::new((
        RichText::new(fill::PLAY)
            .font(icon_font(16., true))
            .color(BACKGROUND),
        RichText::new(label).font(semibold(15.)).color(BACKGROUND),
    ))
    .fill(ACCENT)
    .corner_radius(8)
    .min_size(egui::vec2(132., 42.))
}

/// A quieter button with an icon in front.
pub fn icon_button<'a>(glyph: &'a str, label: &'a str) -> egui::Button<'a> {
    egui::Button::new((icon(glyph, TEXT), RichText::new(label).font(semibold(14.))))
        .corner_radius(8)
        .min_size(egui::vec2(0., 42.))
}

/// A page heading.
pub fn title(ui: &mut egui::Ui, text: &str) {
    ui.add_space(4.);
    ui.label(RichText::new(text).font(display(30.)).color(TEXT));
    ui.add_space(10.);
}

/// Section heading inside a page.
pub fn section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(14.);
    ui.label(RichText::new(text).font(semibold(18.)).color(TEXT));
    ui.add_space(6.);
}

/// A short, centred empty state: what is missing and what works today.
/// Returns true when the optional button was clicked.
pub fn empty_state(
    ui: &mut egui::Ui,
    glyph: &str,
    heading: &str,
    body: &str,
    button: Option<&str>,
) -> bool {
    let mut clicked = false;
    ui.add_space(40.);
    ui.vertical_centered(|ui| {
        ui.set_max_width(440.);
        ui.label(
            RichText::new(glyph)
                .font(icon_font(44., false))
                .color(MUTED),
        );
        ui.add_space(6.);
        ui.label(RichText::new(heading).font(semibold(18.)).color(TEXT));
        ui.label(RichText::new(body).color(MUTED));
        if let Some(label) = button {
            ui.add_space(10.);
            clicked = ui
                .add(egui::Button::new(RichText::new(label).font(semibold(14.))).corner_radius(8))
                .clicked();
        }
    });
    clicked
}

pub use egui_phosphor::regular as glyph;
