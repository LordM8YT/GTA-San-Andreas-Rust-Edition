//! The server console: `server.cfg`, `+command` arguments, stdin, rcon,
//! `ExecuteCommand` and player commands all run through `Console::execute`.
use crate::{
    cfg,
    script::{self, out, Shared},
};
use std::{fs, net::SocketAddr, path::PathBuf};

pub const HELP: &str = "\
exec <file>                 run a cfg file
set|sets|setr <name> <val>  set a convar (sets = shown as server info)
sv_hostname <name>          server name (1-24 characters)
sv_maxclients <1-20>        player slots
endpoint_add_tcp <ip:port>  game endpoint (also rcon over UDP)
ensure|start|stop|restart <resource>
refresh                     rescan resources/
add_ace|remove_ace <principal> <object> <allow|deny>
add_principal|remove_principal <child> <parent>
test_ace <principal> <object>, list_aces
status, clientkick <id> <reason>, cmdlist, quit [reason]";

pub struct Console {
    pub shared: Shared,
    pub booting: bool,
    pub boot_resources: Vec<String>,
    pub endpoints: Vec<SocketAddr>,
    pub icon: Option<PathBuf>,
    pub quit: Option<String>,
    exec_depth: usize,
}
impl Console {
    pub fn new(shared: Shared) -> Self {
        Self {
            shared,
            booting: true,
            boot_resources: Vec::new(),
            endpoints: Vec::new(),
            icon: None,
            quit: None,
            exec_depth: 0,
        }
    }
    fn say(&self, text: impl AsRef<str>) {
        out(&self.shared, text);
    }
    pub fn execute_line(&mut self, source: u32, line: &str) {
        for words in cfg::parse_line(line) {
            self.execute(source, words, line.trim());
        }
    }
    pub fn execute(&mut self, source: u32, words: Vec<String>, raw: &str) {
        let Some(first) = words.first() else { return };
        let command = first.to_ascii_lowercase();
        let args = &words[1..];
        if source != 0 {
            // Players may run unrestricted script commands; built-ins, convars
            // and restricted commands need `command.<name>` in the ACL.
            let restricted = self
                .shared
                .borrow()
                .commands
                .get(&command)
                .is_none_or(|c| c.restricted);
            let allowed = self
                .shared
                .borrow()
                .acl
                .allowed(&format!("player.{source}"), &format!("command.{command}"));
            if restricted && !allowed {
                self.say(format!(
                    "Access denied for command {command} (player {source})"
                ));
                return;
            }
        }
        if script::run_command(&self.shared, &command, source, args, raw) {
            return;
        }
        let arg = |i: usize| args.get(i).map(String::as_str).unwrap_or("");
        match command.as_str() {
            "exec" => self.exec(arg(0)),
            "set" | "sets" | "setr" | "seta" if args.len() >= 2 => {
                let mut state = self.shared.borrow_mut();
                if command == "sets" {
                    state.server_info.insert(args[0].clone());
                }
                state.convars.insert(args[0].clone(), args[1].clone());
                drop(state);
                self.convar_changed(&args[0]);
            }
            "endpoint_add_tcp" => match arg(0).parse() {
                Ok(address) if self.booting => self.endpoints.push(address),
                Ok(_) => self.say("Endpoints apply at server start; restart the server."),
                Err(_) => self.say(format!("Invalid endpoint {}", arg(0))),
            },
            // Gameplay is TCP; rcon listens on the TCP endpoint's port over UDP.
            "endpoint_add_udp" => {}
            "ensure" | "start" | "restart" | "stop" if !args.is_empty() => {
                for name in args {
                    self.resource_command(&command, name);
                }
            }
            "refresh" => match script::refresh(&self.shared) {
                Ok(added) => self.say(format!("Found {added} new resources.")),
                Err(error) => self.say(format!("Refresh failed: {error:#}")),
            },
            "add_ace" | "remove_ace" if args.len() >= 2 => {
                let mut state = self.shared.borrow_mut();
                if command == "add_ace" {
                    let allow = !arg(2).eq_ignore_ascii_case("deny");
                    state.acl.add_ace(&args[0], &args[1], allow);
                } else {
                    state.acl.remove_ace(&args[0], &args[1]);
                }
            }
            "add_principal" | "remove_principal" if args.len() >= 2 => {
                let mut state = self.shared.borrow_mut();
                if command == "add_principal" {
                    state.acl.add_principal(&args[0], &args[1]);
                } else {
                    state.acl.remove_principal(&args[0], &args[1]);
                }
            }
            "test_ace" if args.len() >= 2 => {
                let allowed = self.shared.borrow().acl.allowed(&args[0], &args[1]);
                self.say(format!(
                    "{} -> {}: {}",
                    args[0],
                    args[1],
                    if allowed { "allowed" } else { "denied" }
                ));
            }
            "list_aces" => {
                let lines = self.shared.borrow().acl.describe();
                for line in lines {
                    self.say(line);
                }
            }
            "load_server_icon" => {
                self.icon = Some(PathBuf::from(arg(0)));
                self.say("Server icon recorded; the SARE browser does not show icons yet.");
            }
            "status" | "players" => self.status(),
            "resources" => {
                let lines: Vec<String> = self
                    .shared
                    .borrow()
                    .resources
                    .iter()
                    .map(|(name, r)| {
                        format!("{:9} {name}", if r.started { "started" } else { "stopped" })
                    })
                    .collect();
                for line in lines {
                    self.say(line);
                }
            }
            "clientkick" if !args.is_empty() => {
                let session = self.shared.borrow().session.clone();
                match (session, arg(0).parse::<u32>()) {
                    (Some(session), Ok(id)) => {
                        let reason = if args.len() > 1 {
                            args[1..].join(" ")
                        } else {
                            "Kicked.".into()
                        };
                        session.drop_player(id, &reason);
                    }
                    _ => self.say("Usage: clientkick <id> <reason>"),
                }
            }
            "cmdlist" | "help" => {
                self.say(HELP);
                let commands: Vec<String> = self.shared.borrow().commands.keys().cloned().collect();
                if !commands.is_empty() {
                    self.say(format!("Script commands: {}", commands.join(", ")));
                }
            }
            "quit" => {
                self.quit = Some(if args.is_empty() {
                    "Server shutting down.".into()
                } else {
                    args.join(" ")
                });
            }
            _ if args.len() == 1 && self.is_convar(&command) => {
                self.shared
                    .borrow_mut()
                    .convars
                    .insert(first.clone(), args[0].clone());
                self.convar_changed(first);
            }
            _ if args.is_empty() && self.shared.borrow().convars.contains_key(first.as_str()) => {
                let value = self.shared.borrow().convars[first.as_str()].clone();
                self.say(format!("\"{first}\" is \"{value}\""));
            }
            _ => self.say(format!("No such command {first}.")),
        }
    }
    fn is_convar(&self, name: &str) -> bool {
        name.starts_with("sv_")
            || name.starts_with("rcon_")
            || ["steam_webapikey", "onesync", "gamename"].contains(&name)
            || self.shared.borrow().convars.contains_key(name)
    }
    fn convar_changed(&self, name: &str) {
        if name.eq_ignore_ascii_case("sv_maxclients") {
            let state = self.shared.borrow();
            let value = state.convar(name).and_then(|v| v.parse::<usize>().ok());
            match value {
                Some(n @ 1..=sa_net::MAX_PLAYERS) => {
                    if let Some(session) = &state.session {
                        session.set_max_clients(n);
                    }
                }
                _ => {
                    drop(state);
                    self.say(format!(
                        "sv_maxclients must be 1-{}; using {}",
                        sa_net::MAX_PLAYERS,
                        sa_net::MAX_PLAYERS
                    ));
                }
            }
        }
    }
    fn exec(&mut self, file: &str) {
        if self.exec_depth >= 8 {
            self.say("exec nesting too deep");
            return;
        }
        let text = match fs::metadata(file) {
            Ok(m) if m.len() > 256 * 1024 => {
                self.say(format!("{file} exceeds 256 KiB"));
                return;
            }
            Ok(_) => fs::read_to_string(file),
            Err(e) => Err(e),
        };
        match text {
            Ok(text) => {
                self.exec_depth += 1;
                for line in text.lines() {
                    self.execute_line(0, line);
                }
                self.exec_depth -= 1;
            }
            Err(error) => self.say(format!("Could not execute {file}: {error}")),
        }
    }
    fn resource_command(&mut self, command: &str, name: &str) {
        if script::is_category(name) && !self.booting {
            for member in script::category_members(&self.shared, name) {
                self.resource_command(command, &member);
            }
            return;
        }
        if self.booting {
            match command {
                "ensure" | "start" | "restart" => {
                    if !self.boot_resources.iter().any(|r| r == name) {
                        self.boot_resources.push(name.into());
                    }
                }
                _ => self.boot_resources.retain(|r| r != name),
            }
            return;
        }
        let started = self
            .shared
            .borrow()
            .resources
            .get(name)
            .is_some_and(|r| r.started);
        let result = match command {
            "stop" => script::stop(&self.shared, name),
            "start" if started => Ok(()),
            "start" => script::start(&self.shared, name),
            "restart" if !started => Err(anyhow::anyhow!("{name} is not started")),
            _ if started => {
                script::stop(&self.shared, name).and_then(|_| script::start(&self.shared, name))
            }
            _ => script::start(&self.shared, name),
        };
        if let Err(error) = result {
            self.say(format!("Couldn't {command} resource {name}: {error:#}"));
        }
    }
    fn status(&self) {
        let state = self.shared.borrow();
        let hostname = state.convar("sv_hostname").unwrap_or("").to_string();
        let max = state
            .session
            .as_ref()
            .map_or(sa_net::MAX_PLAYERS, |s| s.max_clients());
        let mut lines = vec![
            format!("hostname: {hostname}"),
            format!("players:  {}/{max}", state.peers.len()),
            format!(
                "gametype: {}  map: {}",
                state.convar("gametype").unwrap_or("-"),
                state.convar("mapname").unwrap_or("-")
            ),
            "id   name                      position".into(),
        ];
        for peer in &state.peers {
            let [x, y, z] = crate::natives::world_position(&peer.pose);
            lines.push(format!(
                "{:<4} {:<25} {x:.0}, {y:.0}, {z:.0}",
                peer.id, peer.name
            ));
        }
        drop(state);
        for line in lines {
            self.say(line);
        }
    }
}
