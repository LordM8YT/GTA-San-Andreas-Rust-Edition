//! Server browser laid out like the FiveM client's: tabs, a searchable
//! table of servers, a details panel with one Connect button, and direct
//! connect as a dialog.
use super::{
    input, input_limit, log_error, primary, surface, Launcher, Session, ACCENT, MUTED,
    SURFACE, SURFACE_HIGH, TEXT,
};
use eframe::egui::{self, Color32, RichText};

#[derive(Clone, Copy, PartialEq, Default)]
pub enum ServerTab {
    #[default]
    All,
    Favorites,
    History,
}
/// One row in the table, whether live from the relay or saved locally.
#[derive(Clone)]
struct Row {
    key: String,
    name: String,
    detail: String,
    players: Option<(usize, usize)>,
    compatible: bool,
    session: Session,
    saved: Option<usize>,
}

fn initial(name: &str) -> String {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into())
}

/// A table row: square icon, name and detail, players and status columns.
fn server_row(ui: &mut egui::Ui, row: &Row, selected: bool) -> egui::Response {
    let width = ui.available_width();
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width, 58.), egui::Sense::click());
    let fill = if selected {
        Color32::from_rgb(48, 34, 28)
    } else if response.hovered() {
        SURFACE_HIGH
    } else {
        SURFACE
    };
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4, fill);
    if selected {
        painter.rect_filled(
            egui::Rect::from_min_size(rect.left_top(), egui::vec2(3., rect.height())),
            0,
            ACCENT,
        );
    }
    let icon = egui::Rect::from_center_size(
        rect.left_center() + egui::vec2(32., 0.),
        egui::vec2(36., 36.),
    );
    painter.rect_filled(icon, 4, Color32::from_rgb(62, 36, 24));
    painter.text(
        icon.center(),
        egui::Align2::CENTER_CENTER,
        initial(&row.name),
        egui::FontId::proportional(18.),
        ACCENT,
    );
    let text_left = icon.right() + 14.;
    let columns = width > 560.;
    let name_width = if columns { width - 340. } else { width - 80. };
    let name = egui::text::LayoutJob::simple_singleline(
        row.name.clone(),
        egui::FontId::proportional(17.),
        TEXT,
    );
    let galley = ui.fonts_mut(|f| f.layout_job(name));
    painter
        .with_clip_rect(egui::Rect::from_min_size(
            egui::pos2(text_left, rect.top()),
            egui::vec2(name_width.max(40.), rect.height()),
        ))
        .galley(egui::pos2(text_left, rect.top() + 9.), galley, TEXT);
    painter.text(
        egui::pos2(text_left, rect.top() + 34.),
        egui::Align2::LEFT_TOP,
        &row.detail,
        egui::FontId::proportional(13.),
        MUTED,
    );
    if columns {
        let players = row
            .players
            .map(|(n, max)| format!("{n} / {max}"))
            .unwrap_or_else(|| "—".into());
        painter.text(
            egui::pos2(rect.right() - 230., rect.center().y),
            egui::Align2::LEFT_CENTER,
            players,
            egui::FontId::proportional(15.),
            TEXT,
        );
        let (status, color) = if !row.compatible {
            ("Update needed", Color32::from_rgb(230, 120, 90))
        } else if row.players.is_some_and(|(n, max)| n >= max) {
            ("Full", Color32::from_rgb(230, 170, 90))
        } else if row.players.is_some() {
            ("Online", Color32::from_rgb(120, 205, 130))
        } else {
            ("Saved", MUTED)
        };
        painter.circle_filled(
            egui::pos2(rect.right() - 118., rect.center().y),
            4.,
            color,
        );
        painter.text(
            egui::pos2(rect.right() - 108., rect.center().y),
            egui::Align2::LEFT_CENTER,
            status,
            egui::FontId::proportional(14.),
            color,
        );
    }
    response
}

