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
mod local_mods;
const GOLD: Color32 = Color32::from_rgb(223, 183, 120);
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
    Browser(String, Vec<sa_net::relay::Listing>, Duration),
    Cache(sa_net::resources::CachePlan),
    Mods(Vec<local_mods::Entry>),
    Cleared(u64),
}
struct Launcher {
    config: LauncherConfig,
    settings: Settings,
    page: Page,
    message: String,
    validation: Option<(PathBuf, sa_client::install::Validation)>,
    worker: Option<mpsc::Receiver<Result<Work, String>>>,
    servers: Vec<sa_net::relay::Listing>,
    browser_time: Option<Duration>,
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
        style.visuals.panel_fill = Color32::from_rgb(30, 36, 33);
        style.visuals.override_text_color = Some(Color32::from_rgb(239, 233, 221));
        style.visuals.selection.bg_fill = Color32::from_rgb(105, 83, 44);
        style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(74, 80, 65);
        style.spacing.item_spacing = egui::vec2(12., 12.);
        style.spacing.button_padding = egui::vec2(14., 9.);
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(17.));
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(13.));
        ctx.set_theme(egui::Theme::Dark);
        ctx.set_style_of(egui::Theme::Dark, style);

        if config.player.is_empty() {
            config.player = "Player".into();
        }
        if config.relay.is_empty() {
            config.relay = "127.0.0.1:7778".into();
        }
        if detect && config.game_dir.as_os_str().is_empty() {
            config.game_dir = sa_client::install::candidates()
                .into_iter()
                .find(|p| p.join("models").is_dir())
                .unwrap_or_default();
        }
        let mut app = Self {
            config,
            settings,
            page: Page::Home,
            message: String::new(),
            validation: None,
            worker: None,
            servers: Vec::new(),
            browser_time: None,
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
            if let Ok(scale) = std::env::var("SARE_PREVIEW_SCALE")
                .unwrap_or_default()
                .parse::<f32>()
            {
                ctx.set_pixels_per_point(scale.clamp(1., 3.));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(
                    if std::env::var_os("SARE_PREVIEW_COMPACT").is_some() {
                        egui::vec2(640., 480.)
                    } else {
                        egui::vec2(1100., 760.)
                    },
                ));
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
    }
    fn poll(&mut self) {
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
                                "Installation ready. Your original files stay untouched.".into()
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
                        Ok(Work::Browser(address, _, _))
                            if address != self.config.relay || !self.relay_mode =>
                        {
                            self.servers.clear();
                            self.browser_time = None;
                            self.message="Relay selection changed. Refresh the browser for the selected relay.".into();
                        }
                        Ok(Work::Browser(_, servers, elapsed)) => {
                            self.servers = servers;
                            self.browser_time = Some(elapsed);
                            self.message = format!("{} public sessions found.", self.servers.len());
                        }
                        Ok(Work::Cache(plan)) => {
                            self.cache = Some(plan);
                            self.message =
                                "Cache preview ready. Review the list before deleting.".into();
                        }
                        Ok(Work::Mods(entries)) => {
                            self.mods = entries;
                            self.message = "Local resources inspected.".into();
                        }
                        Ok(Work::Cleared(bytes)) => {
                            self.cache = None;
                            self.message = format!(
                                "Removed {:.1} MiB of unused cached downloads.",
                                bytes as f64 / 1048576.
                            );
                        }
                        Err(error) => self.message = sa_client::diagnostics::explain(&error),
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
            Err(error) => self.message = format!("Could not start SARE: {error:#}"),
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
        if let Err(error) = self.config.save() {
            self.message = format!("Could not save favorites: {error}");
        }
    }
    fn browse(&mut self) {
        let address = self.config.relay.clone();
        self.message = "Contacting the relay directory...".into();
        self.servers.clear();
        self.browser_time = None;
        self.work(move||{let selected=address.clone();let address=address.parse().map_err(|_|anyhow::anyhow!("Enter the relay IP and port, for example 192.168.1.10:7778."))?;let start=Instant::now();let servers=sa_net::relay::browse(address).map_err(|e|anyhow::anyhow!("Cannot reach the relay directory: {e}. Check the relay address and ask the host to start it."))?;Ok(Work::Browser(selected,servers,start.elapsed()))});
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
        ui.add_space(22.);
        ui.label(
            RichText::new("San Andreas")
                .font(egui::FontId::new(
                    54.,
                    egui::FontFamily::Name("street".into()),
                ))
                .color(GOLD),
        );
        ui.heading("Your next free roam starts here.");
        ui.label("Explore the state, bring your own cars and characters, or ride with friends.");
        ui.add_space(22.);
        let checkpoint =
            sa_client::progress::Save::load(&sa_client::config_dir().join("progress.json"))
                .ok()
                .flatten();
        let label = if checkpoint.is_some() {
            "Continue free roam"
        } else {
            "Play free roam"
        };
        if ui
            .add_enabled(
                self.ready(),
                egui::Button::new(RichText::new(label).size(23.))
                    .min_size(egui::vec2(270., 60.))
                    .fill(Color32::from_rgb(100, 79, 43)),
            )
            .clicked()
        {
            self.start(Session::Offline)
        }
        ui.add_space(16.);
        self.dashboard(ui);
        ui.add_space(14.);
        if let Some(save) = checkpoint {
            ui.label(format!(
                "Checkpoint: {} / {}. Safe positions are checked again when the game loads.",
                save.ped, save.car
            ));
        }
        if !self.validation.as_ref().is_some_and(|(_, v)| v.ready()) {
            ui.label("First time here? Choose your original PC installation in Settings.");
            if ui.button("Choose installation").clicked() {
                self.page = Page::Settings;
            }
        }
        ui.separator();
        ui.label("Offline play needs no account or external service.");
        ui.small(
            "In-game shortcuts: F7 cars / F8 characters / F6 wardrobe. More controls in Pause.",
        );
    }
    fn dashboard(&mut self, ui: &mut egui::Ui) {
        if ui.available_width() >= 780. {
            ui.columns(3, |columns| {
                for (index, column) in columns.iter_mut().enumerate() {
                    self.dashboard_card(column, index);
                }
            });
        } else {
            for index in 0..3 {
                self.dashboard_card(ui, index);
                ui.add_space(10.);
            }
        }
    }
    fn dashboard_card(&mut self, ui: &mut egui::Ui, index: usize) {
        egui::Frame::new()
            .fill(if index == 0 {
                Color32::from_rgb(43, 55, 42)
            } else {
                Color32::from_rgb(39, 44, 41)
            })
            .inner_margin(egui::Margin::same(18))
            .corner_radius(8)
            .show(ui, |ui| {
                ui.set_min_width((ui.available_width() - 2.).max(100.));
                ui.set_min_height(150.);
                match index {
                    0 => {
                        ui.label(RichText::new("PLAY TOGETHER").color(GOLD).strong());
                        ui.label(
                            "Find a session on your relay or connect with a friend's join code.",
                        );
                        ui.add_space(8.);
                        if ui.button("Browse servers").clicked() {
                            self.page = Page::Multiplayer;
                            self.browse();
                        }
                    }
                    1 => {
                        ui.label(RichText::new("LAST LAUNCHED SESSION").color(GOLD).strong());
                        if let Some(recent) = self.config.last_session.clone() {
                            ui.label(&recent.name);
                            ui.label(if recent.relay {
                                "Relay / join code"
                            } else {
                                "Direct connection"
                            });
                            if ui
                                .add_enabled(self.ready(), egui::Button::new("Reconnect"))
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
                        } else {
                            ui.label("Your next multiplayer launch will appear here.");
                        }
                    }
                    _ => {
                        ui.label(RichText::new("FAVORITES").color(GOLD).strong());
                        ui.label(format!("{} saved sessions", self.config.favorites.len()));
                        ui.label("Keep trusted hosts close. Room codes may change after restart.");
                        ui.add_space(8.);
                        if ui.button("Open favorites").clicked() {
                            self.page = Page::Multiplayer;
                        }
                    }
                }
            });
    }
    fn multiplayer(&mut self, ui: &mut egui::Ui) {
        ui.heading("Play together");
        ui.label("Up to 20 players. Use a trusted LAN/VPN for this prototype.");
        ui.horizontal_wrapped(|ui| {
            ui.label("Player name");
            ui.add(egui::TextEdit::singleline(&mut self.config.player).char_limit(24));
        });
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.relay_mode, true, "Relay / join code");
            ui.selectable_value(&mut self.relay_mode, false, "Direct IP");
        });
        if self.relay_mode {
            ui.label("Relay address (IP:port)");
            if ui.text_edit_singleline(&mut self.config.relay).changed() {
                self.servers.clear();
                self.browser_time = None;
            }
            ui.label("Join code");
            ui.add(egui::TextEdit::singleline(&mut self.code).char_limit(12));
        } else {
            ui.label("Host address (IP:port)");
            ui.text_edit_singleline(&mut self.direct);
        }
        ui.checkbox(
            &mut self.settings.auto_mod_downloads,
            "Automatically download required server mods",
        );
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(self.ready(), egui::Button::new("Join session"))
                .clicked()
            {
                let session = if self.relay_mode {
                    Session::Relay {
                        address: self.config.relay.clone(),
                        code: self.code.clone(),
                    }
                } else {
                    Session::Direct(self.direct.clone())
                };
                self.start(session);
            }
            if ui.button("Save favorite").clicked() {
                self.favorite(if self.relay_mode {
                    "Saved relay session".into()
                } else {
                    self.direct.clone()
                });
            }
            if self.relay_mode
                && ui
                    .add_enabled(
                        self.worker.is_none(),
                        egui::Button::new("Refresh server browser"),
                    )
                    .clicked()
            {
                self.browse();
            }
        });
        if let Some(time) = self.browser_time {
            ui.label(format!(
                "Directory request: {:.0} ms (includes connecting; not gameplay ping)",
                time.as_secs_f64() * 1000.
            ));
        }
        for server in self.servers.clone() {
            ui.horizontal_wrapped(|ui| {
                ui.label(format!(
                    "{}   {} / {} players",
                    server.name, server.players, server.capacity
                ));
                let compatible = server.version == sa_net::VERSION;
                ui.label(if compatible {
                    "Compatible"
                } else {
                    "Different protocol ? update both clients and server"
                });
                if ui
                    .add_enabled(
                        compatible && server.players < server.capacity && self.ready(),
                        egui::Button::new("Join"),
                    )
                    .clicked()
                {
                    self.code = server.code;
                    self.start(Session::Relay {
                        address: self.config.relay.clone(),
                        code: self.code.clone(),
                    });
                }
            });
        }
        ui.separator();
        ui.heading("Favorites");
        if self.config.favorites.is_empty() {
            ui.label("Save a direct address or relay session above. Room codes may change after a host restarts.");
        }
        let mut remove = None;
        for (index, favorite) in self.config.favorites.clone().into_iter().enumerate() {
            ui.horizontal_wrapped(|ui| {
                if ui.button(&favorite.name).clicked() {
                    self.relay_mode = favorite.relay;
                    if favorite.relay {
                        self.config.relay = favorite.address;
                        self.code = favorite.code;
                    } else {
                        self.direct = favorite.address;
                    }
                }
                if ui.small_button("Remove").clicked() {
                    remove = Some(index);
                }
            });
        }
        if let Some(index) = remove {
            self.config.favorites.remove(index);
            if let Err(error) = self.config.save() {
                self.message = error.to_string();
            }
        }
        ui.separator();
        ui.label("No public relay or Steam service is configured. Hosts can still create sessions from Multiplayer inside the game.");
    }
    fn resources(&mut self, ui: &mut egui::Ui) {
        ui.heading("Your resources");
        ui.label("Local mods, the current server's resources and downloaded cache are separate.");
        ui.horizontal_wrapped(|ui| {
            if ui
                .add_enabled(
                    self.worker.is_none(),
                    egui::Button::new("Inspect local mods"),
                )
                .clicked()
            {
                let root = local_mods::root();
                self.work(move || Ok(Work::Mods(local_mods::inspect(&root)?)));
            }
            if ui
                .add_enabled(
                    self.worker.is_none(),
                    egui::Button::new("Preview cached downloads"),
                )
                .clicked()
            {
                self.work(move || {
                    Ok(Work::Cache(sa_net::resources::inspect_cache(
                        &sa_client::cache_dir(),
                    )?))
                });
            }
        });
        for entry in &self.mods {
            ui.label(format!(
                "{} / {} / {:.1} MiB",
                entry.name,
                entry.status,
                entry.bytes as f64 / 1048576.
            ));
            if let Some(error) = &entry.error {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
        }
        ui.separator();
        ui.heading("Server resources");
        ui.label(if self.child.is_some(){"Open /mods in the game for the current session's resource list and download progress."}else{"No game was started by this launcher. The runtime restores local mods when you disconnect."});
        if let Some(plan) = self.cache.clone() {
            ui.separator();
            ui.heading("Cache cleanup preview");
            ui.label(format!(
                "{} cached sessions; {:.1} MiB total including reusable blobs.",
                plan.packs.len(),
                plan.bytes as f64 / 1048576.
            ));
            for pack in &plan.packs {
                ui.label(format!(
                    "{} / {:.1} MiB / {}",
                    if pack.names.is_empty() {
                        pack.fingerprint[..12].into()
                    } else {
                        pack.names.join(", ")
                    },
                    pack.bytes as f64 / 1048576.,
                    if pack.complete {
                        "Inventory recorded"
                    } else {
                        "Incomplete or invalid inventory"
                    }
                ));
            }
            ui.label("Delete removes these cached session folders and reusable blobs. Original files and your local mods are preserved. Required mods download again next time.");
            if plan.busy {
                ui.colored_label(GOLD,"A game session is using the cache. Close all multiplayer sessions before cleanup.");
            }
            if ui
                .add_enabled(
                    !plan.busy && self.worker.is_none() && self.child.is_none(),
                    egui::Button::new("Delete the previewed cached downloads"),
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
        }
    }
    fn settings_ui(&mut self, ui: &mut egui::Ui) {
        ui.heading("Installation & client settings");
        ui.label("Original PC San Andreas folder");
        let mut text = self.config.game_dir.to_string_lossy().into_owned();
        if ui.text_edit_singleline(&mut text).changed() {
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
        if let Some((_, report)) = &self.validation {
            for error in &report.errors {
                ui.colored_label(Color32::LIGHT_RED, error);
            }
            for warning in &report.warnings {
                ui.colored_label(GOLD, warning);
            }
        }
        ui.separator();
        ui.add_enabled_ui(self.child.is_none(),|ui|{ui.label("Renderer (game restart required)");egui::ComboBox::from_id_salt("renderer").selected_text(self.settings.renderer.name()).show_ui(ui,|ui|{for value in [Renderer::Auto,Renderer::Vulkan,Renderer::DirectX12]{if cfg!(windows)||value!=Renderer::DirectX12 {ui.selectable_value(&mut self.settings.renderer,value,value.name());}}});ui.checkbox(&mut self.settings.fullscreen,"Fullscreen");ui.checkbox(&mut self.settings.vsync,"VSync");ui.checkbox(&mut self.settings.auto_mod_downloads,"Automatically download required server mods");ui.label("Graphics profile");ui.horizontal_wrapped(|ui|{for(index,name)in ["Performance","Balanced","Quality","Ultra"].iter().enumerate(){if ui.button(*name).clicked(){self.settings.apply_preset(index);}}});ui.label(format!("Current profile: {}. More graphics, audio and gameplay controls are available in the game.",self.settings.preset()));if ui.button("Save client settings").clicked(){let result=(||->anyhow::Result<()>{let _guard=sa_client::launch::RuntimeGuard::acquire()?;self.config.save()?;self.settings.sanitize();self.settings.save()})();self.message=match result{Ok(())=>"Client settings saved.".into(),Err(e)=>format!("Could not save settings: {e}")};}});
    }
    fn help(&mut self, ui: &mut egui::Ui) {
        ui.heading("Help & diagnostics");
        ui.label(format!(
            "SARE {} / build {} / network protocol {}",
            sa_client::VERSION,
            sa_client::BUILD_ID,
            sa_net::VERSION
        ));
        ui.label("This source build may include local changes. Reports use the last observed runtime GPU and renderer.");
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
        ui.label("Use a complete client package containing both launcher and runtime. Automatic updating is unavailable until a signed release channel exists.");
        ui.hyperlink_to(
            "Project and download information",
            "https://github.com/LordM8YT/GTA-San-Andreas-Rust-Edition",
        );
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
            if self.preview_frames >= 6 && self.worker.is_none() && !self.preview_requested {
                self.preview_requested = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
            }
            ctx.request_repaint();
        }
        self.poll();
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.page = Page::Home;
        }
        ui.add_space(8.);
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new("SARE")
                    .font(egui::FontId::new(
                        32.,
                        egui::FontFamily::Name("street".into()),
                    ))
                    .color(GOLD),
            );
            ui.label("San Andreas Rust Edition");
        });
        ui.add_space(4.);
        let pages = [
            (Page::Home, "Free roam"),
            (Page::Multiplayer, "Play together"),
            (Page::Resources, "Resources"),
            (Page::Settings, "Settings"),
            (Page::Help, "Help"),
        ];
        if ui.available_width() < 640. {
            egui::ComboBox::from_id_salt("navigation")
                .selected_text(pages.iter().find(|(page, _)| *page == self.page).unwrap().1)
                .show_ui(ui, |ui| {
                    for (page, label) in pages {
                        ui.selectable_value(&mut self.page, page, label);
                    }
                });
        } else {
            ui.horizontal_wrapped(|ui| {
                for (page, label) in pages {
                    if ui.selectable_label(self.page == page, label).clicked() {
                        self.page = page;
                    }
                }
            });
        }
        ui.separator();
        let version = format!(
            "Client {}   Build {}   Protocol {}",
            sa_client::VERSION,
            sa_client::BUILD_ID,
            sa_net::VERSION
        );
        let hint = "Tab / D-pad: focus   Enter / A: choose   Esc / B: back";
        let width = ui.available_width().max(1.);
        let message_height = ui
            .painter()
            .layout(
                self.message.clone(),
                egui::FontId::proportional(17.),
                Color32::WHITE,
                width - 30.,
            )
            .size()
            .y;
        let footer_height = message_height
            + [version.as_str(), hint]
                .iter()
                .map(|s| {
                    ui.painter()
                        .layout(
                            s.to_string(),
                            egui::FontId::proportional(13.),
                            Color32::WHITE,
                            width,
                        )
                        .size()
                        .y
                })
                .sum::<f32>()
            + 48.;
        egui::ScrollArea::vertical()
            .id_salt(("launcher-body", self.page as u8))
            .auto_shrink([false, false])
            .max_height((ui.available_height() - footer_height).max(20.))
            .show(ui, |ui| {
                ui.set_max_width(960.);
                match self.page {
                    Page::Home => self.home(ui),
                    Page::Multiplayer => self.multiplayer(ui),
                    Page::Resources => self.resources(ui),
                    Page::Settings => self.settings_ui(ui),
                    Page::Help => self.help(ui),
                }
            });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if self.worker.is_some() {
                ui.spinner();
            }
            ui.label(&self.message);
        });
        ui.small(version);
        ui.small(hint);
        if self.worker.is_some() || self.child.is_some() || self.gilrs.is_some() {
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
            .fill(Color32::from_rgb(30, 36, 33))
            .inner_margin(egui::Margin::same(20))
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
fn main() -> eframe::Result {
    let compact = std::env::var_os("SARE_SCREENSHOT_TO").is_some()
        && std::env::var_os("SARE_PREVIEW_COMPACT").is_some();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size(if compact { [640., 480.] } else { [1100., 760.] })
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
        app.worker = Some(rx);
        tx.send(Ok(Work::Browser(
            "different relay".into(),
            vec![sa_net::relay::Listing {
                name: "Stale server".into(),
                code: "ABCDEF123456".into(),
                players: 1,
                capacity: 20,
                version: sa_net::VERSION,
            }],
            Duration::from_millis(2),
        )))
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
