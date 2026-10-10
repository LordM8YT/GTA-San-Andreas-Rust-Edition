//! FiveM-style chat (T) and F8 console. Server scripts drive the chat with
//! the standard chat resource events; typed text goes back as
//! `_chat:messageEntered` and `/commands` as `__cfx_internal:commandFallback`.
use egui::{Color32, RichText};
use serde_json::Value as Json;
use std::{collections::VecDeque, time::Instant};

const MAX_MESSAGES: usize = 100;
const MAX_LOG: usize = 400;
const FADE_AFTER: f32 = 12.0;

struct Message {
    color: Color32,
    author: String,
    text: String,
    at: Instant,
}
/// Something the runtime must do for text typed by the player.
#[derive(Debug, PartialEq)]
pub enum Submitted {
    Say(String),
    /// Without the leading slash.
    Command(String),
}
#[derive(Default)]
pub struct Chat {
    pub open: bool,
    pub console_open: bool,
    input: String,
    console_input: String,
    focus: bool,
    messages: VecDeque<Message>,
    log: VecDeque<String>,
    suggestions: Vec<(String, String)>,
    history: Vec<String>,
    history_index: Option<usize>,
    pub submitted: Vec<Submitted>,
}

fn color(value: &Json) -> Color32 {
    let channel = |i: usize| value.get(i).and_then(Json::as_u64).unwrap_or(255).min(255) as u8;
    if value.is_array() {
        Color32::from_rgb(channel(0), channel(1), channel(2))
    } else {
        Color32::WHITE
    }
}
fn text(value: &Json) -> String {
    let text = match value {
        Json::String(s) => s.clone(),
        Json::Null => String::new(),
        other => other.to_string(),
    };
    // FiveM colour codes (^0-^9) are not rendered; strip them.
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '^'
            && chars
                .peek()
                .is_some_and(|n| n.is_ascii_digit() || *n == '*' || *n == '_' || *n == 'r')
        {
            chars.next();
        } else if !c.is_control() || c == '\n' {
            out.push(c);
        }
    }
    out.chars().take(512).collect()
}

impl Chat {
    pub fn active(&self) -> bool {
        self.open || self.console_open
    }
    pub fn open_chat(&mut self) {
        self.open = true;
        self.focus = true;
        self.input.clear();
        self.history_index = None;
    }
    pub fn toggle_console(&mut self) {
        self.console_open = !self.console_open;
        self.focus = self.console_open;
    }
    pub fn close(&mut self) {
        self.open = false;
        self.console_open = false;
    }
    pub fn log(&mut self, line: impl Into<String>) {
        if self.log.len() >= MAX_LOG {
            self.log.pop_front();
        }
        self.log.push_back(line.into());
    }
    pub fn add_message(&mut self, author: &str, body: &str, color: Color32) {
        if self.messages.len() >= MAX_MESSAGES {
            self.messages.pop_front();
        }
        self.log(if author.is_empty() {
            body.to_string()
        } else {
            format!("{author}: {body}")
        });
        self.messages.push_back(Message {
            color,
            author: author.into(),
            text: body.into(),
            at: Instant::now(),
        });
    }
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Handle a server event for the chat resource; false if unrelated.
    pub fn handle_event(&mut self, name: &str, args: &[Json]) -> bool {
        let first = args.first().unwrap_or(&Json::Null);
        match name {
            "chat:addMessage" => {
                let (colour, parts) = if first.is_object() {
                    let parts: Vec<String> = first["args"]
                        .as_array()
                        .map(|a| a.iter().map(text).collect())
                        .unwrap_or_default();
                    (color(&first["color"]), parts)
                } else {
                    (Color32::WHITE, vec![text(first)])
                };
                match parts.as_slice() {
                    [] => {}
                    [only] => self.add_message("", only, colour),
                    [author, rest @ ..] => self.add_message(author, &rest.join(" "), colour),
                }
            }
            // Legacy: TriggerClientEvent('chatMessage', id, author, {r,g,b}, text)
            "chatMessage" => {
                let body = args.get(2).map(text).unwrap_or_default();
                self.add_message(
                    &text(first),
                    &body,
                    color(args.get(1).unwrap_or(&Json::Null)),
                );
            }
            "chat:clear" => self.clear(),
            "chat:addSuggestion" => {
                let help = args.get(1).map(text).unwrap_or_default();
                self.add_suggestion(text(first), help);
            }
            "chat:addSuggestions" => {
                for entry in first.as_array().into_iter().flatten() {
                    self.add_suggestion(text(&entry["name"]), text(&entry["help"]));
                }
            }
            "chat:removeSuggestion" => {
                let name = text(first);
                self.suggestions.retain(|(n, _)| *n != name);
            }
            _ => return false,
        }
        true
    }
    fn add_suggestion(&mut self, name: String, help: String) {
        let name = if name.starts_with('/') {
            name
        } else {
            format!("/{name}")
        };
        if self.suggestions.len() < 256 {
            self.suggestions.retain(|(n, _)| *n != name);
            self.suggestions.push((name, help));
            self.suggestions.sort();
        }
    }
    pub fn reset_server_state(&mut self) {
        self.suggestions.clear();
    }

