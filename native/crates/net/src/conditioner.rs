//! A bad connection on purpose, for testing (SARE Studio › Test › Server).
//!
//! [`Conditioner`] is a TCP relay on this computer: players connect to it and
//! it passes their bytes to the real server and back, adding ping, jitter,
//! a bandwidth cap and packet loss. SARE plays over TCP, which never loses
//! bytes; a lost packet shows up as the sender waiting to resend it. So loss
//! here holds the stream back for a resend timeout instead of dropping data.
//! Losses come in bursts (Gilbert–Elliott model), like on real Wi-Fi.
//!
//! The design follows lightyear's link conditioner
//! (`crates/io/link/src/conditioner.rs`, MIT OR Apache-2.0), written anew for
//! SARE's TCP streams without Bevy.
use std::{
    io::{self, Read, Write},
    net::{Shutdown, SocketAddr, TcpListener, TcpStream},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, Instant},
};

/// What the connection between players and the server is like.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Link {
    /// Added round trip in milliseconds (half each way).
    pub ping_ms: u32,
    /// Random extra delay each way, from 0 up to this.
    pub jitter_ms: u32,
    /// Share of packets lost, 0.0 to 0.5.
    pub loss: f32,
    /// Bytes per second each way; 0 is unlimited.
    pub bandwidth: u32,
}

impl Default for Link {
    fn default() -> Self {
        Link::PERFECT
    }
}

impl Link {
    /// The preset this is, if any.
    pub fn preset(&self) -> Option<&'static str> {
        Link::PRESETS
            .iter()
            .find(|(_, link)| link == self)
            .map(|(name, _)| *name)
    }
    pub const PERFECT: Link = Link {
        ping_ms: 0,
        jitter_ms: 0,
        loss: 0.0,
        bandwidth: 0,
    };
    /// Ready-made connections to pick from, by name.
    pub const PRESETS: [(&'static str, Link); 5] = [
        ("Perfect", Link::PERFECT),
        (
            "Good home internet",
            Link {
                ping_ms: 40,
                jitter_ms: 5,
                loss: 0.0,
                bandwidth: 0,
            },
        ),
        (
            "Wi-Fi far from the router",
            Link {
                ping_ms: 80,
                jitter_ms: 30,
                loss: 0.01,
                bandwidth: 1_000_000,
            },
        ),
        (
            "Mobile 4G",
            Link {
                ping_ms: 120,
                jitter_ms: 40,
                loss: 0.02,
                bandwidth: 500_000,
            },
        ),
        (
            "Bad connection",
            Link {
                ping_ms: 300,
                jitter_ms: 100,
                loss: 0.05,
                bandwidth: 64_000,
            },
        ),
    ];
    fn clamped(self) -> Link {
        Link {
            ping_ms: self.ping_ms.min(5_000),
            jitter_ms: self.jitter_ms.min(2_000),
            loss: if self.loss.is_finite() {
                self.loss.clamp(0.0, 0.5)
            } else {
                0.0
            },
            bandwidth: self.bandwidth,
        }
    }
}

/// How long TCP waits before it resends a lost packet, at least.
const RESEND_MIN: Duration = Duration::from_millis(200);
/// Average length of a burst of losses, in packets.
const BURST: f32 = 3.0;
/// Largest piece passed on at once: about one Wi-Fi packet's worth, so loss
/// and bandwidth act per packet rather than per big read.
const PACKET: usize = 1400;
/// Pieces waiting to be passed on, per direction, before reading stops.
const QUEUE: usize = 4096;

/// Closed connections are forgotten once this many were made.
const MAX_KEPT: usize = 64;

/// Counters for one player's connection.
#[derive(Debug, Default)]
pub struct Traffic {
    /// Bytes from the player to the server.
    pub up: AtomicU64,
    /// Bytes from the server to the player.
    pub down: AtomicU64,
    /// Packets lost (each one held the stream back for a resend).
    pub lost: AtomicU64,
    pub open: AtomicBool,
    /// A [`Conditioner::measure`] connection, not a player.
    probe: bool,
}

struct Shared {
    link: Mutex<Link>,
    stop: AtomicBool,
    connections: Mutex<Vec<(Arc<Traffic>, TcpStream, TcpStream)>>,
}

/// A running relay. Dropping it closes every connection through it.
pub struct Conditioner {
    address: SocketAddr,
    probe: SocketAddr,
    shared: Arc<Shared>,
}

