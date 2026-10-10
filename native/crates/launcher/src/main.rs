#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use eframe::egui::{self, Color32, RichText};
use sa_client::{
    launch::{Launch, Session},
    settings::{Renderer, Settings},
    Favorite, LauncherConfig,
};
use std::{
    path::PathBuf,
    process::{Child, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
mod artwork;
mod local_mods;
mod servers;
mod updater;
// Dark neutral surfaces with one warm accent, like the FiveM client.
const BACKGROUND: Color32 = Color32::from_rgb(14, 14, 18);
const SURFACE: Color32 = Color32::from_rgb(24, 24, 30);
const SURFACE_HIGH: Color32 = Color32::from_rgb(34, 34, 42);
const MUTED: Color32 = Color32::from_rgb(150, 150, 165);
const TEXT: Color32 = Color32::from_rgb(236, 236, 242);
const ACCENT: Color32 = Color32::from_rgb(240, 98, 32);
#[derive(Clone, Copy, PartialEq)]
enum Page {
    Home,
    Multiplayer,
    Resources,
    Settings,
    Help,
}
enum Work {
    Validation(PathBuf, sa_client::install::Validation),
    Cache(sa_net::resources::CachePlan),
    Mods(Vec<local_mods::Entry>),
    Cleared(u64),
    Artwork(PathBuf, egui::ColorImage),
}
type BrowserReply = (
    String,
    Result<Vec<sa_net::relay::Listing>, String>,
    Duration,
);
struct Launcher {
    config: LauncherConfig,
    hero: Option<egui::TextureHandle>,
    pending_artwork: Option<egui::ColorImage>,
    updates: updater::Updates,
    settings: Settings,
    page: Page,
    message: String,
    validation: Option<(PathBuf, sa_client::install::Validation)>,
    worker: Option<mpsc::Receiver<Result<Work, String>>>,
    servers: Vec<sa_net::relay::Listing>,
    browser_time: Option<Duration>,
    browser_worker: Option<mpsc::Receiver<BrowserReply>>,
    browser_error: Option<String>,
    connection_error: Option<String>,
    browser_initialized: bool,
    search: String,
    favorites_only: bool,
    server_tab: servers::ServerTab,
    selected_server: Option<String>,
    direct_open: bool,
    resource_tab: usize,
    mods_inspected: bool,
    resource_error: Option<String>,
    work_scope: Page,
    details: String,
    settings_message: Option<String>,
    code: String,
    direct: String,
    relay_mode: bool,
    child: Option<Child>,
    log: Option<PathBuf>,
    report: String,
    cache: Option<sa_net::resources::CachePlan>,
    mods: Vec<local_mods::Entry>,
    gilrs: Option<gilrs::Gilrs>,
    refresh: Instant,
    preview_frames: u32,
    preview_requested: bool,
}
impl Launcher {
    fn new(ctx: &egui::Context) -> Self {
        Self::configured(ctx, LauncherConfig::load(), Settings::load(), true)
    }
    fn configured(
        ctx: &egui::Context,
        mut config: LauncherConfig,
        settings: Settings,
        detect: bool,
    ) -> Self {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "street".into(),
            egui::FontData::from_static(include_bytes!(
                "../../runtime/assets/UnifrakturCook-Bold.ttf"
            ))
            .into(),
        );
        fonts.families.insert(
            egui::FontFamily::Name("street".into()),
            vec!["street".into()],
        );
        ctx.set_fonts(fonts);
        let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
        style.visuals = egui::Visuals::dark();
        style.wrap_mode = Some(egui::TextWrapMode::Wrap);
        style.visuals.panel_fill = BACKGROUND;
        style.visuals.window_fill = SURFACE;
        style.visuals.extreme_bg_color = Color32::from_rgb(10, 10, 13);
        style.visuals.widgets.inactive.bg_fill = SURFACE_HIGH;
        style.visuals.widgets.inactive.weak_bg_fill = SURFACE_HIGH;
        style.visuals.widgets.active.bg_stroke = egui::Stroke::new(2., ACCENT);
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1., ACCENT);
        style.visuals.widgets.active.bg_fill = Color32::from_rgb(72, 40, 26);
        style.visuals.widgets.noninteractive.bg_stroke =
            egui::Stroke::new(1., Color32::from_rgb(40, 40, 50));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(27.));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(16.));
        style.visuals.override_text_color = Some(TEXT);
        style.visuals.selection.bg_fill = Color32::from_rgb(120, 52, 20);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(46, 46, 56);
        for widget in [
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
            &mut style.visuals.widgets.open,
        ] {
            widget.corner_radius = egui::CornerRadius::same(4);
        }
        style.spacing.scroll = egui::style::ScrollStyle::solid();
        style.spacing.item_spacing = egui::vec2(12., 12.);
        style.spacing.button_padding = egui::vec2(14., 9.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(17.));
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(13.));
        ctx.set_theme(egui::Theme::Dark);
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark));
        ctx.set_style_of(egui::Theme::Dark, style);

        if config.player.is_empty() {
            config.player = "Player".into();
        }
        if detect && config.game_dir.as_os_str().is_empty() {
            config.game_dir = sa_client::install::candidates()
                .into_iter()
                .find(|p| p.join("models").is_dir())
                .unwrap_or_default();
        }
        let hero = config
            .hero_image
            .as_ref()
            .and_then(|path| artwork::load(path).ok())
            .map(|image| ctx.load_texture("hero", image, egui::TextureOptions::LINEAR));
        let mut app = Self {
            config,
            hero,
            pending_artwork: None,
            updates: updater::Updates::new(
                detect && std::env::var_os("SARE_SCREENSHOT_TO").is_none(),
            ),
            settings,
            page: Page::Home,
            message: String::new(),
            validation: None,
            worker: None,
            servers: Vec::new(),
            browser_time: None,
            browser_worker: None,
            browser_error: None,
            connection_error: None,
            browser_initialized: false,
            search: String::new(),
            favorites_only: false,
            server_tab: servers::ServerTab::All,
            selected_server: None,
            direct_open: false,
            resource_tab: 0,
            mods_inspected: false,
            resource_error: None,
            work_scope: Page::Home,
            details: String::new(),
            settings_message: None,
            code: String::new(),
            direct: "127.0.0.1:7777".into(),
            relay_mode: true,
            child: None,
            log: None,
            report: String::new(),
            cache: None,
            mods: Vec::new(),
            gilrs: if detect {
                gilrs::Gilrs::new().ok()
            } else {
                None
            },
            refresh: Instant::now(),
            preview_frames: 0,
            preview_requested: false,
        };
        if std::env::var_os("SARE_SCREENSHOT_TO").is_some() {
            app.page = match std::env::var("SARE_PREVIEW_PAGE")
                .unwrap_or_default()
                .as_str()
            {
                "settings" => Page::Settings,
                "resources" => Page::Resources,
                "multiplayer" => Page::Multiplayer,
                "help" => Page::Help,
                _ => Page::Home,
            };
            app.resource_tab = std::env::var("SARE_PREVIEW_RESOURCE_TAB")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0)
                .min(2);
            app.direct_open = std::env::var_os("SARE_PREVIEW_DIRECT").is_some();
            app.favorites_only = std::env::var_os("SARE_PREVIEW_FAVORITES").is_some();
            if let Ok(scale) = std::env::var("SARE_PREVIEW_SCALE")
                .unwrap_or_default()
                .parse::<f32>()
            {
                ctx.set_pixels_per_point(scale.clamp(1., 3.));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(preview_size()));
            }
        }
        if !app.config.game_dir.as_os_str().is_empty() {
            app.validate();
        }
        app
    }
    fn work(&mut self, job: impl FnOnce() -> anyhow::Result<Work> + Send + 'static) {
        if self.worker.is_some() {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.worker = Some(rx);
        self.work_scope = self.page;
        self.resource_error = None;
        std::thread::spawn(move || {
            let result = job().map_err(|e| format!("{e:#}"));
            let _ = tx.send(result);
        });
    }
    fn validate(&mut self) {
        let path = self.config.game_dir.clone();
        self.validation = None;
        self.message = "Checking original installation (read only)...".into();
        self.work(move || {
            Ok(Work::Validation(
                path.clone(),
                sa_client::install::validate(&path),
            ))
        });
    }
    fn ready(&self) -> bool {
        self.validation
            .as_ref()
            .is_some_and(|(p, v)| p == &self.config.game_dir && v.ready())
            && self.child.is_none()
            && self.worker.is_none()
            && !self.updates.restarting
            && !self.updates.recovery_blocked
    }
    fn poll(&mut self) {
        if let Some(worker) = &self.browser_worker {
            match worker.try_recv() {
                Ok((address, result, elapsed)) => {
                    self.browser_worker = None;
                    if address == self.config.relay {
                        match result {
                            Ok(servers) => {
                                self.servers = servers;
                                self.browser_time = Some(elapsed);
                                self.browser_error = None;
                            }
                            Err(error) => {
                                log_error(&error);
                                self.servers.clear();
                                self.browser_time = None;
                                self.browser_error = Some(error);
                            }
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.browser_worker = None;
                    self.browser_error =
                        Some("Directory background task stopped. Retry the request.".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(worker) = &self.worker {
            match worker.try_recv() {
                Ok(result) => {
                    self.worker = None;
                    match result {
                        Ok(Work::Validation(path, _)) if path != self.config.game_dir => {
                            self.validation = None;
                            self.message="Installation changed. Validate the selected folder before playing.".into();
                        }
                        Ok(Work::Validation(path, result)) => {
                            self.message = if result.ready() {
                                "Ready to play.".into()
                            } else {
                                "Installation needs attention. Open Settings to review the missing files.".into()
                            };
                            if result.ready() && path == self.config.game_dir {
                                if let Err(error) = self.config.save() {
                                    self.message = format!(
                                        "Installation valid, but cannot save selection: {error}"
                                    );
                                }
                            }
                            self.validation = Some((path, result));
                        }
                        Ok(Work::Cache(plan)) => {
                            self.cache = Some(plan);
                            self.message =
                                "Cache preview ready. Review the list before deleting.".into();
                        }
                        Ok(Work::Mods(entries)) => {
                            self.mods = entries;
                            self.mods_inspected = true;
                            self.message = "Local resources inspected.".into();
                        }
                        Ok(Work::Cleared(bytes)) => {
                            self.cache = None;
                            self.message = format!(
                                "Removed {:.1} MiB of unused cached downloads.",
                                bytes as f64 / 1048576.
                            );
                        }
                        Ok(Work::Artwork(path, image)) => {
                            self.pending_artwork = Some(image);
                            self.config.hero_image = Some(path);
                            self.message = match self.config.save() {
                                Ok(()) => "Home artwork saved.".into(),
                                Err(e) => format!("Could not save artwork setting: {e}"),
                            };
                        }
                        Err(error) => {
                            log_error(&error);
                            self.details = error.clone();
                            if self.work_scope == Page::Resources {
                                self.resource_error = Some(error);
                            }
                            self.message = "Background check failed. See Details in Help.".into();
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.worker = None;
                    self.message = "The background check stopped. Try again.".into();
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.refresh.elapsed() > Duration::from_millis(250) {
            self.refresh = Instant::now();
            if let Some(child) = &mut self.child {
                match child.try_wait() {
                    Ok(Some(status)) => {
                        self.child = None;
                        self.settings = Settings::load();
                        self.message = if status.success() {
                            "Game closed. Any saved offline checkpoint is available on the home page.".into()
                        } else {
                            "The game stopped with an error. Open Help to preview the diagnostic report.".into()
                        };
                        self.update_report();
                    }
                    Ok(None) => {}
                    Err(error) => self.message = format!("Cannot check game process: {error}"),
                }
            }
        }
    }
    fn update_report(&mut self) {
        let log = self
            .log
            .as_ref()
            .and_then(|p| sa_client::diagnostics::tail(p).ok())
            .unwrap_or_default();
        self.report = sa_client::diagnostics::report(
            &log,
            if self.child.is_some() {
                "Runtime process running"
            } else {
                "Runtime closed or not launched"
            },
        );
    }
    fn start(&mut self, session: Session) {
        let network = !matches!(session, Session::Offline);
        if !self.ready() {
            self.message =
                "Choose and validate the installation, and close any running game first.".into();
            return;
        }
        let recent = match &session {
            Session::Offline => None,
            Session::Direct(address) => Some(Favorite {
                name: "Direct session".into(),
                address: address.clone(),
                relay: false,
                code: String::new(),
            }),
            Session::Relay { address, code } => Some(Favorite {
                name: "Relay session".into(),
                address: address.clone(),
                relay: true,
                code: code.clone(),
            }),
        };
        let result = (|| -> anyhow::Result<(Child, PathBuf)> {
            let exe = std::env::current_exe()?
                .parent()
                .unwrap()
                .join(if cfg!(windows) {
                    "sa-runtime.exe"
                } else {
                    "sa-runtime"
                });
            let _guard = sa_client::launch::RuntimeGuard::acquire()?;
            self.config.save()?;
            self.settings.sanitize();
            self.settings.save()?;
            let mods = exe.parent().unwrap().join("mods");
            // Development builds use the repository mods folder; packaged builds use their sibling folder.
            let repo_mods = exe
                .parent()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .map(|p| p.join("mods"));
            let mods = if mods.is_dir() {
                mods
            } else {
                repo_mods.filter(|p| p.is_dir()).unwrap_or(mods)
            };
            let launch = Launch {
                game: self.config.game_dir.clone(),
                mods,
                cache: sa_client::cache_dir(),
                player: self.config.player.clone(),
                session,
            };
            let mut command = launch.command(&exe)?;
            let logs = sa_client::config_dir().join("logs");
            std::fs::create_dir_all(&logs)?;
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_nanos();
            let log = logs.join(format!("runtime-{stamp}.log"));
            let file = std::fs::File::create(&log)?;
            command
                .stdout(Stdio::from(file.try_clone()?))
                .stderr(Stdio::from(file));
            drop(_guard);
            Ok((command.spawn()?, log))
        })();
        match result {
            Ok((child, log)) => {
                if network {
                    self.connection_error = None;
                }
                self.child = Some(child);
                self.log = Some(log);
                if recent.is_some() {
                    self.config.last_session = recent;
                    if let Err(error) = self.config.save() {
                        eprintln!("Could not save recent session: {error}");
                    }
                }
                self.message =
                    "Starting the native game. Map loading continues in the game window.".into();
            }
            Err(error) => {
                let error = format!("Could not start SARE: {error:#}");
                log_error(&error);
                if network {
                    self.connection_error = Some(error);
                } else {
                    self.details = error;
                    self.message = "Could not start SARE. See Details in Help.".into();
                }
            }
        }
    }
    fn favorite(&mut self, name: String) {
        if self.config.favorites.len() >= 32 {
            self.message = "The favorite list is full (32). Remove one first.".into();
            return;
        }
        let address = if self.relay_mode {
            self.config.relay.clone()
        } else {
            self.direct.clone()
        };
        let item = Favorite {
            name,
            address,
            relay: self.relay_mode,
            code: if self.relay_mode {
                self.code.clone()
            } else {
                String::new()
            },
        };
        if !self.config.favorites.contains(&item) {
            self.config.favorites.push(item);
        }
        self.connection_error = None;
        if let Err(error) = self.config.save() {
            let error = format!("Could not save favorites: {error}");
            log_error(&error);
            self.connection_error = Some(error);
        }
    }
    fn browse(&mut self) {
        self.browser_initialized = true;
        if self.browser_worker.is_some() {
            return;
        }
        self.browser_error = None;
        self.servers.clear();
        self.browser_time = None;
        if self.config.relay.trim().is_empty() {
            return;
        }
        let address = self.config.relay.clone();
        let (tx, rx) = mpsc::channel();
        self.browser_worker = Some(rx);
        std::thread::spawn(move || {
            let start = Instant::now();
            let result = address
                .parse()
                .map_err(|_| "Enter a relay IP and port in Direct connect.".to_owned())
                .and_then(|parsed| {
                    sa_net::relay::browse(parsed)
                        .map_err(|e| format!("Cannot reach the relay directory: {e}"))
                });
            let _ = tx.send((address, result, start.elapsed()));
        });
    }
    fn connection_valid(&self) -> bool {
        let address = if self.relay_mode {
            &self.config.relay
        } else {
            &self.direct
        };
        address
            .parse::<std::net::SocketAddr>()
            .is_ok_and(|a| a.port() != 0)
            && (!self.relay_mode
                || (self.code.trim().len() == 12
                    && self.code.trim().bytes().all(|b| b.is_ascii_hexdigit())))
    }
    fn controller_events(&mut self, raw: &mut egui::RawInput) {
        if let Some(gilrs) = &mut self.gilrs {
            while let Some(event) = gilrs.next_event() {
                let (button, pressed) = match event.event {
                    gilrs::EventType::ButtonPressed(b, _) => (b, true),
                    gilrs::EventType::ButtonReleased(b, _) => (b, false),
                    _ => continue,
                };
                if let Some(event) = controller_event(button, pressed) {
                    raw.events.push(event);
                }
            }
        }
    }
    fn home(&mut self, ui: &mut egui::Ui) {
        let checkpoint =
            sa_client::progress::Save::load(&sa_client::config_dir().join("progress.json"))
                .ok()
                .flatten();
        let width = ui.available_width();
        let hero_height = (ui.available_height() * 0.48).clamp(240., 440.);
        ui.allocate_ui(egui::vec2(width, hero_height), |ui| {
            let rect = egui::Rect::from_min_size(
                ui.next_widget_position(),
                egui::vec2(width, hero_height),
            );
            artwork::background(ui, rect, self.hero.as_ref());
            egui::Frame::new().inner_margin(28).show(ui, |ui| {
                ui.set_min_size(egui::vec2((width - 56.).max(100.), hero_height - 56.));
                ui.add_space(14.);
                ui.label(
                    RichText::new("Back to San Andreas.")
                        .size(if width < 600. { 30. } else { 40. })
                        .strong(),
                );
                ui.label(
                    RichText::new("Your streets. Your story.")
                        .size(18.)
                        .color(MUTED),
                );
                ui.add_space(30.);
                ui.horizontal_wrapped(|ui| {
                    let label = if checkpoint.is_some() {
                        "Continue free roam"
                    } else {
                        "Start free roam"
                    };
                    if ui
                        .add_enabled(
                            self.ready(),
                            egui::Button::new(
                                RichText::new(format!("      {label}"))
                                    .color(Color32::WHITE)
                                    .strong(),
                            )
                            .min_size(egui::vec2(218., 48.))
                            .fill(ACCENT),
                        )
                        .clicked()
                    {
                        self.start(Session::Offline);
                    }
                    ui.vertical(|ui| {
                        if let Some(save) = &checkpoint {
                            ui.label(saved_place(save.position));
                            ui.label(RichText::new(&save.car).color(MUTED));
                        } else {
                            ui.label("Grove Street");
                            ui.label(RichText::new("A new free roam").color(MUTED));
                        }
                    });
                });
                if !self.validation.as_ref().is_some_and(|(_, v)| v.ready()) {
                    ui.add_space(8.);
                    if ui.link("Choose your installation in Settings").clicked() {
                        self.page = Page::Settings;
                    }
                }
            });
        });
        ui.add_space(20.);
        self.dashboard(ui);
    }
    fn dashboard(&mut self, ui: &mut egui::Ui) {
        if ui.available_width() >= 720. {
            ui.columns(3, |columns| {
                for (index, column) in columns.iter_mut().enumerate() {
                    self.dashboard_card(column, index);
                }
            });
        } else {
            for index in 0..3 {
                self.dashboard_card(ui, index);
                ui.add_space(12.);
            }
        }
    }
    fn dashboard_card(&mut self, ui: &mut egui::Ui, index: usize) {
        ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
            egui::Frame::new()
                .fill(SURFACE)
                .inner_margin(20)
                .corner_radius(10)
                .show(ui, |ui| {
                    ui.set_min_width((ui.available_width() - 2.).max(100.));
                    ui.set_min_height(176.);
                    let card_top = ui.next_widget_position().y;
                    ui.label(
                        RichText::new(["Browse servers", "Last session", "Favorites"][index])
                            .size(20.)
                            .strong(),
                    );
                    ui.add_space(8.);
                    let recent = self.config.last_session.clone();
                    let body = match index {
                        0 => "Find friends on your relay, or join with a code.".to_owned(),
                        1 => recent.as_ref().map(|r| r.name.clone()).unwrap_or_else(|| {
                            "Join a server to keep your next session here.".into()
                        }),
                        _ if self.config.favorites.is_empty() => {
                            "Save a trusted host from Servers to find it here.".into()
                        }
                        _ => format!(
                            "{} saved sessions, ready when you are.",
                            self.config.favorites.len()
                        ),
                    };
                    ui.label(RichText::new(body).color(MUTED));
                    // Bottom alignment keeps all card actions on one baseline.
                    ui.add_space((card_top + 138. - ui.next_widget_position().y).max(8.));
                    match index {
                        0 => {
                            if ui
                                .add(
                                    egui::Button::new("Browse servers")
                                        .min_size(egui::vec2(150., 38.)),
                                )
                                .clicked()
                            {
                                self.page = Page::Multiplayer;
                                self.browse();
                            }
                        }
                        1 => {
                            if let Some(recent) = recent {
                                if ui
                                    .add_enabled(
                                        self.ready(),
                                        egui::Button::new("Reconnect")
                                            .min_size(egui::vec2(150., 38.)),
                                    )
                                    .clicked()
                                {
                                    self.start(if recent.relay {
                                        Session::Relay {
                                            address: recent.address,
                                            code: recent.code,
                                        }
                                    } else {
                                        Session::Direct(recent.address)
                                    });
                                }
                            }
                        }
                        _ => {
                            let label = if self.config.favorites.is_empty() {
                                "Find a server"
                            } else {
                                "Open favorites"
                            };
                            if ui
                                .add(egui::Button::new(label).min_size(egui::vec2(150., 38.)))
                                .clicked()
                            {
                                self.page = Page::Multiplayer;
                                self.favorites_only = !self.config.favorites.is_empty();
                            }
                        }
                    }
                });
        });
    }
    fn resources(&mut self, ui: &mut egui::Ui) {
        ui.heading("Resources");
        ui.label(RichText::new("Inspect your mods and downloaded resources.").color(MUTED));
        ui.horizontal_wrapped(|ui| {
            for (index, label) in ["Local mods", "Server resources", "Download cache"]
                .iter()
                .enumerate()
            {
                ui.selectable_value(&mut self.resource_tab, index, *label);
            }
        });
        ui.separator();
        if let Some(error) = &self.resource_error {
            ui.colored_label(
                ACCENT,
                "Resource inspection failed. Retry after reviewing Details.",
            );
            ui.collapsing("Details", |ui| {
                ui.label(error);
            });
        }
        let inspect = ui
            .add_enabled(
                self.worker.is_none() && self.resource_tab != 1,
                egui::Button::new("Refresh resources"),
            )
            .clicked();
        if self.worker.is_none()
            && (inspect
                || (self.resource_error.is_none()
                    && ((self.resource_tab == 0 && !self.mods_inspected)
                        || (self.resource_tab == 2 && self.cache.is_none()))))
        {
            match self.resource_tab {
                0 => {
                    let root = local_mods::root();
                    self.work(move || Ok(Work::Mods(local_mods::inspect(&root)?)));
                }
                2 => {
                    self.work(move || {
                        Ok(Work::Cache(sa_net::resources::inspect_cache(
                            &sa_client::cache_dir(),
                        )?))
                    });
                }
                _ => {}
            }
        }
        if self.worker.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Inspecting resources...");
            });
        }
        match self.resource_tab {
            0 => {
                if self.mods_inspected && self.mods.is_empty() {
                    surface(ui, |ui| {
                        ui.label("No local native mods found.");
                        ui.small(
                            "Add compatible resources to the client's mods folder, then refresh.",
                        );
                    });
                }
                for entry in &self.mods {
                    surface(ui, |ui| {
                        ui.label(RichText::new(&entry.name).strong());
                        ui.label(format!(
                            "{} / {} / {}",
                            entry.kind,
                            entry.status,
                            entry
                                .bytes
                                .map(|bytes| format!("{:.2} MiB", bytes as f64 / 1048576.))
                                .unwrap_or_else(|| "Size unavailable".into())
                        ));
                        if let Some(error) = &entry.error {
                            ui.collapsing("Inspection error", |ui| {
                                ui.colored_label(Color32::LIGHT_RED, error);
                            });
                        }
                    });
                }
                ui.small("Manifest status is shown here. Asset compatibility is validated by the game when it loads a resource.");
            }
            1 => {
                surface(ui, |ui| {
                    ui.label(if self.child.is_some() {
                        "Session resource details are available in the game."
                    } else {
                        "No active session started by this launcher."
                    });
                    ui.small("The runtime does not export its active resource inventory to the launcher. Open /mods in the game to inspect names, types, validation errors and download progress.");
                });
            }
            _ => {
                if let Some(plan) = self.cache.clone() {
                    ui.label(
                        RichText::new(format!(
                            "{:.2} MiB total cache usage",
                            plan.bytes as f64 / 1048576.
                        ))
                        .strong(),
                    );
                    ui.small("Includes reusable download blobs and cached session resources.");
                    if plan.packs.is_empty() {
                        ui.label("No cached session inventories found.");
                    }
                    for pack in &plan.packs {
                        surface(ui, |ui| {
                            ui.label(
                                RichText::new(if pack.names.is_empty() {
                                    pack.fingerprint.chars().take(12).collect()
                                } else {
                                    pack.names.join(", ")
                                })
                                .strong(),
                            );
                            ui.label(format!(
                                "Native server resource cache / {:.2} MiB / {}",
                                pack.bytes as f64 / 1048576.,
                                if pack.complete {
                                    "Inventory recorded"
                                } else {
                                    "Incomplete or invalid inventory"
                                }
                            ));
                            ui.collapsing("Version details", |ui| {ui.label(&pack.fingerprint);ui.small("A recorded inventory does not indicate that this resource is currently active.");});
                        });
                    }
                    ui.separator();
                    ui.label("Cleanup removes only the previewed download cache. Required resources download again next time.");
                    if plan.busy || self.child.is_some() {
                        ui.colored_label(ACCENT, "Close all game sessions before cleanup. Active resources are protected.");
                    }
                    if ui
                        .add_enabled(
                            !plan.busy
                                && self.worker.is_none()
                                && self.child.is_none()
                                && plan.bytes > 0,
                            egui::Button::new("Delete previewed cached downloads"),
                        )
                        .clicked()
                    {
                        self.work(move || {
                            Ok(Work::Cleared(sa_net::resources::cleanup_cache(
                                &sa_client::cache_dir(),
                                &plan,
                            )?))
                        });
                    }
                    ui.small("Local mods and original game files are preserved.");
                }
            }
        }
    }
    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        let before = serde_json::to_value((&self.settings, &self.config)).ok();
        ui.heading("Settings");
        ui.label(
            RichText::new("Changes to game settings apply on the next game launch.").color(MUTED),
        );
        ui.add_enabled_ui(self.child.is_none(), |ui| {
            if ui.add(primary("Save client settings")).clicked() {
                let result = (|| -> anyhow::Result<()> {
                    let _guard = sa_client::launch::RuntimeGuard::acquire()?;
                    self.config.save()?;
                    self.settings.sanitize();
                    self.settings.save()
                })();
                self.message = match result {
                    Ok(()) => "Client settings saved.".into(),
                    Err(e) => format!("Could not save settings: {e}"),
                };
                self.settings_message = Some(self.message.clone());
            }
        });
        if let Some(message) = &self.settings_message {
            ui.label(message);
        }
        ui.add_space(8.);
        ui.label(RichText::new("Installation").strong().size(21.));
        ui.label("Original PC San Andreas folder");
        let mut text = self.config.game_dir.to_string_lossy().into_owned();
        if ui
            .add(
                egui::TextEdit::singleline(&mut text)
                    .desired_width(ui.available_width().min(720.))
                    .margin(egui::vec2(12., 11.)),
            )
            .changed()
        {
            self.config.game_dir = text.into();
            self.validation = None;
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.worker.is_none(), egui::Button::new("Choose folder..."))
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Choose original San Andreas installation")
                    .pick_folder()
                {
                    self.config.game_dir = path;
                    self.validate();
                }
            }
            if ui
                .add_enabled(
                    self.worker.is_none(),
                    egui::Button::new("Validate installation"),
                )
                .clicked()
            {
                self.validate();
            }
        });
        if let Some((path, report)) = &self.validation {
            if path == &self.config.game_dir && report.ready() {
                ui.colored_label(ACCENT, "Installation validated. Ready to play.");
            }
            for error in &report.errors {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
            for warning in &report.warnings {
                ui.colored_label(ACCENT, warning);
            }
        }
        ui.separator();
        ui.label(RichText::new("Display").strong().size(21.));
        ui.add_enabled_ui(self.child.is_none(), |ui| {
            ui.label("Renderer (game restart required)");
            egui::ComboBox::from_id_salt("renderer")
                .selected_text(self.settings.renderer.name())
                .show_ui(ui, |ui| {
                    for value in [Renderer::Auto, Renderer::Vulkan, Renderer::DirectX12] {
                        if cfg!(windows) || value != Renderer::DirectX12 {
                            ui.selectable_value(&mut self.settings.renderer, value, value.name());
                        }
                    }
                });
            ui.checkbox(&mut self.settings.fullscreen, "Fullscreen");
            ui.checkbox(&mut self.settings.vsync, "VSync");
            ui.label("Graphics profile");
            ui.horizontal_wrapped(|ui| {
                for (index, name) in ["Performance", "Balanced", "Quality", "Ultra"]
                    .iter()
                    .enumerate()
                {
                    if ui
                        .add(egui::Button::new(*name).selected(self.settings.preset() == *name))
                        .clicked()
                    {
                        self.settings.apply_preset(index);
                    }
                }
            });
            ui.label(format!("Current profile: {}", self.settings.preset()))
                .on_hover_text(
                    "More graphics, audio and gameplay controls are available in the game.",
                );
        });
        ui.separator();
        ui.label(RichText::new("Downloads").strong().size(21.));
        ui.add_enabled_ui(self.child.is_none(), |ui| {
            ui.checkbox(
                &mut self.settings.auto_mod_downloads,
                "Automatically download required server mods",
            );
        });
        ui.collapsing("Client updates", |ui| {
            let mut automatic = !self.config.disable_auto_updates;
            if ui
                .checkbox(
                    &mut automatic,
                    "Automatically update this client from GitHub",
                )
                .changed()
            {
                self.config.disable_auto_updates = !automatic;
                if let Err(error) = self.config.save() {
                    self.message = format!("Could not save update preference: {error}");
                }
            }
            self.updates_ui(ui);
        });
        ui.separator();
        ui.label(RichText::new("Appearance").strong().size(21.));
        ui.label("Home artwork");
        ui.label(
            RichText::new(
                "Choose your own gameplay screenshot (PNG). Optional and stored only on this PC.",
            )
            .color(MUTED),
        );
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.worker.is_none(), egui::Button::new("Choose image..."))
                .clicked()
            {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("PNG image", &["png"])
                    .pick_file()
                {
                    self.work(move || Ok(Work::Artwork(path.clone(), artwork::load(&path)?)));
                }
            }
            if self.config.hero_image.is_some() && ui.button("Use default background").clicked() {
                self.config.hero_image = None;
                self.hero = None;
                self.message = match self.config.save() {
                    Ok(()) => "Default background restored.".into(),
                    Err(e) => format!("Could not save artwork setting: {e}"),
                };
            }
        });
        if self.config.hero_image.is_some() && self.hero.is_none() {
            ui.colored_label(ACCENT,"Artwork unavailable. Using the default background; choose the image again to restore it.");
        }
        if before != serde_json::to_value((&self.settings, &self.config)).ok() {
            self.settings_message = None;
        }
    }
    fn flush_update_settings(&mut self) -> bool {
        if !self.updates.has_staged() {
            return true;
        }
        let result = (|| -> anyhow::Result<()> {
            let _guard = sa_client::launch::RuntimeGuard::acquire()?;
            self.config.save()?;
            self.settings.sanitize();
            self.settings.save()
        })();
        match result {
            Ok(()) => true,
            Err(e) => {
                self.message = format!("Update waiting: could not save client settings: {e}");
                false
            }
        }
    }
    fn updates_ui(&mut self, ui: &mut egui::Ui) {
        ui.label(&self.updates.status);
        if !self.updates.last_result.is_empty() {
            ui.small(&self.updates.last_result);
        }
        if ui
            .add_enabled(
                self.updates.available() && !self.updates.busy() && !self.updates.restarting,
                egui::Button::new(if self.updates.has_staged() {
                    "Install update and restart"
                } else {
                    "Check and download update"
                }),
            )
            .clicked()
        {
            let postpone = self.child.is_some() || !self.flush_update_settings();
            self.updates.manual_install(postpone, ui.ctx());
        }
        ui.small("Updates replace client program files after the game closes. Local mods, settings, saves and the original game are kept.");
    }
    fn help(&mut self, ui: &mut egui::Ui) {
        ui.heading("Help");
        ui.label(RichText::new("Controls").strong().size(21.));
        ui.label("Tab / D-pad: focus. Enter / A: choose. Esc / B: back to Home.");
        ui.label("In game: F7 cars, F8 characters, F6 wardrobe. More controls in Pause.");
        ui.separator();
        ui.label(RichText::new("Diagnostics").strong().size(21.));
        ui.label("Offline play needs no account. Saved positions are checked when the game loads.");
        ui.label("Reports use the last observed runtime GPU and renderer.");
        ui.collapsing("System details", |ui| {
            if self.details.is_empty() {
                ui.label("No launcher background errors recorded this session.");
            } else {
                ui.label(&self.details);
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("Preview diagnostic report").clicked() {
                self.update_report();
            }
            if ui.button("Open logs folder").clicked() {
                open_folder(sa_client::config_dir().join("logs"), &mut self.message);
            }
            if !self.report.is_empty() && ui.button("Save reviewed report...").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .set_file_name("SARE-report.txt")
                    .save_file()
                {
                    let protected = path.symlink_metadata().is_ok_and(|m| m.is_symlink())
                        || path
                            .canonicalize()
                            .ok()
                            .zip(self.config.game_dir.canonicalize().ok())
                            .is_some_and(|(p, game)| p.starts_with(game))
                        || path
                            .parent()
                            .and_then(|p| p.canonicalize().ok())
                            .zip(self.config.game_dir.canonicalize().ok())
                            .is_some_and(|(p, game)| p.starts_with(game));
                    self.message = if protected {
                        "Choose a report location outside the original installation.".into()
                    } else {
                        match std::fs::write(path, &self.report) {
                            Ok(()) => "Report saved. Nothing was sent automatically.".into(),
                            Err(e) => format!("Could not save report: {e}"),
                        }
                    };
                }
            }
        });
        ui.label("Review before sharing. Join codes, credentials and personal paths are removed from the excerpt. No report is uploaded automatically.");
        if !self.report.is_empty() {
            ui.add(
                egui::TextEdit::multiline(&mut self.report)
                    .desired_rows(14)
                    .desired_width(f32::INFINITY)
                    .code_editor(),
            );
        }
        ui.separator();
        ui.label(RichText::new("About").strong().size(21.));
        ui.label(format!(
            "SARE {} / build {} / network protocol {}",
            sa_client::VERSION,
            sa_client::BUILD_ID,
            sa_net::VERSION
        ));
        self.updates_ui(ui);
        ui.label("Use a complete client package containing both launcher and runtime. Packaged clients update from tested GitHub releases; source builds use Git/Cargo.");
        ui.hyperlink_to(
            "Project and download information",
            "https://github.com/LordM8YT/GTA-San-Andreas-Rust-Edition",
        );
    }
}
fn input(ui: &mut egui::Ui, value: &mut String, hint: &str) -> egui::Response {
    input_limit(ui, value, hint, 1024)
}
fn input_limit(ui: &mut egui::Ui, value: &mut String, hint: &str, limit: usize) -> egui::Response {
    ui.add(
        egui::TextEdit::singleline(value)
            .hint_text(hint)
            .desired_width(ui.available_width().min(720.))
            .margin(egui::vec2(12., 11.))
            .char_limit(limit),
    )
}
fn primary(label: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(label).color(Color32::WHITE).strong())
        .fill(ACCENT)
        .min_size(egui::vec2(100., 40.))
}
fn surface(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(6)
        .inner_margin(20)
        .show(ui, |ui| {
            ui.set_min_width((ui.available_width() - 1.).max(0.));
            content(ui);
        });
}
fn log_error(error: &str) {
    if cfg!(test) {
        return;
    } // Unit tests must not append to the user profile.
    use std::io::Write;
    let directory = sa_client::config_dir().join("logs");
    if std::fs::create_dir_all(&directory).is_ok() {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("launcher.log"))
        {
            let _ = writeln!(file, "{:?}: {error}", std::time::SystemTime::now());
        }
    }
}
fn open_folder(path: PathBuf, message: &mut String) {
    if let Err(error) = std::fs::create_dir_all(&path) {
        *message = error.to_string();
        return;
    }
    let program = if cfg!(windows) {
        "explorer.exe"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(error) = std::process::Command::new(program).arg(path).spawn() {
        *message = format!("Could not open the folder: {error}");
    }
}
impl Launcher {
    fn content(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if let Some(path) = std::env::var_os("SARE_SCREENSHOT_TO") {
            let image = ctx.input(|input| {
                input.events.iter().find_map(|event| {
                    if let egui::Event::Screenshot { image, .. } = event {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
            });
            if let Some(image) = image {
                let result = save_preview(std::path::Path::new(&path), &image);
                if let Err(error) = result {
                    eprintln!("Preview failed: {error}");
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            self.preview_frames = self.preview_frames.saturating_add(1);
            if self.preview_frames >= 6
                && self.worker.is_none()
                && self.browser_worker.is_none()
                && (self.page != Page::Resources
                    || (self.resource_tab == 0 && self.mods_inspected)
                    || self.resource_tab == 1
                    || (self.resource_tab == 2 && self.cache.is_some())
                    || self.resource_error.is_some())
                && !self.preview_requested
            {
                self.preview_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            ctx.request_repaint();
        }
        self.poll();
        if let Some(image) = self.pending_artwork.take() {
            self.hero = Some(ctx.load_texture("hero", image, egui::TextureOptions::LINEAR));
        }
        let postpone_update = self.child.is_some() || !self.flush_update_settings();
        self.updates
            .poll(!self.config.disable_auto_updates, postpone_update, &ctx);
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.page = Page::Home;
        }
        let pages = [
            (Page::Home, "Home"),
            (Page::Multiplayer, "Play"),
            (Page::Resources, "Resources"),
            (Page::Settings, "Settings"),
            (Page::Help, "Help"),
        ];
        egui::Panel::bottom("footer")
            .frame(
                egui::Frame::new()
                    .fill(BACKGROUND)
                    .inner_margin(egui::Margin::symmetric(24, 10)),
            )
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("SARE {}", sa_client::VERSION))
                            .small()
                            .color(MUTED),
                    );
                    ui.separator();
                    if self.worker.is_some() {
                        ui.spinner();
                    }
                    let status = if self.message.is_empty() {
                        "Ready to configure"
                    } else {
                        &self.message
                    };
                    let compact: String = status.chars().take(100).collect();
                    ui.label(RichText::new(compact).small())
                        .on_hover_text(status);
                    if self.updates.busy() || self.updates.has_staged() {
                        ui.separator();
                        ui.label(RichText::new(&self.updates.status).small().color(ACCENT));
                    }
                });
            });
        // FiveM-style top bar: logo, page tabs with an accent underline, and
        // the player name on the right. Tabs stay keyboard/controller focusable.
        egui::Panel::top("topbar")
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgb(10, 10, 13))
                    .inner_margin(egui::Margin::symmetric(24, 10)),
            )
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("SARE")
                            .font(egui::FontId::new(
                                32.,
                                egui::FontFamily::Name("street".into()),
                            ))
                            .color(ACCENT),
                    );
                    ui.add_space(18.);
                    let compact = ui.available_width() < 760.;
                    for (page, label) in pages {
                        let active = self.page == page;
                        let text = if compact {
                            label.to_string()
                        } else {
                            format!("        {label}")
                        };
                        let response = ui.add(
                            egui::Button::new(
                                RichText::new(text)
                                    .size(16.)
                                    .color(if active { TEXT } else { MUTED }),
                            )
                            .frame(false)
                            .min_size(egui::vec2(0., 40.)),
                        );
                        if !compact {
                            nav_icon(
                                ui,
                                response.rect.left_center() + egui::vec2(12., 0.),
                                page,
                                if active { ACCENT } else { MUTED },
                            );
                        }
                        if active {
                            let r = response.rect;
                            ui.painter().rect_filled(
                                egui::Rect::from_min_max(
                                    egui::pos2(r.left(), r.bottom() + 6.),
                                    egui::pos2(r.right(), r.bottom() + 9.),
                                ),
                                1,
                                ACCENT,
                            );
                        }
                        if response.clicked() {
                            self.page = page;
                        }
                    }
                    if !compact {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let name = if self.config.player.trim().is_empty() {
                                "Player"
                            } else {
                                self.config.player.as_str()
                            };
                            egui::Frame::new()
                                .fill(SURFACE_HIGH)
                                .corner_radius(16)
                                .inner_margin(egui::Margin::symmetric(12, 6))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        let (dot, _) = ui.allocate_exact_size(
                                            egui::vec2(8., 8.),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().circle_filled(
                                            dot.center(),
                                            4.,
                                            Color32::from_rgb(120, 205, 130),
                                        );
                                        ui.label(RichText::new(name).size(14.));
                                    });
                                });
                        });
                    }
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(BACKGROUND).inner_margin(28))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("launcher-body", self.page as u8))
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.page {
                        Page::Home => self.home(ui),
                        Page::Multiplayer => self.multiplayer(ui),
                        Page::Resources => self.resources(ui),
                        Page::Settings => self.settings_ui(ui),
                        Page::Help => self.help(ui),
                    });
            });
        if self.worker.is_some()
            || self.browser_worker.is_some()
            || self.child.is_some()
            || self.gilrs.is_some()
            || self.updates.available()
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }
}
impl eframe::App for Launcher {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        self.controller_events(raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Frame::new()
            .fill(BACKGROUND)
            .inner_margin(0)
            .show(ui, |ui| self.content(ui));
    }
}
fn controller_event(button: gilrs::Button, pressed: bool) -> Option<egui::Event> {
    let (key, shift) = match button {
        gilrs::Button::DPadDown | gilrs::Button::DPadRight => (egui::Key::Tab, false),
        gilrs::Button::DPadUp | gilrs::Button::DPadLeft => (egui::Key::Tab, true),
        gilrs::Button::South => (egui::Key::Enter, false),
        gilrs::Button::East => (egui::Key::Escape, false),
        _ => return None,
    };
    Some(egui::Event::Key {
        key,
        physical_key: None,
        pressed,
        repeat: false,
        modifiers: egui::Modifiers {
            shift,
            ..Default::default()
        },
    })
}
fn save_preview(path: &std::path::Path, image: &egui::ColorImage) -> anyhow::Result<()> {
    let root = std::env::var_os("SARE_CONFIG_DIR")
        .ok_or_else(|| anyhow::anyhow!("Preview requires an isolated SARE_CONFIG_DIR"))?;
    let root = PathBuf::from(root).canonicalize()?;
    anyhow::ensure!(
        path.parent()
            .and_then(|p| p.canonicalize().ok())
            .is_some_and(|p| p.starts_with(&root)),
        "Preview must stay inside the isolated profile"
    );
    let file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    let mut encoder = png::Encoder::new(file, image.size[0] as u32, image.size[1] as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(
        &image
            .pixels
            .iter()
            .flat_map(|pixel| pixel.to_array())
            .collect::<Vec<_>>(),
    )?;
    Ok(())
}
fn nav_icon(ui: &egui::Ui, center: egui::Pos2, page: Page, color: Color32) {
    let painter = ui.painter();
    let stroke = egui::Stroke::new(1.5, color);
    let p = |x, y| center + egui::vec2(x, y);
    match page {
        Page::Home => {
            painter.add(egui::Shape::line(
                vec![p(-8., 0.), p(0., -7.), p(8., 0.)],
                stroke,
            ));
            painter.add(egui::Shape::line(
                vec![p(-6., -1.), p(-6., 7.), p(6., 7.), p(6., -1.)],
                stroke,
            ));
        }
        Page::Multiplayer => {
            for y in [-5., 3.] {
                painter.rect_stroke(
                    egui::Rect::from_min_size(p(-7., y), egui::vec2(14., 5.)),
                    1,
                    stroke,
                    egui::StrokeKind::Inside,
                );
                painter.circle_filled(p(4., y + 2.5), 1., color);
            }
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
fn preview_size() -> egui::Vec2 {
    if std::env::var_os("SARE_SCREENSHOT_TO").is_some() {
        if let Ok(size) = std::env::var("SARE_PREVIEW_SIZE") {
            if let Some((w, h)) = size.split_once('x') {
                if let (Ok(w), Ok(h)) = (w.parse::<f32>(), h.parse::<f32>()) {
                    return egui::vec2(w.clamp(640., 4000.), h.clamp(480., 2200.));
                }
            }
        }
        if std::env::var_os("SARE_PREVIEW_COMPACT").is_some() {
            return egui::vec2(640., 480.);
        }
    }
    egui::vec2(1280., 720.)
}
fn saved_place(position: [f32; 3]) -> &'static str {
    let [x, y, _] = position;
    if (x - 2500.).hypot(y + 1670.) < 350. {
        "Near Grove Street"
    } else if x < -1000. && y > -1000. && y < 1600. {
        "San Fierro"
    } else if x > 800. && y > 500. {
        "Las Venturas area"
    } else if y < -500. {
        "Los Santos area"
    } else {
        "San Andreas"
    }
}
fn main() -> eframe::Result {
    if updater::helper_mode() {
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(preview_size())
            .with_min_inner_size([640., 480.]),
        ..Default::default()
    };
    eframe::run_native(
        "SARE - San Andreas Rust Edition",
        options,
        Box::new(|cc| Ok(Box::new(Launcher::new(&cc.egui_ctx)))),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wait_browser(app: &mut Launcher) {
        let start = Instant::now();
        while app.browser_worker.is_some() {
            app.poll();
            assert!(start.elapsed() < Duration::from_secs(8));
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    #[test]
    fn browser_setup_errors_empty_directory_and_real_join_are_independent() {
        let context = egui::Context::default();
        let mut app = Launcher::configured(
            &context,
            LauncherConfig::default(),
            Settings::default(),
            false,
        );
        assert!(app.config.relay.is_empty());
        assert!(!app.connection_valid());
        app.config.relay = "127.0.0.1:7778".into();
        app.code = "ABCDEF123456".into();
        assert!(app.connection_valid());
        app.code = "invalid".into();
        assert!(!app.connection_valid());
        app.code.clear();
        app.config.relay.clear();
        app.browse();
        assert!(app.browser_worker.is_none());
        assert!(app.browser_error.is_none());
        app.config.relay = "not an IP address".into();
        let previous_message = app.message.clone();
        app.browse();
        wait_browser(&mut app);
        assert!(app.browser_error.is_some());
        assert_eq!(
            app.message, previous_message,
            "Browser failure must not become a global status error"
        );
        let unavailable = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = unavailable.local_addr().unwrap();
        drop(unavailable);
        app.config.relay = address.to_string();
        app.browse();
        wait_browser(&mut app);
        assert!(app.browser_error.as_ref().unwrap().contains("Cannot reach"));
        let relay = sa_net::relay::Relay::start("127.0.0.1:0".parse().unwrap()).unwrap();
        app.config.relay = relay.address.to_string();
        app.browse();
        assert!(
            app.browser_error.is_none(),
            "Retry clears old errors immediately"
        );
        wait_browser(&mut app);
        assert!(app.browser_error.is_none());
        assert!(app.servers.is_empty());
        assert!(app.browser_time.is_some());
        let (_host, publication) =
            sa_net::Session::host_relay(relay.address, "Launcher test host", true).unwrap();
        let start = Instant::now();
        while publication.report().code.is_empty() {
            assert!(start.elapsed() < Duration::from_secs(8));
            std::thread::sleep(Duration::from_millis(10));
        }
        app.browse();
        wait_browser(&mut app);
        assert_eq!(app.servers.len(), 1);
        assert_eq!(app.servers[0].name, "Launcher test host");
        let guest =
            sa_net::Session::join_relay(relay.address, &app.servers[0].code, "Launcher test guest")
                .unwrap();
        let start = Instant::now();
        while !guest
            .update(sa_net::Pose::default())
            .is_some_and(|r| r.connected)
        {
            assert!(start.elapsed() < Duration::from_secs(8));
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(app.browser_error.is_none());
    }
    #[test]
    fn slow_directory_does_not_block_native_rendering_or_offline_readiness() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (accepted_tx, accepted_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let service = std::thread::spawn(move || {
            let (_socket, _) = listener.accept().unwrap();
            accepted_tx.send(()).unwrap();
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
        });
        let context = egui::Context::default();
        let mut app = Launcher::configured(
            &context,
            LauncherConfig::default(),
            Settings::default(),
            false,
        );
        app.config.relay = address.to_string();
        app.validation = Some((app.config.game_dir.clone(), Default::default()));
        let start = Instant::now();
        app.browse();
        accepted_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(app.ready(), "Directory work must not disable offline play");
        app.page = Page::Multiplayer;
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280., 720.),
                )),
                ..Default::default()
            },
            |ui| app.content(ui),
        );
        output.textures_delta.clear();
        assert!(app.browser_worker.is_some());
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "UI waited for an unresponsive relay"
        );
        release_tx.send(()).unwrap();
        service.join().unwrap();
        wait_browser(&mut app);
        assert!(app.browser_error.is_some());
        assert!(app.ready());
    }
    #[test]
    fn settings_fields_have_desktop_height_and_keyboard_focus() {
        let context = egui::Context::default();
        let _app = Launcher::configured(
            &context,
            LauncherConfig::default(),
            Settings::default(),
            false,
        );
        let mut value = "Installation path".to_owned();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1280., 720.),
                )),
                ..Default::default()
            },
            |ui| {
                let response = input(ui, &mut value, "Path");
                assert!(
                    (40. ..=44.).contains(&response.rect.height()),
                    "Field height: {}",
                    response.rect.height()
                );
                response.request_focus();
                assert!(response.has_focus());
            },
        );
        output.textures_delta.clear();
    }
    #[test]
    fn top_navigation_is_reachable_by_keyboard_and_controller() {
        let context = egui::Context::default();
        let mut app = Launcher::configured(
            &context,
            LauncherConfig::default(),
            Settings::default(),
            false,
        );
        let mut frame = |events| {
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1280., 720.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| app.content(ui),
            );
            output.textures_delta.clear();
        };
        frame(vec![]);
        frame(vec![]);
        for _ in 0..2 {
            frame(vec![
                controller_event(gilrs::Button::DPadDown, true).unwrap()
            ]);
            frame(vec![
                controller_event(gilrs::Button::DPadDown, false).unwrap()
            ]);
        }
        frame(vec![controller_event(gilrs::Button::South, true).unwrap()]);
        frame(vec![controller_event(gilrs::Button::South, false).unwrap()]);
        assert!(app.page == Page::Multiplayer);
        let mut output = context.run_ui(
            egui::RawInput {
                events: vec![controller_event(gilrs::Button::East, true).unwrap()],
                ..Default::default()
            },
            |ui| app.content(ui),
        );
        output.textures_delta.clear();
        assert!(app.page == Page::Home);
    }
    #[test]
    fn stale_installation_and_browser_results_cannot_enable_the_wrong_session() {
        let context = egui::Context::default();
        let mut app = Launcher::configured(
            &context,
            LauncherConfig::default(),
            Settings::default(),
            false,
        );
        app.config.game_dir = "new folder".into();
        let (tx, rx) = mpsc::channel();
        app.worker = Some(rx);
        tx.send(Ok(Work::Validation(
            "old folder".into(),
            Default::default(),
        )))
        .unwrap();
        app.poll();
        assert!(app.validation.is_none());
        assert!(!app.ready());
        assert!(app.message.contains("changed"));
        let (tx, rx) = mpsc::channel();
        app.browser_worker = Some(rx);
        tx.send((
            "different relay".into(),
            Ok(vec![sa_net::relay::Listing {
                name: "Stale server".into(),
                code: "ABCDEF123456".into(),
                players: 1,
                capacity: 20,
                version: sa_net::VERSION,
            }]),
            Duration::from_millis(2),
        ))
        .unwrap();
        app.poll();
        assert!(app.servers.is_empty());
        assert!(app.browser_time.is_none());
    }
    #[test]
    fn controller_events_move_egui_focus_and_activate_once() {
        let context = egui::Context::default();
        let mut clicked = 0;
        let mut frame = |events: Vec<egui::Event>| {
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(640., 480.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| {
                    if ui.button("First").clicked() {
                        clicked += 1;
                    }
                    let _ = ui.button("Second");
                },
            );
            output.textures_delta.clear();
        };
        frame(vec![]);
        frame(vec![
            controller_event(gilrs::Button::DPadDown, true).unwrap()
        ]);
        let first = context.memory(|memory| memory.focused());
        assert!(first.is_some());
        frame(vec![controller_event(gilrs::Button::South, true).unwrap()]);
        frame(vec![controller_event(gilrs::Button::South, false).unwrap()]);
        frame(vec![
            controller_event(gilrs::Button::DPadDown, true).unwrap()
        ]);
        assert_ne!(first, context.memory(|memory| memory.focused()));
        assert_eq!(clicked, 1);
    }
    #[test]
    fn back_returns_to_home_and_disabled_launch_cannot_spawn() {
        let context = egui::Context::default();
        let mut app = Launcher::configured(
            &context,
            LauncherConfig::default(),
            Settings::default(),
            false,
        );
        app.page = Page::Settings;
        let mut output = context.run_ui(
            egui::RawInput {
                events: vec![controller_event(gilrs::Button::East, true).unwrap()],
                ..Default::default()
            },
            |ui| app.content(ui),
        );
        output.textures_delta.clear();
        assert!(app.page == Page::Home);
        assert!(!app.ready());
        app.start(Session::Offline);
        assert!(app.child.is_none());
    }
}
