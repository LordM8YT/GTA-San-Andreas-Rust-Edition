//! Traffic shared between players.
//!
//! Every game drives the traffic around its own player and sends it to the
//! host, which passes it on to players nearby. Other players' cars are drawn
//! from those updates, smoothed between them and settled on this game's own
//! road surface. Roads near a player with a lower ID belong to that player's
//! traffic, so a group of players sees one set of cars instead of several,
//! and where two sets meet the higher ID's cars give way.
use super::{GpuBatch, State};
use crate::car_contacts::Footprint;
use glam::Vec3;
use sa_net::TrafficPose;

/// Another player's traffic car.
pub(super) struct RemoteTraffic {
    pub owner: u32,
    pub id: u32,
    pub model: usize,
    pub position: Vec3,
    pub yaw: f32,
    pub speed: f32,
    pitch: f32,
    roll: f32,
    /// Where the owner last said the car was, moved on by its speed since.
    target: Vec3,
    target_yaw: f32,
    last: TrafficPose,
    pub size: [f32; 3],
    clearance: f32,
    pub batches: Vec<GpuBatch>,
}

impl RemoteTraffic {
    pub fn footprint(&self) -> Footprint {
        Footprint::car(self.position, self.yaw, self.size)
    }
    pub fn velocity(&self) -> Vec3 {
        Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos()) * self.speed
    }
    /// A police car of another player's chase, siren and lights on.
    pub fn siren(&self) -> bool {
        self.last.siren
    }
}

/// Meshes made per frame for newly seen cars, so a crowded street arriving
/// at once does not stall a frame.
const NEW_PER_FRAME: usize = 4;

/// Move a remote car towards its latest reported pose. Between updates the
/// reported pose rolls on at the reported speed, and the drawn car eases
/// after it, so cars glide instead of stepping ten times a second.
fn follow(car: &mut RemoteTraffic, pose: TrafficPose, dt: f32) {
    let forward = |yaw: f32| Vec3::new(yaw.sin(), 0.0, yaw.cos());
    if pose != car.last {
        car.last = pose;
        car.target = Vec3::from_array(pose.position());
        car.target_yaw = pose.yaw();
        car.speed = pose.speed();
    } else {
        car.target += forward(car.target_yaw) * car.speed * dt;
    }
    if car.position.distance(car.target) > 15.0 {
        car.position = car.target;
        car.yaw = car.target_yaw;
        return;
    }
    let ease = 1.0 - (-8.0 * dt).exp();
    car.position += (car.target - car.position) * ease;
    let turn = car.target_yaw - car.yaw;
    car.yaw += turn.sin().atan2(turn.cos()) * ease;
}

impl State {
    /// Players whose traffic has right of way over ours: lower IDs that are
    /// sending traffic. Returns their positions, for keeping our new cars
    /// off their roads.
    pub(super) fn traffic_owners_before_us(&self) -> Vec<Vec3> {
        let mut owners: Vec<u32> = self.remote_traffic.iter().map(|c| c.owner).collect();
        owners.sort_unstable();
        owners.dedup();
        owners
            .into_iter()
            .filter(|&owner| owner < self.script_local_id)
            .filter_map(|owner| {
                let actor = self.remote_actors.iter().find(|a| a.id == owner)?;
                Some(
                    actor
                        .current
                        .vehicle
                        .filter(|_| actor.current.driving)
                        .map_or(Vec3::from_array(actor.current.position), |car| {
                            Vec3::from_array(car.position)
                        }),
                )
            })
            .collect()
    }

    /// Bodies of other players' traffic our own traffic must stop for.
    pub(super) fn traffic_with_right_of_way(&self) -> Vec<Footprint> {
        self.remote_traffic
            .iter()
            .filter(|c| c.owner < self.script_local_id)
            .map(RemoteTraffic::footprint)
            .collect()
    }

