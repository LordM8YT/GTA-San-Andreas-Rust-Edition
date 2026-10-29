//! Player-hosted prototype. Networking never runs on the render thread.
//! Host assigns identities and relays poses; movement is client-authoritative.
use serde::{Deserialize, Serialize};
pub mod identity;
pub mod passengers;
pub mod relay;
pub mod resources;
pub use passengers::{PassengerSeat, RideReply, RideRequest, RideResult};
use std::{
    collections::{HashMap, VecDeque},
    io::{self, Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

pub const MAX_PLAYERS: usize = 20;
pub const DEFAULT_PORT: u16 = 7777;
pub const VERSION: u32 = 8;
/// Script events: FiveM-style `TriggerServerEvent` / `TriggerClientEvent`.
/// Names are ASCII identifiers; payloads are JSON arrays of arguments.
pub const MAX_EVENT_NAME: usize = 64;
pub const MAX_EVENT_PAYLOAD: usize = 4 * 1024;
const MAX_INBOX: usize = 256;
/// Per-guest event budget per second; excess events are discarded, not queued.
const EVENT_BUDGET: u32 = 30;
const TICK: Duration = Duration::from_millis(50);
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_FRAME: usize = 16 * 1024;
/// Largest client script bundle a host serves or a client accepts.
pub const MAX_SCRIPT_BYTES: usize = 8 * 1024 * 1024;
/// Script bundles a guest may have requested at once.
const MAX_SCRIPT_REQUESTS: usize = 64;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Pose {
    /// Feet or vehicle ground contact, relative to runtime ORIGIN.
    pub position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub speed: f32,
    pub driving: bool,
    pub moving: bool,
    pub interior: u8,
    /// Indices in the version-checked, ordered session catalogs.
    #[serde(default)]
    pub car_model: u16,
    #[serde(default)]
    pub ped_model: u16,
    #[serde(default)]
    pub clothes: u16,
    /// The peer's one personal car, independently of their walking pose.
    /// Membership identity owns this record; no arbitrary vehicle IDs accepted.
    #[serde(default)]
    pub vehicle: Option<VehiclePose>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ride: Option<PassengerSeat>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ride_request: Option<RideRequest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ride_reply: Option<RideReply>,
    /// Wanted stars, 0 to 6.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub wanted: u8,
}
fn is_zero(value: &u8) -> bool {
    *value == 0
}
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct VehiclePose {
    pub position: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
    pub speed: f32,
    pub interior: u8,
}
impl VehiclePose {
    fn valid(self) -> bool {
        valid_motion(self.position, self.yaw, self.pitch, self.roll, self.speed)
    }
}
fn valid_motion(position: [f32; 3], yaw: f32, pitch: f32, roll: f32, speed: f32) -> bool {
    position.iter().all(|v| v.is_finite() && v.abs() <= 20000.0)
        && [yaw, pitch, roll, speed].iter().all(|v| v.is_finite())
        && yaw.abs() <= 100000.0
        && pitch.abs() <= 4.0
        && roll.abs() <= 4.0
        && speed.abs() <= 200.0
}
impl Pose {
    fn valid(self) -> bool {
        valid_motion(self.position, self.yaw, self.pitch, self.roll, self.speed)
            && self.car_model < 256
            && self.ped_model < 256
            && self.vehicle.is_none_or(VehiclePose::valid)
            && (!self.driving || self.vehicle.is_some())
            && self.ride.is_none_or(PassengerSeat::valid)
            && self
                .ride_request
                .is_none_or(|request| request.sequence != 0)
            && self.wanted <= 6
    }
}
/// Traffic cars one player's game may share at once.
pub const MAX_TRAFFIC: usize = 32;
/// Players this far apart do not receive each other's traffic.
const TRAFFIC_RANGE: f32 = 400.0;
/// Traffic not refreshed for this long is gone (its owner left or moved away).
const TRAFFIC_STALE: Duration = Duration::from_millis(800);
/// One ambient traffic car a player's game drives, as the other players see
/// it. Compact on the wire: centimetres, milliradians, decimetres a second.
/// Each game settles the car on its own road surface, so no height detail
/// is needed beyond the rough position.
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct TrafficPose {
    /// Stable while the car exists in its owner's game.
    pub id: u32,
    /// Index in the session's car catalog.
    pub model: u16,
    pub position: [i32; 3],
    pub yaw: i16,
    pub speed: i16,
    /// A police car with its siren and lights on.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub siren: bool,
}
impl TrafficPose {
    pub fn new(id: u32, model: u16, position: [f32; 3], yaw: f32, speed: f32) -> Self {
        let yaw = yaw.sin().atan2(yaw.cos());
        Self {
            id,
            model,
            position: position.map(|v| (v.clamp(-20000.0, 20000.0) * 100.0).round() as i32),
            yaw: (yaw * 1000.0).round() as i16,
            speed: (speed.clamp(-200.0, 200.0) * 10.0).round() as i16,
            siren: false,
        }
    }
    pub fn position(&self) -> [f32; 3] {
        self.position.map(|v| v as f32 / 100.0)
    }
    pub fn yaw(&self) -> f32 {
        f32::from(self.yaw) / 1000.0
    }
    pub fn speed(&self) -> f32 {
        f32::from(self.speed) / 10.0
    }
    fn valid(&self) -> bool {
        self.model < 256
            && self.position.iter().all(|v| v.abs() <= 2_000_000)
            && self.yaw.abs() <= 3150
            && self.speed.abs() <= 2000
    }
}
fn valid_traffic(cars: &[TrafficPose]) -> bool {
    cars.len() <= MAX_TRAFFIC && cars.iter().all(TrafficPose::valid)
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Peer {
    pub id: u32,
    pub name: String,
    pub pose: Pose,
}
/// One script event. On a server, `source` is the sending player's ID; on a
/// client, events always come from the server (`source == 0`).
#[derive(Clone, Debug, PartialEq)]
pub struct NetEvent {
    pub source: u32,
    pub name: String,
    pub payload: String,
}
static IDENTITY_SECRET: std::sync::OnceLock<String> = std::sync::OnceLock::new();
/// The player's secret key for every session this process joins. Servers
/// never see it: they get a signature from a key derived from it for that
/// server only (see [`identity`]).
pub fn set_identity_secret(secret: &str) {
    let _ = IDENTITY_SECRET.set(secret.to_string());
}
/// The unsigned identity of protocol 8 and earlier: a hash of the secret and
/// the server's salt. Anyone who learns it can repeat it, so it is only sent
/// as a claim for migrating old characters, never used as a license.
fn legacy_identity(secret: &str, salt: &str) -> String {
    resources::hash(format!("sare-identity\n{secret}\n{salt}").as_bytes())
}
/// A player's identity on a host, after the signature check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerIdentity {
    /// The player's public key for this server (64 hex digits).
    pub license: String,
    /// The protocol 8 identity the player's game computed for this server,
    /// when the server still has its `identity.salt`. A claim, not a proof.
    pub legacy: Option<String>,
}
/// Sent with `Welcome` when the host has a server key.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Challenge {
    server_key: String,
    nonce: String,
    /// Signature over both nonces and the player ID (identity.rs).
    proof: String,
    /// Old servers' salt, so the game can name its old identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    legacy_salt: Option<String>,
}

pub fn valid_event(name: &str, payload: &str) -> bool {
    !name.is_empty()
        && name.len() <= MAX_EVENT_NAME
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_:-.".contains(&b))
        && payload.len() <= MAX_EVENT_PAYLOAD
        && serde_json::from_str::<Vec<serde_json::Value>>(payload).is_ok()
}
enum Outbound {
    Event {
        target: Option<u32>,
        name: String,
        payload: String,
    },
    Drop {
        target: u32,
        reason: String,
    },
}
#[derive(Clone, Debug, Default)]
pub struct Report {
    /// Round trip through the gameplay connection, including relay and host processing.
    pub round_trip: Option<Duration>,
    pub status: String,
    pub connected: bool,
    pub local_id: u32,
    pub peers: Vec<Peer>,
    pub revision: u64,
}
#[derive(Serialize, Deserialize)]
enum Message {
    Ping(u64),
    Pong(u64),
    Hello {
        version: u32,
        name: String,
        #[serde(default)]
        resources: Option<String>,
        /// Client nonce for the identity check; absent before protocol 8.1.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        nonce: Option<String>,
        /// The game shares traffic. Older games leave it out and never
        /// receive traffic messages.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        traffic: bool,
    },
    Welcome {
        id: u32,
        /// Protocol 8 salt. Always empty now, so older games send no
        /// unsigned identity.
        #[serde(default)]
        salt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        challenge: Option<Challenge>,
        /// The host relays traffic; games only send theirs when it does.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        traffic: bool,
    },
    /// Older games: an unsigned identity hash. Ignored.
    Identity(String),
    /// Clients: the signed answer to `Welcome`'s challenge.
    Auth {
        key: String,
        signature: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        legacy: Option<String>,
    },
    Pose(Pose),
    Snapshot(Vec<Peer>),
    Reject(String),
    ResourceQuery {
        version: u32,
    },
    ResourceManifest(resources::Manifest),
    ResourceFile {
        sha256: String,
    },
    ResourceChunk {
        sha256: String,
        offset: usize,
        data: Vec<u8>,
        done: bool,
    },
    Event {
        name: String,
        payload: String,
    },
    /// Client -> host during gameplay: send this published script bundle as
    /// `ResourceChunk`s. Only clients told a hash by a script event ask.
    ScriptFile {
        sha256: String,
    },
    /// Client -> host: the traffic this game drives right now.
    Traffic(Vec<TrafficPose>),
    /// Host -> client: another player's traffic near this one.
    TrafficOf {
        owner: u32,
        cars: Vec<TrafficPose>,
    },
}
fn safe_name(name: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).take(24).collect();
    if name.trim().is_empty() {
        "Player".into()
    } else {
        name
    }
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid multiplayer packet")
}

