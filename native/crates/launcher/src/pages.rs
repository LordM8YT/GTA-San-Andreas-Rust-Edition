//! SARE Client pages built on the experience list: Home, Discover, Library,
//! the experience page, Create, Profile and the social pages. Launching
//! reuses the launcher's existing start paths.
use crate::{
    experience::{self, Experience, Kind},
    local_mods,
    shell::{
        self, glyph,
        palette::{ACCENT, BACKGROUND, MUTED, SURFACE, TEXT},
        RowAction,
    },
    Launcher, Page,
};
use eframe::egui::{self, Color32, RichText};
use sa_client::launch::Session;

impl Launcher {
    /// Everything to play; bundled examples live under Create as templates.
    fn games(&self) -> Vec<Experience> {
        self.experiences
            .iter()
            .filter(|e| !e.template)
            .cloned()
            .collect()
    }
    fn find(&self, id: &str) -> Option<Experience> {
        self.experiences.iter().find(|e| e.id == id).cloned()
    }
    pub(super) fn open_experience(&mut self, id: &str) {
        self.selected = Some(id.to_owned());
        self.page = Page::Details;
    }
    fn needs_setup(&self, experience: &Experience) -> bool {
        experience.kind == Kind::SanAndreas && !self.ready() && self.child.is_none()
    }
    fn can_play(&self, experience: &Experience) -> bool {
        match experience.kind {
            Kind::SanAndreas => self.ready(),
            _ => self.testbed_ready(),
        }
    }
    fn play(&mut self, experience: &Experience) {
        match &experience.kind {
            Kind::SanAndreas => self.start(Session::Offline),
            Kind::Testbed => self.start_testbed(),
            Kind::Pack(folder) => self.start_pack(folder.clone()),
        }
    }
    /// Show a grid of tiles; a click opens the experience page.
    fn grid(&mut self, ui: &mut egui::Ui, list: &[Experience]) {
        let refs: Vec<&Experience> = list.iter().collect();
        if let Some(index) = shell::card_grid(ui, &refs, &mut self.covers, self.hero.as_ref()) {
            let id = list[index].id.clone();
            self.open_experience(&id);
        }
    }
    /// One shelf on Home; "See all" goes to `all`.
    fn shelf(&mut self, ui: &mut egui::Ui, heading: &str, list: &[Experience], all: Page) {
        if list.is_empty() {
            return;
        }
        let refs: Vec<&Experience> = list.iter().collect();
        match shell::shelf(ui, heading, &refs, &mut self.covers, self.hero.as_ref()) {
            Some(RowAction::Open(index)) => {
                let id = list[index].id.clone();
                self.open_experience(&id);
            }
            Some(RowAction::SeeAll) => self.page = all,
            None => {}
        }
    }
    /// The main button for an experience: Play, or Set up for San Andreas
    /// without an installation.
    fn primary_action(&mut self, ui: &mut egui::Ui, experience: &Experience, label: &str) {
        if self.needs_setup(experience) {
            if ui
                .add(shell::icon_button(glyph::FOLDER_OPEN, "Set up"))
                .on_hover_text("Choose your own San Andreas installation in Settings")
                .clicked()
            {
                self.page = Page::Settings;
            }
        } else if ui
            .add_enabled(self.can_play(experience), shell::play_button(label))
            .on_disabled_hover_text("Close the running game first")
            .clicked()
        {
            self.play(experience);
        }
    }