impl Conditioner {
    /// Relay from a free port on 127.0.0.1 to `server`.
    pub fn start(server: SocketAddr, link: Link) -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let address = listener.local_addr()?;
        let probes = TcpListener::bind("127.0.0.1:0")?;
        probes.set_nonblocking(true)?;
        let probe = probes.local_addr()?;
        let shared = Arc::new(Shared {
            link: Mutex::new(link.clamped()),
            stop: AtomicBool::new(false),
            connections: Mutex::new(Vec::new()),
        });
        let worker = shared.clone();
        thread::Builder::new()
            .name("sare-conditioner".into())
            .spawn(move || accept([listener, probes], server, worker))?;
        Ok(Self {
            address,
            probe,
            shared,
        })
    }
    /// Where players connect instead of the server.
    pub fn address(&self) -> SocketAddr {
        self.address
    }
    pub fn link(&self) -> Link {
        *self.shared.link.lock().unwrap()
    }
    /// Change the connection; open connections follow at once.
    pub fn set_link(&self, link: Link) {
        *self.shared.link.lock().unwrap() = link.clamped();
    }
    /// Players' connections so far, in order, with their counters.
    pub fn traffic(&self) -> Vec<Arc<Traffic>> {
        let connections = self.shared.connections.lock().unwrap();
        connections
            .iter()
            .filter(|(t, _, _)| !t.probe)
            .map(|(t, _, _)| t.clone())
            .collect()
    }
    /// Ask the server for its status through the link, as a player's data
    /// goes: the time it takes is the ping players have now. Blocks for up
    /// to a few seconds; call it off the render thread.
    pub fn measure(&self) -> io::Result<Duration> {
        let start = Instant::now();
        crate::query(self.probe)?;
        Ok(start.elapsed())
    }
}

impl Drop for Conditioner {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        for (traffic, a, b) in self.shared.connections.lock().unwrap().iter() {
            traffic.open.store(false, Ordering::Relaxed);
            let _ = a.shutdown(Shutdown::Both);
            let _ = b.shutdown(Shutdown::Both);
        }
    }
}

/// Players come in on the first listener, measurements on the second.
fn accept(listeners: [TcpListener; 2], server: SocketAddr, shared: Arc<Shared>) {
    while !shared.stop.load(Ordering::Relaxed) {
        let mut idle = true;
        for (index, listener) in listeners.iter().enumerate() {
            let Ok((player, _)) = listener.accept() else {
                continue;
            };
            idle = false;
            let probe = index == 1;
            let shared = shared.clone();
            let _ = thread::Builder::new()
                .name("sare-conditioner-connect".into())
                .spawn(move || {
                    if let Err(error) = connect(player, server, probe, &shared) {
                        if !probe {
                            eprintln!("conditioner: could not reach the server: {error}");
                        }
                    }
                });
        }
        if idle {
            thread::sleep(Duration::from_millis(10));
        }
    }
}

