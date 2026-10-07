//! Headless pose/membership server; no window, GPU, Steam or game installation.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, BufRead, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::mpsc,
    thread,
    time::Duration,
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Config {
    name: String,
    listen: SocketAddr,
    relay: Option<SocketAddr>,
    public: bool,
    mods_dir: PathBuf,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            name: "SA Freeroam".into(),
            listen: "127.0.0.1:7777".parse().unwrap(),
            relay: None,
            public: false,
            mods_dir: "mods".into(),
        }
    }
}
impl Config {
    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.name.trim().is_empty()
                && self.name.chars().count() <= 24
                && !self.name.chars().any(char::is_control),
            "Server name must contain 1–24 printable characters"
        );
        anyhow::ensure!(
            !self.mods_dir.as_os_str().is_empty(),
            "mods_dir must name a resource directory"
        );
        Ok(())
    }
    fn read(path: &Path) -> Result<Self> {
        if !path.exists() {
            let bytes = serde_json::to_vec_pretty(&Self::default())?;
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)?;
            file.write_all(&bytes)?;
            file.write_all(b"\n")?;
            println!(
                "Created {}. Stop and edit it to configure LAN or relay hosting.",
                path.display()
            );
        }
        anyhow::ensure!(
            fs::metadata(path)?.len() <= 16 * 1024,
            "Server config exceeds 16 KiB"
        );
        let config: Self =
            serde_json::from_slice(&fs::read(path)?).context("Invalid server configuration")?;
        config.validate()?;
        Ok(config)
    }
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("sa-server [--config <file>]\nDefault: server.json in the current directory.\nConsole: status, players, help, quit. Maximum: 20 clients.\nDirect TCP or existing project relay; no public relay or Steam service is bundled.");
        return Ok(());
    }
    anyhow::ensure!(
        args.is_empty() || (args.len() == 2 && args[0] == "--config"),
        "Use --config <file> or --help"
    );
    let file = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| "server.json".into());
    let config = Config::read(&file)?;
    let directory = if config.mods_dir.is_absolute() {
        config.mods_dir.clone()
    } else {
        file.parent()
            .unwrap_or(Path::new("."))
            .join(&config.mods_dir)
    };
    let share = sa_scene::share_resources(&directory).context("Cannot export server resources")?;
    println!(
        "Resources: {} packs, {} bytes, fingerprint {}",
        share.manifest.resources.len(),
        share.manifest.total_bytes(),
        share.manifest.fingerprint()?
    );
    let (session, publication) = if let Some(address) = config.relay {
        let (s, p) = sa_net::Session::dedicated_relay_resources(
            address,
            &config.name,
            config.public,
            share,
        )?;
        println!(
            "Dedicated server {} publishing via relay {address}",
            config.name
        );
        (s, Some(p))
    } else {
        let s = sa_net::Session::dedicated_resources(config.listen, &config.name, share)?;
        println!(
            "Dedicated server {} listening on {}",
            config.name, s.address
        );
        (s, None)
    };
    let (commands, receiver) = mpsc::sync_channel(16);
    thread::spawn(move || {
        for line in io::stdin().lock().lines() {
            let Ok(line) = line else {
                break;
            };
            if line.len() <= 256 && commands.send(line).is_err() {
                break;
            }
        }
        // EOF is normal under systemd/hosting panels; it must not stop a server.
    });
    println!("Commands: status, players, help, quit. Movement is client-authoritative; this prototype does not simulate collisions or NPCs.");
    let mut published = String::new();
    let mut count = usize::MAX;
    loop {
        if let Some(publication) = &publication {
            let report = publication.report();
            if !report.code.is_empty() && report.code != published {
                println!("Dedicated server join code: {}", report.code);
                published = report.code;
            }
        }
        if let Some(report) = session.update(sa_net::Pose::default()) {
            anyhow::ensure!(
                report.connected || report.revision == 0,
                "{}",
                report.status
            );
            if report.peers.len() != count {
                count = report.peers.len();
                println!("Players: {count}/{}", sa_net::MAX_PLAYERS);
            }
            while let Ok(command) = receiver.try_recv() {
                match command.trim().to_ascii_lowercase().as_str() {
                    "quit" | "exit" | "stop" => {
                        println!("Stopping server");
                        drop(publication);
                        drop(session);
                        thread::sleep(Duration::from_millis(200));
                        return Ok(());
                    }
                    "status" => println!(
                        "{}; {count}/{} players; {}",
                        config.name,
                        sa_net::MAX_PLAYERS,
                        report.status
                    ),
                    "players" => {
                        for peer in &report.peers {
                            println!("{}: {}", peer.id, peer.name);
                        }
                    }
                    "help" => println!("status, players, help, quit"),
                    "" => {}
                    _ => println!("Unknown command. Use help."),
                }
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn config_defaults_and_invalid_options() {
        let config: Config = serde_json::from_str("{}").unwrap();
        config.validate().unwrap();
        assert!(config.relay.is_none() && !config.public);
        assert_eq!(config.listen.ip(), std::net::Ipv4Addr::LOCALHOST);
        assert!(serde_json::from_str::<Config>(r#"{"relai":"127.0.0.1:7778"}"#).is_err());
        assert!(serde_json::from_str::<Config>(r#"{"listen":"invalid"}"#).is_err());
        for name in [
            "",
            "\n",
            "server\0",
            "this name exceeds twenty four characters",
        ] {
            assert!(Config {
                name: name.into(),
                ..Config::default()
            }
            .validate()
            .is_err());
        }
    }
}