struct Wire {
    stream: TcpStream,
    incoming: Vec<u8>,
    outgoing: Vec<u8>,
    sent: usize,
    last: Instant,
    closed: bool,
}
impl Drop for Wire {
    // Closing a socket with unread input makes Windows send a reset, which
    // can discard a Reject the peer has not read yet. Close the sending side
    // and drain the input briefly so the last message arrives.
    fn drop(&mut self) {
        if self.closed || self.stream.shutdown(std::net::Shutdown::Write).is_err() {
            return;
        }
        let Ok(mut stream) = self.stream.try_clone() else {
            return;
        };
        let _ = thread::Builder::new()
            .name("wire-close".into())
            .spawn(move || {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
                let deadline = Instant::now() + Duration::from_secs(2);
                let mut buffer = [0_u8; 4096];
                while Instant::now() < deadline {
                    match stream.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(_) => {}
                        Err(e)
                            if matches!(
                                e.kind(),
                                io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                            ) => {}
                        Err(_) => break,
                    }
                }
            });
    }
}
impl Wire {
    fn new(stream: TcpStream) -> io::Result<Self> {
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;
        Ok(Self {
            stream,
            incoming: Vec::new(),
            outgoing: Vec::new(),
            sent: 0,
            last: Instant::now(),
            closed: false,
        })
    }
    fn queue(&mut self, message: &Message) -> io::Result<()> {
        let data = serde_json::to_vec(message).map_err(|_| invalid())?;
        if data.len() > MAX_FRAME
            || self.outgoing.len() - self.sent + data.len() + 4 > MAX_FRAME * 4
        {
            return Err(invalid());
        }
        self.outgoing.drain(..self.sent);
        self.sent = 0;
        self.outgoing.extend((data.len() as u32).to_le_bytes());
        self.outgoing.extend(data);
        Ok(())
    }
    fn flush(&mut self) -> io::Result<()> {
        // Bounded work per peer, including partial writes.
        for _ in 0..4 {
            if self.sent == self.outgoing.len() {
                break;
            }
            match self.stream.write(&self.outgoing[self.sent..]) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => self.sent += n,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    fn read(&mut self) -> io::Result<Vec<Message>> {
        if self.closed {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let mut buffer = [0; 4096];
        let mut messages = Vec::new();
        for _ in 0..4 {
            match self.stream.read(&mut buffer) {
                Ok(0) => {
                    self.closed = true;
                    break;
                }
                Ok(n) => self.incoming.extend(&buffer[..n]),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                Err(e) => return Err(e),
            }
        }
        while self.incoming.len() >= 4 && messages.len() < 32 {
            let len = u32::from_le_bytes(self.incoming[..4].try_into().unwrap()) as usize;
            if len == 0 || len > MAX_FRAME {
                return Err(invalid());
            }
            if self.incoming.len() < len + 4 {
                break;
            }
            messages
                .push(serde_json::from_slice(&self.incoming[4..len + 4]).map_err(|_| invalid())?);
            self.incoming.drain(..len + 4);
            self.last = Instant::now();
        }
        if self.incoming.len() > MAX_FRAME + 4 {
            return Err(invalid());
        }
        Ok(messages)
    }
}

pub struct Session {
    pub address: SocketAddr,
    stop: Arc<AtomicBool>,
    local: Arc<Mutex<Pose>>,
    report: Arc<Mutex<Report>>,
    mail: Arc<Mailbox>,
    script_keys: Mutex<HashMap<String, String>>,
}
#[derive(Default)]
struct Mailbox {
    outbox: Mutex<Vec<Outbound>>,
    inbox: Mutex<VecDeque<NetEvent>>,
    /// Hosts only: admitted players, including a player host. 0 = MAX_PLAYERS.
    max_clients: std::sync::atomic::AtomicUsize,
    /// Hosts: published client script bundles by SHA-256.
    scripts: Mutex<HashMap<String, Arc<[u8]>>>,
    /// Clients: bundles to request, and verified bundles received.
    script_requests: Mutex<Vec<String>>,
    script_files: Mutex<Vec<(String, Vec<u8>)>>,
    /// Hosts: the server key, the old salt for migration claims, and
    /// joined players' checked identities.
    server_key: Mutex<Option<Arc<identity::ServerKey>>>,
    identity_salt: Mutex<String>,
    identities: Mutex<HashMap<u32, PlayerIdentity>>,
    /// This game's own traffic, and other players' traffic by owner.
    traffic_out: Mutex<Vec<TrafficPose>>,
    traffic_in: Mutex<HashMap<u32, (Instant, Vec<TrafficPose>)>>,
}
impl Mailbox {
    fn receive(&self, event: NetEvent) {
        let mut inbox = self.inbox.lock().unwrap();
        if inbox.len() < MAX_INBOX {
            inbox.push_back(event);
        }
    }
    fn take(&self) -> Vec<Outbound> {
        std::mem::take(&mut *self.outbox.lock().unwrap())
    }
    fn capacity(&self) -> usize {
        match self.max_clients.load(Ordering::Relaxed) {
            0 => MAX_PLAYERS,
            n => n.min(MAX_PLAYERS),
        }
    }
}
impl Session {
    pub fn host(address: SocketAddr, name: &str) -> io::Result<Self> {
        Self::host_resources(address, name, resources::Share::default())
    }
    pub fn host_resources(
        address: SocketAddr,
        name: &str,
        share: resources::Share,
    ) -> io::Result<Self> {
        Self::host_resources_kind(address, name, share, true)
    }
    /// Headless membership host: all twenty slots belong to actual clients.
    pub fn dedicated_resources(
        address: SocketAddr,
        name: &str,
        share: resources::Share,
    ) -> io::Result<Self> {
        Self::host_resources_kind(address, name, share, false)
    }
    fn host_resources_kind(
        address: SocketAddr,
        name: &str,
        share: resources::Share,
        host_player: bool,
    ) -> io::Result<Self> {
        share.manifest.validate()?;
        let listener = TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        let session = Self::new(listener.local_addr()?, "Starting host");
        let (stop, local, report) = session.shared();
        let mail = session.mail.clone();
        let name = safe_name(name);
        thread::Builder::new()
            .name("multiplayer-host".into())
            .spawn(move || {
                host_worker(
                    listener,
                    name,
                    stop,
                    local,
                    report,
                    share,
                    host_player,
                    mail,
                );
            })?;
        Ok(session)
    }
    pub fn join(address: SocketAddr, name: &str) -> io::Result<Self> {
        Self::join_resources(address, name, None)
    }
    pub fn join_resources(
        address: SocketAddr,
        name: &str,
        fingerprint: Option<String>,
    ) -> io::Result<Self> {
        let session = Self::new(address, "Connecting...");
        let (stop, local, report) = session.shared();
        let mail = session.mail.clone();
        let name = safe_name(name);
        thread::Builder::new()
            .name("multiplayer-client".into())
            .spawn(move || {
                let result = (|| {
                    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
                    client_stream(
                        stream,
                        address,
                        name,
                        &stop,
                        &local,
                        &report,
                        fingerprint,
                        &mail,
                    )
                })();
                let status = match result {
                    Ok(()) => "Disconnected".into(),
                    Err(e)
                        if matches!(
                            e.kind(),
                            io::ErrorKind::UnexpectedEof
                                | io::ErrorKind::ConnectionReset
                                | io::ErrorKind::ConnectionAborted
                        ) =>
                    {
                        "Host disconnected".into()
                    }
                    Err(e) => format!("Connection ended: {e}"),
                };
                publish(&report, &status, false, 0, Vec::new());
            })?;
        Ok(session)
    }
    fn new(address: SocketAddr, status: &str) -> Self {
        Self {
            address,
            stop: Arc::new(AtomicBool::new(false)),
            local: Arc::new(Mutex::new(Pose::default())),
            report: Arc::new(Mutex::new(Report {
                status: status.into(),
                ..Report::default()
            })),
            mail: Arc::default(),
            script_keys: Mutex::default(),
        }
    }
    #[allow(clippy::type_complexity)]
    fn shared(&self) -> (Arc<AtomicBool>, Arc<Mutex<Pose>>, Arc<Mutex<Report>>) {
        (self.stop.clone(), self.local.clone(), self.report.clone())
    }
    /// Share the traffic this game drives with nearby players.
    pub fn set_traffic(&self, mut cars: Vec<TrafficPose>) {
        cars.truncate(MAX_TRAFFIC);
        *self.mail.traffic_out.lock().unwrap() = cars;
    }
    /// Other players' traffic near this player, by owner.
    pub fn traffic(&self) -> Vec<(u32, Vec<TrafficPose>)> {
        let mut incoming = self.mail.traffic_in.lock().unwrap();
        incoming.retain(|_, (at, _)| at.elapsed() < TRAFFIC_STALE);
        incoming
            .iter()
            .map(|(owner, (_, cars))| (*owner, cars.clone()))
            .collect()
    }
    /// Latest-only mailbox: a slow renderer cannot grow a network queue.
    /// Queue a script event. Hosts send to one player (`Some(id)`) or all
    /// (`None`); clients always send to the server and ignore `target`.
    pub fn trigger(&self, target: Option<u32>, name: &str, payload: &str) -> io::Result<()> {
        if !valid_event(name, payload) {
            return Err(invalid());
        }
        let mut outbox = self.mail.outbox.lock().unwrap();
        if outbox.len() >= MAX_INBOX {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        outbox.push(Outbound::Event {
            target,
            name: name.into(),
            payload: payload.into(),
        });
        Ok(())
    }
    /// Received script events, oldest first.
    pub fn events(&self) -> Vec<NetEvent> {
        self.mail.inbox.lock().unwrap().drain(..).collect()
    }
    /// Hosts only: disconnect a player with a reason shown to them.
    pub fn drop_player(&self, id: u32, reason: &str) {
        self.mail.outbox.lock().unwrap().push(Outbound::Drop {
            target: id,
            reason: reason
                .chars()
                .filter(|c| !c.is_control())
                .take(256)
                .collect(),
        });
    }
    /// Hosts only: serve `data` to clients that request its SHA-256, which is
    /// returned. Publishing again under the same `key` replaces the old bundle.
    pub fn publish_script(&self, key: &str, data: Vec<u8>) -> io::Result<String> {
        if data.len() > MAX_SCRIPT_BYTES {
            return Err(io::Error::other("Client script bundle is too large"));
        }
        let sha256 = resources::hash(&data);
        let mut keys = self.script_keys.lock().unwrap();
        let mut scripts = self.mail.scripts.lock().unwrap();
        if let Some(old) = keys.insert(key.to_string(), sha256.clone()) {
            if old != sha256 && !keys.values().any(|v| v == &old) {
                scripts.remove(&old);
            }
        }
        scripts.insert(sha256.clone(), data.into());
        Ok(sha256)
    }
    /// Hosts only: stop serving the bundle published under `key`.
    pub fn unpublish_script(&self, key: &str) {
        let mut keys = self.script_keys.lock().unwrap();
        if let Some(old) = keys.remove(key) {
            if !keys.values().any(|v| v == &old) {
                self.mail.scripts.lock().unwrap().remove(&old);
            }
        }
    }
    /// Clients only: ask the host for a published bundle.
    pub fn request_script(&self, sha256: &str) {
        let mut requests = self.mail.script_requests.lock().unwrap();
        if requests.len() < MAX_SCRIPT_REQUESTS {
            requests.push(sha256.to_string());
        }
    }
    /// Clients only: bundles received and verified against their SHA-256.
    pub fn take_scripts(&self) -> Vec<(String, Vec<u8>)> {
        std::mem::take(&mut *self.mail.script_files.lock().unwrap())
    }
    /// Hosts only: the key that players sign their identity for. Set it
    /// before players join; without one, players have no license.
    pub fn set_server_key(&self, key: identity::ServerKey) {
        *self.mail.server_key.lock().unwrap() = Some(Arc::new(key));
    }
    /// Hosts only: the salt of protocol 8 identities. With one, players'
    /// games also report their old identity as a migration claim.
    pub fn set_identity_salt(&self, salt: &str) {
        *self.mail.identity_salt.lock().unwrap() =
            salt.chars().filter(|c| !c.is_control()).take(128).collect();
    }
    /// Hosts only: a joined player's license (their signature-checked
    /// public key for this server, 64 hex digits).
    pub fn identity(&self, id: u32) -> Option<String> {
        self.player_identity(id).map(|i| i.license)
    }
    /// Hosts only: the license and any old-identity claim.
    pub fn player_identity(&self, id: u32) -> Option<PlayerIdentity> {
        self.mail.identities.lock().unwrap().get(&id).cloned()
    }
    /// Hosts only: admitted player limit (1..=MAX_PLAYERS), like `sv_maxclients`.
    pub fn set_max_clients(&self, count: usize) {
        self.mail
            .max_clients
            .store(count.clamp(1, MAX_PLAYERS), Ordering::Relaxed);
    }
    pub fn max_clients(&self) -> usize {
        self.mail.capacity()
    }
    pub fn update(&self, pose: Pose) -> Option<Report> {
        if pose.valid() {
            if let Ok(mut local) = self.local.try_lock() {
                *local = pose;
            }
        }
        self.report.try_lock().ok().map(|r| r.clone())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
fn publish(report: &Mutex<Report>, status: &str, connected: bool, id: u32, peers: Vec<Peer>) {
    let mut r = report.lock().unwrap();
    r.status = status.into();
    r.connected = connected;
    if !connected {
        r.round_trip = None;
    }
    r.local_id = id;
    r.peers = peers;
    r.revision = r.revision.wrapping_add(1);
}
struct Guest {
    wire: Wire,
    peer: Peer,
    ready: bool,
    hello: bool,
    assets: bool,
    transfer: Option<(String, Arc<[u8]>, usize)>,
    scripts: VecDeque<String>,
    budget: (Instant, u32),
    /// (client nonce, server nonce) while the identity check is open.
    challenge: Option<(String, String)>,
    identity: Option<PlayerIdentity>,
    /// Shares traffic, and the traffic it last sent.
    traffic_ok: bool,
    traffic: Vec<TrafficPose>,
}
#[allow(clippy::too_many_arguments)]
fn host_worker(
    listener: TcpListener,
    name: String,
    stop: Arc<AtomicBool>,
    local: Arc<Mutex<Pose>>,
    report: Arc<Mutex<Report>>,
    share: resources::Share,
    host_player: bool,
    mail: Arc<Mailbox>,
) {
    let fingerprint = share
        .manifest
        .fingerprint()
        .expect("validated host inventory");
    let mut guests: Vec<Guest> = Vec::new();
    let mut traffic_tick = false;
    let mut seats = passengers::SeatBook::default();
    let mut next_id = 1_u32;
    let mut next_tick = Instant::now();
    let status = format!("Hosting {}", listener.local_addr().unwrap());
    while !stop.load(Ordering::Relaxed) {
        for _ in 0..MAX_PLAYERS {
            let Ok((stream, _)) = listener.accept() else {
                break;
            };
            let Ok(mut wire) = Wire::new(stream) else {
                continue;
            };
            if guests.len() >= MAX_PLAYERS * 2 {
                let _ = wire.queue(&Message::Reject("Session full (20 players)".into()));
                let _ = wire.flush();
                continue;
            }
            guests.push(Guest {
                wire,
                peer: Peer {
                    id: next_id,
                    name: String::new(),
                    pose: Pose::default(),
                },
                ready: false,
                hello: false,
                assets: false,
                transfer: None,
                scripts: VecDeque::new(),
                budget: (Instant::now(), 0),
                challenge: None,
                identity: None,
                traffic_ok: false,
                traffic: Vec::new(),
            });
            next_id = next_id.wrapping_add(1).max(1);
        }
        let mut source = Vec::new();
        if host_player {
            source.push(Peer {
                id: 0,
                name: name.clone(),
                pose: *local.lock().unwrap(),
            });
        }
        source.extend(guests.iter().filter(|g| g.ready).map(|g| g.peer.clone()));
        seats.reconcile(&source);
        let mut admitted = guests.iter().filter(|g| g.hello).count();
        guests.retain_mut(|guest| {
            if guest.wire.last.elapsed() > TIMEOUT {
                return false;
            }
            let Ok(messages) = guest.wire.read() else {
                return false;
            };
            for message in messages {
                match message {
                    Message::ResourceQuery { version } if !guest.hello && !guest.assets && version == VERSION => {
                        guest.assets = true;
                        if guest.wire.queue(&Message::ResourceManifest(share.manifest.clone())).is_err() { return false; }
                    }
                    Message::ResourceQuery { .. } if !guest.hello && !guest.assets => {
                        let _=guest.wire.queue(&Message::Reject("Incompatible multiplayer version. Update client and host to the same build.".into()));let _=guest.wire.flush();return false;
                    }
                    Message::ScriptFile { sha256 } if guest.hello => {
                        if guest.scripts.len() >= MAX_SCRIPT_REQUESTS { return false; }
                        guest.scripts.push_back(sha256);
                    }
                    Message::ResourceFile { sha256 } if guest.assets && guest.transfer.is_none() => {
                        let Some(data) = share.blob(&sha256) else { return false; };
                        guest.transfer = Some((sha256, data, 0));
                    }
                    Message::Hello { version, name, resources, nonce, traffic } if !guest.hello && !guest.assets => {
                        if resources.as_ref().is_some_and(|value| value != &fingerprint)
                            || (!share.manifest.resources.is_empty() && resources.as_ref() != Some(&fingerprint)) {
                            let _ = guest.wire.queue(&Message::Reject("Server resource versions do not match. Prepare server mods before joining.".into()));
                            let _ = guest.wire.flush();
                            return false;
                        }
                        if version != VERSION {
                            let _ = guest
                                .wire
                                .queue(&Message::Reject("Incompatible multiplayer version".into()));
                            let _ = guest.wire.flush();
                            return false;
                        }
                        let capacity = mail.capacity();
                        if admitted >= capacity.saturating_sub(usize::from(host_player)) {
                            let _ = guest
                                .wire
                                .queue(&Message::Reject(format!("Session full ({capacity} players)")));
                            let _ = guest.wire.flush();
                            return false;
                        }
                        admitted += 1;
                        guest.peer.name = safe_name(&name);
                        guest.hello = true;
                        guest.traffic_ok = traffic;
                        let key = mail.server_key.lock().unwrap().clone();
                        let challenge = match (key, nonce) {
                            (Some(key), Some(client)) if identity::valid_hex32(&client) => {
                                let client = client.to_ascii_lowercase();
                                let server = identity::nonce();
                                let proof = identity::sign_welcome(&key, &client, &server, guest.peer.id);
                                let salt = mail.identity_salt.lock().unwrap().clone();
                                guest.challenge = Some((client, server.clone()));
                                Some(Challenge {
                                    server_key: key.public_hex(),
                                    nonce: server,
                                    proof,
                                    legacy_salt: (!salt.is_empty()).then_some(salt),
                                })
                            }
                            _ => None,
                        };
                        if guest
                            .wire
                            .queue(&Message::Welcome {
                                id: guest.peer.id,
                                salt: String::new(),
                                challenge,
                                traffic: true,
                            })
                            .is_err()
                        {
                            return false;
                        }
                    }
                    // Older games' unsigned identity: no license.
                    Message::Identity(_) if guest.hello => {}
                    Message::Auth { key, signature, legacy } if guest.hello && guest.identity.is_none() => {
                        let Some((client, server)) = guest.challenge.take() else { return false; };
                        let Some(server_key) = mail.server_key.lock().unwrap().as_ref().map(|k| k.public_hex()) else { return false; };
                        let key = key.to_ascii_lowercase();
                        let legacy = legacy.map(|l| l.to_ascii_lowercase());
                        let legacy_ok = legacy.as_deref().is_none_or(identity::valid_hex32)
                            && (legacy.is_none() || !mail.identity_salt.lock().unwrap().is_empty());
                        if !legacy_ok || !identity::verify_auth(&server_key, &client, &server, guest.peer.id, &key, legacy.as_deref(), &signature) {
                            let _ = guest.wire.queue(&Message::Reject("Player identity check failed.".into()));
                            let _ = guest.wire.flush();
                            return false;
                        }
                        guest.identity = Some(PlayerIdentity { license: key, legacy });
                    }
                    Message::Ping(nonce) if guest.hello => {
                        if guest.wire.queue(&Message::Pong(nonce)).is_err(){return false;}
                    }
                    Message::Traffic(cars) if guest.hello && guest.traffic_ok && valid_traffic(&cars) => {
                        guest.traffic = cars;
                    }
                    Message::Pose(pose) if guest.hello && pose.valid() => {
                        guest.peer.pose = seats.apply(guest.peer.id, pose, &source);
                        guest.ready = true;
                    }
                    Message::Event { name, payload } if guest.hello && valid_event(&name, &payload) => {
                        if guest.budget.0.elapsed() >= Duration::from_secs(1) {
                            guest.budget = (Instant::now(), 0);
                        }
                        guest.budget.1 += 1;
                        if guest.budget.1 <= EVENT_BUDGET {
                            mail.receive(NetEvent { source: guest.peer.id, name, payload });
                        }
                    }
                    _ => return false,
                }
            }
            if guest.transfer.is_none() {
                if let Some(sha256) = guest.scripts.pop_front() {
                    // An unknown or replaced bundle ends at once, so the
                    // client moves on to its next request.
                    let data = mail.scripts.lock().unwrap().get(&sha256).cloned();
                    guest.transfer = Some((sha256, data.unwrap_or_else(|| Arc::from([])), 0));
                }
            }
            if let Some((sha256, data, offset)) = &mut guest.transfer {
                // One bounded chunk per iteration, alongside ordinary gameplay.
                if guest.wire.outgoing.len() - guest.wire.sent < MAX_FRAME {
                    let end = (*offset + 2048).min(data.len());
                    let done = end == data.len();
                    if guest.wire.queue(&Message::ResourceChunk { sha256: sha256.clone(), offset: *offset, data: data[*offset..end].to_vec(), done }).is_err() { return false; }
                    *offset = end;
                    guest.wire.last = Instant::now();
                    if done { guest.transfer = None; }
                }
            }
            guest.wire.flush().is_ok()
        });
        for outbound in mail.take() {
            match outbound {
                Outbound::Event {
                    target,
                    name,
                    payload,
                } => {
                    let message = Message::Event { name, payload };
                    for guest in guests
                        .iter_mut()
                        .filter(|g| g.hello && target.is_none_or(|id| id == g.peer.id))
                    {
                        // A full queue drops the event; it is not a protocol error.
                        let _ = guest.wire.queue(&message);
                    }
                }
                Outbound::Drop { target, reason } => {
                    guests.retain_mut(|guest| {
                        if guest.peer.id != target {
                            return true;
                        }
                        let _ = guest.wire.queue(&Message::Reject(reason.clone()));
                        let _ = guest.wire.flush();
                        false
                    });
                }
            }
        }
        if Instant::now() >= next_tick {
            next_tick = Instant::now() + TICK;
            let mut peers = Vec::new();
            if host_player {
                peers.push(Peer {
                    id: 0,
                    name: name.clone(),
                    pose: *local.lock().unwrap(),
                });
            }
            peers.extend(guests.iter().filter(|g| g.ready).map(|g| g.peer.clone()));
            seats.reconcile(&peers);
            let source = peers.clone();
            for peer in &mut peers {
                peer.pose = if host_player && peer.id == 0 {
                    seats.apply(peer.id, peer.pose, &source)
                } else {
                    seats.decorate(peer.id, peer.pose, &source)
                };
            }
            *mail.identities.lock().unwrap() = guests
                .iter()
                .filter_map(|g| g.identity.clone().map(|i| (g.peer.id, i)))
                .collect();
            publish(&report, &status, true, 0, peers.clone());
            let snapshot = Message::Snapshot(peers);
            guests.retain_mut(|guest| {
                !guest.ready || (guest.wire.queue(&snapshot).is_ok() && guest.wire.flush().is_ok())
            });
            traffic_tick = !traffic_tick;
            if traffic_tick {
                relay_traffic(&mut guests, host_player, &local, &mail);
            }
        }
        thread::sleep(Duration::from_millis(5));
    }
    publish(&report, "Disconnected", false, 0, Vec::new());
}
/// Pass every player's traffic to the nearby players, at half the pose rate.
fn relay_traffic(guests: &mut [Guest], host_player: bool, local: &Mutex<Pose>, mail: &Mailbox) {
    let near = |a: [f32; 3], b: [f32; 3]| {
        let (dx, dz) = (a[0] - b[0], a[2] - b[2]);
        dx * dx + dz * dz <= TRAFFIC_RANGE * TRAFFIC_RANGE
    };
    let host_pose = host_player.then(|| local.lock().unwrap().position);
    let mut owners: Vec<(u32, [f32; 3], Vec<TrafficPose>)> = guests
        .iter()
        .filter(|g| g.ready && !g.traffic.is_empty())
        .map(|g| (g.peer.id, g.peer.pose.position, g.traffic.clone()))
        .collect();
    if let Some(at) = host_pose {
        let cars = mail.traffic_out.lock().unwrap().clone();
        if !cars.is_empty() {
            owners.push((0, at, cars));
        }
    }
    for (owner, at, cars) in owners {
        if let Some(host) = host_pose.filter(|_| owner != 0) {
            if near(at, host) {
                mail.traffic_in
                    .lock()
                    .unwrap()
                    .insert(owner, (Instant::now(), cars.clone()));
            }
        }
        let message = Message::TrafficOf { owner, cars };
        for guest in guests.iter_mut().filter(|g| {
            g.ready && g.traffic_ok && g.peer.id != owner && near(at, g.peer.pose.position)
        }) {
            // A full queue drops this update; the next one follows shortly.
            let _ = guest.wire.queue(&message);
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn client_stream(
    stream: TcpStream,
    address: SocketAddr,
    name: String,
    stop: &AtomicBool,
    local: &Mutex<Pose>,
    report: &Mutex<Report>,
    fingerprint: Option<String>,
    mail: &Mailbox,
) -> io::Result<()> {
    let mut wire = Wire::new(stream)?;
    // Without a player secret there is nothing to sign: no identity check.
    let client_nonce = IDENTITY_SECRET.get().map(|_| identity::nonce());
    wire.queue(&Message::Hello {
        version: VERSION,
        name,
        resources: fingerprint,
        nonce: client_nonce.clone(),
        traffic: true,
    })?;
    let mut id = None;
    let mut host_traffic = false;
    let mut traffic_tick = false;
    let mut next_tick = Instant::now();
    let mut next_ping = Instant::now();
    let mut ping: Option<(u64, Instant)> = None;
    let mut nonce = 0u64;
    // Script bundles: requested hashes in order, and the one being received.
    let mut wanted: VecDeque<String> = VecDeque::new();
    let mut receiving: Vec<u8> = Vec::new();
    while !stop.load(Ordering::Relaxed) {
        if wire.last.elapsed() > TIMEOUT {
            return Err(io::ErrorKind::TimedOut.into());
        }
        for message in wire.read()? {
            match message {
                Message::Welcome {
                    id: assigned,
                    challenge,
                    traffic,
                    ..
                } if id.is_none() && assigned != 0 => {
                    id = Some(assigned);
                    host_traffic = traffic;
                    if let (Some(secret), Some(client), Some(challenge)) =
                        (IDENTITY_SECRET.get(), &client_nonce, challenge)
                    {
                        // The server must prove it holds the key it named
                        // before the game signs anything for that key.
                        if !identity::verify_welcome(
                            &challenge.server_key,
                            client,
                            &challenge.nonce,
                            assigned,
                            &challenge.proof,
                        ) {
                            return Err(io::Error::other("The server failed its identity check."));
                        }
                        let legacy = challenge
                            .legacy_salt
                            .as_deref()
                            .map(|salt| legacy_identity(secret, salt));
                        let (key, signature) = identity::sign_auth(
                            secret,
                            &challenge.server_key,
                            client,
                            &challenge.nonce,
                            assigned,
                            legacy.as_deref(),
                        );
                        wire.queue(&Message::Auth {
                            key,
                            signature,
                            legacy,
                        })?;
                    }
                }
                Message::Snapshot(peers) if id.is_some() => {
                    let local_id = id.unwrap();
                    if peers.len() > MAX_PLAYERS
                        || peers.is_empty()
                        || !peers.iter().any(|p| p.id == local_id)
                        || peers.iter().any(|p| !p.pose.valid() || p.name.len() > 96)
                        || peers
                            .iter()
                            .enumerate()
                            .any(|(i, p)| peers[..i].iter().any(|other| other.id == p.id))
                    {
                        return Err(invalid());
                    }
                    publish(
                        report,
                        &format!("Connected to {address}"),
                        true,
                        local_id,
                        peers,
                    );
                }
                Message::TrafficOf { owner, cars } if id.is_some() && host_traffic => {
                    if !valid_traffic(&cars) || Some(owner) == id {
                        return Err(invalid());
                    }
                    mail.traffic_in
                        .lock()
                        .unwrap()
                        .insert(owner, (Instant::now(), cars));
                }
                Message::Pong(reply) if id.is_some() => {
                    if let Some((expected, sent)) = ping {
                        if expected == reply {
                            report.lock().unwrap().round_trip = Some(sent.elapsed());
                            ping = None;
                        } else {
                            return Err(invalid());
                        }
                    } else {
                        return Err(invalid());
                    }
                }
                Message::Event { name, payload } if id.is_some() => {
                    if !valid_event(&name, &payload) {
                        return Err(invalid());
                    }
                    mail.receive(NetEvent {
                        source: 0,
                        name,
                        payload,
                    });
                }
                Message::ResourceChunk {
                    sha256,
                    offset,
                    data,
                    done,
                } if id.is_some() && wanted.front() == Some(&sha256) => {
                    if offset != receiving.len()
                        || data.len() > 2048
                        || receiving.len() + data.len() > MAX_SCRIPT_BYTES
                        || (data.is_empty() && !done)
                    {
                        return Err(invalid());
                    }
                    receiving.extend(data);
                    if done {
                        wanted.pop_front();
                        let bytes = std::mem::take(&mut receiving);
                        if resources::hash(&bytes) == sha256 {
                            mail.script_files.lock().unwrap().push((sha256, bytes));
                        }
                    }
                }
                Message::Reject(reason) => return Err(io::Error::other(reason)),
                _ => return Err(invalid()),
            }
        }
        if id.is_some() {
            for sha256 in std::mem::take(&mut *mail.script_requests.lock().unwrap()) {
                if wanted.len() < MAX_SCRIPT_REQUESTS
                    && !wanted.contains(&sha256)
                    && wire
                        .queue(&Message::ScriptFile {
                            sha256: sha256.clone(),
                        })
                        .is_ok()
                {
                    wanted.push_back(sha256);
                }
            }
            for outbound in mail.take() {
                if let Outbound::Event { name, payload, .. } = outbound {
                    let _ = wire.queue(&Message::Event { name, payload });
                }
            }
        }
        if id.is_some() && Instant::now() >= next_tick {
            next_tick = Instant::now() + TICK;
            wire.queue(&Message::Pose(*local.lock().unwrap()))?;
            traffic_tick = !traffic_tick;
            if host_traffic && traffic_tick {
                let cars = mail.traffic_out.lock().unwrap().clone();
                wire.queue(&Message::Traffic(cars))?;
            }
        }
        if id.is_some() && ping.is_none() && Instant::now() >= next_ping {
            nonce = nonce.wrapping_add(1);
            ping = Some((nonce, Instant::now()));
            next_ping = Instant::now() + Duration::from_secs(1);
            wire.queue(&Message::Ping(nonce))?;
        }
        wire.flush()?;
        thread::sleep(Duration::from_millis(5));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn traffic_reaches_nearby_players_and_skips_older_games() {
        let host = Session::host(address(), "Host").unwrap();
        let a = Session::join(host.address, "A").unwrap();
        let b = Session::join(host.address, "B").unwrap();
        // An older game that never announced traffic support.
        let (mut old, _, _) = handshake(&host, None);
        old.queue(&Message::Pose(Pose::default())).unwrap();
        old.flush().unwrap();
        let car = TrafficPose::new(7, 12, [10.0, 2.5, -40.25], 3.0, 13.4);
        assert_eq!(car.position(), [10.0, 2.5, -40.25]);
        assert!((car.yaw() - 3.0).abs() < 0.001 && (car.speed() - 13.4).abs() < 0.01);
        a.set_traffic(vec![car]);
        host.set_traffic(vec![TrafficPose::new(1, 3, [0.0; 3], 0.0, 0.0)]);
        let far = Pose {
            position: [5000.0, 0.0, 5000.0],
            ..Pose::default()
        };
        let deadline = Instant::now() + Duration::from_secs(30);
        let (mut b_saw_a, mut b_saw_host, mut host_saw_a, mut a_saw_host) =
            (false, false, false, false);
        while !(b_saw_a && b_saw_host && host_saw_a && a_saw_host) {
            assert!(Instant::now() < deadline, "traffic never arrived");
            for session in [&host, &a, &b] {
                session.update(Pose::default());
            }
            let ids = |s: &Session| {
                s.traffic()
                    .into_iter()
                    .map(|(owner, cars)| (owner, cars.len()))
                    .collect::<Vec<_>>()
            };
            let (seen_b, seen_host, seen_a) = (ids(&b), ids(&host), ids(&a));
            let a_id = a.report.lock().unwrap().local_id;
            b_saw_a |= seen_b.contains(&(a_id, 1));
            b_saw_host |= seen_b.contains(&(0, 1));
            host_saw_a |= seen_host.contains(&(a_id, 1));
            a_saw_host |= seen_a.contains(&(0, 1));
            assert!(
                seen_a.iter().all(|(owner, _)| *owner != a_id),
                "own traffic echoed"
            );
            thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(b.traffic().iter().find(|(o, _)| *o != 0).unwrap().1[0], car);
        // The older game got no traffic messages and is still connected.
        let mut messages = Vec::new();
        let until = Instant::now() + Duration::from_millis(300);
        while Instant::now() < until {
            old.queue(&Message::Pose(Pose::default())).unwrap();
            old.flush().unwrap();
            messages.extend(old.read().unwrap());
            thread::sleep(Duration::from_millis(20));
        }
        assert!(messages
            .iter()
            .all(|m| !matches!(m, Message::TrafficOf { .. })));
        // A player far away stops receiving it.
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            assert!(Instant::now() < deadline, "far player kept traffic");
            host.update(Pose::default());
            a.update(Pose::default());
            b.update(far);
            if b.traffic().is_empty() {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
    }
    #[test]
    fn gameplay_round_trip_is_measured_on_the_active_connection() {
        let host = Session::host(address(), "Host").unwrap();
        let guest = Session::join(host.address, "Guest").unwrap();
        wait(|| {
            host.update(Pose::default());
            guest.update(Pose::default());
            report(&guest).round_trip.is_some()
        });
        let measured = report(&guest).round_trip.unwrap();
        assert!(measured > Duration::ZERO && measured < TIMEOUT);
    }
    #[test]
    fn published_script_bundles_reach_requesting_clients_verified() {
        let host = Session::host(address(), "Host").unwrap();
        let guest = Session::join(host.address, "Guest").unwrap();
        wait(|| {
            host.update(Pose::default());
            report(&guest).local_id != 0
        });
        let big: Vec<u8> = (0..20_000).map(|i| (i % 251) as u8).collect();
        let sha = host.publish_script("demo", big.clone()).unwrap();
        // Unknown hashes end immediately; later requests still arrive.
        guest.request_script(&"0".repeat(64));
        guest.request_script(&sha);
        let mut received = Vec::new();
        wait(|| {
            host.update(Pose::default());
            guest.update(Pose::default());
            received.extend(guest.take_scripts());
            !received.is_empty()
        });
        assert_eq!(received, vec![(sha.clone(), big)]);
        // Replaced bundles are no longer served.
        let newer = host.publish_script("demo", b"new".to_vec()).unwrap();
        assert!(!host.mail.scripts.lock().unwrap().contains_key(&sha));
        host.unpublish_script("demo");
        assert!(!host.mail.scripts.lock().unwrap().contains_key(&newer));
    }
    #[test]
    fn identities_are_signed_per_server_and_stay_off_snapshots() {
        set_identity_secret("player-secret");
        let secret = IDENTITY_SECRET.get().unwrap().clone();
        let mut seen = Vec::new();
        for salt in ["", "old-salt"] {
            let key = identity::ServerKey::generate();
            let public = key.public_hex();
            let host = Session::host(address(), "Host").unwrap();
            host.set_server_key(key);
            host.set_identity_salt(salt);
            let guest = Session::join(host.address, "Guest").unwrap();
            let mut id = 0;
            wait(|| {
                host.update(Pose::default());
                id = report(&guest).local_id;
                id != 0 && host.identity(id).is_some()
            });
            let checked = host.player_identity(id).unwrap();
            assert_eq!(checked.license, identity::license(&secret, &public));
            // Old servers also get the old identity, as a claim only.
            let legacy = (!salt.is_empty()).then(|| legacy_identity(&secret, salt));
            assert_eq!(checked.legacy, legacy);
            assert!(!serde_json::to_string(&report(&guest).peers)
                .unwrap()
                .contains(&checked.license));
            seen.push(checked.license);
        }
        assert_ne!(seen[0], seen[1]);
        // Without a server key the server gets no identity.
        let host = Session::host(address(), "Host").unwrap();
        let guest = Session::join(host.address, "Guest").unwrap();
        wait(|| report(&guest).local_id != 0 && report(&host).peers.len() >= 2);
        assert_eq!(host.identity(report(&guest).local_id), None);
    }

    #[test]
    fn wanted_stars_and_sirens_travel_only_when_set() {
        let quiet = serde_json::to_string(&Pose::default()).unwrap();
        assert!(!quiet.contains("wanted"), "older games see nothing new");
        let old: Pose = serde_json::from_str(&quiet).unwrap();
        assert_eq!(old.wanted, 0);
        let wanted = Pose {
            wanted: 3,
            ..Pose::default()
        };
        let back: Pose = serde_json::from_str(&serde_json::to_string(&wanted).unwrap()).unwrap();
        assert_eq!(back.wanted, 3);
        assert!(back.valid());
        assert!(!Pose {
            wanted: 7,
            ..wanted
        }
        .valid());
        let mut car = TrafficPose::new(1, 2, [0.0; 3], 0.0, 0.0);
        assert!(!serde_json::to_string(&car).unwrap().contains("siren"));
        car.siren = true;
        let back: TrafficPose =
            serde_json::from_str(&serde_json::to_string(&car).unwrap()).unwrap();
        assert!(back.siren);
    }

    /// A raw client speaking the protocol by hand.
    fn handshake(host: &Session, nonce: Option<String>) -> (Wire, u32, Option<Challenge>) {
        let mut wire = Wire::new(TcpStream::connect(host.address).unwrap()).unwrap();
        wire.queue(&Message::Hello {
            version: VERSION,
            name: "Raw".into(),
            resources: None,
            nonce,
            traffic: false,
        })
        .unwrap();
        wire.flush().unwrap();
        let mut welcome = None;
        wait(|| {
            host.update(Pose::default());
            for message in wire.read().unwrap() {
                if let Message::Welcome { id, challenge, .. } = message {
                    welcome = Some((id, challenge));
                }
            }
            welcome.is_some()
        });
        let (id, challenge) = welcome.unwrap();
        (wire, id, challenge)
    }
    fn closed(host: &Session, wire: &mut Wire) {
        wait(|| {
            host.update(Pose::default());
            let _ = wire.flush();
            wire.read().is_err() || wire.closed
        });
    }

    #[test]
    fn replayed_forged_and_unsigned_identities_are_refused() {
        let host = Session::host(address(), "Host").unwrap();
        host.set_server_key(identity::ServerKey::generate());
        host.set_identity_salt("old-salt");

        // An honest answer, recorded by an eavesdropper.
        let client = identity::nonce();
        let (mut wire, id, challenge) = handshake(&host, Some(client.clone()));
        let challenge = challenge.expect("signed challenge");
        assert!(identity::verify_welcome(
            &challenge.server_key,
            &client,
            &challenge.nonce,
            id,
            &challenge.proof
        ));
        let (key, signature) = identity::sign_auth(
            "victim",
            &challenge.server_key,
            &client,
            &challenge.nonce,
            id,
            None,
        );
        let recorded = Message::Auth {
            key: key.clone(),
            signature,
            legacy: None,
        };
        wire.queue(&recorded).unwrap();
        wire.flush().unwrap();
        wait(|| {
            host.update(Pose::default());
            host.identity(id).is_some()
        });
        assert_eq!(host.identity(id), Some(key.clone()));

        // Replaying it on a new connection fails: new nonces, new player ID.
        let (mut replay, other, _) = handshake(&host, Some(client.clone()));
        replay.queue(&recorded).unwrap();
        closed(&host, &mut replay);
        assert_eq!(host.identity(other), None);

        // Claiming someone's old identity without a matching signature fails.
        let client = identity::nonce();
        let (mut forged, other, challenge) = handshake(&host, Some(client.clone()));
        let challenge = challenge.unwrap();
        let (key, signature) = identity::sign_auth(
            "attacker",
            &challenge.server_key,
            &client,
            &challenge.nonce,
            other,
            None,
        );
        forged
            .queue(&Message::Auth {
                key,
                signature,
                legacy: Some("a".repeat(64)),
            })
            .unwrap();
        closed(&host, &mut forged);
        assert_eq!(host.identity(other), None);

        // An older game's unsigned identity is accepted as a connection
        // but gives no license.
        let (mut old, other, challenge) = handshake(&host, None);
        assert!(challenge.is_none());
        old.queue(&Message::Identity("b".repeat(64))).unwrap();
        old.queue(&Message::Pose(Pose::default())).unwrap();
        wait(|| {
            host.update(Pose::default());
            let _ = old.flush();
            let _ = old.read();
            report(&host).peers.iter().any(|p| p.id == other)
        });
        assert_eq!(host.identity(other), None);
        drop(wire);
    }
    fn address() -> SocketAddr {
        "127.0.0.1:0".parse().unwrap()
    }
    fn wait(mut condition: impl FnMut() -> bool) {
        let start = Instant::now();
        while !condition() {
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "network test timed out"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn report(s: &Session) -> Report {
        s.update(Pose::default());
        s.report.lock().unwrap().clone()
    }
    fn pair() -> (TcpStream, Wire) {
        let listener = TcpListener::bind(address()).unwrap();
        let sender = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let receiver = Wire::new(listener.accept().unwrap().0).unwrap();
        (sender, receiver)
    }
    #[test]
    fn fragmented_and_combined_frames_preserve_message_boundaries() {
        let (mut sender, mut wire) = pair();
        let data = serde_json::to_vec(&Message::Welcome {
            id: 7,
            salt: String::new(),
            challenge: None,
            traffic: false,
        })
        .unwrap();
        let mut frame = (data.len() as u32).to_le_bytes().to_vec();
        frame.extend(data);
        sender.write_all(&frame[..2]).unwrap();
        thread::sleep(Duration::from_millis(20));
        assert!(wire.read().unwrap().is_empty());
        sender.write_all(&frame[2..]).unwrap();
        sender.write_all(&frame).unwrap();
        let mut received = Vec::new();
        wait(|| {
            received.extend(wire.read().unwrap());
            received.len() == 2
        });
        assert!(received
            .iter()
            .all(|m| matches!(m, Message::Welcome { id: 7, .. })));
    }
    #[test]
    fn oversized_frames_and_slow_writer_queues_are_bounded() {
        let (mut sender, mut wire) = pair();
        sender
            .write_all(&((MAX_FRAME + 1) as u32).to_le_bytes())
            .unwrap();
        wait(|| wire.read().is_err());
        let (_, mut wire) = pair();
        let message = Message::Reject("x".repeat(8000));
        for _ in 0..8 {
            wire.queue(&message).unwrap();
        }
        assert!(wire.queue(&message).is_err());
    }
    #[test]
    fn rejects_invalid_pose_and_sanitizes_names() {
        assert!(Pose::default().valid());
        assert!(!Pose {
            yaw: f32::NAN,
            ..Pose::default()
        }
        .valid());
        assert!(!Pose {
            position: [f32::INFINITY, 0.0, 0.0],
            ..Pose::default()
        }
        .valid());
        assert!(!Pose {
            speed: 300.0,
            ..Pose::default()
        }
        .valid());
        assert!(!Pose {
            car_model: 256,
            ..Pose::default()
        }
        .valid());
        assert!(!Pose {
            ped_model: u16::MAX,
            ..Pose::default()
        }
        .valid());
        assert_eq!(safe_name("\n\0"), "Player");
        assert!(!Pose {
            driving: true,
            ..Pose::default()
        }
        .valid());
        for car in [
            VehiclePose {
                position: [f32::NAN, 0.0, 0.0],
                ..VehiclePose::default()
            },
            VehiclePose {
                position: [20001.0, 0.0, 0.0],
                ..VehiclePose::default()
            },
            VehiclePose {
                speed: 201.0,
                ..VehiclePose::default()
            },
            VehiclePose {
                pitch: f32::INFINITY,
                ..VehiclePose::default()
            },
        ] {
            assert!(!Pose {
                vehicle: Some(car),
                ..Pose::default()
            }
            .valid());
        }
        assert_eq!(safe_name(&"p".repeat(50)).len(), 24);
    }
    #[test]
    fn passenger_request_roundtrip_follows_driver_and_leaves_without_disconnect() {
        let host = Session::host(address(), "Driver").unwrap();
        let guest = Session::join(host.address, "Passenger").unwrap();
        let mut driver = Pose {
            vehicle: Some(VehiclePose::default()),
            ..Pose::default()
        };
        wait(|| {
            host.update(driver);
            guest.update(Pose::default());
            guest
                .report
                .lock()
                .unwrap()
                .clone()
                .peers
                .iter()
                .any(|p| p.id == 0 && p.pose.vehicle.is_some())
        });
        let mut rider = Pose {
            ride_request: Some(RideRequest {
                sequence: 1,
                owner: Some(0),
            }),
            ..Pose::default()
        };
        wait(|| {
            host.update(driver);
            guest.update(rider);
            guest
                .report
                .lock()
                .unwrap()
                .clone()
                .peers
                .iter()
                .any(|p| p.name == "Passenger" && p.pose.ride.is_some())
        });
        driver.vehicle.as_mut().unwrap().position = [10.0, 0.0, 0.0];
        driver.vehicle.as_mut().unwrap().speed = 10.0;
        wait(|| {
            host.update(driver);
            guest.update(rider);
            guest
                .report
                .lock()
                .unwrap()
                .clone()
                .peers
                .iter()
                .any(|p| p.name == "Passenger" && p.pose.position[0] > 10.0 && p.pose.speed == 10.0)
        });
        rider.ride_request = Some(RideRequest {
            sequence: 2,
            owner: None,
        });
        wait(|| {
            guest.update(rider);
            let r = guest.report.lock().unwrap().clone();
            r.connected
                && r.peers.iter().any(|p| {
                    p.name == "Passenger"
                        && p.pose.ride.is_none()
                        && p.pose
                            .ride_reply
                            .is_some_and(|reply| reply.result == RideResult::Left)
                })
        });
        rider.ride_request = Some(RideRequest {
            sequence: 3,
            owner: Some(0),
        });
        wait(|| {
            guest.update(rider);
            let r = guest.report.lock().unwrap().clone();
            r.connected
                && r.peers.iter().any(|p| {
                    p.name == "Passenger"
                        && p.pose
                            .ride_reply
                            .is_some_and(|reply| reply.result == RideResult::TooFar)
                })
        });
    }
    #[test]
    fn twenty_passenger_snapshots_fit_the_bounded_frame() {
        let pose = Pose {
            position: [-20000.0, 20000.0, -20000.0],
            yaw: 100000.0,
            pitch: 4.0,
            roll: 4.0,
            speed: 200.0,
            car_model: 255,
            ped_model: 255,
            clothes: u16::MAX,
            vehicle: Some(VehiclePose {
                position: [-20000.0, 20000.0, -20000.0],
                yaw: 100000.0,
                pitch: 4.0,
                roll: 4.0,
                speed: 200.0,
                interior: 255,
            }),
            ride: Some(PassengerSeat {
                owner: u32::MAX,
                seat: 3,
            }),
            ride_reply: Some(RideReply {
                sequence: u32::MAX,
                result: RideResult::Interior,
            }),
            ..Pose::default()
        };
        let peers: Vec<_> = (0..MAX_PLAYERS)
            .map(|id| Peer {
                id: id as u32,
                name: "p".repeat(24),
                pose,
            })
            .collect();
        assert!(serde_json::to_vec(&Message::Snapshot(peers)).unwrap().len() < MAX_FRAME);
    }
    #[test]
    fn twenty_players_relay_poses_reject_overflow_and_reuse_a_departed_slot() {
        let host = Session::host(address(), "Host").unwrap();
        let mut clients: Vec<_> = (1..MAX_PLAYERS)
            .map(|i| Session::join(host.address, &format!("Player{i}")).unwrap())
            .collect();
        let pose = Pose {
            position: [12.0, 3.0, -4.0],
            car_model: 7,
            ped_model: 3,
            clothes: 0b101,
            driving: true,
            speed: 20.0,
            vehicle: Some(VehiclePose {
                position: [12.0, 3.0, -4.0],
                speed: 20.0,
                ..VehiclePose::default()
            }),
            ..Pose::default()
        };
        wait(|| {
            for client in &clients {
                client.update(pose);
            }
            let r = report(&host);
            r.peers.len() == MAX_PLAYERS
                && r.peers.iter().skip(1).all(|p| p.pose == pose)
                && clients
                    .iter()
                    .all(|c| c.update(pose).is_some_and(|r| r.peers.len() == MAX_PLAYERS))
        });
        let overflow = Session::join(host.address, "Overflow").unwrap();
        wait(|| report(&overflow).status.contains("Session full"));
        assert_eq!(report(&host).peers.len(), MAX_PLAYERS);
        let parked = Pose {
            driving: false,
            position: [400.0, 3.0, -4.0],
            speed: 0.0,
            vehicle: Some(VehiclePose {
                speed: 0.0,
                ..pose.vehicle.unwrap()
            }),
            ..pose
        };
        wait(|| {
            for client in &clients {
                client.update(parked);
            }
            report(&host).peers.iter().skip(1).all(|p| p.pose == parked)
        });
        drop(clients.pop());
        wait(|| report(&host).peers.len() == MAX_PLAYERS - 1);
        let replacement = Session::join(host.address, "Replacement").unwrap();
        wait(|| report(&replacement).connected && report(&host).peers.len() == MAX_PLAYERS);
        wait(|| {
            let r = report(&replacement);
            let parked_owners: Vec<_> = r
                .peers
                .iter()
                .filter(|p| p.name.starts_with("Player"))
                .collect();
            parked_owners.len() == MAX_PLAYERS - 2 && parked_owners.iter().all(|p| p.pose == parked)
        });
        drop(host);
        wait(|| !report(&replacement).connected);
    }
    #[test]
    fn bad_version_and_impersonated_snapshots_are_rejected_without_losing_host() {
        let host = Session::host(address(), "Host").unwrap();
        let mut intruder = Wire::new(TcpStream::connect(host.address).unwrap()).unwrap();
        intruder
            .queue(&Message::Hello {
                version: VERSION + 1,
                name: "Bad".into(),
                resources: None,
                nonce: None,
                traffic: false,
            })
            .unwrap();
        intruder.flush().unwrap();
        let mut rejected = false;
        wait(|| {
            for m in intruder.read().unwrap() {
                rejected |= matches!(m, Message::Reject(_));
            }
            rejected
        });
        let client = Session::join(host.address, "Good").unwrap();
        wait(|| report(&client).connected);
        let mut intruder = Wire::new(TcpStream::connect(host.address).unwrap()).unwrap();
        intruder.queue(&Message::Snapshot(vec![])).unwrap();
        intruder.flush().unwrap();
        wait(|| intruder.read().is_err());
        assert!(report(&host).connected);
        assert!(report(&client).connected);
    }
    #[test]
    fn script_events_flow_both_ways_and_hosts_can_drop_and_cap_players() {
        let server =
            Session::dedicated_resources(address(), "Server", resources::Share::default()).unwrap();
        server.set_max_clients(1);
        let guest = Session::join(server.address, "Guest").unwrap();
        wait(|| report(&server).peers.len() == 1);
        guest
            .trigger(None, "chat:messageEntered", r#"["Guest","hi"]"#)
            .unwrap();
        let mut received = Vec::new();
        wait(|| {
            received.extend(server.events());
            !received.is_empty()
        });
        let id = report(&server).peers[0].id;
        assert_eq!(received[0].source, id);
        assert_eq!(received[0].payload, r#"["Guest","hi"]"#);
        server
            .trigger(Some(id), "chat:addMessage", r#"[{"args":["hi"]}]"#)
            .unwrap();
        let mut delivered = Vec::new();
        wait(|| {
            report(&guest);
            delivered.extend(guest.events());
            !delivered.is_empty()
        });
        assert_eq!(delivered[0].name, "chat:addMessage");
        assert!(guest.trigger(None, "bad name", "[]").is_err());
        assert!(guest.trigger(None, "ok", "{}").is_err());
        let overflow = Session::join(server.address, "Overflow").unwrap();
        wait(|| {
            report(&overflow)
                .status
                .contains("Session full (1 players)")
        });
        server.drop_player(id, "Kicked by admin");
        wait(|| report(&guest).status.contains("Kicked by admin"));
        wait(|| report(&server).peers.is_empty());
    }
    #[test]
    fn dedicated_server_has_twenty_real_slots_and_no_phantom_host() {
        let server =
            Session::dedicated_resources(address(), "Server", resources::Share::default()).unwrap();
        wait(|| report(&server).connected);
        assert!(report(&server).peers.is_empty());
        let mut clients: Vec<_> = (0..MAX_PLAYERS)
            .map(|i| Session::join(server.address, &format!("Client{i}")).unwrap())
            .collect();
        wait(|| {
            report(&server).peers.len() == MAX_PLAYERS
                && clients.iter().all(|c| report(c).peers.len() == MAX_PLAYERS)
        });
        assert!(report(&server)
            .peers
            .iter()
            .all(|p| p.id != 0 && p.name != "Server"));
        let overflow = Session::join(server.address, "Overflow").unwrap();
        wait(|| report(&overflow).status.contains("Session full"));
        drop(clients.pop());
        wait(|| report(&server).peers.len() == MAX_PLAYERS - 1);
        let replacement = Session::join(server.address, "Replacement").unwrap();
        wait(|| report(&replacement).connected && report(&server).peers.len() == MAX_PLAYERS);
        drop(server);
        wait(|| !report(&replacement).connected);
    }
}
