//! The command bar under Output: a line of Lua run against the world being
//! edited (see `sa_editor::command_bar`), one undo step per command.
use super::BuildMode;
use crate::ui_theme::MUTED;
use egui::{RichText, Ui};

/// Commands typed earlier (Up and Down go through them).
#[derive(Default)]
pub struct CommandLine {
    text: String,
    history: Vec<String>,
    at: Option<usize>,
}

/// Most commands kept for Up and Down.
const HISTORY: usize = 50;

impl BuildMode {
    pub fn run_command(&mut self, line: &str) {
        let line = line.trim();
        if line.is_empty() {
            return;
        }
        self.commit_preview();
        self.info(format!("> {line}"));
        match sa_editor::command_bar::run(&mut self.doc, line, &self.selection) {
            Ok(outcome) => {
                for (text, error) in outcome.output {
                    if error {
                        self.error(text);
                    } else {
                        self.info(text);
                    }
                }
                if let Some(command) = outcome.command {
                    self.execute(command);
                }
            }
            Err(error) => self.error(format!("{error:#}")),
        }
    }
}

pub fn ui(build: &mut BuildMode, ui: &mut Ui) {
    let mut run = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new(">").monospace().color(MUTED));
        let line = &mut build.ui.command;
        let response = ui.add(
            egui::TextEdit::singleline(&mut line.text)
                .font(egui::TextStyle::Monospace)
                .hint_text("Lua: for _, lamp in ipairs(world.tagged('Lamp')) do lamp.visible = false end")
                .desired_width(ui.available_width() - 50.0),
        );
        if response.has_focus() {
            let (up, down) = ui.input(|i| {
                (i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowDown))
            });
            if up && !line.history.is_empty() {
                let at = line.at.map_or(line.history.len() - 1, |a| a.saturating_sub(1));
                line.at = Some(at);
                line.text = line.history[at].clone();
            }
            if down {
                match line.at {
                    Some(a) if a + 1 < line.history.len() => {
                        line.at = Some(a + 1);
                        line.text = line.history[a + 1].clone();
                    }
                    _ => {
                        line.at = None;
                        line.text.clear();
                    }
                }
            }
        }
        let entered = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (ui.small_button("Run").clicked() || entered) && !line.text.trim().is_empty() {
            let text = std::mem::take(&mut line.text);
            line.history.retain(|h| *h != text);
            line.history.push(text.clone());
            if line.history.len() > HISTORY {
                line.history.remove(0);
            }
            line.at = None;
            run = Some(text);
            if entered {
                response.request_focus();
            }
        }
        response.on_hover_text(
            "Runs a line of Lua on the world you are editing, like a world script: world.find, world.tagged, object.position, object:set, object:clone, print. selection() gives the selected objects. Ctrl+Z undoes it.",
        );
    });
    if let Some(text) = run {
        build.run_command(&text);
    }
}
