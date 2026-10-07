//! Outbound-only player hosting. The service discovers rooms and tunnels TCP;
//! the player's existing host still assigns identities and runs membership.
use super::*;
use std::collections::HashMap;
use std::sync::atomic::AtomicUsize;

const CONTROL_LIMIT: usize = 8192;
const IO_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_ROOMS: usize = 32;
const MAX_CONNECTIONS: usize = 256;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Listing {
    pub code: String,
    pub name: String,
    pub players: usize,
    pub capacity: usize,
    pub version: u32,
}
#[derive(Debug, Serialize, Deserialize)]
enum Control {
    Register {
        version: u32,
        name: String,
        public: bool,
    },
    Registered {
        code: String,
        key: String,
    },
    Heartbeat {
        players: usize,
    },
    Tickets(Vec<String>),
    List,
    Listings(Vec<Listing>),
    Join {
        version: u32,
        code: String,
    },
    Attach {
        code: String,
        key: String,
        ticket: String,
    },
    Ready,
    Error(String),
}
fn send(stream: &mut TcpStream, message: &Control) -> io::Result<()> {
    let bytes = serde_json::to_vec(message).map_err(|_| invalid())?;
    if bytes.len() > CONTROL_LIMIT {
        return Err(invalid());
    }
    stream.write_all(&(bytes.len() as u32).to_le_bytes())?;
    stream.write_all(&bytes)
}
fn receive(stream: &mut TcpStream) -> io::Result<Control> {
    let deadline = Instant::now() + IO_TIMEOUT;
    fn read_until(
        stream: &mut TcpStream,
        mut bytes: &mut [u8],
        deadline: Instant,
    ) -> io::Result<()> {
        while !bytes.is_empty() {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .ok_or(io::ErrorKind::TimedOut)?;
            stream.set_read_timeout(Some(remaining.max(Duration::from_millis(1))))?;
            let n = stream.read(bytes)?;
            if n == 0 {
                return Err(io::ErrorKind::UnexpectedEof.into());
            }
            bytes = &mut bytes[n..];
        }
        Ok(())
    }
    let mut size = [0; 4];
    read_until(stream, &mut size, deadline)?;
    let size = u32::from_le_bytes(size) as usize;
    if size == 0 || size > CONTROL_LIMIT {
        return Err(invalid());
    }
    let mut bytes = vec![0; size];
    read_until(stream, &mut bytes, deadline)?;
    let message = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
    if let Control::Error(reason) = message {
        return Err(io::Error::other(reason));
    }
    Ok(message)
}
fn connect(address: SocketAddr) -> io::Result<TcpStream> {
    let stream = TcpStream::connect_timeout(&address, IO_TIMEOUT)?;
    configure(&stream)?;
    Ok(stream)
}
fn configure(stream: &TcpStream) -> io::Result<()> {
    // Accepted sockets can inherit listener nonblocking mode on Windows.
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(IO_TIMEOUT))?;
    stream.set_write_timeout(Some(IO_TIMEOUT))?;
    stream.set_nodelay(true)
}
fn token(bytes: usize) -> io::Result<String> {
    let mut random = vec![0; bytes];
    getrandom::fill(&mut random).map_err(|e| io::Error::other(e.to_string()))?;
    Ok(random.iter().map(|b| format!("{b:02X}")).collect())
}
fn code(value: &str) -> io::Result<String> {
    let value = value.trim().to_ascii_uppercase();
    if value.len() != 12 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(io::Error::other("Use a 12-character join code."));
    }
    Ok(value)
}

pub fn browse(address: SocketAddr) -> io::Result<Vec<Listing>> {
    let mut stream = connect(address)?;
    send(&mut stream, &Control::List)?;
    match receive(&mut stream)? {
        Control::Listings(rooms)
            if rooms.len() <= MAX_ROOMS
                && rooms.iter().all(|r| {
                    code(&r.code).is_ok()
                        && r.name.len() <= 96
                        && r.players <= MAX_PLAYERS
                        && r.capacity == MAX_PLAYERS
                        && r.version == VERSION
                }) =>
        {
            Ok(rooms)
        }
        _ => Err(invalid()),
    }
}

