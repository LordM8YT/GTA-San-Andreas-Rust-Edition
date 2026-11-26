//! SARE directory: friends and online status, with no accounts or passwords.
//!
//! A player's SARE ID is an Ed25519 key derived from their existing secret
//! (`identity.key`) with its own domain, so it is unrelated to the separate
//! license they have on each server: servers never see it. Its friend code
//! (`ABCD-EFGH-JKLM`) is a short hash of the public key that players swap.
//!
//! The client sends a signed heartbeat about once a minute with its name,
//! what it is playing and the friend codes it has added. The directory keeps
//! this in memory only and answers with the friends who are online **and**
//! have added the player back, so nobody can watch a stranger by adding
//! their code. Nothing is written to disk; a restart forgets everyone until
//! their next heartbeat.
//!
//! The service speaks plain HTTP on a local port and is meant to sit behind
//! a TLS reverse proxy (see docs/sare-directory.md).
use crate::identity::{hex, unhex};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    io::{self, BufRead, BufReader, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};

const ID_DOMAIN: &str = "sare-id-v1";
const SIGN_DOMAIN: &str = "sare-directory-v1";
/// A heartbeat counts this long; clients send one every minute.
pub const ONLINE_FOR: Duration = Duration::from_secs(150);
pub const HEARTBEAT_EVERY: Duration = Duration::from_secs(60);
pub const MAX_FRIENDS: usize = 200;
const MAX_BODY: usize = 16 * 1024;
const MAX_USERS: usize = 200_000;
const MAX_CONNECTIONS: usize = 64;
/// Heartbeats more than this far from the directory's clock are refused.
const CLOCK_SLACK: u64 = 300;
/// Crockford base32 without I, L, O and U.
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// The player's SARE ID key, from the secret in `identity.key`.
pub fn sare_id(secret: &str) -> SigningKey {
    let seed: [u8; 32] = Sha256::digest(format!("{ID_DOMAIN}\n{secret}")).into();
    SigningKey::from_bytes(&seed)
}
/// The friend code of a SARE ID public key: 60 bits as `XXXX-XXXX-XXXX`.
pub fn friend_code(public: &VerifyingKey) -> String {
    let digest = Sha256::digest([ID_DOMAIN.as_bytes(), b"\n", public.as_bytes()].concat());
    let bits = u64::from_be_bytes(digest[..8].try_into().unwrap()) >> 4;
    let mut code = String::with_capacity(14);
    for i in 0..12 {
        if i == 4 || i == 8 {
            code.push('-');
        }
        code.push(ALPHABET[((bits >> (55 - i * 5)) & 31) as usize] as char);
    }
    code
}
/// A friend code as typed: case and separators are forgiven, the letters
/// that look like digits are read as those digits.
pub fn normalize_code(text: &str) -> Option<String> {
    let mut letters = String::new();
    for c in text.chars() {
        let c = match c.to_ascii_uppercase() {
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        if c == '-' || c == ' ' {
            continue;
        }
        if !c.is_ascii() || !ALPHABET.contains(&(c as u8)) {
            return None;
        }
        letters.push(c);
    }
    (letters.len() == 12).then(|| format!("{}-{}-{}", &letters[..4], &letters[4..8], &letters[8..]))
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Playing {
    /// What the player is in: a world name, or "San Andreas".
    pub world: String,
    /// The server address friends can join, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Heartbeat {
    /// SARE ID public key, hex.
    pub key: String,
    pub name: String,
    /// Unix seconds when sent.
    pub time: u64,
    #[serde(default)]
    pub playing: Option<Playing>,
    /// Friend codes this player has added.
    #[serde(default)]
    pub friends: Vec<String>,
}
/// What goes over the wire: the heartbeat as JSON text and its signature.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Signed {
    pub payload: String,
    pub signature: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Friend {
    pub code: String,
    pub name: String,
    #[serde(default)]
    pub playing: Option<Playing>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Reply {
    /// Friends online right now who have added this player too.
    pub online: Vec<Friend>,
}

fn message(payload: &str) -> Vec<u8> {
    format!("{SIGN_DOMAIN}\n{payload}").into_bytes()
}
pub fn sign(key: &SigningKey, heartbeat: &Heartbeat) -> Signed {
    let payload = serde_json::to_string(heartbeat).expect("heartbeat serializes");
    let signature = hex(&key.sign(&message(&payload)).to_bytes());
    Signed { payload, signature }
}

fn clean_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).take(24).collect();
    if name.trim().is_empty() {
        "Player".into()
    } else {
        name
    }
}
fn clean_playing(playing: Option<Playing>) -> Option<Playing> {
    let playing = playing?;
    let world: String = playing
        .world
        .chars()
        .filter(|c| !c.is_control())
        .take(48)
        .collect();
    let server = playing
        .server
        .filter(|s| s.len() <= 128 && s.parse::<SocketAddr>().is_ok());
    Some(Playing { world, server })
}

struct User {
    name: String,
    playing: Option<Playing>,
    friends: HashSet<String>,
    seen: Instant,
}
/// The directory's state: who is online and whom they added.
#[derive(Default)]
pub struct Directory {
    users: HashMap<String, User>,
    last_sweep: Option<Instant>,
}
impl Directory {
    /// Check and record one heartbeat; `now` is Unix seconds.
    pub fn heartbeat(&mut self, signed: &Signed, now: u64) -> Result<Reply, String> {
        if signed.payload.len() > MAX_BODY {
            return Err("Heartbeat too large".into());
        }
        let heartbeat: Heartbeat =
            serde_json::from_str(&signed.payload).map_err(|_| "Invalid heartbeat")?;
        let key = unhex::<32>(&heartbeat.key)
            .and_then(|k| VerifyingKey::from_bytes(&k).ok())
            .ok_or("Invalid SARE ID")?;
        let signature = unhex::<64>(&signed.signature).ok_or("Invalid signature")?;
        key.verify(
            &message(&signed.payload),
            &Signature::from_bytes(&signature),
        )
        .map_err(|_| "Signature check failed")?;
        if heartbeat.time.abs_diff(now) > CLOCK_SLACK {
            return Err("Your clock is off by more than five minutes".into());
        }
        if heartbeat.friends.len() > MAX_FRIENDS {
            return Err("Too many friends".into());
        }
        let friends: HashSet<String> = heartbeat
            .friends
            .iter()
            .filter_map(|c| normalize_code(c))
            .collect();
        let code = friend_code(&key);
        self.sweep();
        if !self.users.contains_key(&code) && self.users.len() >= MAX_USERS {
            return Err("The directory is full; try again later".into());
        }
        self.users.insert(
            code.clone(),
            User {
                name: clean_name(&heartbeat.name),
                playing: clean_playing(heartbeat.playing),
                friends: friends.clone(),
                seen: Instant::now(),
            },
        );
        let mut online: Vec<Friend> = friends
            .iter()
            .filter_map(|friend| {
                let user = self.users.get(friend)?;
                (user.seen.elapsed() < ONLINE_FOR && user.friends.contains(&code)).then(|| Friend {
                    code: friend.clone(),
                    name: user.name.clone(),
                    playing: user.playing.clone(),
                })
            })
            .collect();
        online.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(Reply { online })
    }
    /// Forget players who stopped sending heartbeats.
    fn sweep(&mut self) {
        if self
            .last_sweep
            .is_some_and(|at| at.elapsed() < Duration::from_secs(30))
        {
            return;
        }
        self.last_sweep = Some(Instant::now());
        self.users.retain(|_, u| u.seen.elapsed() < ONLINE_FOR);
    }
    pub fn online(&self) -> usize {
        self.users
            .values()
            .filter(|u| u.seen.elapsed() < ONLINE_FOR)
            .count()
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// One HTTP/1.1 request: `POST /v1/heartbeat` or `GET /health`.
fn handle(stream: TcpStream, directory: &Mutex<Directory>) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5)))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    let mut reader = BufReader::new(stream.try_clone()?).take((MAX_BODY + 4096) as u64);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
    let mut length = 0_usize;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 || header.trim().is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                length = value.trim().parse().unwrap_or(usize::MAX);
            }
        }
    }
    let (status, body) = match (method, path) {
        ("GET", "/health") => {
            let online = directory.lock().unwrap().online();
            ("200 OK", format!("{{\"online\":{online}}}"))
        }
        ("POST", "/v1/heartbeat") if length <= MAX_BODY => {
            let mut data = vec![0; length];
            reader.read_exact(&mut data)?;
            match serde_json::from_slice::<Signed>(&data) {
                Ok(signed) => match directory.lock().unwrap().heartbeat(&signed, unix_now()) {
                    Ok(reply) => ("200 OK", serde_json::to_string(&reply)?),
                    Err(e) => (
                        "400 Bad Request",
                        serde_json::json!({ "error": e }).to_string(),
                    ),
                },
                Err(_) => ("400 Bad Request", "{\"error\":\"Invalid request\"}".into()),
            }
        }
        ("POST", "/v1/heartbeat") => ("413 Payload Too Large", "{}".into()),
        _ => ("404 Not Found", "{}".into()),
    };
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )?;
    stream.flush()
}

