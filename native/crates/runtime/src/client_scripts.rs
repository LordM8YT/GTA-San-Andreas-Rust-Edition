//! The server's client scripts: downloads bundles, feeds the sandbox what
//! scripts may read each frame and applies what they asked for.
use super::{State, ORIGIN};
use sa_lua::client::{Output, PlayerView, View};
use winit::keyboard::KeyCode;

/// SA world coordinates of a pose (`[x - ORIGIN.x, z, ORIGIN.y - y]`), as
/// sa-server reports them.
fn world_position(position: [f32; 3]) -> [f32; 3] {
    [
        position[0] + ORIGIN[0],
        ORIGIN[1] - position[2],
        position[1],
    ]
}

impl State {
    pub(super) fn update_client_scripts(&mut self) {
        let Some(session) = self.network_session.as_ref().filter(|_| !self.network_host) else {
            return;
        };
        for sha256 in self.scripts.take_requests() {
            session.request_script(&sha256);
        }
        for (sha256, bytes) in session.take_scripts() {
            if let Err(error) = self.scripts.load(&sha256, &bytes) {
                self.chat
                    .log(format!("Client script bundle rejected: {error}"));
            }
        }
        let local = self.local_pose();
        let players = self
            .script_peers
            .iter()
            .map(|peer| {
                let pose = if peer.id == self.script_local_id {
                    local
                } else {
                    peer.pose
                };
                PlayerView {
                    id: peer.id,
                    name: peer.name.clone(),
                    position: world_position(pose.position),
                    heading: pose.yaw.to_degrees().rem_euclid(360.0),
                    in_vehicle: pose.driving,
                }
            })
            .collect();
        self.scripts.tick(View {
            local_id: self.script_local_id,
            players,
        });
        for output in self.scripts.take_outputs() {
            match output {
                Output::ServerEvent { name, payload } => {
                    if let Some(session) = &self.network_session {
                        if session.trigger(None, &name, &payload).is_err() {
                            self.chat.log(format!("Script event {name} dropped"));
                        }
                    }
                }
                Output::Teleport(position) => self.pending_coords = Some(position),
                Output::Heading(heading) => self.yaw = heading.to_radians(),
                Output::Console(line) => self.chat.log(line),
            }
        }
        // Menus and dialogs need the mouse; give it back to the game after.
        let wants = self.scripts.ui().wants_cursor();
        if wants && self.captured {
            self.capture(false);
            self.keys.clear();
            self.script_cursor = true;
        } else if !wants {
            self.release_script_cursor();
        }
    }
    pub(super) fn release_script_cursor(&mut self) {
        if std::mem::take(&mut self.script_cursor)
            && self.menu.page.is_none()
            && !self.chat.active()
        {
            self.capture(true);
        }
    }
}

/// FiveM key names for RegisterKeyMapping.
pub(super) fn key_name(code: KeyCode) -> Option<&'static str> {
    Some(match code {
        KeyCode::F1 => "F1",
        KeyCode::F2 => "F2",
        KeyCode::F11 => "F11",
        KeyCode::F12 => "F12",
        KeyCode::KeyB => "B",
        KeyCode::KeyC => "C",
        KeyCode::KeyH => "H",
        KeyCode::KeyJ => "J",
        KeyCode::KeyK => "K",
        KeyCode::KeyL => "L",
        KeyCode::KeyN => "N",
        KeyCode::KeyO => "O",
        KeyCode::KeyU => "U",
        KeyCode::KeyY => "Y",
        KeyCode::KeyZ => "Z",
        KeyCode::Tab => "TAB",
        KeyCode::Home => "HOME",
        KeyCode::End => "END",
        KeyCode::Insert => "INSERT",
        KeyCode::Delete => "DELETE",
        KeyCode::PageUp => "PAGEUP",
        KeyCode::PageDown => "PAGEDOWN",
        _ => return None,
    })
}
