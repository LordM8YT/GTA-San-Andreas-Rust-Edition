//! Player-hosted prototype. Networking never runs on the render thread.
//! Host assigns identities and relays poses; movement is client-authoritative.
use serde::{Deserialize, Serialize};
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
    }
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
    },
    Welcome {
        id: u32,
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
                    Message::Hello { version, name, resources } if !guest.hello && !guest.assets => {
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
                        if guest
                            .wire
                            .queue(&Message::Welcome { id: guest.peer.id })
                            .is_err()
                        {
                            return false;
                        }
                    }
                    Message::Ping(nonce) if guest.hello => {
                        if guest.wire.queue(&Message::Pong(nonce)).is_err(){return false;}
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
            publish(&report, &status, true, 0, peers.clone());
            let snapshot = Message::Snapshot(peers);
            guests.retain_mut(|guest| {
                !guest.ready || (guest.wire.queue(&snapshot).is_ok() && guest.wire.flush().is_ok())
            });
        }
        thread::sleep(Duration::from_millis(5));
    }
    publish(&report, "Disconnected", false, 0, Vec::new());
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
    wire.queue(&Message::Hello {
        version: VERSION,
        name,
        resources: fingerprint,
    })?;
    let mut id = None;
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
                Message::Welcome { id: assigned } if id.is_none() && assigned != 0 => {
                    id = Some(assigned)
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
        let data = serde_json::to_vec(&Message::Welcome { id: 7 }).unwrap();
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
            .all(|m| matches!(m, Message::Welcome { id: 7 })));
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
