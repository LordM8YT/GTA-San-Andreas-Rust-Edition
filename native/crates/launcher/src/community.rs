//! The community server list: a JSON file in the SARE repository that server
//! owners add their server to with a pull request. The client downloads it,
//! then asks every server directly for its name, players and world
//! (`sa_net::query`), so the numbers shown are live and come from the server
//! itself. No SARE-run service is involved.
use serde::Deserialize;
use std::{
    net::{SocketAddr, ToSocketAddrs},
    sync::mpsc,
    time::{Duration, Instant},
};

/// Where the list lives. `SARE_SERVER_LIST` overrides it with another
/// `https://` URL or a local file, for testing a list before a pull request.
pub const LIST_URL: &str =
    "https://raw.githubusercontent.com/LordM8YT/SARE/main/community/servers.json";
const MAX_SERVERS: usize = 200;
const MAX_LIST_BYTES: u64 = 256 * 1024;
/// Servers asked at the same time.
const PARALLEL: usize = 16;

#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Entry {
    pub name: String,
    /// `host:port`; the host may be a DNS name.
    pub address: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub website: String,
}
/// Newer lists may carry more fields; this client ignores them.
#[derive(Deserialize)]
struct List {
    version: u32,
    servers: Vec<Entry>,
}

#[derive(Clone, Debug)]
pub enum Status {
    Online {
        /// The resolved address the game connects to.
        address: SocketAddr,
        info: sa_net::ServerInfo,
        ping: Duration,
    },
    Offline(String),
}
#[derive(Clone, Debug)]
pub struct Server {
    pub entry: Entry,
    pub status: Status,
}

pub fn parse(data: &[u8]) -> Result<Vec<Entry>, String> {
    let list: List =
        serde_json::from_slice(data).map_err(|e| format!("The server list is not valid: {e}"))?;
    if list.version != 1 {
        return Err("The server list needs a newer SARE Client.".into());
    }
    let mut entries: Vec<Entry> = list
        .servers
        .into_iter()
        .filter(|e| {
            let name = e.name.trim();
            !name.is_empty()
                && name.chars().count() <= 48
                && !e.name.chars().any(char::is_control)
                && e.address.len() <= 128
                && e.address.rsplit_once(':').is_some_and(|(host, port)| {
                    !host.is_empty() && port.parse::<u16>().is_ok_and(|p| p != 0)
                })
                && e.description.chars().count() <= 200
                && (e.website.is_empty()
                    || (e.website.starts_with("https://") && e.website.len() <= 200))
        })
        .take(MAX_SERVERS)
        .collect();
    let mut seen = std::collections::HashSet::new();
    entries.retain(|e| seen.insert(e.address.to_ascii_lowercase()));
    Ok(entries)
}

fn download(source: &str) -> Result<Vec<u8>, String> {
    if !source.starts_with("https://") {
        return std::fs::read(source).map_err(|e| format!("Could not read {source}: {e}"));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .https_only(true)
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(3)
        .build()
        .into();
    let mut response = agent
        .get(source)
        .call()
        .map_err(|e| format!("Could not download the server list: {e}"))?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_LIST_BYTES)
        .read_to_vec()
        .map_err(|e| format!("Could not download the server list: {e}"))
}

fn resolve(address: &str) -> Result<SocketAddr, String> {
    let mut found: Vec<SocketAddr> = address
        .to_socket_addrs()
        .map_err(|_| "Address not found".to_string())?
        .collect();
    // The game connects over IPv4 first, like Direct connect.
    found.sort_by_key(|a| !a.is_ipv4());
    found.into_iter().next().ok_or("Address not found".into())
}

pub fn probe(entry: &Entry) -> Status {
    let address = match resolve(&entry.address) {
        Ok(address) => address,
        Err(error) => return Status::Offline(error),
    };
    let start = Instant::now();
    match sa_net::query(address) {
        Ok(info) => Status::Online {
            address,
            info,
            ping: start.elapsed(),
        },
        Err(error) if error.kind() == std::io::ErrorKind::InvalidData => {
            Status::Offline("Older server version, or not a SARE server".into())
        }
        Err(_) => Status::Offline("Not responding".into()),
    }
}

/// Probe every entry, a few at a time, in list order.
pub fn probe_all(entries: Vec<Entry>) -> Vec<Server> {
    let mut servers = Vec::with_capacity(entries.len());
    for chunk in entries.chunks(PARALLEL) {
        let (tx, rx) = mpsc::channel();
        for (index, entry) in chunk.iter().cloned().enumerate() {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let status = probe(&entry);
                let _ = tx.send((index, Server { entry, status }));
            });
        }
        drop(tx);
        let mut done: Vec<_> = rx.iter().collect();
        done.sort_by_key(|(index, _)| *index);
        servers.extend(done.into_iter().map(|(_, server)| server));
    }
    servers
}

/// Download the list from `source` and probe it.
pub fn load(source: &str) -> Result<Vec<Server>, String> {
    let entries = parse(&download(source)?)?;
    Ok(probe_all(entries))
}

/// The list the client uses: `SARE_SERVER_LIST`, or the repository's.
pub fn source() -> String {
    std::env::var("SARE_SERVER_LIST")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| LIST_URL.into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_skip_bad_and_duplicate_entries_and_ignore_new_fields() {
        let entries = parse(
            br#"{"version":1,"future":true,"servers":[
                {"name":"Town RP","address":"play.example.org:7777","description":"Roleplay","tags":["rp"]},
                {"name":"Copy","address":"PLAY.example.org:7777"},
                {"name":"","address":"1.2.3.4:7777"},
                {"name":"No port","address":"1.2.3.4"},
                {"name":"Zero","address":"1.2.3.4:0"},
                {"name":"Bad site","address":"1.2.3.4:7777","website":"http://x"}
            ]}"#,
        )
        .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "Town RP");
        assert!(parse(br#"{"version":2,"servers":[]}"#).is_err());
        assert!(parse(b"not json").is_err());
    }

    #[test]
    fn probes_read_live_servers_and_report_missing_ones() {
        let host = sa_net::Session::dedicated_resources(
            "127.0.0.1:0".parse().unwrap(),
            "Live",
            Default::default(),
        )
        .unwrap();
        let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let closed_address = closed.local_addr().unwrap();
        drop(closed);
        let servers = probe_all(vec![
            Entry {
                name: "Listed".into(),
                address: host.address.to_string(),
                description: String::new(),
                website: String::new(),
            },
            Entry {
                name: "Gone".into(),
                address: closed_address.to_string(),
                description: String::new(),
                website: String::new(),
            },
        ]);
        assert!(
            matches!(&servers[0].status, Status::Online { info, address, .. }
                if info.name == "Live" && *address == host.address)
        );
        assert!(matches!(&servers[1].status, Status::Offline(_)));
    }

    #[test]
    fn the_bundled_list_parses() {
        let data = include_bytes!("../../../../community/servers.json");
        parse(data).unwrap();
    }
}