/// Run the directory on `listener` until the process ends.
pub fn serve(listener: TcpListener) -> io::Result<()> {
    let directory = Arc::new(Mutex::new(Directory::default()));
    let active = Arc::new(AtomicUsize::new(0));
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if active.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
            continue;
        }
        active.fetch_add(1, Ordering::Relaxed);
        let (directory, active) = (directory.clone(), active.clone());
        std::thread::spawn(move || {
            let _ = handle(stream, &directory);
            active.fetch_sub(1, Ordering::Relaxed);
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(seed: &str) -> (SigningKey, String) {
        let key = sare_id(seed);
        let code = friend_code(&key.verifying_key());
        (key, code)
    }
    fn beat(key: &SigningKey, name: &str, friends: &[&str], time: u64) -> Signed {
        sign(
            key,
            &Heartbeat {
                key: hex(key.verifying_key().as_bytes()),
                name: name.into(),
                time,
                playing: Some(Playing {
                    world: "Harbor RP".into(),
                    server: Some("203.0.113.5:7777".into()),
                }),
                friends: friends.iter().map(|s| s.to_string()).collect(),
            },
        )
    }

    #[test]
    fn friend_codes_are_stable_short_and_forgiving() {
        let (key, code) = player("secret-a");
        assert_eq!(code, friend_code(&sare_id("secret-a").verifying_key()));
        assert_ne!(code, player("secret-b").1);
        assert_eq!(code.len(), 14);
        assert_eq!(
            normalize_code(&code.to_lowercase().replace('-', " ")),
            Some(code.clone())
        );
        assert_eq!(
            normalize_code("OOOO-IIII-LLLL").as_deref(),
            Some("0000-1111-1111")
        );
        assert_eq!(normalize_code("ABC"), None);
        assert_eq!(normalize_code("ABCD-EFGH-JKLU"), None);
        // The SARE ID is not any server license of the same secret.
        let license = crate::identity::license("secret-a", &"ab".repeat(32));
        assert_ne!(license, hex(key.verifying_key().as_bytes()));
    }

    #[test]
    fn only_mutual_online_friends_are_visible() {
        let mut directory = Directory::default();
        let now = 1_800_000_000;
        let (alice, a) = player("alice");
        let (bob, b) = player("bob");
        let (eve, _) = player("eve");
        // Eve adds Alice, but Alice never added Eve: Eve sees nothing.
        directory
            .heartbeat(&beat(&alice, "Alice", &[&b], now), now)
            .unwrap();
        assert!(directory
            .heartbeat(&beat(&eve, "Eve", &[&a], now), now)
            .unwrap()
            .online
            .is_empty());
        // Bob has not added Alice yet.
        assert!(directory
            .heartbeat(&beat(&bob, "Bob", &[], now), now)
            .unwrap()
            .online
            .is_empty());
        let reply = directory
            .heartbeat(&beat(&bob, "Bob", &[&a.to_lowercase()], now), now)
            .unwrap();
        assert_eq!(reply.online.len(), 1);
        assert_eq!(reply.online[0].name, "Alice");
        assert_eq!(reply.online[0].code, a);
        assert_eq!(reply.online[0].playing.as_ref().unwrap().world, "Harbor RP");
        let reply = directory
            .heartbeat(&beat(&alice, "Alice", &[&b], now), now)
            .unwrap();
        assert_eq!(reply.online[0].name, "Bob");
    }

    #[test]
    fn forged_stale_and_oversized_heartbeats_are_refused() {
        let mut directory = Directory::default();
        let now = 1_800_000_000;
        let (alice, _) = player("alice");
        let (mallory, _) = player("mallory");
        let mut forged = beat(&alice, "Alice", &[], now);
        forged.signature = beat(&mallory, "Alice", &[], now).signature;
        assert!(directory.heartbeat(&forged, now).is_err());
        let mut tampered = beat(&alice, "Alice", &[], now);
        tampered.payload = tampered.payload.replace("Alice", "Admin");
        assert!(directory.heartbeat(&tampered, now).is_err());
        assert!(directory
            .heartbeat(&beat(&alice, "Alice", &[], now - 3600), now)
            .is_err());
        let many: Vec<String> = (0..=MAX_FRIENDS).map(|i| format!("{i:012}")).collect();
        let many: Vec<&str> = many.iter().map(String::as_str).collect();
        assert!(directory
            .heartbeat(&beat(&alice, "Alice", &many, now), now)
            .is_err());
        // A bad server address is dropped, not trusted.
        let odd = Heartbeat {
            key: hex(alice.verifying_key().as_bytes()),
            name: "\u{7}Alice".into(),
            time: now,
            playing: Some(Playing {
                world: "W".into(),
                server: Some("evil".into()),
            }),
            friends: vec![],
        };
        directory.heartbeat(&sign(&alice, &odd), now).unwrap();
        assert_eq!(directory.users.values().next().unwrap().name, "Alice");
        assert_eq!(
            directory
                .users
                .values()
                .next()
                .unwrap()
                .playing
                .as_ref()
                .unwrap()
                .server,
            None
        );
    }

    #[test]
    fn the_http_service_answers_heartbeats() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || serve(listener));
        let (alice, _) = player("alice");
        let body = serde_json::to_string(&beat(&alice, "Alice", &[], unix_now())).unwrap();
        let mut stream = TcpStream::connect(address).unwrap();
        write!(
            stream,
            "POST /v1/heartbeat HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200"), "{response}");
        assert!(response.ends_with("{\"online\":[]}"), "{response}");
        let mut stream = TcpStream::connect(address).unwrap();
        write!(stream, "GET /health HTTP/1.1\r\n\r\n").unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).unwrap();
        assert!(response.ends_with("{\"online\":1}"), "{response}");
    }
}