#[derive(Clone, Debug, Default)]
pub struct HostReport {
    pub code: String,
    pub status: String,
}
/// Own alongside Session; dropping it withdraws the room and closes tunnels.
pub struct Publication {
    stop: Arc<AtomicBool>,
    report: Arc<Mutex<HostReport>>,
}
impl Publication {
    pub fn report(&self) -> HostReport {
        self.report.lock().unwrap().clone()
    }
}
impl Drop for Publication {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
impl Session {
    pub fn host_relay(
        address: SocketAddr,
        name: &str,
        public: bool,
    ) -> io::Result<(Self, Publication)> {
        let session = Self::host("127.0.0.1:0".parse().unwrap(), name)?;
        let publication = Publication {
            stop: session.stop.clone(),
            report: Arc::new(Mutex::new(HostReport {
                code: String::new(),
                status: "Publishing session...".into(),
            })),
        };
        let local_address = session.address;
        let session_report = session.report.clone();
        let stop = session.stop.clone();
        let report = publication.report.clone();
        let name = safe_name(name);
        thread::Builder::new()
            .name("session-publication".into())
            .spawn(move || {
                let result = publish_room(
                    address,
                    local_address,
                    &name,
                    public,
                    &stop,
                    &session_report,
                    &report,
                );
                if let Err(error) = result {
                    *report.lock().unwrap() = HostReport {
                        code: String::new(),
                        status: format!("Relay disconnected: {error}"),
                    };
                    // Fail closed: do not leave a supposedly published game hosting.
                    stop.store(true, Ordering::Relaxed);
                }
            })?;
        Ok((session, publication))
    }
    pub fn join_relay(address: SocketAddr, join_code: &str, name: &str) -> io::Result<Self> {
        let join_code = code(join_code)?;
        let session = Self::new(address, "Joining player-hosted session...");
        let (stop, local, report) = session.shared();
        let name = safe_name(name);
        thread::Builder::new()
            .name("relay-guest".into())
            .spawn(move || {
                let result = (|| {
                    let mut stream = connect(address)?;
                    send(
                        &mut stream,
                        &Control::Join {
                            version: VERSION,
                            code: join_code,
                        },
                    )?;
                    if !matches!(receive(&mut stream)?, Control::Ready) {
                        return Err(invalid());
                    }
                    client_stream(stream, address, name, &stop, &local, &report)
                })();
                let status = match result {
                    Ok(()) => "Disconnected".into(),
                    Err(e) => format!("Session ended: {e}"),
                };
                publish(&report, &status, false, 0, Vec::new());
            })?;
        Ok(session)
    }
}
fn publish_room(
    relay: SocketAddr,
    local: SocketAddr,
    name: &str,
    public: bool,
    stop: &Arc<AtomicBool>,
    session_report: &Mutex<Report>,
    report: &Mutex<HostReport>,
) -> io::Result<()> {
    let mut control = connect(relay)?;
    send(
        &mut control,
        &Control::Register {
            version: VERSION,
            name: name.into(),
            public,
        },
    )?;
    let Control::Registered { code: room, key } = receive(&mut control)? else {
        return Err(invalid());
    };
    code(&room)?;
    if key.len() != 64 {
        return Err(invalid());
    }
    *report.lock().unwrap() = HostReport {
        code: room.clone(),
        status: "Online via relay".into(),
    };
    let tunnels = Arc::new(AtomicUsize::new(0));
    while !stop.load(Ordering::Relaxed) {
        let players = session_report.lock().unwrap().peers.len().max(1);
        send(&mut control, &Control::Heartbeat { players })?;
        let Control::Tickets(tickets) = receive(&mut control)? else {
            return Err(invalid());
        };
        if tickets.len() > MAX_PLAYERS {
            return Err(invalid());
        }
        for ticket in tickets {
            if ticket.len() != 64 {
                return Err(invalid());
            }
            if tunnels.load(Ordering::Relaxed) >= MAX_PLAYERS - 1 {
                continue;
            }
            tunnels.fetch_add(1, Ordering::Relaxed);
            let active = tunnels.clone();
            let (room, key, stop) = (room.clone(), key.clone(), stop.clone());
            let spawned = thread::Builder::new()
                .name("host-relay-tunnel".into())
                .spawn(move || {
                    let result = (|| {
                        let mut remote = connect(relay)?;
                        send(
                            &mut remote,
                            &Control::Attach {
                                code: room,
                                key,
                                ticket,
                            },
                        )?;
                        if !matches!(receive(&mut remote)?, Control::Ready) {
                            return Err(invalid());
                        }
                        tunnel(remote, connect(local)?, &stop)
                    })();
                    if let Err(e) = result {
                        eprintln!("Host relay tunnel ended: {e}");
                    }
                    active.fetch_sub(1, Ordering::Relaxed);
                });
            if let Err(e) = spawned {
                tunnels.fetch_sub(1, Ordering::Relaxed);
                return Err(e);
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}

struct Room {
    listing: Listing,
    key: String,
    public: bool,
    stop: Arc<AtomicBool>,
    pending: HashMap<String, (TcpStream, Instant, bool)>,
    active: usize,
}
type Rooms = Arc<Mutex<HashMap<String, Room>>>;
/// A directory/tunnel service, not a game simulation or asset server.
pub struct Relay {
    pub address: SocketAddr,
    stop: Arc<AtomicBool>,
}
impl Relay {
    pub fn start(address: SocketAddr) -> io::Result<Self> {
        let listener = TcpListener::bind(address)?;
        listener.set_nonblocking(true)?;
        let relay = Self {
            address: listener.local_addr()?,
            stop: Arc::new(AtomicBool::new(false)),
        };
        let stop = relay.stop.clone();
        let rooms: Rooms = Arc::default();
        let connections = Arc::new(AtomicUsize::new(0));
        thread::Builder::new()
            .name("relay-listener".into())
            .spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    if let Ok((stream, _)) = listener.accept() {
                        if connections.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
                            continue;
                        }
                        connections.fetch_add(1, Ordering::Relaxed);
                        let (rooms, stop, connections) =
                            (rooms.clone(), stop.clone(), connections.clone());
                        let release = connections.clone();
                        if thread::Builder::new()
                            .name("relay-connection".into())
                            .spawn(move || {
                                let _ = serve(stream, &rooms, &stop);
                                connections.fetch_sub(1, Ordering::Relaxed);
                            })
                            .is_err()
                        {
                            release.fetch_sub(1, Ordering::Relaxed);
                        }
                    } else {
                        thread::sleep(Duration::from_millis(5));
                    }
                }
                for room in rooms.lock().unwrap().values() {
                    room.stop.store(true, Ordering::Relaxed);
                }
            })?;
        Ok(relay)
    }
}
impl Drop for Relay {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
fn reject(stream: &mut TcpStream, reason: &str) -> io::Result<()> {
    send(stream, &Control::Error(reason.into()))
}
fn serve(mut stream: TcpStream, rooms: &Rooms, service_stop: &AtomicBool) -> io::Result<()> {
    configure(&stream)?;
    match receive(&mut stream)? {
        Control::Register {
            version,
            name,
            public,
        } => {
            if version != VERSION {
                return reject(&mut stream, "Incompatible runtime version");
            }
            let room_code = token(6)?;
            let key = token(32)?;
            let room_stop = Arc::new(AtomicBool::new(false));
            {
                let mut rooms = rooms.lock().unwrap();
                if rooms.len() >= MAX_ROOMS || rooms.contains_key(&room_code) {
                    return reject(&mut stream, "Relay capacity reached");
                }
                rooms.insert(
                    room_code.clone(),
                    Room {
                        listing: Listing {
                            code: room_code.clone(),
                            name: safe_name(&name),
                            players: 1,
                            capacity: MAX_PLAYERS,
                            version,
                        },
                        key: key.clone(),
                        public,
                        stop: room_stop.clone(),
                        pending: HashMap::new(),
                        active: 0,
                    },
                );
            }
            let result = (|| {
                send(
                    &mut stream,
                    &Control::Registered {
                        code: room_code.clone(),
                        key,
                    },
                )?;
                while !service_stop.load(Ordering::Relaxed) {
                    let Control::Heartbeat { players } = receive(&mut stream)? else {
                        return Err(invalid());
                    };
                    if !(1..=MAX_PLAYERS).contains(&players) {
                        return Err(invalid());
                    }
                    let tickets = {
                        let mut rooms = rooms.lock().unwrap();
                        let room = rooms.get_mut(&room_code).ok_or_else(invalid)?;
                        room.listing.players = players;
                        room.pending
                            .retain(|_, (_, time, _)| time.elapsed() < Duration::from_secs(2));
                        room.pending
                            .iter_mut()
                            .filter_map(|(ticket, (_, _, notified))| {
                                if *notified {
                                    None
                                } else {
                                    *notified = true;
                                    Some(ticket.clone())
                                }
                            })
                            .collect()
                    };
                    send(&mut stream, &Control::Tickets(tickets))?;
                }
                Ok(())
            })();
            room_stop.store(true, Ordering::Relaxed);
            rooms.lock().unwrap().remove(&room_code);
            result
        }
        Control::List => {
            let mut listings: Vec<_> = rooms
                .lock()
                .unwrap()
                .values()
                .filter(|r| r.public)
                .map(|r| r.listing.clone())
                .collect();
            listings.sort_by(|a, b| a.code.cmp(&b.code));
            send(&mut stream, &Control::Listings(listings))
        }
        Control::Join {
            version,
            code: value,
        } => {
            if version != VERSION {
                return reject(&mut stream, "Incompatible runtime version");
            }
            let value = code(&value)?;
            let mut rooms = rooms.lock().unwrap();
            let Some(room) = rooms.get_mut(&value) else {
                return reject(&mut stream, "Session not found");
            };
            if room.active + room.pending.len() >= MAX_PLAYERS - 1 {
                return reject(&mut stream, "Session full (20 players)");
            }
            room.pending
                .insert(token(32)?, (stream, Instant::now(), false));
            Ok(())
        }
        Control::Attach {
            code: value,
            key,
            ticket,
        } => {
            let value = code(&value)?;
            let attached = {
                let mut rooms = rooms.lock().unwrap();
                let Some(room) = rooms.get_mut(&value) else {
                    return reject(&mut stream, "Session not found");
                };
                if key != room.key {
                    return reject(&mut stream, "Invalid host credential");
                }
                let Some((guest, time, _)) = room.pending.remove(&ticket) else {
                    return reject(&mut stream, "Join request expired");
                };
                if time.elapsed() >= Duration::from_secs(2) {
                    return reject(&mut stream, "Join request expired");
                }
                room.active += 1;
                (guest, room.stop.clone())
            };
            let (mut guest, room_stop) = attached;
            let result = (|| {
                send(&mut stream, &Control::Ready)?;
                send(&mut guest, &Control::Ready)?;
                tunnel(stream, guest, &room_stop)
            })();
            if let Some(room) = rooms.lock().unwrap().get_mut(&value) {
                room.active = room.active.saturating_sub(1);
            }
            result
        }
        _ => Err(invalid()),
    }
}

/// One worker and bounded buffers for both directions; partial writes are retained.
fn tunnel(mut a: TcpStream, mut b: TcpStream, stop: &AtomicBool) -> io::Result<()> {
    a.set_nonblocking(true)?;
    b.set_nonblocking(true)?;
    let (mut ab, mut ba) = (Vec::new(), Vec::new());
    let mut last = Instant::now();
    while !stop.load(Ordering::Relaxed) && last.elapsed() < TIMEOUT {
        for direction in 0..2 {
            let (source, destination, pending) = if direction == 0 {
                (&mut a, &mut b, &mut ab)
            } else {
                (&mut b, &mut a, &mut ba)
            };
            if pending.len() < 16 * 1024 {
                let mut buffer = [0; 4096];
                match source.read(&mut buffer) {
                    Ok(0) => return Ok(()),
                    Ok(n) => {
                        pending.extend_from_slice(&buffer[..n]);
                        last = Instant::now();
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(e),
                }
            }
            if !pending.is_empty() {
                match destination.write(pending) {
                    Ok(0) => return Ok(()),
                    Ok(n) => {
                        pending.drain(..n);
                        last = Instant::now();
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(e),
                }
            }
        }
        thread::sleep(Duration::from_millis(2));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn wait(mut condition: impl FnMut() -> bool) {
        let start = Instant::now();
        while !condition() {
            assert!(
                start.elapsed() < Duration::from_secs(8),
                "relay test timed out"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
    fn relay() -> Relay {
        Relay::start("127.0.0.1:0".parse().unwrap()).unwrap()
    }
    fn host(relay: &Relay, public: bool) -> (Session, Publication, String) {
        let (host, publication) = Session::host_relay(relay.address, "Host", public).unwrap();
        wait(|| !publication.report().code.is_empty());
        let code = publication.report().code;
        (host, publication, code)
    }
    fn report(session: &Session) -> Report {
        session.update(Pose::default()).unwrap()
    }

    #[test]
    fn public_browser_and_private_code_relay_actual_game_poses() {
        let relay = relay();
        let (public, public_listing, public_code) = host(&relay, true);
        let (private, private_listing, private_code) = host(&relay, false);
        let listed = browse(relay.address).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].code, public_code);
        assert!(!listed.iter().any(|r| r.code == private_code));
        let guest =
            Session::join_relay(relay.address, &private_code.to_lowercase(), "Guest").unwrap();
        wait(|| report(&guest).connected && report(&private).peers.len() == 2);
        let pose = Pose {
            position: [120.0, 14.0, -35.0],
            driving: true,
            yaw: 0.9,
            pitch: 0.12,
            ..Pose::default()
        };
        wait(|| {
            guest.update(pose);
            private
                .update(Pose::default())
                .unwrap()
                .peers
                .iter()
                .any(|p| p.name == "Guest" && p.pose == pose)
        });
        drop((private, private_listing));
        wait(|| !report(&guest).connected);
        drop((public, public_listing));
        wait(|| browse(relay.address).unwrap().is_empty());
        // A private room has no browser entry; wait for its control connection
        // to withdraw too, rather than racing a final outstanding join request.
        wait(|| {
            let mut stream = connect(relay.address).unwrap();
            send(
                &mut stream,
                &Control::Join {
                    version: VERSION,
                    code: private_code.clone(),
                },
            )
            .unwrap();
            receive(&mut stream).is_err_and(|e| e.to_string().contains("Session not found"))
        });
        let absent = Session::join_relay(relay.address, &private_code, "Late").unwrap();
        wait(|| report(&absent).status.contains("Session not found"));
    }

    #[test]
    fn relay_preserves_twenty_player_capacity_and_reuses_departed_slots() {
        let relay = relay();
        let (host, _publication, code) = host(&relay, true);
        let mut guests = Vec::new();
        for i in 0..MAX_PLAYERS - 1 {
            let guest = Session::join_relay(relay.address, &code, &format!("Guest{i}")).unwrap();
            wait(|| report(&guest).connected);
            guests.push(guest);
        }
        wait(|| report(&host).peers.len() == MAX_PLAYERS);
        wait(|| browse(relay.address).unwrap()[0].players == MAX_PLAYERS);
        let extra = Session::join_relay(relay.address, &code, "Extra").unwrap();
        wait(|| report(&extra).status.contains("Session full"));
        assert!(!report(&extra).connected);
        guests.pop();
        wait(|| report(&host).peers.len() == MAX_PLAYERS - 1);
        let replacement = Session::join_relay(relay.address, &code, "Replacement").unwrap();
        wait(|| report(&replacement).connected);
        assert!(report(&replacement).local_id > MAX_PLAYERS as u32 - 1);
    }

    #[test]
    fn invalid_codes_credentials_versions_and_oversized_control_are_rejected() {
        let relay = relay();
        let (_host, _publication, room) = host(&relay, false);
        assert!(Session::join_relay(relay.address, "../etc", "Guest").is_err());
        let mut stream = connect(relay.address).unwrap();
        send(
            &mut stream,
            &Control::Attach {
                code: room.clone(),
                key: "wrong".into(),
                ticket: "wrong".into(),
            },
        )
        .unwrap();
        assert!(receive(&mut stream)
            .unwrap_err()
            .to_string()
            .contains("Invalid host credential"));
        let mut stream = connect(relay.address).unwrap();
        send(
            &mut stream,
            &Control::Join {
                code: room,
                version: VERSION + 1,
            },
        )
        .unwrap();
        assert!(receive(&mut stream)
            .unwrap_err()
            .to_string()
            .contains("Incompatible"));
        let mut stream = connect(relay.address).unwrap();
        stream
            .write_all(&((CONTROL_LIMIT + 1) as u32).to_le_bytes())
            .unwrap();
        assert!(receive(&mut stream).is_err());
        assert!(browse(relay.address).unwrap().is_empty());
    }

    #[test]
    fn relay_shutdown_closes_host_and_joined_session() {
        let relay = relay();
        let (host, publication, code) = host(&relay, true);
        let guest = Session::join_relay(relay.address, &code, "Guest").unwrap();
        wait(|| report(&guest).connected);
        drop(relay);
        wait(|| publication.report().status.contains("Relay disconnected"));
        wait(|| !report(&host).connected && !report(&guest).connected);
    }
}