    pub(super) fn home(&mut self, ui: &mut egui::Ui) {
        // Featured: worlds with their own cover first, then the rest.
        let mut featured: Vec<Experience> = self.games();
        featured.sort_by_key(|e| {
            let own_cover = e
                .pack_folder()
                .is_some_and(|f| f.join("cover.png").is_file());
            (!own_cover, !e.rich, !matches!(e.kind, Kind::Pack(_)))
        });
        if !featured.is_empty() {
            let index = self.featured % featured.len();
            let experience = featured[index].clone();
            let width = ui.available_width();
            let height = (width * 0.36).clamp(230., 360.);
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
            if shell::banner(ui, rect, &experience, &mut self.covers, self.hero.as_ref()).clicked()
            {
                self.open_experience(&experience.id);
            }
            let inner = rect.shrink2(egui::vec2(32., 28.));
            let text_width = (inner.width() * 0.55).max(260.);
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(egui::Rect::from_min_size(
                        egui::pos2(inner.left(), inner.bottom() - 150.),
                        egui::vec2(text_width, 150.),
                    ))
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    ui.spacing_mut().item_spacing.y = 4.;
                    ui.label(
                        RichText::new(&experience.name)
                            .font(shell::display(if width < 700. { 28. } else { 36. }))
                            .color(Color32::WHITE),
                    );
                    ui.add(
                        egui::Label::new(
                            RichText::new(&experience.description).color(Color32::from_gray(210)),
                        )
                        .truncate(),
                    );
                    ui.add_space(10.);
                    ui.horizontal(|ui| {
                        self.primary_action(ui, &experience, "Play");
                        if ui.add(shell::icon_button(glyph::INFO, "Details")).clicked() {
                            self.open_experience(&experience.id);
                        }
                    });
                },
            );
            if featured.len() > 1 {
                // Dots and arrows to step through the featured worlds.
                let corner = egui::pos2(inner.right(), inner.bottom() - 16.);
                ui.scope_builder(
                    egui::UiBuilder::new()
                        .max_rect(egui::Rect::from_min_max(
                            egui::pos2(corner.x - 220., corner.y - 18.),
                            egui::pos2(corner.x, corner.y + 18.),
                        ))
                        .layout(egui::Layout::right_to_left(egui::Align::Center)),
                    |ui| {
                        ui.spacing_mut().item_spacing.x = 6.;
                        let arrow = |glyph: &'static str| {
                            egui::Button::new(shell::icon(glyph, Color32::WHITE))
                                .fill(Color32::from_black_alpha(120))
                                .corner_radius(16)
                                .min_size(egui::vec2(32., 32.))
                        };
                        if ui.add(arrow(glyph::CARET_RIGHT)).clicked() {
                            self.featured = (index + 1) % featured.len();
                        }
                        if ui.add(arrow(glyph::CARET_LEFT)).clicked() {
                            self.featured = (index + featured.len() - 1) % featured.len();
                        }
                        ui.add_space(6.);
                        for dot in (0..featured.len()).rev() {
                            let (r, _) =
                                ui.allocate_exact_size(egui::vec2(8., 8.), egui::Sense::hover());
                            ui.painter().circle_filled(
                                r.center(),
                                3.5,
                                if dot == index {
                                    Color32::WHITE
                                } else {
                                    Color32::from_white_alpha(90)
                                },
                            );
                        }
                    },
                );
            }
        }

        if let Some(recent) = self.config.last_session.clone() {
            shell::section(ui, "Jump back in");
            egui::Frame::new()
                .fill(SURFACE)
                .corner_radius(10)
                .inner_margin(egui::Margin::symmetric(16, 12))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width().min(520.));
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(glyph::HARD_DRIVES)
                                .font(shell::icon_font(22., false))
                                .color(MUTED),
                        );
                        ui.vertical(|ui| {
                            ui.spacing_mut().item_spacing.y = 0.;
                            ui.label(RichText::new(&recent.name).font(shell::semibold(14.5)));
                            ui.label(
                                RichText::new("Last server you joined")
                                    .size(12.5)
                                    .color(MUTED),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .add_enabled(
                                    self.join_ready(),
                                    egui::Button::new(
                                        RichText::new("Reconnect").font(shell::semibold(14.)),
                                    )
                                    .corner_radius(8),
                                )
                                .clicked()
                            {
                                self.start(if recent.relay {
                                    Session::Relay {
                                        address: recent.address.clone(),
                                        code: recent.code.clone(),
                                    }
                                } else {
                                    Session::Direct(recent.address.clone())
                                });
                            }
                        });
                    });
                });
        }

        let (worlds, built_in): (Vec<Experience>, Vec<Experience>) = self
            .games()
            .into_iter()
            .partition(|e| matches!(e.kind, Kind::Pack(_)));
        self.shelf(ui, "Worlds on this PC", &worlds, Page::Library);
        self.shelf(ui, "Built in", &built_in, Page::Library);

        ui.add_space(22.);
        egui::Frame::new()
            .fill(SURFACE)
            .corner_radius(12)
            .inner_margin(egui::Margin::symmetric(22, 18))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(glyph::CUBE)
                            .font(shell::icon_font(30., false))
                            .color(TEXT),
                    );
                    ui.add_space(6.);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 2.;
                        ui.label(RichText::new("Make your own").font(shell::semibold(17.)));
                        ui.label(
                            RichText::new("Build a world in SARE Studio and play it here.")
                                .color(MUTED),
                        );
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(shell::icon_button(glyph::PLUS, "Create")).clicked() {
                            self.page = Page::Create;
                        }
                    });
                });
            });
    }

    pub(super) fn experience_page(&mut self, ui: &mut egui::Ui) {
        let Some(experience) = self.selected.clone().and_then(|id| self.find(&id)) else {
            self.page = Page::Library;
            return;
        };
        if ui
            .add(
                egui::Button::new((
                    shell::icon(glyph::ARROW_LEFT, MUTED),
                    RichText::new("Back").color(MUTED),
                ))
                .frame(false),
            )
            .clicked()
        {
            self.page = if experience.template {
                Page::Create
            } else {
                Page::Home
            };
        }
        ui.add_space(6.);
        let width = ui.available_width();
        let height = (width * 0.34).clamp(200., 340.);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::hover());
        shell::cover(ui, rect, &experience, &mut self.covers, self.hero.as_ref());
        ui.add_space(18.);

        let checkpoint = (experience.kind == Kind::SanAndreas)
            .then(|| {
                sa_client::progress::Save::load(&sa_client::config_dir().join("progress.json"))
                    .ok()
                    .flatten()
            })
            .flatten();
        let wide = width >= 760.;
        let side = if wide { 260. } else { width };
        let info = |ui: &mut egui::Ui| {
            ui.label(
                RichText::new(&experience.name)
                    .font(shell::display(32.))
                    .color(TEXT),
            );
            let byline = match experience.kind {
                Kind::SanAndreas => experience.creator.clone(),
                _ => format!("By {}", experience.creator),
            };
            ui.label(RichText::new(byline).color(MUTED));
            ui.add_space(12.);
            ui.label(&experience.description);
            ui.add_space(18.);
            ui.label(
                RichText::new("About")
                    .font(shell::semibold(16.))
                    .color(TEXT),
            );
            ui.add_space(2.);
            let mut rows: Vec<(&str, String)> = vec![(
                "Type",
                match &experience.kind {
                    Kind::SanAndreas => "Needs your own San Andreas installation".into(),
                    Kind::Testbed => "Built in, no game files needed".into(),
                    Kind::Pack(_) => "Standalone world, no game files needed".into(),
                },
            )];
            if let Some(version) = &experience.version {
                rows.push(("Version", version.clone()));
            }
            if let Some(license) = &experience.license {
                rows.push(("Licence", license.clone()));
            }
            if let Some(folder) = experience.pack_folder() {
                rows.push(("Folder", folder.display().to_string()));
            }
            if experience.editable {
                rows.push(("SARE Studio", "Can be edited".into()));
            }
            egui::Grid::new(("about", &experience.id))
                .num_columns(2)
                .spacing(egui::vec2(24., 8.))
                .show(ui, |ui| {
                    for (key, value) in rows {
                        ui.label(RichText::new(key).color(MUTED));
                        ui.label(value);
                        ui.end_row();
                    }
                });
        };
        let actions = |this: &mut Self, ui: &mut egui::Ui| {
            ui.spacing_mut().item_spacing.y = 8.;
            if let (true, Some(folder)) = (experience.template, experience.pack_folder()) {
                if ui
                    .add_enabled(
                        this.testbed_ready(),
                        egui::Button::new((
                            shell::icon(glyph::PLUS, BACKGROUND),
                            RichText::new("Use this template")
                                .font(shell::semibold(15.))
                                .color(BACKGROUND),
                        ))
                        .fill(ACCENT)
                        .corner_radius(8)
                        .min_size(egui::vec2(132., 42.)),
                    )
                    .on_hover_text("Copy it into a new world and open it in SARE Studio")
                    .on_disabled_hover_text("Close the running game first")
                    .clicked()
                {
                    this.create_from(Some(folder.clone()), &experience.name);
                }
                if ui
                    .add_enabled(
                        this.testbed_ready(),
                        shell::icon_button(glyph::PLAY, "Play"),
                    )
                    .clicked()
                {
                    this.play(&experience);
                }
                return;
            }
            let label = if checkpoint.is_some() {
                "Continue"
            } else {
                "Play"
            };
            this.primary_action(ui, &experience, label);
            if let Some(save) = &checkpoint {
                ui.label(
                    RichText::new(format!(
                        "{} · {}",
                        crate::saved_place(save.position),
                        save.car
                    ))
                    .size(12.5)
                    .color(MUTED),
                );
            }
            if let Some(folder) = experience.pack_folder().cloned() {
                if experience.editable
                    && ui
                        .add_enabled(
                            this.testbed_ready(),
                            shell::icon_button(glyph::PENCIL_SIMPLE, "Edit in Studio"),
                        )
                        .clicked()
                {
                    this.edit_world(folder.clone());
                }
                if ui
                    .add(shell::icon_button(glyph::FOLDER_OPEN, "Open folder"))
                    .clicked()
                {
                    crate::open_folder(folder, &mut this.message);
                }
            }
        };
        if wide {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(width - side - 40., 0.),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(width - side - 40.);
                        info(ui)
                    },
                );
                ui.add_space(24.);
                ui.allocate_ui_with_layout(
                    egui::vec2(side, 0.),
                    egui::Layout::top_down_justified(egui::Align::Center),
                    |ui| actions(self, ui),
                );
            });
        } else {
            ui.vertical(|ui| actions(self, ui));
            ui.add_space(12.);
            info(ui);
        }
    }

    pub(super) fn discover(&mut self, ui: &mut egui::Ui) {
        shell::title(ui, "Discover");
        ui.horizontal(|ui| {
            for (index, label) in ["All", "Worlds", "Built in"].into_iter().enumerate() {
                let active = self.discover_filter == index;
                let chip = egui::Button::new(
                    RichText::new(label)
                        .font(shell::semibold(13.5))
                        .color(if active { BACKGROUND } else { TEXT }),
                )
                .fill(if active { TEXT } else { SURFACE })
                .corner_radius(16)
                .min_size(egui::vec2(0., 30.));
                if ui.add(chip).clicked() {
                    self.discover_filter = index;
                }
            }
        });
        ui.add_space(14.);
        let filter = self.discover_filter;
        let found: Vec<Experience> = self
            .games()
            .into_iter()
            .filter(|e| e.matches(&self.catalog_search))
            .filter(|e| match filter {
                1 => matches!(e.kind, Kind::Pack(_)),
                2 => !matches!(e.kind, Kind::Pack(_)),
                _ => true,
            })
            .collect();
        if !self.catalog_search.trim().is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{} result{} for \u{201c}{}\u{201d}",
                        found.len(),
                        if found.len() == 1 { "" } else { "s" },
                        self.catalog_search.trim()
                    ))
                    .color(MUTED),
                );
                if ui.small_button("Clear").clicked() {
                    self.catalog_search.clear();
                }
            });
            ui.add_space(8.);
        }
        if found.is_empty() {
            if shell::empty_state(
                ui,
                glyph::MAGNIFYING_GLASS,
                "Nothing found",
                "Try another name or creator.",
                Some("Clear search"),
            ) {
                self.catalog_search.clear();
                self.discover_filter = 0;
            }
        } else {
            self.grid(ui, &found);
        }
        if filter != 2 {
            self.community_worlds(ui);
        }
    }

    /// Discover's second half: worlds from the community catalog.
    fn community_worlds(&mut self, ui: &mut egui::Ui) {
        if !self.world_list_requested {
            self.browse_worlds();
        }
        ui.add_space(22.);
        ui.horizontal(|ui| {
            ui.label(RichText::new("From the community").font(shell::semibold(18.)));
            if self.world_list_worker.is_some() {
                ui.spinner();
            } else if ui.small_button("Refresh").clicked() {
                self.browse_worlds();
            }
        });
        ui.add_space(6.);
        if let Some(message) = self.world_install_message.clone() {
            ui.label(RichText::new(message).color(ACCENT));
            ui.add_space(6.);
        }
        if let Some(error) = &self.world_list_error {
            ui.label(
                RichText::new(format!("The world list could not be loaded: {error}")).color(MUTED),
            );
            return;
        }
        let content = crate::local_mods::content_root();
        let worlds: Vec<_> = self
            .world_list
            .iter()
            .filter(|w| {
                let query = self.catalog_search.trim().to_lowercase();
                query.is_empty()
                    || [&w.name, &w.author, &w.description]
                        .iter()
                        .any(|t| t.to_lowercase().contains(&query))
            })
            .cloned()
            .collect();
        if worlds.is_empty() && self.world_list_worker.is_none() {
            ui.label(
                RichText::new(if self.world_list.is_empty() {
                    "No shared worlds yet. Creators add theirs to community/worlds.json in the SARE repository."
                } else {
                    "No shared worlds match this search."
                })
                .size(12.5)
                .color(MUTED),
            );
            return;
        }
        let installing = self.world_install.as_ref().map(|(name, _)| name.clone());
        for world in worlds {
            let state = crate::world_catalog::state(&world, &content);
            egui::Frame::new()
                .fill(SURFACE)
                .corner_radius(6)
                .inner_margin(14)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&world.name).font(shell::semibold(16.)));
                            let by = if world.author.is_empty() {
                                String::new()
                            } else {
                                format!("by {} · ", world.author)
                            };
                            ui.label(
                                RichText::new(format!(
                                    "{by}version {} · {} · {:.1} MiB",
                                    world.version,
                                    world.license,
                                    world.bytes as f64 / 1048576.
                                ))
                                .size(12.5)
                                .color(MUTED),
                            );
                            if !world.description.is_empty() {
                                ui.label(&world.description);
                            }
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if installing.as_deref() == Some(world.name.as_str()) {
                                ui.spinner();
                                ui.label("Installing...");
                                return;
                            }
                            let (label, enabled) = match state {
                                crate::world_catalog::State::NotInstalled => ("Install", true),
                                crate::world_catalog::State::Update => ("Update", true),
                                crate::world_catalog::State::Installed => ("Installed", false),
                                crate::world_catalog::State::Conflict => {
                                    ("You have a world with this id", false)
                                }
                            };
                            if ui
                                .add_enabled(
                                    enabled && installing.is_none(),
                                    egui::Button::new(label),
                                )
                                .clicked()
                            {
                                self.install_world(world.clone());
                            }
                        });
                    });
                });
            ui.add_space(6.);
        }
    }

    pub(super) fn library(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            shell::title(ui, "Library");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(shell::icon_button(glyph::FOLDER_OPEN, "Content folder"))
                    .on_hover_text("Add or remove worlds by adding or deleting their folders here")
                    .clicked()
                {
                    crate::open_folder(local_mods::content_root(), &mut self.message);
                }
                if ui
                    .add(shell::icon_button(glyph::ARROWS_CLOCKWISE, "Refresh"))
                    .on_hover_text("Look for new folders in content/")
                    .clicked()
                {
                    self.scan_packs();
                }
            });
        });
        let (worlds, built_in): (Vec<Experience>, Vec<Experience>) = self
            .games()
            .into_iter()
            .partition(|e| matches!(e.kind, Kind::Pack(_)));
        shell::section(ui, "Worlds");
        if worlds.is_empty() {
            if shell::empty_state(
                ui,
                glyph::BUILDINGS,
                "No worlds yet",
                "Copy a content pack into the content folder, or make one in Create.",
                Some("Create a world"),
            ) {
                self.page = Page::Create;
            }
        } else {
            self.grid(ui, &worlds);
        }
        shell::section(ui, "Built in");
        self.grid(ui, &built_in);
    }

    pub(super) fn create(&mut self, ui: &mut egui::Ui) {
        shell::title(ui, "Create");
        ui.horizontal(|ui| {
            ui.label(RichText::new("Name").color(MUTED));
            ui.add(
                egui::TextEdit::singleline(&mut self.new_world_name)
                    .hint_text("My World")
                    .desired_width(280.)
                    .margin(egui::vec2(12., 9.))
                    .char_limit(64),
            )
            .on_hover_text("Name for the next world you start");
        });
        shell::section(ui, "Start a new world");
        let templates: Vec<Experience> = self
            .experiences
            .iter()
            .filter(|e| e.template)
            .cloned()
            .collect();
        let refs: Vec<&Experience> = templates.iter().collect();
        match shell::template_grid(ui, &refs, &mut self.covers) {
            Some(shell::TemplatePick::Blank) if self.testbed_ready() => {
                self.create_from(None, "My World")
            }
            Some(shell::TemplatePick::Blank) => {
                self.message = "Close the running game first.".into()
            }
            Some(shell::TemplatePick::Open(index)) => {
                let id = templates[index].id.clone();
                self.open_experience(&id);
            }
            None => {}
        }
        let projects: Vec<Experience> = self
            .experiences
            .iter()
            .filter(|e| e.editable && !e.template)
            .cloned()
            .collect();
        shell::section(ui, "Your worlds");
        if projects.is_empty() {
            ui.label(RichText::new("Worlds you make show up here.").color(MUTED));
        } else {
            self.grid(ui, &projects);
        }
    }

    /// Make a new world, blank or copied from a template folder, and open
    /// it in Build Mode. `fallback` names it when the name field is empty.
    pub(super) fn create_from(&mut self, template: Option<std::path::PathBuf>, fallback: &str) {
        let name = match self.new_world_name.trim() {
            "" => fallback.to_owned(),
            name => name.to_owned(),
        };
        let root = local_mods::content_root();
        let id = sa_editor::project::new_id(&name, &root);
        let folder = root.join(&id);
        let made = match &template {
            None => sa_editor::project::Project::create(&folder, &id, &name).map(|_| ()),
            Some(source) => copy_template(source, &folder, &id, &name),
        };
        match made {
            Ok(()) => {
                self.new_world_name.clear();
                self.scan_packs();
                self.edit_world(folder);
            }
            Err(error) => self.message = format!("Could not create the world: {error:#}"),
        }
    }

    pub(super) fn profile(&mut self, ui: &mut egui::Ui) {
        let name = self.player_name().to_owned();
        ui.add_space(8.);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(88., 88.), egui::Sense::hover());
            shell::avatar(ui.painter(), rect.center(), 44., &name);
            ui.add_space(10.);
            ui.vertical(|ui| {
                ui.add_space(16.);
                ui.label(RichText::new(&name).font(shell::display(30.)).color(TEXT));
                if let Ok(code) = &self.friends.code {
                    ui.label(RichText::new(format!("Friend code {code}")).color(MUTED));
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Local player").color(MUTED));
                    ui.label(RichText::new("·").color(MUTED));
                    if ui.link("Change name").clicked() {
                        self.page = Page::Settings;
                    }
                });
            });
        });
        let creations: Vec<Experience> = self
            .experiences
            .iter()
            .filter(|e| e.editable && e.creator == experience::LOCAL_CREATOR)
            .cloned()
            .collect();
        shell::section(ui, "Creations");
        if creations.is_empty() {
            if shell::empty_state(
                ui,
                glyph::CUBE,
                "No creations yet",
                "Worlds you build in Create show up here.",
                Some("Go to Create"),
            ) {
                self.page = Page::Create;
            }
        } else {
            self.grid(ui, &creations);
        }
    }

    pub(super) fn friends(&mut self, ui: &mut egui::Ui) {
        shell::title(ui, "Friends");
        let url = crate::friends::directory_url(&self.config.directory);
        // Your code.
        egui::Frame::new()
            .fill(SURFACE)
            .corner_radius(6)
            .inner_margin(16)
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.label(RichText::new("Your friend code").color(MUTED).size(12.5));
                match self.friends.code.clone() {
                    Ok(code) => {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&code).font(shell::display(26.)).color(TEXT));
                            if ui.button("Copy").clicked() {
                                ui.ctx().copy_text(code.clone());
                                self.friends_message = Some("Friend code copied.".into());
                            }
                        });
                        ui.label(
                            RichText::new("Give it to friends and add theirs. You see each other once you have both added the other. Servers never see this code.")
                                .size(12.5)
                                .color(MUTED),
                        );
                    }
                    Err(error) => {
                        ui.label(format!("No SARE ID: {error}"));
                    }
                }
            });
        ui.add_space(10.);
        // Add a friend.
        ui.horizontal_wrapped(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.friend_code_input)
                    .hint_text("Friend code (XXXX-XXXX-XXXX)")
                    .margin(egui::vec2(10., 8.))
                    .desired_width(220.),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.friend_name_input)
                    .hint_text("Name (optional)")
                    .margin(egui::vec2(10., 8.))
                    .desired_width(180.),
            );
            if ui.button("Add friend").clicked() {
                self.add_friend();
            }
        });
        if let Some(message) = &self.friends_message {
            ui.label(RichText::new(message).color(ACCENT));
        }
        ui.add_space(6.);
        if url.is_empty() {
            ui.label(
                RichText::new("Online status needs a SARE directory, and none is set up yet. You can already swap codes; set a directory under Settings when one runs.")
                    .size(12.5)
                    .color(MUTED),
            );
        } else if let Some(error) = &self.friends.error {
            ui.label(RichText::new(error).size(12.5).color(MUTED));
        }
        if self.config.friends.is_empty() {
            if shell::empty_state(
                ui,
                glyph::USERS,
                "No friends added yet",
                "Add a friend's code above. Until then you can still play together: host a server or share a join code.",
                Some("Open Servers"),
            ) {
                self.page = Page::Multiplayer;
            }
            return;
        }
        shell::section(ui, "Your friends");
        let mut remove = None;
        let mut join = None;
        for (index, friend) in self.config.friends.clone().iter().enumerate() {
            let online = self
                .friends
                .online
                .iter()
                .find(|f| f.code == friend.code)
                .cloned();
            egui::Frame::new()
                .fill(SURFACE)
                .corner_radius(6)
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        let shown = if friend.name.is_empty() {
                            online
                                .as_ref()
                                .map(|o| o.name.clone())
                                .unwrap_or_else(|| friend.code.clone())
                        } else {
                            friend.name.clone()
                        };
                        let (rect, _) =
                            ui.allocate_exact_size(egui::vec2(36., 36.), egui::Sense::hover());
                        shell::avatar(ui.painter(), rect.center(), 18., &shown);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&shown).font(shell::semibold(15.)));
                            let status = match &online {
                                Some(o) => match &o.playing {
                                    Some(p) => format!("Online · playing {}", p.world),
                                    None => "Online".into(),
                                },
                                None if url.is_empty() => friend.code.clone(),
                                None => {
                                    format!("Offline, or has not added you yet · {}", friend.code)
                                }
                            };
                            let color = if online.is_some() {
                                Color32::from_rgb(120, 205, 130)
                            } else {
                                MUTED
                            };
                            ui.label(RichText::new(status).size(12.5).color(color));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("Remove").clicked() {
                                remove = Some(index);
                            }
                            if let Some(server) = online
                                .as_ref()
                                .and_then(|o| o.playing.as_ref()?.server.clone())
                            {
                                if ui
                                    .add_enabled(self.join_ready(), egui::Button::new("Join"))
                                    .clicked()
                                {
                                    join = Some(server);
                                }
                            }
                        });
                    });
                });
            ui.add_space(4.);
        }
        if let Some(index) = remove {
            self.config.friends.remove(index);
            self.save_friends();
        }
        if let Some(server) = join {
            self.start(Session::Direct(server));
        }
    }

    fn add_friend(&mut self) {
        let Some(code) = sa_net::directory::normalize_code(&self.friend_code_input) else {
            self.friends_message =
                Some("That is not a friend code. It looks like XXXX-XXXX-XXXX.".into());
            return;
        };
        if self.friends.code.as_deref() == Ok(code.as_str()) {
            self.friends_message = Some("That is your own code.".into());
            return;
        }
        if self.config.friends.iter().any(|f| f.code == code) {
            self.friends_message = Some("You already added this friend.".into());
            return;
        }
        if self.config.friends.len() >= sa_net::directory::MAX_FRIENDS {
            self.friends_message = Some("Your friends list is full.".into());
            return;
        }
        let name: String = self
            .friend_name_input
            .trim()
            .chars()
            .filter(|c| !c.is_control())
            .take(24)
            .collect();
        self.config
            .friends
            .push(sa_client::FriendEntry { code, name });
        self.friend_code_input.clear();
        self.friend_name_input.clear();
        self.friends_message = Some("Friend added.".into());
        self.save_friends();
        self.heartbeat(true);
    }

    fn save_friends(&mut self) {
        if let Err(error) = self.config.save() {
            let error = format!("Could not save friends: {error}");
            crate::log_error(&error);
            self.friends_message = Some(error);
        }
    }

    pub(super) fn messages(&mut self, ui: &mut egui::Ui) {
        shell::title(ui, "Messages");
        if shell::empty_state(
            ui,
            glyph::CHAT_CIRCLE,
            "No messages",
            "Private messages are not built yet. Chat works inside servers, and Friends shows who is online.",
            Some("Open Servers"),
        ) {
            self.page = Page::Multiplayer;
        }
    }

    pub(super) fn player_name(&self) -> &str {
        match self.config.player.trim() {
            "" => "Player",
            _ => self.config.player.as_str(),
        }
    }
}

