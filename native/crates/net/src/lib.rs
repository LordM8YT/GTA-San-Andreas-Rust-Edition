//! Player-hosted prototype. Networking never runs on the render thread.
//! Host assigns identities and relays poses; movement is client-authoritative.
use serde::{Deserialize, Serialize};
pub mod passengers;
pub mod relay;
pub mod resources;
pub use passengers::{PassengerSeat, RideReply, RideRequest, RideResult};
use std::{
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
pub const VERSION: u32 = 6;
const TICK: Duration = Duration::from_millis(50);
const TIMEOUT: Duration = Duration::from_secs(10);
const MAX_FRAME: usize = 16 * 1024;

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
        let name = safe_name(name);
        thread::Builder::new()
            .name("multiplayer-host".into())
            .spawn(move || {
                host_worker(listener, name, stop, local, report, share, host_player);
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
        let name = safe_name(name);
        thread::Builder::new()
            .name("multiplayer-client".into())
            .spawn(move || {
                let result = (|| {
                    let stream = TcpStream::connect_timeout(&address, Duration::from_secs(3))?;
                    client_stream(stream, address, name, &stop, &local, &report, fingerprint)
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
        }
    }
    #[allow(clippy::type_complexity)]
    fn shared(&self) -> (Arc<AtomicBool>, Arc<Mutex<Pose>>, Arc<Mutex<Report>>) {
        (self.stop.clone(), self.local.clone(), self.report.clone())
    }
    /// Latest-only mailbox: a slow renderer cannot grow a network queue.
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
}
fn host_worker(
    listener: TcpListener,
    name: String,
    stop: Arc<AtomicBool>,
    local: Arc<Mutex<Pose>>,
    report: Arc<Mutex<Report>>,
    share: resources::Share,
    host_player: bool,
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
                        if admitted >= MAX_PLAYERS - usize::from(host_player) {
                            let _ = guest
                                .wire
                                .queue(&Message::Reject("Session full (20 players)".into()));
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
                    _ => return false,
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
fn client_stream(
    stream: TcpStream,
    address: SocketAddr,
    name: String,
    stop: &AtomicBool,
    local: &Mutex<Pose>,
    report: &Mutex<Report>,
    fingerprint: Option<String>,
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
                Message::Reject(reason) => return Err(io::Error::other(reason)),
                _ => return Err(invalid()),
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