    /// Send our traffic and follow everyone else's.
    pub(super) fn update_shared_traffic(&mut self, dt: f32) {
        let Some(session) = &self.network_session else {
            self.remote_traffic.clear();
            return;
        };
        session.set_traffic(
            self.traffic
                .cars
                .iter()
                .filter_map(|car| {
                    let mut pose = TrafficPose::new(
                        u32::try_from(car.id).ok()?,
                        u16::try_from(car.catalog).ok().filter(|m| *m < 256)?,
                        car.position.to_array(),
                        car.yaw,
                        car.speed,
                    );
                    pose.siren = car.siren();
                    Some(pose)
                })
                .collect(),
        );
        let incoming = session.traffic();
        let local = self.script_local_id;
        self.remote_traffic.retain(|car| {
            incoming.iter().any(|(owner, cars)| {
                *owner == car.owner
                    && cars
                        .iter()
                        .any(|p| p.id == car.id && usize::from(p.model) == car.model)
            })
        });
        let mut made = 0;
        for (owner, cars) in &incoming {
            if *owner == local {
                continue;
            }
            for pose in cars {
                if let Some(car) = self
                    .remote_traffic
                    .iter_mut()
                    .find(|c| c.owner == *owner && c.id == pose.id)
                {
                    follow(car, *pose, dt);
                    continue;
                }
                let model = usize::from(pose.model);
                if made >= NEW_PER_FRAME || model >= self.car_catalog.len() {
                    continue;
                }
                made += 1;
                self.ensure_car_loaded(model);
                let source = if model == self.active_car {
                    self.car.as_ref()
                } else {
                    self.car_catalog.get(model).and_then(|c| c.as_ref())
                };
                let Some((car, batches)) = source else {
                    continue;
                };
                let at = Vec3::from_array(pose.position());
                self.remote_traffic.push(RemoteTraffic {
                    owner: *owner,
                    id: pose.id,
                    model,
                    position: at,
                    yaw: pose.yaw(),
                    speed: pose.speed(),
                    pitch: 0.0,
                    roll: 0.0,
                    target: at,
                    target_yaw: pose.yaw(),
                    last: *pose,
                    size: car.size,
                    clearance: car.clearance,
                    batches: crate::multiplayer::duplicate(&self.device, batches),
                });
            }
        }
        let ground = |at: Vec3, max: f32| self.collision.as_ref()?.ground_below(at, max);
        for car in &mut self.remote_traffic {
            // Sit on this game's road surface: the owner's world may have
            // streamed a little differently.
            let (body, roll) =
                crate::traffic::surface(&ground, car.position, car.target.y, car.yaw, car.size);
            if let Some((y, pitch)) = body {
                car.position.y = y;
                car.pitch = pitch;
            }
            car.roll = roll;
            let rotation = sa_scene::vehicle::model_rotation(car.yaw, car.pitch, car.roll);
            let offset = car.position + Vec3::Y * car.clearance;
            for batch in &car.batches {
                let raw = crate::car_damage::pose_car(batch, &Default::default(), rotation, offset);
                self.queue
                    .write_buffer(&batch.buffer, 0, bytemuck::cast_slice(&raw));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car() -> RemoteTraffic {
        RemoteTraffic {
            owner: 1,
            id: 1,
            model: 0,
            position: Vec3::ZERO,
            yaw: 0.0,
            speed: 0.0,
            pitch: 0.0,
            roll: 0.0,
            target: Vec3::ZERO,
            target_yaw: 0.0,
            last: TrafficPose::default(),
            size: sa_scene::vehicle::DEFAULT_SIZE,
            clearance: 0.5,
            batches: Vec::new(),
        }
    }

    #[test]
    fn remote_cars_glide_between_updates() {
        let mut car = car();
        let pose = TrafficPose::new(1, 0, [0.0, 0.0, 2.0], 0.0, 10.0);
        follow(&mut car, pose, 0.016);
        // Eases towards the report instead of jumping there.
        assert!(car.position.z > 0.0 && car.position.z < 2.0);
        // With no new report the car keeps rolling at the reported speed.
        let mut previous = car.position.z;
        for _ in 0..30 {
            follow(&mut car, pose, 0.016);
            assert!(car.position.z > previous);
            previous = car.position.z;
        }
        // A far jump (respawn, teleport) snaps.
        follow(
            &mut car,
            TrafficPose::new(1, 0, [100.0, 0.0, 0.0], 1.0, 0.0),
            0.016,
        );
        assert_eq!(car.position, Vec3::new(100.0, 0.0, 0.0));
        assert!((car.yaw - 1.0).abs() < 0.001);
    }
}