/// Copy a template pack into `folder` as a new world of the player's own:
/// new id and name, no author (so it counts as made on this PC).
fn copy_template(
    source: &std::path::Path,
    folder: &std::path::Path,
    id: &str,
    name: &str,
) -> anyhow::Result<()> {
    let (project, mut document) = sa_editor::project::Project::open(source)?;
    document.info.name = name.to_owned();
    project.save_as(&document, folder, id)?;
    let manifest_path = folder.join(sa_scene::content_pack::MANIFEST);
    let mut manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path)?)?;
    if let Some(fields) = manifest.as_object_mut() {
        fields.remove("author");
        fields.insert("version".into(), "0.1.0".into());
    }
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest)? + "\n",
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::copy_template;
    use std::path::Path;

    #[test]
    fn a_template_becomes_a_new_world_and_the_template_is_untouched() {
        let source = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../content/example-town");
        let before = std::fs::read(source.join("content.json")).unwrap();
        let root = std::env::temp_dir().join(format!("sare-template-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let folder = root.join("my-town");
        copy_template(&source, &folder, "my-town", "My Town").unwrap();
        let manifest = sa_scene::content_pack::Manifest::read(&folder).unwrap();
        assert_eq!(manifest.id, "my-town");
        assert_eq!(manifest.name, "My Town");
        assert!(manifest.author.is_none());
        assert!(
            manifest.world.layout.is_some(),
            "the copy opens in SARE Studio"
        );
        assert!(folder.join("world.glb").is_file());
        assert_eq!(std::fs::read(source.join("content.json")).unwrap(), before);
        assert!(!source.join("layout.json").exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