impl Launcher {
    fn rows(&self) -> Vec<Row> {
        let search = self.search.to_lowercase();
        let mut rows: Vec<Row> = match self.server_tab {
            ServerTab::All => self
                .servers
                .iter()
                .map(|server| Row {
                    key: format!("live:{}", server.code),
                    name: server.name.clone(),
                    detail: format!("Join code {} · relay {}", server.code, self.config.relay),
                    players: Some((server.players, server.capacity)),
                    compatible: server.version == sa_net::VERSION,
                    session: Session::Relay {
                        address: self.config.relay.clone(),
                        code: server.code.clone(),
                    },
                    saved: self.config.favorites.iter().position(|f| {
                        f.relay && f.address == self.config.relay && f.code == server.code
                    }),
                })
                .collect(),
            ServerTab::Favorites => self
                .config
                .favorites
                .iter()
                .enumerate()
                .map(|(index, f)| saved_row(f, Some(index)))
                .collect(),
            ServerTab::History => self
                .config
                .last_session
                .iter()
                .map(|f| saved_row(f, None))
                .collect(),
        };
        rows.retain(|r| r.name.to_lowercase().contains(&search));
        rows
    }

    pub(super) fn multiplayer(&mut self, ui: &mut egui::Ui) {
        if !self.browser_initialized {
            self.browser_initialized = true;
            if !self.config.relay.trim().is_empty() {
                self.browse();
            }
        }
        if self.favorites_only {
            self.server_tab = ServerTab::Favorites;
            self.favorites_only = false;
        }
        ui.horizontal(|ui| {
            for (tab, label) in [
                (ServerTab::All, "All servers"),
                (ServerTab::Favorites, "Favorites"),
                (ServerTab::History, "History"),
            ] {
                let active = self.server_tab == tab;
                let response = ui.add(
                    egui::Button::new(
                        RichText::new(label)
                            .size(17.)
                            .color(if active { TEXT } else { MUTED }),
                    )
                    .frame(false),
                );
                if active {
                    let r = response.rect;
                    ui.painter().rect_filled(
                        egui::Rect::from_min_max(
                            egui::pos2(r.left(), r.bottom() + 2.),
                            egui::pos2(r.right(), r.bottom() + 5.),
                        ),
                        1,
                        ACCENT,
                    );
                }
                if response.clicked() {
                    self.server_tab = tab;
                    self.selected_server = None;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Direct connect").clicked() {
                    self.relay_mode = false;
                    self.direct_open = true;
                }
                if ui
                    .add_enabled(
                        self.browser_worker.is_none() && !self.config.relay.trim().is_empty(),
                        egui::Button::new("⟳ Refresh"),
                    )
                    .clicked()
                {
                    self.browse();
                }
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Search servers")
                        .desired_width(240.)
                        .margin(egui::vec2(12., 9.)),
                );
            });
        });
        ui.add_space(12.);
        self.browser_notices(ui);

        let rows = self.rows();
        let wide = ui.available_width() >= 980.;
        let list_width = if wide {
            ui.available_width() - 380.
        } else {
            ui.available_width()
        };
        ui.horizontal_top(|ui| {
            ui.allocate_ui(egui::vec2(list_width, 0.), |ui| {
                ui.vertical(|ui| {
                    if rows.len() > 0 && list_width > 560. {
                        ui.horizontal(|ui| {
                            ui.add_space(64.);
                            ui.label(RichText::new("SERVER").small().color(MUTED));
                            ui.add_space((list_width - 64. - 300.).max(10.));
                            ui.label(RichText::new("PLAYERS").small().color(MUTED));
                            ui.add_space(46.);
                            ui.label(RichText::new("STATUS").small().color(MUTED));
                        });
                    }
                    ui.spacing_mut().item_spacing.y = 4.;
                    for row in &rows {
                        let selected = self.selected_server.as_deref() == Some(row.key.as_str());
                        let response = server_row(ui, row, selected);
                        if response.clicked() {
                            self.selected_server = Some(row.key.clone());
                        }
                        if response.double_clicked() && row.compatible && self.ready() {
                            self.start(row.session.clone());
                        }
                    }
                    if rows.is_empty() {
                        self.empty_tab(ui);
                    }
                });
            });
            if wide {
                ui.add_space(16.);
                ui.vertical(|ui| {
                    ui.set_width(364.);
                    self.details(ui, &rows);
                });
            }
        });
        if !wide {
            ui.add_space(12.);
            self.details(ui, &rows);
        }
        self.direct_dialog(ui);
    }

    fn browser_notices(&mut self, ui: &mut egui::Ui) {
        if self.server_tab != ServerTab::All {
            return;
        }
        if self.browser_worker.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading server directory...");
            });
        } else if let Some(error) = self.browser_error.clone() {
            surface(ui, |ui| {
                ui.colored_label(
                    ACCENT,
                    "Server directory unavailable. Offline free roam is still available.",
                );
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Retry").clicked() {
                        self.browse();
                    }
                    if ui.button("Configure connection").clicked() {
                        self.relay_mode = true;
                        self.direct_open = true;
                    }
                });
                ui.collapsing("Details", |ui| {
                    ui.label(error);
                    ui.small("Check the relay address and ask the host whether the relay is running. Technical errors are recorded in the launcher log.");
                });
            });
        } else if self.config.relay.trim().is_empty() {
            surface(ui, |ui| {
                ui.label(RichText::new("No relay configured").strong());
                ui.label("Enter a relay supplied by your host to browse its public servers. Direct IP connections also work without a directory.");
                ui.horizontal_wrapped(|ui| {
                    if ui.button("Configure relay").clicked() {
                        self.relay_mode = true;
                        self.direct_open = true;
                    }
                    if ui.button("Direct connect").clicked() {
                        self.relay_mode = false;
                        self.direct_open = true;
                    }
                });
            });
        } else if self.browser_time.is_none() {
            ui.label("Refresh to load servers from your configured relay.");
        } else if self
            .config
            .relay
            .parse::<std::net::SocketAddr>()
            .is_ok_and(|a| a.ip().is_loopback())
        {
            ui.small("Local relay: this address connects only to a service on this PC.");
        }
        if let Some(error) = self.connection_error.clone() {
            surface(ui, |ui| {
                ui.colored_label(ACCENT, "The connection action could not be completed.");
                if ui.button("Review connection").clicked() {
                    self.direct_open = true;
                }
                ui.collapsing("Connection details", |ui| {
                    ui.label(error);
                });
            });
        }
        ui.add_space(4.);
    }

    fn empty_tab(&mut self, ui: &mut egui::Ui) {
        let text = match self.server_tab {
            ServerTab::All if !self.search.is_empty() && !self.servers.is_empty() => {
                "No servers match this search."
            }
            ServerTab::All if self.browser_time.is_some() => {
                "No public servers on this relay yet. A host can publish one from the game or sa-server."
            }
            ServerTab::All => "",
            ServerTab::Favorites => "No favorites yet. Select a server and add it to favorites.",
            ServerTab::History => "Servers you join appear here.",
        };
        if !text.is_empty() {
            ui.add_space(12.);
            ui.label(RichText::new(text).color(MUTED));
        }
    }

    fn details(&mut self, ui: &mut egui::Ui, rows: &[Row]) {
        let row = rows
            .iter()
            .find(|r| self.selected_server.as_deref() == Some(r.key.as_str()))
            .or(rows.first())
            .cloned();
        let Some(row) = row else { return };
        egui::Frame::new()
            .fill(SURFACE)
            .corner_radius(6)
            .inner_margin(20)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new(&row.name).size(24.).strong());
                ui.label(RichText::new(&row.detail).small().color(MUTED));
                ui.add_space(10.);
                if let Some((players, capacity)) = row.players {
                    ui.label(format!("{players} of {capacity} players"));
                    ui.add(
                        egui::ProgressBar::new(players as f32 / capacity.max(1) as f32)
                            .fill(ACCENT)
                            .desired_height(6.),
                    );
                }
                if !row.compatible {
                    ui.colored_label(
                        Color32::from_rgb(230, 120, 90),
                        format!(
                            "This server needs a different SARE version (protocol {}).",
                            sa_net::VERSION
                        ),
                    );
                }
                ui.add_space(14.);
                let full = row.players.is_some_and(|(n, max)| n >= max);
                let connect = ui.add_enabled(
                    row.compatible && !full && self.ready(),
                    primary("CONNECT").min_size(egui::vec2(ui.available_width(), 48.)),
                );
                if connect.clicked() {
                    self.start(row.session.clone());
                }
                if !self.ready() {
                    ui.small("Choose a valid installation in Settings to connect.");
                }
                ui.horizontal_wrapped(|ui| match (row.saved, self.server_tab) {
                    (None, _) => {
                        if ui.button("☆ Add to favorites").clicked() {
                            self.save_row(&row);
                        }
                    }
                    (Some(index), _) => {
                        ui.label(RichText::new("★ Favorite").color(ACCENT));
                        if ui.button("Remove").clicked() {
                            self.config.favorites.remove(index);
                            self.selected_server = None;
                            if let Err(e) = self.config.save() {
                                let error = format!("Could not save favorites: {e}");
                                log_error(&error);
                                self.connection_error = Some(error);
                            }
                        }
                        if ui.button("Edit").clicked() {
                            self.edit_session(&row.session);
                        }
                    }
                });
            });
    }

    fn save_row(&mut self, row: &Row) {
        match &row.session {
            Session::Relay { address, code } => {
                self.relay_mode = true;
                self.config.relay = address.clone();
                self.code = code.clone();
            }
            Session::Direct(address) => {
                self.relay_mode = false;
                self.direct = address.clone();
            }
            Session::Offline => return,
        }
        self.favorite(row.name.clone());
    }

    fn edit_session(&mut self, session: &Session) {
        match session {
            Session::Relay { address, code } => {
                self.relay_mode = true;
                self.config.relay = address.clone();
                self.code = code.clone();
                self.servers.clear();
                self.browser_time = None;
                self.browser_error = None;
            }
            Session::Direct(address) => {
                self.relay_mode = false;
                self.direct = address.clone();
            }
            Session::Offline => {}
        }
        self.direct_open = true;
    }

    fn direct_dialog(&mut self, ui: &mut egui::Ui) {
        if !self.direct_open {
            return;
        }
        let ctx = ui.ctx().clone();
        let modal = egui::Modal::new(egui::Id::new("direct-connect"))
            .frame(egui::Frame::new().fill(SURFACE).corner_radius(6).inner_margin(24))
            .show(&ctx, |ui| {
                ui.set_width(460.);
                ui.label(RichText::new("Direct connect").size(22.).strong());
                ui.spacing_mut().item_spacing.y = 8.;
                ui.label("Player name");
                input_limit(ui, &mut self.config.player, "Player", 24);
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.relay_mode, false, "IP address");
                    ui.selectable_value(&mut self.relay_mode, true, "Relay / join code");
                });
                if self.relay_mode {
                    ui.label("Relay address (IP:port)");
                    if input(ui, &mut self.config.relay, "Relay supplied by your host").changed() {
                        self.servers.clear();
                        self.browser_time = None;
                        self.browser_error = None;
                    }
                    ui.label("Join code");
                    input_limit(ui, &mut self.code, "12-character code", 12);
                } else {
                    ui.label("Server address (IP:port)");
                    input(ui, &mut self.direct, "127.0.0.1:7777");
                }
                ui.add_space(6.);
                let mut close = false;
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(self.ready() && self.connection_valid(), primary("Connect"))
                        .clicked()
                    {
                        close = true;
                        self.start(if self.relay_mode {
                            Session::Relay {
                                address: self.config.relay.clone(),
                                code: self.code.clone(),
                            }
                        } else {
                            Session::Direct(self.direct.clone())
                        });
                    }
                    if ui
                        .add_enabled(self.connection_valid(), egui::Button::new("Save favorite"))
                        .clicked()
                    {
                        self.favorite(if self.relay_mode {
                            "Saved relay session".into()
                        } else {
                            self.direct.clone()
                        });
                    }
                    if ui.button("Save relay").clicked() {
                        match self.config.save() {
                            Ok(()) => {
                                self.connection_error = None;
                                self.message = "Connection saved.".into();
                                if !self.config.relay.trim().is_empty() {
                                    self.browse();
                                }
                            }
                            Err(e) => {
                                let error = format!("Could not save connection: {e}");
                                log_error(&error);
                                self.connection_error = Some(error);
                            }
                        }
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
                ui.small("Servers run sa-server, or a player hosts from Multiplayer in the game.");
                close
            });
        if modal.inner || modal.should_close() {
            self.direct_open = false;
        }
    }
}

fn saved_row(f: &sa_client::Favorite, saved: Option<usize>) -> Row {
    Row {
        key: format!("saved:{}:{}:{}", f.relay, f.address, f.code),
        name: f.name.clone(),
        detail: if f.relay {
            format!("Join code {} · relay {}", f.code, f.address)
        } else {
            f.address.clone()
        },
        players: None,
        compatible: true,
        session: if f.relay {
            Session::Relay {
                address: f.address.clone(),
                code: f.code.clone(),
            }
        } else {
            Session::Direct(f.address.clone())
        },
        saved,
    }
}
