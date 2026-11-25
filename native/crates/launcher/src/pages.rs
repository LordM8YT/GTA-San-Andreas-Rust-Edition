//! SARE Client pages built on the experience list: Discover, Library,
//! Create, Profile and the social pages. Launching reuses the launcher's
//! existing start paths.
use crate::{
    experience::{self, Experience, Kind},
    local_mods,
    shell::{
        self,
        palette::{MUTED, SURFACE, TEXT},
        CardAction, CardState,
    },
    Launcher, Page,
};
use eframe::egui::{self, RichText};
use sa_client::launch::Session;

impl Launcher {
    fn needs_setup(&self, experience: &Experience) -> bool {
        experience.kind == Kind::SanAndreas && !self.ready() && self.child.is_none()
    }
    fn can_play(&self, experience: &Experience) -> bool {
        match experience.kind {
            Kind::SanAndreas => self.ready(),
            _ => self.testbed_ready(),
        }
    }
    /// Show a grid and carry out what the player clicked.
    fn grid(&mut self, ui: &mut egui::Ui, list: &[Experience]) {
        let cards: Vec<(&Experience, CardState)> = list
            .iter()
            .map(|e| {
                (
                    e,
                    CardState {
                        can_play: self.can_play(e),
                        needs_setup: self.needs_setup(e),
                    },
                )
            })
            .collect();
        let chosen = shell::card_grid(ui, &cards, &mut self.covers, self.hero.as_ref());
        if let Some((index, action)) = chosen {
            let experience = list[index].clone();
            self.act(&experience, action);
        }
    }
    pub(super) fn act(&mut self, experience: &Experience, action: CardAction) {
        match (action, &experience.kind) {
            (CardAction::Setup, _) => self.page = Page::Settings,
            (CardAction::Play, Kind::SanAndreas) => self.start(Session::Offline),
            (CardAction::Play, Kind::Testbed) => self.start_testbed(),
            (CardAction::Play, Kind::Pack(folder)) => self.start_pack(folder.clone()),
            (CardAction::Edit, Kind::Pack(folder)) => self.edit_world(folder.clone()),
            (CardAction::Edit, _) => {}
        }
    }

