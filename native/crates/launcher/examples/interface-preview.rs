//! Render the actual in-game menus without a GTA installation.
//! Usage: interface-preview <main|pause|hud|map|settings> <width> <height> <output.png>
//! Use an isolated SARE_CONFIG_DIR; all gameplay data here is a preview fixture.
use eframe::egui;
use std::path::PathBuf;
#[path = "../../../ui/theme.rs"]
mod ui_theme;
mod settings {
    pub use sa_client::settings::*;
}
#[allow(dead_code)]
#[path = "../../runtime/src/streaming/locations.rs"]
mod streaming;
// The preview exercises drawing, not every gameplay action exposed by Menu.
#[allow(dead_code)]
#[path = "../../runtime/src/menu.rs"]
mod menu;

struct Preview {
    menu: menu::Menu,
    output: PathBuf,
    frames: usize,
    requested: bool,
}
impl eframe::App for Preview {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let screenshot = ctx.input(|input| {
            input.events.iter().find_map(|event| {
                if let egui::Event::Screenshot { image, .. } = event {
                    Some(image.clone())
                } else {
                    None
                }
            })
        });
        if let Some(image) = screenshot {
            let result = (|| -> anyhow::Result<()> {
                let file = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&self.output)?;
                let mut encoder =
                    png::Encoder::new(file, image.size[0] as u32, image.size[1] as u32);
                encoder.set_color(png::ColorType::Rgba);
                encoder.set_depth(png::BitDepth::Eight);
                encoder.write_header()?.write_image_data(
                    &image
                        .pixels
                        .iter()
                        .flat_map(|p| p.to_array())
                        .collect::<Vec<_>>(),
                )?;
                Ok(())
            })();
            if let Err(error) = result {
                eprintln!("Preview failed: {error:#}");
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        ui_theme::horizon(ui.painter(), ctx.content_rect());
        let _ = self.menu.draw(
            &ctx,
            [2500.0, -1670.0, 13.0],
            false,
            Some(19.4),
            0.0,
            0.0,
            &[],
        );
        ui.painter().text(
            ctx.content_rect().center_bottom() - egui::vec2(0.0, 12.0),
            egui::Align2::CENTER_BOTTOM,
            "Interface preview / sample data",
            egui::FontId::proportional(11.0),
            ui_theme::MUTED,
        );
        self.frames += 1;
        if self.frames >= 10 && !self.requested {
            self.requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(Default::default()));
        }
        ctx.request_repaint();
    }
}
fn main() -> anyhow::Result<()> {
    let args: Vec<_> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 5,
        "Usage: interface-preview <main|pause|hud|map|settings> <width> <height> <output.png>"
    );
    anyhow::ensure!(
        std::env::var_os("SARE_CONFIG_DIR").is_some(),
        "Set an isolated SARE_CONFIG_DIR"
    );
    let scenario = args[1].clone();
    anyhow::ensure!(
        ["main", "pause", "hud", "map", "settings"].contains(&scenario.as_str()),
        "Unknown preview page"
    );
    let width: f32 = args[2].parse()?;
    let height: f32 = args[3].parse()?;
    anyhow::ensure!(
        (640.0..=4000.0).contains(&width) && (480.0..=2200.0).contains(&height),
        "Unsupported preview size"
    );
    let output = PathBuf::from(&args[4]);
    anyhow::ensure!(!output.exists(), "Preview output already exists");
    eframe::run_native(
        "SARE interface preview",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default().with_inner_size([width, height]),
            ..Default::default()
        },
        Box::new(move |cc| {
            let mut menu = menu::Menu::new(&cc.egui_ctx, vec!["Example resource".into()]);
            menu.settings = settings::Settings::default();
            menu.settings.show_hud = true;
            menu.settings.show_minimap = true;
            menu.settings.show_speedometer = true;
            menu.zone = Some("Grove Street".into());
            menu.clock = (19, 24);
            menu.has_played = scenario != "main";
            menu.page = match scenario.as_str() {
                "pause" => Some(menu::Page::Pause),
                "hud" => None,
                "map" => Some(menu::Page::Map),
                "settings" => Some(menu::Page::Settings),
                _ => Some(menu::Page::Main),
            };
            if scenario == "hud" {
                menu.network_active = true;
                menu.network_players = vec!["Player".into(), "Preview guest".into()];
                menu.network_status = "Preview session".into();
                menu.weapon_hud = Some(("Micro SMG".into(), 32, 180, false, false));
            }
            Ok(Box::new(Preview {
                menu,
                output,
                frames: 0,
                requested: false,
            }))
        }),
    )
    .map_err(|error| anyhow::anyhow!("{error}"))?;
    Ok(())
}
