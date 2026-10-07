//! One bounded background loader. The renderer keeps the old region until a
//! complete replacement is ready; no GTA files are written.
use anyhow::Result;
use sa_scene::{Scene, WorldLoader};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::time::{Duration, Instant};

pub const ORIGIN: [f32; 2] = [2500.0, -1670.0];
pub const RADIUS: f32 = 400.0;
pub const DESTINATIONS: [(&str, [f32; 2]); 9] = [
    ("Grove Street", ORIGIN),
    ("Downtown Los Santos", [1480.0, -1730.0]),
    ("Santa Maria Beach", [350.0, -1800.0]),
    ("Los Santos Airport", [1700.0, -2450.0]),
    ("Countryside", [200.0, -500.0]),
    ("San Fierro", [-2000.0, 300.0]),
    ("Las Venturas", [2000.0, 1500.0]),
    ("Desert", [-500.0, 1900.0]),
    ("Mount Chiliad", [-2300.0, -1600.0]),
];
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Region {
    pub center: [f32; 2],
    pub interior: u8,
}
#[derive(Clone, Copy)]
pub struct Interior {
    pub name: &'static str,
    pub id: u8,
    pub position: [f32; 3],
}
pub const INTERIORS: [Interior; 3] = [
    Interior {
        name: "CJ's House",
        id: 3,
        position: [2496.05, -1692.93, 1013.75],
    },
    Interior {
        name: "Sweet's House",
        id: 1,
        position: [2526.46, -1679.09, 1014.5],
    },
    Interior {
        name: "Madd Dogg's Mansion",
        id: 5,
        position: [1263.08, -785.309, 1090.96],
    },
];
pub fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}
pub struct Streamer {
    requests: SyncSender<Region>,
    results: Receiver<(Region, Result<Scene>)>,
    pending: Option<Region>,
    retry_after: Option<Instant>,
    connected: bool,
}
impl Streamer {
    pub fn new(mut loader: WorldLoader) -> Self {
        let (requests, receiver) = mpsc::sync_channel::<Region>(1);
        let (sender, results) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("world-stream".into())
            .spawn(move || {
                while let Ok(region) = receiver.recv() {
                    let started = std::time::Instant::now();
                    let scene = if region.interior == 0 {
                        loader.load(region.center, ORIGIN, RADIUS)
                    } else {
                        loader.load_interior(region.center, ORIGIN, 50.0, region.interior)
                    };
                    if let Ok(ref scene) = scene {
                        eprintln!(
                            "Loaded {} placements / {} triangles in {:.2}s",
                            scene.placements,
                            scene.triangles,
                            started.elapsed().as_secs_f32()
                        );
                    }
                    if sender.send((region, scene)).is_err() {
                        break;
                    }
                }
            })
            .expect("world loader thread");
        Self {
            requests,
            results,
            pending: None,
            retry_after: None,
            connected: true,
        }
    }
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn retry_later(&mut self) {
        self.retry_after = Some(Instant::now() + Duration::from_secs(5));
    }
    pub fn request(&mut self, center: [f32; 2]) {
        self.request_region(Region {
            center,
            interior: 0,
        });
    }
    pub fn request_region(&mut self, region: Region) {
        if self.connected
            && self.pending.is_none()
            && self
                .retry_after
                .is_none_or(|deadline| Instant::now() >= deadline)
            && self.requests.try_send(region).is_ok()
        {
            self.pending = Some(region);
        }
    }
    pub fn poll(&mut self) -> Option<(Region, Result<Scene>)> {
        if !self.connected {
            return None;
        }
        match self.results.try_recv() {
            Ok(result) => {
                self.pending = None;
                if result.1.is_err() {
                    self.retry_later();
                } else {
                    self.retry_after = None;
                }
                Some(result)
            }
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => {
                self.connected = false;
                Some((
                    self.pending.take().unwrap_or(Region {
                        center: ORIGIN,
                        interior: 0,
                    }),
                    Err(anyhow::anyhow!(
                        "Kartlasteren stoppet. Start spillet på nytt for å laste flere områder."
                    )),
                ))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_load_waits_before_retry_and_worker_loss_clears_pending() {
        let (requests, receiver) = mpsc::sync_channel::<Region>(1);
        let (sender, results) = mpsc::sync_channel(1);
        let mut stream = Streamer {
            requests,
            results,
            pending: None,
            retry_after: None,
            connected: true,
        };
        stream.request(ORIGIN);
        assert_eq!(
            receiver.try_recv().unwrap(),
            Region {
                center: ORIGIN,
                interior: 0
            }
        );
        sender
            .send((
                Region {
                    center: ORIGIN,
                    interior: 0,
                },
                Err(anyhow::anyhow!("test load failure")),
            ))
            .unwrap();
        assert!(stream.poll().unwrap().1.is_err());
        stream.request(ORIGIN);
        assert!(receiver.try_recv().is_err());
        assert!(!stream.pending());
        stream.retry_after = Some(Instant::now() - Duration::from_secs(1));
        stream.request(ORIGIN);
        assert!(stream.pending());
        drop(sender);
        assert!(stream.poll().unwrap().1.is_err());
        assert!(!stream.pending());
        assert!(stream.poll().is_none());
        stream.request(ORIGIN);
        assert!(!stream.pending());
    }
}