    pub(super) fn discover(&mut self, ui: &mut egui::Ui) {
        shell::title(
            ui,
            "Discover",
            "Worlds and games you can play. Online sharing of community experiences is not built yet, so this lists what is on this PC.",
        );
        let found: Vec<Experience> = self
            .experiences
            .iter()
            .filter(|e| e.matches(&self.catalog_search))
            .cloned()
            .collect();
        if !self.catalog_search.trim().is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!(
                        "{} result{} for \"{}\"",
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
                Page::Discover,
                "Nothing matches your search",
                "Try another name or creator. Worlds you add to the content folder show up here.",
                Some("Clear search"),
            ) {
                self.catalog_search.clear();
            }
            return;
        }
        self.grid(ui, &found);
    }

    pub(super) fn library(&mut self, ui: &mut egui::Ui) {
        shell::title(ui, "Library", "Everything installed on this PC.");
        let (worlds, built_in): (Vec<Experience>, Vec<Experience>) = self
            .experiences
            .iter()
            .cloned()
            .partition(|e| matches!(e.kind, Kind::Pack(_)));
        ui.add_space(8.);
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Worlds")
                    .font(shell::semibold(18.))
                    .color(TEXT),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .button("Open content folder")
                    .on_hover_text("Add or remove worlds by adding or deleting their folders here")
                    .clicked()
                {
                    crate::open_folder(local_mods::content_root(), &mut self.message);
                }
                if ui
                    .button("Refresh")
                    .on_hover_text("Look for new folders in content/")
                    .clicked()
                {
                    self.scan_packs();
                }
            });
        });
        if worlds.is_empty() {
            if shell::empty_state(
                ui,
                Page::Library,
                "No worlds installed",
                "Content packs you copy into the content folder, and worlds you make in Create, appear here.",
                Some("Create a world"),
            ) {
                self.page = Page::Create;
            }
        } else {
            self.grid(ui, &worlds);
        }
        shell::section(ui, "Built in and your game");
        self.grid(ui, &built_in);
        ui.label(
            RichText::new(
                "Favorites and recently played are planned. Saved servers live under Servers.",
            )
            .size(12.5)
            .color(MUTED),
        );
    }

    pub(super) fn create(&mut self, ui: &mut egui::Ui) {
        shell::title(
            ui,
            "Create",
            "Make your own world in SARE Studio (Build Mode). No San Andreas installation needed.",
        );
        egui::Frame::new()
            .fill(SURFACE)
            .corner_radius(10)
            .inner_margin(egui::Margin::symmetric(22, 20))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("New world").size(19.).strong());
                ui.label(
                    RichText::new(
                        "Starts with flat ground. Build Mode saves it in the content folder.",
                    )
                    .color(MUTED),
                );
                ui.add_space(6.);
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.new_world_name)
                            .hint_text("World name")
                            .desired_width(260.)
                            .margin(egui::vec2(12., 9.))
                            .char_limit(64),
                    );
                    let create = ui
                        .add_enabled(self.testbed_ready(), crate::primary("Create and open"))
                        .on_disabled_hover_text("Close the running game first");
                    if create.clicked() {
                        self.create_world();
                    }
                });
            });
        let projects: Vec<Experience> = self
            .experiences
            .iter()
            .filter(|e| e.editable)
            .cloned()
            .collect();
        shell::section(ui, "Worlds you can edit");
        if projects.is_empty() {
            shell::empty_state(
                ui,
                Page::Create,
                "No Build Mode worlds yet",
                "Create one above. Worlds made in Build Mode show up here for editing.",
                None,
            );
        } else {
            self.grid(ui, &projects);
        }
        ui.add_space(6.);
        ui.label(
            RichText::new("Templates and Lua resources: see docs/build-mode.md and docs/creating-resources.md in the SARE repository.")
                .size(12.5)
                .color(MUTED),
        );
    }

    /// Make a new Build Mode world and open it (shared by Home and Create).
    pub(super) fn create_world(&mut self) {
        let name = match self.new_world_name.trim() {
            "" => "My World".to_owned(),
            name => name.to_owned(),
        };
        let root = local_mods::content_root();
        let id = sa_editor::project::new_id(&name, &root);
        match sa_editor::project::Project::create(&root.join(&id), &id, &name) {
            Ok((project, _)) => {
                self.new_world_name.clear();
                self.scan_packs();
                self.edit_world(project.folder);
            }
            Err(error) => self.message = format!("Could not create the world: {error:#}"),
        }
    }

    pub(super) fn profile(&mut self, ui: &mut egui::Ui) {
        let name = self.player_name().to_owned();
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(84., 84.), egui::Sense::hover());
            shell::avatar(ui.painter(), rect.center(), 42., &name);
            ui.add_space(8.);
            ui.vertical(|ui| {
                ui.add_space(14.);
                ui.label(RichText::new(&name).size(28.).strong().color(TEXT));
                ui.label(
                    RichText::new(
                        "Local player name, used on servers. There are no SARE accounts yet.",
                    )
                    .color(MUTED),
                );
                if ui.link("Change name in Settings").clicked() {
                    self.page = Page::Settings;
                }
            });
        });
        ui.add_space(12.);
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
                Page::Profile,
                "No creations yet",
                "Worlds you build in Create show up on your profile.",
                Some("Go to Create"),
            ) {
                self.page = Page::Create;
            }
        } else {
            self.grid(ui, &creations);
        }
    }

    pub(super) fn friends(&mut self, ui: &mut egui::Ui) {
        shell::title(ui, "Friends", "");
        if shell::empty_state(
            ui,
            Page::Friends,
            "Friends need SARE accounts",
            "SARE has no accounts yet, so there is no friends list or online status. You can still play together today: host a server or share a join code under Servers.",
            Some("Open Servers"),
        ) {
            self.page = Page::Multiplayer;
        }
    }

    pub(super) fn messages(&mut self, ui: &mut egui::Ui) {
        shell::title(ui, "Messages", "");
        if shell::empty_state(
            ui,
            Page::Messages,
            "No messages yet",
            "Private messages need SARE accounts, which do not exist yet. Chat works inside servers while you play.",
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