    fn submit(&mut self, line: String) {
        let line = line.trim().to_string();
        if line.is_empty() {
            return;
        }
        if self.history.last() != Some(&line) {
            self.history.push(line.clone());
            if self.history.len() > 50 {
                self.history.remove(0);
            }
        }
        self.submitted.push(match line.strip_prefix('/') {
            Some(command) => Submitted::Command(command.to_string()),
            None => Submitted::Say(line),
        });
    }
    fn recall(&mut self, ctx: &egui::Context, console: bool) {
        let (up, down) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowUp),
                i.key_pressed(egui::Key::ArrowDown),
            )
        });
        if self.history.is_empty() || !(up || down) {
            return;
        }
        let last = self.history.len() - 1;
        self.history_index = Some(match (self.history_index, up) {
            (None, true) => last,
            (Some(i), true) => i.saturating_sub(1),
            (Some(i), false) => (i + 1).min(last),
            (None, false) => return,
        });
        let entry = self.history[self.history_index.unwrap()].clone();
        if console {
            self.console_input = entry;
        } else {
            self.input = entry;
        }
    }

    pub fn draw(&mut self, ctx: &egui::Context, scale: f32) {
        let now = Instant::now();
        let screen = ctx.content_rect();
        let font = egui::FontId::proportional(15.0 * scale);
        egui::Area::new("chat".into())
            .anchor(
                egui::Align2::LEFT_TOP,
                [18.0 * scale, screen.height() * 0.28],
            )
            .interactable(self.open)
            .show(ctx, |ui| {
                ui.set_max_width(520.0 * scale);
                let visible: Vec<&Message> = self
                    .messages
                    .iter()
                    .rev()
                    .take(10)
                    .filter(|m| {
                        self.open || now.duration_since(m.at).as_secs_f32() < FADE_AFTER + 1.0
                    })
                    .collect();
                for message in visible.into_iter().rev() {
                    let age = now.duration_since(message.at).as_secs_f32();
                    let alpha = if self.open {
                        1.0
                    } else {
                        (FADE_AFTER + 1.0 - age).clamp(0.0, 1.0)
                    };
                    let fade = |c: Color32| c.gamma_multiply(alpha);
                    egui::Frame::new()
                        .fill(Color32::from_black_alpha((120.0 * alpha) as u8))
                        .inner_margin(egui::Margin::symmetric(8, 3))
                        .corner_radius(4)
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = 4.0;
                                if !message.author.is_empty() {
                                    ui.label(
                                        RichText::new(format!("{}:", message.author))
                                            .font(font.clone())
                                            .strong()
                                            .color(fade(message.color)),
                                    );
                                }
                                ui.label(
                                    RichText::new(&message.text)
                                        .font(font.clone())
                                        .color(fade(Color32::WHITE)),
                                );
                            });
                        });
                    ui.add_space(2.0);
                }
                if self.open {
                    ui.add_space(4.0);
                    let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.input)
                            .font(font.clone())
                            .desired_width(500.0 * scale)
                            .char_limit(256)
                            .hint_text("Say something, or /command"),
                    );
                    if self.focus || !response.has_focus() {
                        response.request_focus();
                        self.focus = false;
                    }
                    self.recall(ctx, false);
                    if self.input.starts_with('/') {
                        let typed = self
                            .input
                            .split_whitespace()
                            .next()
                            .unwrap_or("")
                            .to_ascii_lowercase();
                        for (name, help) in self
                            .suggestions
                            .iter()
                            .filter(|(n, _)| n.starts_with(&typed))
                            .take(6)
                        {
                            ui.label(
                                RichText::new(format!("{name}  {help}"))
                                    .font(font.clone())
                                    .color(Color32::from_gray(200)),
                            );
                        }
                    }
                    if enter {
                        let line = std::mem::take(&mut self.input);
                        self.submit(line);
                        self.open = false;
                    }
                }
            });
        if self.console_open {
            let height = screen.height() * 0.42;
            egui::Area::new("f8-console".into())
                .anchor(egui::Align2::LEFT_TOP, [0.0, 0.0])
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                  egui::Frame::new().fill(Color32::from_black_alpha(225)).inner_margin(10).show(ui, |ui| {
                    ui.set_width(screen.width() - 20.0);
                    ui.set_height(height);
                    ui.label(RichText::new("Console (F8)  —  connect <ip:port>, disconnect, quit, clear, /server commands").color(Color32::from_gray(170)));
                    let height = ui.available_height() - 34.0;
                    egui::ScrollArea::vertical()
                        .max_height(height)
                        .stick_to_bottom(true)
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            for line in &self.log {
                                ui.monospace(line);
                            }
                        });
                    let enter = ctx.input(|i| i.key_pressed(egui::Key::Enter));
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.console_input)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY),
                    );
                    if self.focus || !response.has_focus() {
                        response.request_focus();
                        self.focus = false;
                    }
                    self.recall(ctx, true);
                    if enter {
                        let line = std::mem::take(&mut self.console_input);
                        self.log(format!("> {line}"));
                        // The console runs commands without a slash, like FiveM.
                        let line = line.trim().trim_start_matches('/').to_string();
                        if line == "clear" {
                            self.log.clear();
                        } else if !line.is_empty() {
                            self.submit(format!("/{line}"));
                        }
                    }
                  });
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn chat_resource_events_and_submissions() {
        let mut chat = Chat::default();
        assert!(chat.handle_event(
            "chat:addMessage",
            &[serde_json::json!({"color":[255,0,0],"args":["^1Bob","hi ^2there"]})]
        ));
        assert_eq!(chat.messages[0].author, "Bob");
        assert_eq!(chat.messages[0].text, "hi there");
        assert_eq!(chat.messages[0].color, Color32::from_rgb(255, 0, 0));
        assert!(chat.handle_event(
            "chatMessage",
            &[
                "SYSTEM".into(),
                serde_json::json!([0, 255, 0]),
                "restart".into()
            ]
        ));
        assert_eq!(chat.messages[1].text, "restart");
        chat.handle_event(
            "chat:addSuggestions",
            &[serde_json::json!([{"name":"/respawn","help":""},{"name":"ping","help":"pong"}])],
        );
        assert_eq!(chat.suggestions.len(), 2);
        assert!(chat.suggestions.iter().any(|(n, _)| n == "/ping"));
        chat.handle_event("chat:clear", &[]);
        assert!(chat.messages.is_empty());
        assert!(!chat.handle_event("other:event", &[]));
        chat.submit("hello".into());
        chat.submit("/respawn now".into());
        assert_eq!(
            chat.submitted,
            [
                Submitted::Say("hello".into()),
                Submitted::Command("respawn now".into())
            ]
        );
    }
}
