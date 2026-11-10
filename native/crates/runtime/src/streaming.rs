//! One bounded background loader. The renderer keeps the old region until a
//! complete replacement is ready; no GTA files are written.
use anyhow::Result;
use sa_scene::{mlo::MloDefinition, InteriorPortal, WorldLoader};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
use std::time::{Duration, Instant};

mod locations;
pub use locations::*;
pub fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}
/// Convert a GTA SA compass heading (0=north, clockwise) to runtime camera yaw
/// (0=south, increasing toward east).
pub fn camera_yaw_from_sa_heading(heading_degrees: f32) -> f32 {
    std::f32::consts::PI - heading_degrees.to_radians()
}
pub enum Retired {
    Scene {
        batches: Vec<crate::GpuBatch>,
        collision: Option<sa_scene::collision::CollisionWorld>,
        water: Option<std::sync::Arc<sa_scene::water::WaterMap>>,
    },
    Upload(Box<crate::upload::Upload>),
}
enum Work {
    Load(Region),
    Retire(Retired),
}
pub struct Streamer {
    requests: SyncSender<Work>,
    results: Receiver<(Region, Result<crate::upload::PreparedScene>)>,
    pending: Option<Region>,
    retry_after: Option<Instant>,
    connected: bool,
    portals: Vec<InteriorPortal>,
    mlos: Vec<MloDefinition>,
}
impl Streamer {
    pub fn new(mut loader: WorldLoader) -> Self {
        let portals = loader.interior_portals();
        let mlos = loader.mlo_definitions().to_vec();
        let (requests, receiver) = mpsc::sync_channel::<Work>(1);
        let (sender, results) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("world-stream".into())
            .spawn(move || {
                while let Ok(work) = receiver.recv() {
                    let region = match work {
                        Work::Load(region) => region,
                        Work::Retire(retired) => {
                            retired.release();
                            continue;
                        }
                    };
                    let started = std::time::Instant::now();
                    let scene = if region.interior == 0 {
                        loader.load(region.center, ORIGIN, RADIUS)
                    } else {
                        loader.load_interior(region.center, ORIGIN, 50.0, region.interior)
                    }
                    .map(crate::upload::prepare_scene);
                    if let Ok(ref scene) = scene {
                        eprintln!(
                            "Loaded {} placements / {} triangles and prepared textures in {:.2}s",
                            scene.scene.placements,
                            scene.scene.triangles,
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
            portals,
            mlos,
        }
    }
    /// MLO metadata loaded from resource manifests. Coordinates passed to the
    /// room helpers below are in the MLO's local space, before any instance transform.
    pub fn mlo_definitions(&self) -> &[MloDefinition] {
        &self.mlos
    }
    /// Resolve a room by MLO name using local-space bounds.
    pub fn mlo_room_at(&self, name: &str, local_position: [f32; 3]) -> Option<usize> {
        self.mlos
            .iter()
            .find(|mlo| mlo.name.eq_ignore_ascii_case(name))
            .and_then(|mlo| mlo.room_at(local_position))
    }
    /// Return entity indices reachable from a room through open MLO portals.
    /// Exterior-shell entities (entities without a room) remain visible.
    pub fn mlo_visible_entities(&self, name: &str, room: usize) -> Option<Vec<usize>> {
        let mlo = self
            .mlos
            .iter()
            .find(|mlo| mlo.name.eq_ignore_ascii_case(name))?;
        let visible_rooms = mlo.visible_rooms(room);
        Some(mlo.visible_entity_indices(&visible_rooms))
    }
    /// Find the closest ENEX or resource portal containing a player in the requested interior.
    pub fn portal_at(&self, position: [f32; 3], interior: u8) -> Option<InteriorPortal> {
        self.portals
            .iter()
            .filter(|portal| portal.source_interior == interior)
            .filter(|portal| {
                (0..3)
                    .all(|axis| (position[axis] - portal.source[axis]).abs() <= portal.radius[axis])
            })
            .min_by(|a, b| {
                let distance = |portal: &InteriorPortal| {
                    (0..3)
                        .map(|axis| (position[axis] - portal.source[axis]).powi(2))
                        .sum::<f32>()
                };
                distance(a).total_cmp(&distance(b))
            })
            .cloned()
    }
    pub fn pending(&self) -> bool {
        self.pending.is_some()
    }
    pub fn retire(&mut self, retired: Retired) -> Option<Retired> {
        match self.requests.try_send(Work::Retire(retired)) {
            Ok(()) => None,
            Err(mpsc::TrySendError::Full(Work::Retire(retired)))
            | Err(mpsc::TrySendError::Disconnected(Work::Retire(retired))) => Some(retired),
            _ => unreachable!(),
        }
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
            && self.requests.try_send(Work::Load(region)).is_ok()
        {
            self.pending = Some(region);
        }
    }
    pub fn poll(&mut self) -> Option<(Region, Result<crate::upload::PreparedScene>)> {
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
                        "The world loader stopped. Restart the game to load more areas."
                    )),
                ))
            }
        }
    }
}
impl Retired {
    fn release(self) {
        match self {
            Self::Scene {
                batches,
                collision,
                water,
            } => {
                drop(batches);
                drop(collision);
                drop(water);
            }
            Self::Upload(upload) => drop(upload),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sa_enex_headings_face_the_correct_runtime_direction() {
        let close = |actual: f32, expected: f32| {
            assert!((actual - expected).abs() < 0.0001);
        };
        close(camera_yaw_from_sa_heading(0.0), std::f32::consts::PI);
        close(
            camera_yaw_from_sa_heading(90.0),
            std::f32::consts::FRAC_PI_2,
        );
        close(camera_yaw_from_sa_heading(180.0), 0.0);
        close(
            camera_yaw_from_sa_heading(270.0),
            -std::f32::consts::FRAC_PI_2,
        );
    }
    #[test]
    fn resource_enex_portals_work_in_both_directions_and_respect_bounds() {
        let portal = InteriorPortal {
            source: [100.0, 200.0, 10.0],
            destination: [250.0, 300.0, 1000.0],
            radius: [1.5, 1.5, 3.0],
            source_interior: 0,
            target_interior: 5,
            enter_angle: 90.0,
            exit_angle: 180.0,
            flags: 0,
            name: "ppalace".into(),
        };
        portal.validate().unwrap();
        let reverse = portal.reverse().unwrap();
        let (requests, _receiver) = mpsc::sync_channel(1);
        let (_sender, results) = mpsc::sync_channel(1);
        let stream = Streamer {
            requests,
            results,
            pending: None,
            retry_after: None,
            connected: true,
            portals: vec![portal.clone(), reverse.clone()],
            mlos: Vec::new(),
        };
        assert_eq!(stream.portal_at(portal.source, 0), Some(portal));
        assert_eq!(stream.portal_at(reverse.source, 5), Some(reverse.clone()));
        assert!(stream.portal_at([105.0, 200.0, 10.0], 0).is_none());
    }
    #[test]
    fn mlo_room_lookup_and_visibility_are_available_to_streaming() {
        let mlo = MloDefinition {
            name: "police_station".into(),
            entities: vec![
                sa_scene::mlo::MloEntity {
                    model_id: 1,
                    position: [0.0; 3],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    room: Some(0),
                },
                sa_scene::mlo::MloEntity {
                    model_id: 1,
                    position: [0.0; 3],
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    room: None,
                },
            ],
            rooms: vec![sa_scene::mlo::MloRoom {
                name: "lobby".into(),
                min: [-5.0; 3],
                max: [5.0; 3],
                flags: 0,
                portal_count: 0,
                floor_id: 0,
                blend: 0.0,
                timecycle: None,
                exterior_visibility_depth: 0,
                entities: vec![0],
            }],
            portals: Vec::new(),
            entity_sets: Vec::new(),
            timecycle_modifiers: Vec::new(),
        };
        let (requests, _receiver) = mpsc::sync_channel(1);
        let (_sender, results) = mpsc::sync_channel(1);
        let stream = Streamer {
            requests,
            results,
            pending: None,
            retry_after: None,
            connected: true,
            portals: Vec::new(),
            mlos: vec![mlo],
        };
        assert_eq!(
            stream.mlo_room_at("POLICE_STATION", [0.0, 0.0, 0.0]),
            Some(0)
        );
        assert_eq!(stream.mlo_room_at("missing", [0.0; 3]), None);
        assert_eq!(
            stream.mlo_visible_entities("police_station", 0),
            Some(vec![0, 1])
        );
    }
    #[test]
    fn retirement_is_nonblocking_preserves_full_queue_and_does_not_mark_loading() {
        let (requests, receiver) = mpsc::sync_channel::<Work>(1);
        let (_sender, results) = mpsc::sync_channel(1);
        let mut stream = Streamer {
            requests,
            results,
            pending: None,
            retry_after: None,
            connected: true,
            portals: Vec::new(),
            mlos: Vec::new(),
        };
        let empty = || Retired::Scene {
            batches: Vec::new(),
            collision: None,
            water: None,
        };
        assert!(stream.retire(empty()).is_none());
        assert!(!stream.pending());
        let retained = stream
            .retire(empty())
            .expect("full queue must return ownership");
        stream.request(ORIGIN);
        assert!(
            !stream.pending(),
            "loading must wait for cleanup queue capacity"
        );
        assert!(matches!(receiver.try_recv(), Ok(Work::Retire(_))));
        assert!(stream.retire(retained).is_none());
        assert!(matches!(receiver.try_recv(), Ok(Work::Retire(_))));
        stream.request(ORIGIN);
        assert!(stream.pending());
        assert!(matches!(receiver.try_recv(), Ok(Work::Load(_))));
        drop(receiver);
        assert!(stream.retire(empty()).is_some());
    }
    #[test]
    fn failed_load_waits_before_retry_and_worker_loss_clears_pending() {
        let (requests, receiver) = mpsc::sync_channel::<Work>(1);
        let (sender, results) = mpsc::sync_channel(1);
        let mut stream = Streamer {
            requests,
            results,
            pending: None,
            retry_after: None,
            connected: true,
            portals: Vec::new(),
            mlos: Vec::new(),
        };
        stream.request(ORIGIN);
        assert_eq!(
            match receiver.try_recv().unwrap() {
                Work::Load(region) => region,
                _ => panic!("expected load"),
            },
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
