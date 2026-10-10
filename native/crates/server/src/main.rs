//! Headless SARE server laid out like a FiveM server: `server.cfg`,
//! `resources/[category]/<resource>/fxmanifest.lua` and server-side Lua.
//! No window, GPU, Steam or game installation is needed.
mod acl;
mod cfg;
mod cfxlua;
mod console;
mod manifest;
mod natives;
mod script;
mod template;
mod update;

use anyhow::{Context, Result};
use console::Console;
use script::{out, Shared, State};
use serde_json::Value as Json;
use std::{
    cell::RefCell,
    collections::BTreeMap,
    io::{self, BufRead},
    net::{SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    rc::Rc,
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

const USAGE: &str = "sa-server [+exec server.cfg] [+set name value] [+ensure resource] ...
Run in a server-data folder containing server.cfg and resources/.
Without arguments: exec server.cfg, creating a FiveM-style template when absent.";

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}\n\nConsole commands:\n{}", console::HELP);
        return Ok(());
    }
    anyhow::ensure!(
        args.is_empty() || args[0].starts_with('+'),
        "Arguments use FiveM syntax, e.g. +exec server.cfg. See --help."
    );
    let mut commands = cfg::parse_arguments(&args);
    if commands.is_empty() {
        if !Path::new("server.cfg").exists() {
            template::create(Path::new("."))?;
        }
        commands.push(vec!["exec".into(), "server.cfg".into()]);
    }

    let Server {
        shared,
        mut console,
        session,
        publication,
    } = boot(commands, PathBuf::from("resources"))?;
    let rcon = if publication.is_none() {
        match UdpSocket::bind(session.address) {
            Ok(socket) => {
                socket.set_nonblocking(true)?;
                Some(socket)
            }
            Err(error) => {
                out(
                    &shared,
                    format!("rcon unavailable on UDP {}: {error}", session.address),
                );
                None
            }
        }
    } else {
        None
    };
    let mut rcon_failures: BTreeMap<std::net::IpAddr, Instant> = BTreeMap::new();

    let (lines, input) = mpsc::sync_channel::<String>(16);
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if line.len() <= 1024 && lines.send(line).is_err() {
                break;
            }
        }
        // EOF is normal under systemd/hosting panels; it must not stop a server.
    });
    out(&shared, "Server started. Type help for commands.");
    let mut updater = update::install_root()
        .filter(|_| {
            shared
                .borrow()
                .convar("sv_autoUpdate")
                .is_none_or(|v| !matches!(v, "0" | "false"))
        })
        .map(update::Updater::new);
    if updater.is_some() {
        out(
            &shared,
            "Automatic server updates are on (set sv_autoUpdate false to disable).",
        );
    }
    let mut restart = false;

    let mut known: BTreeMap<u32, String> = BTreeMap::new();
    let mut published = String::new();
    loop {
        if let Some(publication) = &publication {
            let report = publication.report();
            if !report.code.is_empty() && report.code != published {
                out(
                    &shared,
                    format!("Dedicated server join code: {}", report.code),
                );
                published = report.code;
            }
        }
        pump(&shared, &mut console, &session, &mut known)?;
        while let Ok(line) = input.try_recv() {
            console.execute_line(0, &line);
        }
        if let Some(socket) = &rcon {
            serve_rcon(socket, &mut console, &mut rcon_failures);
        }
        if let Some(updater) = &mut updater {
            let mut log = Vec::new();
            let players = shared.borrow().peers.len();
            let ready = updater.poll(players, &mut log);
            for line in log {
                out(&shared, line);
            }
            if let Some(commit) = ready {
                match updater.install() {
                    Ok(()) => {
                        restart = true;
                        console.quit = Some(format!("installing server update {}", &commit[..12]));
                    }
                    Err(error) => out(
                        &shared,
                        format!(
                            "Server update failed: {error:#}. The current server keeps running."
                        ),
                    ),
                }
            }
        }
        if let Some(reason) = console.quit.take() {
            out(&shared, format!("Stopping server: {reason}"));
            for peer in shared.borrow().peers.clone() {
                session.drop_player(peer.id, &reason);
            }
            let names: Vec<String> = shared
                .borrow()
                .resources
                .iter()
                .filter(|(_, r)| r.started)
                .map(|(n, _)| n.clone())
                .collect();
            for name in names.iter().rev() {
                let _ = script::stop(&shared, name);
            }
            thread::sleep(Duration::from_millis(300));
            drop(publication);
            if restart {
                std::process::exit(update::RESTART_CODE);
            }
            return Ok(());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

struct Server {
    shared: Shared,
    console: Console,
    session: Rc<sa_net::Session>,
    publication: Option<sa_net::relay::Publication>,
}
/// Execute the startup commands, share boot-time native assets, open the
/// endpoint and start the ensured resources, in FiveM's order.
fn boot(commands: Vec<Vec<String>>, resources_dir: PathBuf) -> Result<Server> {
    let shared: Shared = Rc::new(RefCell::new(State::new(resources_dir)));
    {
        let mut state = shared.borrow_mut();
        state
            .convars
            .insert("sv_hostname".into(), "SARE Server".into());
        state
            .convars
            .insert("sv_maxclients".into(), sa_net::MAX_PLAYERS.to_string());
        state.convars.insert(
            "version".into(),
            format!(
                "sare-server {} protocol {}",
                env!("CARGO_PKG_VERSION"),
                sa_net::VERSION
            ),
        );
    }
    let mut console = Console::new(shared.clone());
    for words in commands {
        let raw = words.join(" ");
        console.execute(0, words, &raw);
    }
    console.booting = false;
    let found = script::refresh(&shared).context("Cannot scan resources/")?;
    out(&shared, format!("Found {found} resources."));

    // Native assets are fixed for the server's lifetime: clients download
    // them before joining, so only boot-time ensures can contribute.
    let mut asset_folders = Vec::new();
    for name in boot_order(&shared, &console.boot_resources) {
        let folder = shared.borrow().resources.get(&name).map(|r| r.path.clone());
        match folder {
            Some(folder) if manifest::read(&folder).is_ok_and(|m| m.native_assets) => {
                asset_folders.push(folder)
            }
            Some(_) => {}
            None => out(&shared, format!("Couldn't find resource {name}.")),
        }
    }
    let share = sa_scene::share_resource_folders(asset_folders, true)
        .context("Cannot export native resource assets")?;
    out(
        &shared,
        format!(
            "Native assets: {} resources, {} bytes",
            share.manifest.resources.len(),
            share.manifest.total_bytes()
        ),
    );

    let hostname = shared
        .borrow()
        .convar("sv_hostname")
        .unwrap_or("SARE Server")
        .to_string();
    let hostname: String = hostname
        .chars()
        .filter(|c| !c.is_control())
        .take(24)
        .collect();
    let relay = shared
        .borrow()
        .convar("sv_relay")
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let listed = shared
        .borrow()
        .convar("sv_master1")
        .is_none_or(|v| !v.is_empty());
    let endpoint = console
        .endpoints
        .first()
        .copied()
        .unwrap_or_else(|| SocketAddr::from(([0, 0, 0, 0], sa_net::DEFAULT_PORT)));
    let (session, publication) = if let Some(relay) = relay {
        let address: SocketAddr = relay.parse().context("sv_relay must be ip:port")?;
        let (s, p) = sa_net::Session::dedicated_relay_resources(address, &hostname, listed, share)?;
        out(
            &shared,
            format!(
                "Publishing {hostname} via relay {address} ({})",
                if listed { "listed" } else { "join code only" }
            ),
        );
        (s, Some(p))
    } else {
        let s = sa_net::Session::dedicated_resources(endpoint, &hostname, share)
            .with_context(|| format!("Cannot listen on {endpoint}"))?;
        out(
            &shared,
            format!("Server {hostname} listening on {}", s.address),
        );
        (s, None)
    };
    let session = Rc::new(session);
    let max: usize = shared
        .borrow()
        .convar("sv_maxclients")
        .and_then(|v| v.parse().ok())
        .unwrap_or(sa_net::MAX_PLAYERS);
    session.set_max_clients(max);
    shared.borrow_mut().session = Some(session.clone());

    for name in boot_order(&shared, &console.boot_resources) {
        if let Err(error) = script::start(&shared, &name) {
            out(
                &shared,
                format!("Couldn't start resource {name}: {error:#}"),
            );
        }
    }
    shared.borrow_mut().assets_locked = true;

    Ok(Server {
        shared,
        console,
        session,
        publication,
    })
}

/// One server frame: membership changes, network events, Lua timers and
/// queued `ExecuteCommand` lines.
fn pump(
    shared: &Shared,
    console: &mut Console,
    session: &sa_net::Session,
    known: &mut BTreeMap<u32, String>,
) -> Result<()> {
    if let Some(report) = session.update(sa_net::Pose::default()) {
        anyhow::ensure!(
            report.connected || report.revision == 0,
            "{}",
            report.status
        );
        let current: BTreeMap<u32, String> = report
            .peers
            .iter()
            .map(|p| (p.id, p.name.clone()))
            .collect();
        // Departed peers stay resolvable (GetPlayerName) during playerDropped.
        for (id, name) in known.iter() {
            if !current.contains_key(id) {
                out(shared, format!("Player {name} ({id}) left."));
                script::dispatch(
                    shared,
                    "playerDropped",
                    *id,
                    &[Json::from("Exiting")],
                    false,
                );
                let mut state = shared.borrow_mut();
                state.peers.retain(|p| p.id != *id);
                state.bags.remove(&format!("player:{id}"));
            }
        }
        shared.borrow_mut().peers = report.peers.clone();
        for (id, name) in &current {
            if !known.contains_key(id) {
                out(shared, format!("Player {name} ({id}) joined."));
                // Already connected: a rejecting handler drops the player.
                let rejected = script::dispatch(
                    shared,
                    "playerConnecting",
                    *id,
                    &[Json::from(name.as_str())],
                    false,
                );
                if !rejected {
                    script::dispatch(shared, "playerJoining", *id, &[Json::from(*id)], false);
                }
            }
        }
        *known = current;
    }
    for event in session.events() {
        let Ok(args) = serde_json::from_str::<Vec<Json>>(&event.payload) else {
            continue;
        };
        if event.name == "__cfx_internal:commandFallback" {
            if let Some(line) = args.first().and_then(Json::as_str) {
                console.execute_line(event.source, line.trim_start_matches('/'));
            }
            continue;
        }
        script::dispatch(shared, &event.name, event.source, &args, true);
    }
    script::tick(shared);
    for _ in 0..64 {
        let pending = std::mem::take(&mut shared.borrow_mut().pending);
        if pending.is_empty() {
            break;
        }
        for (source, line) in pending {
            console.execute_line(source, &line);
        }
    }
    Ok(())
}

/// `ensure` order, with dependencies first, without duplicates.
fn boot_order(shared: &Shared, requested: &[String]) -> Vec<String> {
    let requested: Vec<String> = requested
        .iter()
        .flat_map(|name| {
            if script::is_category(name) {
                script::category_members(shared, name)
            } else {
                vec![name.clone()]
            }
        })
        .collect();
    fn visit(shared: &Shared, name: &str, order: &mut Vec<String>, depth: usize) {
        if depth > 16 || order.iter().any(|n| n == name) {
            return;
        }
        let folder = shared.borrow().resources.get(name).map(|r| r.path.clone());
        if let Some(manifest) = folder.and_then(|f| manifest::read(&f).ok()) {
            for dependency in manifest
                .values("dependency")
                .filter(|d| !d.starts_with('/'))
            {
                let dependency =
                    script::resolve(shared, dependency).unwrap_or_else(|| dependency.to_string());
                visit(shared, &dependency, order, depth + 1);
            }
        }
        order.push(name.to_string());
    }
    let mut order = Vec::new();
    for name in &requested {
        visit(shared, name, &mut order, 0);
    }
    order
}

/// Quake-style rcon as used by FiveM: `\xff\xff\xff\xffrcon <password> <command>`.
fn serve_rcon(
    socket: &UdpSocket,
    console: &mut Console,
    failures: &mut BTreeMap<std::net::IpAddr, Instant>,
) {
    let mut buffer = [0u8; 2048];
    for _ in 0..8 {
        let Ok((size, from)) = socket.recv_from(&mut buffer) else {
            return;
        };
        let Some(text) = buffer[..size]
            .strip_prefix(&[0xff; 4])
            .and_then(|rest| std::str::from_utf8(rest).ok())
            .and_then(|rest| rest.strip_prefix("rcon "))
        else {
            continue;
        };
        if failures
            .get(&from.ip())
            .is_some_and(|t| t.elapsed() < Duration::from_secs(1))
        {
            continue;
        }
        let password = console
            .shared
            .borrow()
            .convar("rcon_password")
            .unwrap_or("")
            .to_string();
        let mut words = cfg::parse_line(text).into_iter().next().unwrap_or_default();
        if words.is_empty() {
            continue;
        }
        let given = words.remove(0);
        let reply = if password.is_empty() {
            "The server must set rcon_password to be able to use this command.\n".to_string()
        } else if given != password {
            failures.insert(from.ip(), Instant::now());
            if failures.len() > 1024 {
                failures.clear();
            }
            println!("Invalid rcon password from {from}");
            "Invalid password.\n".to_string()
        } else {
            let command = words.join(" ");
            println!("rcon from {from}: {command}");
            console.shared.borrow_mut().capture = Some(String::new());
            console.execute_line(0, &command);
            console
                .shared
                .borrow_mut()
                .capture
                .take()
                .unwrap_or_default()
        };
        let mut packet = vec![0xff; 4];
        packet.extend(b"print ");
        packet.extend(reply.as_bytes().iter().take(1200));
        let _ = socket.send_to(&packet, from);
    }
}

#[cfg(test)]
mod tests;