fn connect(
    player: TcpStream,
    server: SocketAddr,
    probe: bool,
    shared: &Arc<Shared>,
) -> io::Result<()> {
    player.set_nonblocking(false)?;
    // A test server may still be starting: try for a few seconds.
    let deadline = Instant::now() + Duration::from_secs(10);
    let upstream = loop {
        match TcpStream::connect_timeout(&server, Duration::from_secs(3)) {
            Ok(stream) => break stream,
            Err(_) if Instant::now() < deadline && !shared.stop.load(Ordering::Relaxed) => {
                thread::sleep(Duration::from_millis(100));
            }
            Err(error) => return Err(error),
        }
    };
    let _ = player.set_nodelay(true);
    let _ = upstream.set_nodelay(true);
    let traffic = Arc::new(Traffic {
        probe,
        ..Traffic::default()
    });
    traffic.open.store(true, Ordering::Relaxed);
    let mut connections = shared.connections.lock().unwrap();
    if connections.len() >= MAX_KEPT {
        connections.retain(|(t, _, _)| t.open.load(Ordering::Relaxed));
    }
    connections.push((traffic.clone(), player.try_clone()?, upstream.try_clone()?));
    drop(connections);
    if shared.stop.load(Ordering::Relaxed) {
        // Stopped while this connection was being made.
        let _ = player.shutdown(Shutdown::Both);
        let _ = upstream.shutdown(Shutdown::Both);
        return Ok(());
    }
    let up = (player.try_clone()?, upstream.try_clone()?);
    let down = (upstream, player);
    for (direction, (from, to)) in [(Direction::Up, up), (Direction::Down, down)] {
        let (shared, traffic) = (shared.clone(), traffic.clone());
        pipe(direction, from, to, shared, traffic)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Direction {
    Up,
    Down,
}

/// Read from `from`, hold each piece back as the link says, write to `to`.
fn pipe(
    direction: Direction,
    mut from: TcpStream,
    mut to: TcpStream,
    shared: Arc<Shared>,
    traffic: Arc<Traffic>,
) -> io::Result<()> {
    let (send, receive) = mpsc::sync_channel::<(Instant, Vec<u8>)>(QUEUE);
    let reader_traffic = traffic.clone();
    thread::Builder::new()
        .name("sare-conditioner-read".into())
        .spawn(move || {
            let mut shaper = Shaper::new(seed());
            let mut buffer = [0u8; 16 * 1024];
            loop {
                let read = match from.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let counter = match direction {
                    Direction::Up => &reader_traffic.up,
                    Direction::Down => &reader_traffic.down,
                };
                counter.fetch_add(read as u64, Ordering::Relaxed);
                for piece in buffer[..read].chunks(PACKET) {
                    let link = *shared.link.lock().unwrap();
                    let (release, lost) = shaper.release(Instant::now(), piece.len(), link);
                    if lost {
                        reader_traffic.lost.fetch_add(1, Ordering::Relaxed);
                    }
                    if send.send((release, piece.to_vec())).is_err() {
                        return;
                    }
                }
            }
            // Sender dropped: the writer finishes the queue, then closes.
        })?;
    thread::Builder::new()
        .name("sare-conditioner-write".into())
        .spawn(move || {
            for (release, piece) in receive {
                let now = Instant::now();
                if release > now {
                    thread::sleep(release - now);
                }
                if to.write_all(&piece).is_err() {
                    break;
                }
            }
            let _ = to.shutdown(Shutdown::Write);
            traffic.open.store(false, Ordering::Relaxed);
        })?;
    Ok(())
}

/// Picks when each piece of one direction may go on. Pieces never pass
/// each other: a stream stays in order, as TCP does.
struct Shaper {
    random: u64,
    /// Gilbert–Elliott: in the bad state every packet is lost.
    bad: bool,
    /// The previous piece's release.
    last: Option<Instant>,
    /// When the bandwidth cap lets the next byte through.
    free: Option<Instant>,
}

impl Shaper {
    fn new(seed: u64) -> Self {
        Self {
            random: seed | 1,
            bad: false,
            last: None,
            free: None,
        }
    }
    /// 0.0..1.0 (xorshift64*).
    fn next(&mut self) -> f32 {
        self.random ^= self.random >> 12;
        self.random ^= self.random << 25;
        self.random ^= self.random >> 27;
        let value = self.random.wrapping_mul(0x2545_F491_4F6C_DD1D);
        (value >> 40) as f32 / (1u64 << 24) as f32
    }
    /// Is this packet lost? On average `loss` of packets are, in bursts of
    /// about `BURST`.
    fn lose(&mut self, loss: f32) -> bool {
        if loss <= 0.0 {
            self.bad = false;
            return false;
        }
        let recover = 1.0 / BURST;
        // Stationary share of bad packets: enter / (enter + recover) = loss.
        let enter = (loss * recover / (1.0 - loss)).min(1.0);
        self.bad = if self.bad {
            self.next() >= recover
        } else {
            self.next() < enter
        };
        self.bad
    }
    fn release(&mut self, now: Instant, bytes: usize, link: Link) -> (Instant, bool) {
        let one_way = Duration::from_millis(u64::from(link.ping_ms) / 2);
        let jitter = Duration::from_secs_f32(self.next() * link.jitter_ms as f32 / 1000.0);
        let mut release = now + one_way + jitter;
        let lost = self.lose(link.loss);
        if lost {
            // Resent after a timeout of about one round trip, at least RESEND_MIN.
            release += RESEND_MIN.max(Duration::from_millis(u64::from(link.ping_ms)));
        }
        if link.bandwidth > 0 {
            let start = self.free.map_or(now, |free| free.max(now));
            let free = start + Duration::from_secs_f64(bytes as f64 / f64::from(link.bandwidth));
            self.free = Some(free);
            release = release.max(free);
        } else {
            self.free = None;
        }
        if let Some(last) = self.last {
            release = release.max(last);
        }
        self.last = Some(release);
        (release, lost)
    }
}

fn seed() -> u64 {
    let mut bytes = [0u8; 8];
    if getrandom::fill(&mut bytes).is_err() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        return nanos ^ 0x9E37_79B9_7F4A_7C15;
    }
    u64::from_le_bytes(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A server that sends back what it gets.
    fn echo() -> SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                thread::spawn(move || {
                    let mut stream = stream;
                    let mut buffer = [0u8; 4096];
                    while let Ok(n) = stream.read(&mut buffer) {
                        if n == 0 || stream.write_all(&buffer[..n]).is_err() {
                            break;
                        }
                    }
                });
            }
        });
        address
    }

    fn round_trip(stream: &mut TcpStream, bytes: &[u8]) -> Duration {
        let start = Instant::now();
        stream.write_all(bytes).unwrap();
        let mut back = vec![0u8; bytes.len()];
        stream.read_exact(&mut back).unwrap();
        assert_eq!(back, bytes, "bytes arrive unchanged and in order");
        start.elapsed()
    }

    #[test]
    fn ping_is_added_to_the_round_trip_and_can_change_while_open() {
        let conditioner = Conditioner::start(echo(), Link::PERFECT).unwrap();
        let mut player = TcpStream::connect(conditioner.address()).unwrap();
        player.set_nodelay(true).unwrap();
        assert!(round_trip(&mut player, b"hello") < Duration::from_millis(100));
        conditioner.set_link(Link {
            ping_ms: 200,
            ..Link::PERFECT
        });
        let slow = round_trip(&mut player, b"again");
        assert!(slow >= Duration::from_millis(195), "{slow:?}");
        assert!(slow < Duration::from_millis(1000), "{slow:?}");
        let traffic = conditioner.traffic();
        assert_eq!(traffic.len(), 1);
        assert_eq!(traffic[0].up.load(Ordering::Relaxed), 10);
        assert_eq!(traffic[0].down.load(Ordering::Relaxed), 10);
        assert!(traffic[0].open.load(Ordering::Relaxed));
    }

    #[test]
    fn a_big_burst_keeps_its_order_under_jitter_and_loss() {
        let conditioner = Conditioner::start(
            echo(),
            Link {
                ping_ms: 10,
                jitter_ms: 20,
                loss: 0.2,
                bandwidth: 0,
            },
        )
        .unwrap();
        let mut player = TcpStream::connect(conditioner.address()).unwrap();
        let bytes: Vec<u8> = (0..60_000u32).map(|n| (n % 251) as u8).collect();
        round_trip(&mut player, &bytes);
        let lost: u64 = conditioner
            .traffic()
            .iter()
            .map(|t| t.lost.load(Ordering::Relaxed))
            .sum();
        assert!(lost > 0, "some packets were lost and resent");
    }

    #[test]
    fn dropping_it_closes_the_players() {
        let conditioner = Conditioner::start(echo(), Link::PERFECT).unwrap();
        let mut player = TcpStream::connect(conditioner.address()).unwrap();
        round_trip(&mut player, b"x");
        drop(conditioner);
        player
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut buffer = [0u8; 1];
        assert!(matches!(player.read(&mut buffer), Ok(0) | Err(_)));
    }

    #[test]
    fn measuring_goes_through_the_link_but_is_not_a_player() {
        let host = crate::Session::host("127.0.0.1:0".parse().unwrap(), "Host").unwrap();
        let link = Link {
            ping_ms: 120,
            ..Link::PERFECT
        };
        let conditioner = Conditioner::start(host.address, link).unwrap();
        let ping = conditioner.measure().unwrap();
        assert!(ping >= Duration::from_millis(115), "{ping:?}");
        assert!(ping < Duration::from_secs(2), "{ping:?}");
        assert!(conditioner.traffic().is_empty());
    }

    #[test]
    fn bandwidth_spreads_bytes_out() {
        let mut shaper = Shaper::new(7);
        let now = Instant::now();
        let link = Link {
            bandwidth: 10_000,
            ..Link::PERFECT
        };
        let mut last = now;
        for _ in 0..10 {
            last = shaper.release(now, 1_000, link).0;
        }
        // 10 000 bytes at 10 000 bytes/s: one second.
        let took = last - now;
        assert!(
            took >= Duration::from_millis(990) && took <= Duration::from_millis(1010),
            "{took:?}"
        );
    }

    #[test]
    fn losses_average_out_to_the_setting_and_come_in_bursts() {
        let mut shaper = Shaper::new(42);
        let (mut lost, mut bursts, mut was) = (0, 0, false);
        let packets = 200_000;
        for _ in 0..packets {
            let now = shaper.lose(0.05);
            lost += now as u32;
            bursts += (now && !was) as u32;
            was = now;
        }
        let share = lost as f32 / packets as f32;
        assert!((0.04..0.06).contains(&share), "{share}");
        let mean_burst = lost as f32 / bursts as f32;
        assert!((2.5..3.5).contains(&mean_burst), "{mean_burst}");
        assert!(!Shaper::new(1).lose(0.0));
    }

    #[test]
    fn settings_out_of_range_are_kept_sane() {
        let link = Link {
            ping_ms: 1_000_000,
            jitter_ms: 1_000_000,
            loss: f32::NAN,
            bandwidth: 5,
        }
        .clamped();
        assert_eq!(
            (link.ping_ms, link.jitter_ms, link.loss),
            (5_000, 2_000, 0.0)
        );
        assert_eq!(
            Link {
                loss: 0.9,
                ..Link::PERFECT
            }
            .clamped()
            .loss,
            0.5
        );
    }
}
