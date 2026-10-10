//! Script events from a SARE server, plus chat and F8 console input.
//! Player hosts without a script server relay chat themselves.
use super::{streaming, State, ORIGIN};
use crate::chat::Submitted;
use glam::Vec3;
use sa_scene::collision::Player;
use serde_json::{json, Value as Json};

impl State {
    pub(super) fn handle_server_events(&mut self) {
        let Some(session) = &self.network_session else {
            return;
        };
        let hosting = self.network_host;
        let events = session.events();
        for event in events {
            let Ok(args) = serde_json::from_str::<Vec<Json>>(&event.payload) else {
                continue;
            };
            if hosting {
                self.host_event(event.source, &event.name, &args);
                continue;
            }
            let number = |i: usize| args.get(i).and_then(Json::as_f64).map(|v| v as f32);
            match event.name.as_str() {
                "__sare:setCoords" => {
                    if let (Some(x), Some(y), Some(z)) = (number(0), number(1), number(2)) {
                        self.pending_coords = Some([x, y, z]);
                    }
                }
                "__sare:setHeading" => {
                    if let Some(heading) = number(0) {
                        self.yaw = heading.to_radians();
                    }
                }
                name => {
                    if !self.chat.handle_event(name, &args) {
                        self.chat.log(format!("event {name} {}", event.payload));
                    }
                }
            }
        }
    }
    /// A player host has no Lua: mirror the chat resource's basic behaviour.
    fn host_event(&mut self, source: u32, name: &str, args: &[Json]) {
        if name != "_chat:messageEntered" {
            return;
        }
        let Some(text) = args.get(2).and_then(Json::as_str) else {
            return;
        };
        let author = self
            .menu
            .network_players
            .iter()
            .zip(self.network_peer_ids.iter())
            .find(|(_, id)| **id == source)
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| format!("player {source}"));
        self.broadcast_chat(&author, text);
    }
    fn broadcast_chat(&mut self, author: &str, text: &str) {
        let text: String = text.chars().filter(|c| !c.is_control()).take(256).collect();
        let message = json!([{ "color": [255, 255, 255], "args": [author, text] }]);
        if let Some(session) = &self.network_session {
            let _ = session.trigger(None, "chat:addMessage", &message.to_string());
        }
        self.chat
            .handle_event("chat:addMessage", message.as_array().unwrap());
    }

    pub(super) fn process_chat(&mut self) {
        for submitted in std::mem::take(&mut self.chat.submitted) {
            match submitted {
                Submitted::Say(text) => {
                    if self.network_session.is_none() {
                        self.chat.add_message(
                            "",
                            "Not connected. Use F5 or connect <ip:port> in F8.",
                            egui::Color32::GRAY,
                        );
                    } else if self.network_host {
                        let name = self.menu.player_name.clone();
                        self.broadcast_chat(&name, &text);
                    } else if let Some(session) = &self.network_session {
                        let payload =
                            json!([self.menu.player_name, [255, 255, 255], text]).to_string();
                        if session
                            .trigger(None, "_chat:messageEntered", &payload)
                            .is_err()
                        {
                            self.chat
                                .add_message("", "Message too long.", egui::Color32::GRAY);
                        }
                    }
                }
                Submitted::Command(line) => self.run_command(&line),
            }
        }
    }
    fn run_command(&mut self, line: &str) {
        let mut words = line.split_whitespace();
        let command = words.next().unwrap_or("").to_ascii_lowercase();
        let rest: Vec<&str> = words.collect();
        match command.as_str() {
            "cars" | "peds" | "mp" | "mods" => {
                self.menu.command = format!("/{command}");
                self.menu.submit_command();
                self.chat.close();
                self.capture(false);
            }
            "connect" => match rest.first().and_then(|a| a.parse::<std::net::SocketAddr>().ok()) {
                Some(address) => {
                    if self.network_session.is_some() {
                        self.disconnect_network();
                    }
                    self.menu.relay_mode = false;
                    self.menu.join_address = address.to_string();
                    self.chat.log(format!("Connecting to {address}..."));
                    self.apply_menu_action(Some(crate::menu::Action::Join));
                }
                None => self.chat.log("Usage: connect <ip:port>"),
            },
            "disconnect" => {
                self.disconnect_network();
                self.chat.log("Disconnected.");
            }
            "quit" => self.apply_menu_action(Some(crate::menu::Action::Quit)),
            "help" | "cmdlist" => self.chat.log(
                "Local: /cars /peds /mp /mods, connect <ip:port>, disconnect, quit. Other /commands go to the server.",
            ),
            _ => match &self.network_session {
                Some(session) if !self.network_host => {
                    let payload = json!([line]).to_string();
                    let _ = session.trigger(None, "__cfx_internal:commandFallback", &payload);
                }
                _ => self.chat.add_message("", &format!("Unknown command /{command}."), egui::Color32::GRAY),
            },
        }
    }

    /// Apply a server `SetEntityCoords`: stream the area first when needed,
    /// then stand the player on the nearest ground at that point.
    pub(super) fn apply_pending_coords(&mut self) {
        let Some([x, y, z]) = self.pending_coords else {
            return;
        };
        let target = [x, y];
        let loaded = self.interior == 0
            && self.destination.is_none()
            && streaming::distance(self.region, target) < streaming::RADIUS * 0.5;
        if !loaded {
            if self.destination != Some(target) {
                self.leave_passenger();
                self.interior_destination = None;
                self.destination = Some(target);
            }
            return;
        }
        let Some(world) = &self.collision else { return };
        let point = Vec3::new(x - ORIGIN[0], z, ORIGIN[1] - y);
        let player = world
            .standing_at(point, z + 2.0)
            .unwrap_or_else(|| Player::spawn(world, Vec3::new(point.x, z + 60.0, point.z)));
        self.driving = false;
        self.position = player.eye();
        self.player = Some(player);
        self.pending_coords = None;
        eprintln!("Server moved player to {x:.1}, {y:.1}, {z:.1}");
    }
}
