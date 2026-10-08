//! Bounded, server-owned vehicle lifetime and seats; clients simulate driving.
use crate::{PassengerSeat, Peer, Pose, RideReply, RideResult, VehiclePose};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap},
    time::{Duration, Instant},
};

pub const MAX_VEHICLES: usize = 40;
pub const FIRST_VEHICLE_ID: u32 = 0x8000_0000;
const PARKED_LIFETIME: Duration = Duration::from_secs(300);
const SPAWN_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverGrant {
    pub vehicle: u32,
    pub epoch: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct VehicleRecord {
    pub id: u32,
    pub model: u16,
    pub pose: VehiclePose,
    pub driver: Option<u32>,
    pub epoch: u32,
    pub passengers: [Option<u32>; 3],
}
impl VehicleRecord {
    pub fn valid(&self) -> bool {
        self.id >= FIRST_VEHICLE_ID && self.model < 256 && self.epoch != 0 && self.pose.valid()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct VehicleRequest {
    pub sequence: u32,
    pub action: VehicleAction,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum VehicleAction {
    Spawn { model: u16, pose: VehiclePose },
    Enter { vehicle: u32 },
    Leave,
}
impl VehicleRequest {
    pub(crate) fn valid(self) -> bool {
        self.sequence != 0
            && match self.action {
                VehicleAction::Spawn { model, pose } => model < 256 && pose.valid(),
                VehicleAction::Enter { vehicle } => vehicle >= FIRST_VEHICLE_ID,
                VehicleAction::Leave => true,
            }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct VehicleReply {
    pub sequence: u32,
    pub result: VehicleResult,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VehicleResult {
    Spawned,
    Entered,
    Left,
    Full,
    Missing,
    TooFar,
    Moving,
    Interior,
    Busy,
    Limit,
}
impl VehicleResult {
    pub fn label(self) -> &'static str {
        match self {
            Self::Spawned => "Vehicle spawned. F to leave.",
            Self::Entered => "Driver seat reserved. F to leave.",
            Self::Left => "You left the vehicle.",
            Self::Full => "The driver seat is occupied. G for a passenger seat.",
            Self::Missing => "That vehicle is unavailable.",
            Self::TooFar => "Move closer to the vehicle.",
            Self::Moving => "The car is moving too fast to board.",
            Self::Interior => "You and the vehicle are in different interiors.",
            Self::Busy => "Leave your current seat first, or wait before spawning again.",
            Self::Limit => "Vehicle limit reached. Unoccupied vehicles expire after five minutes.",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct VehicleMotion {
    pub vehicle: u32,
    pub epoch: u32,
    pub sequence: u32,
    pub pose: VehiclePose,
}
impl VehicleMotion {
    pub(crate) fn valid(self) -> bool {
        self.vehicle >= FIRST_VEHICLE_ID
            && self.epoch != 0
            && self.sequence != 0
            && self.pose.valid()
    }
}
struct Entry {
    record: VehicleRecord,
    used: Instant,
    motion_time: Instant,
    motion_sequence: u32,
}
#[derive(Default)]
pub(crate) struct VehicleWorld {
    cars: BTreeMap<u32, Entry>,
    replies: HashMap<u32, VehicleReply>,
    ride_replies: HashMap<u32, RideReply>,
    spawned: HashMap<u32, Instant>,
    next_id: u32,
}
pub(crate) fn newer(sequence: u32, previous: u32) -> bool {
    let difference = sequence.wrapping_sub(previous);
    sequence != 0 && difference != 0 && difference < 0x8000_0000
}
fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter()
        .zip(b)
        .map(|(a, b)| (a - b).powi(2))
        .sum::<f32>()
        .sqrt()
}
impl VehicleWorld {
    pub fn snapshot(&self) -> Vec<VehicleRecord> {
        self.cars.values().map(|c| c.record).collect()
    }
    fn seat(&self, player: u32) -> Option<(u32, u8)> {
        self.cars.values().find_map(|c| {
            if c.record.driver == Some(player) {
                Some((c.record.id, 0))
            } else {
                c.record
                    .passengers
                    .iter()
                    .position(|p| *p == Some(player))
                    .map(|i| (c.record.id, i as u8 + 1))
            }
        })
    }
    fn leave(&mut self, player: u32, now: Instant) {
        for car in self.cars.values_mut() {
            if car.record.driver == Some(player) {
                car.record.driver = None;
                car.record.pose.speed = 0.0;
                car.record.epoch = car.record.epoch.wrapping_add(1).max(1);
                car.used = now;
            }
            for seat in &mut car.record.passengers {
                if *seat == Some(player) {
                    *seat = None;
                    car.used = now;
                }
            }
        }
    }
    pub fn reconcile(&mut self, peers: &[Peer], now: Instant) {
        let present = |id: &u32| peers.iter().any(|p| p.id == *id);
        let departed: Vec<_> = self
            .cars
            .values()
            .flat_map(|c| {
                c.record
                    .driver
                    .into_iter()
                    .chain(c.record.passengers.into_iter().flatten())
            })
            .filter(|id| !present(id))
            .collect();
        for id in departed {
            self.leave(id, now);
        }
        self.replies.retain(|id, _| present(id));
        self.ride_replies.retain(|id, _| present(id));
        self.spawned.retain(|id, _| present(id));
        self.cars.retain(|_, c| {
            c.record.driver.is_some()
                || c.record.passengers.iter().any(Option::is_some)
                || now.duration_since(c.used) < PARKED_LIFETIME
        });
    }
    fn boarding(&self, player: u32, pose: Pose, id: u32) -> Result<(), VehicleResult> {
        if self.seat(player).is_some() {
            return Err(VehicleResult::Busy);
        }
        let car = &self.cars.get(&id).ok_or(VehicleResult::Missing)?.record;
        if pose.interior != car.pose.interior {
            return Err(VehicleResult::Interior);
        }
        if distance(pose.position, car.pose.position) > 6.0 {
            return Err(VehicleResult::TooFar);
        }
        if car.pose.speed.abs() > 5.0 {
            return Err(VehicleResult::Moving);
        }
        Ok(())
    }
    fn request(
        &mut self,
        player: u32,
        pose: Pose,
        action: VehicleAction,
        now: Instant,
    ) -> VehicleResult {
        match action {
            VehicleAction::Leave => {
                self.leave(player, now);
                VehicleResult::Left
            }
            VehicleAction::Enter { vehicle } => {
                if let Err(result) = self.boarding(player, pose, vehicle) {
                    return result;
                }
                let car = self.cars.get_mut(&vehicle).unwrap();
                if car.record.driver.is_some() {
                    return VehicleResult::Full;
                }
                car.record.driver = Some(player);
                car.record.epoch = car.record.epoch.wrapping_add(1).max(1);
                car.motion_sequence = 0;
                car.motion_time = now;
                car.used = now;
                VehicleResult::Entered
            }
            VehicleAction::Spawn {
                model,
                pose: mut car_pose,
            } => {
                if self.seat(player).is_some_and(|(_, seat)| seat != 0)
                    || self
                        .spawned
                        .get(&player)
                        .is_some_and(|last| now.duration_since(*last) < SPAWN_INTERVAL)
                {
                    return VehicleResult::Busy;
                }
                if pose.interior != car_pose.interior {
                    return VehicleResult::Interior;
                }
                if distance(pose.position, car_pose.position) > 8.0 {
                    return VehicleResult::TooFar;
                }
                if self.cars.len() >= MAX_VEHICLES || self.next_id == u32::MAX {
                    return VehicleResult::Limit;
                }
                self.leave(player, now);
                let id = self.next_id.max(FIRST_VEHICLE_ID);
                self.next_id = id + 1;
                car_pose.speed = 0.0;
                self.cars.insert(
                    id,
                    Entry {
                        record: VehicleRecord {
                            id,
                            model,
                            pose: car_pose,
                            driver: Some(player),
                            epoch: 1,
                            passengers: [None; 3],
                        },
                        used: now,
                        motion_time: now,
                        motion_sequence: 0,
                    },
                );
                self.spawned.insert(player, now);
                VehicleResult::Spawned
            }
        }
    }
    fn ride(&mut self, player: u32, pose: Pose, id: u32, now: Instant) -> RideResult {
        if let Err(result) = self.boarding(player, pose, id) {
            return match result {
                VehicleResult::Busy => RideResult::Driving,
                VehicleResult::Missing => RideResult::Missing,
                VehicleResult::TooFar => RideResult::TooFar,
                VehicleResult::Moving => RideResult::Moving,
                VehicleResult::Interior => RideResult::Interior,
                _ => unreachable!(),
            };
        }
        let car = self.cars.get_mut(&id).unwrap();
        let Some(seat) = car.record.passengers.iter_mut().find(|p| p.is_none()) else {
            return RideResult::Full;
        };
        *seat = Some(player);
        car.used = now;
        RideResult::Seated
    }
    pub fn apply(&mut self, player: u32, mut pose: Pose, now: Instant) -> Pose {
        if let Some(request) = pose.vehicle_request.take() {
            if newer(
                request.sequence,
                self.replies.get(&player).map_or(0, |r| r.sequence),
            ) {
                let result = self.request(player, pose, request.action, now);
                self.replies.insert(
                    player,
                    VehicleReply {
                        sequence: request.sequence,
                        result,
                    },
                );
            }
        }
        if let Some(request) = pose.ride_request.take() {
            if newer(
                request.sequence,
                self.ride_replies.get(&player).map_or(0, |r| r.sequence),
            ) {
                let result = if let Some(id) = request.owner {
                    self.ride(player, pose, id, now)
                } else {
                    // Passenger leave must never revoke an unrelated driver grant.
                    if self.seat(player).is_some_and(|(_, s)| s != 0) {
                        self.leave(player, now);
                    }
                    RideResult::Left
                };
                self.ride_replies.insert(
                    player,
                    RideReply {
                        sequence: request.sequence,
                        result,
                    },
                );
            }
        }
        if let Some(motion) = pose.vehicle_motion.take() {
            if let Some(car) = self.cars.get_mut(&motion.vehicle) {
                let elapsed = now.duration_since(car.motion_time).as_secs_f32().min(1.0);
                if car.record.driver == Some(player)
                    && car.record.epoch == motion.epoch
                    && newer(motion.sequence, car.motion_sequence)
                    && motion.pose.valid()
                    && motion.pose.interior == car.record.pose.interior
                    && distance(car.record.pose.position, motion.pose.position)
                        <= 200.0 * elapsed + 3.0
                {
                    car.record.pose = motion.pose;
                    car.motion_sequence = motion.sequence;
                    car.motion_time = now;
                    car.used = now;
                }
            }
        }
        self.decorate(player, pose)
    }
    pub fn decorate(&self, player: u32, mut pose: Pose) -> Pose {
        pose.driver = None;
        pose.ride = None;
        pose.vehicle = None;
        pose.driving = false;
        pose.vehicle_request = None;
        pose.vehicle_motion = None;
        pose.ride_request = None;
        pose.vehicle_reply = self.replies.get(&player).copied();
        pose.ride_reply = self.ride_replies.get(&player).copied();
        if let Some((id, seat)) = self.seat(player) {
            let car = self.cars[&id].record;
            pose.yaw = car.pose.yaw;
            pose.speed = car.pose.speed;
            pose.pitch = car.pose.pitch;
            pose.roll = car.pose.roll;
            pose.interior = car.pose.interior;
            pose.moving = false;
            if seat == 0 {
                pose.driver = Some(DriverGrant {
                    vehicle: id,
                    epoch: car.epoch,
                });
                pose.vehicle = Some(car.pose);
                pose.driving = true;
                pose.car_model = car.model;
                pose.position = car.pose.position;
            } else {
                let seat = PassengerSeat { owner: id, seat };
                pose.position = seat.feet(car.pose);
                pose.ride = Some(seat);
            }
        }
        pose
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn peer(id: u32) -> Peer {
        Peer {
            id,
            name: format!("P{id}"),
            pose: Pose::default(),
        }
    }
    fn command(
        world: &mut VehicleWorld,
        player: u32,
        seq: u32,
        action: VehicleAction,
        now: Instant,
    ) -> Pose {
        world.apply(
            player,
            Pose {
                vehicle_request: Some(VehicleRequest {
                    sequence: seq,
                    action,
                }),
                ..Pose::default()
            },
            now,
        )
    }
    fn spawn(world: &mut VehicleWorld, now: Instant) -> DriverGrant {
        command(
            world,
            1,
            1,
            VehicleAction::Spawn {
                model: 0,
                pose: VehiclePose::default(),
            },
            now,
        )
        .driver
        .unwrap()
    }
    #[test]
    fn same_driver_seat_has_one_winner_and_transfer_changes_epoch() {
        let now = Instant::now();
        let mut world = VehicleWorld::default();
        let old = spawn(&mut world, now);
        command(&mut world, 1, 2, VehicleAction::Leave, now);
        let winner = command(
            &mut world,
            2,
            1,
            VehicleAction::Enter {
                vehicle: old.vehicle,
            },
            now,
        );
        let loser = command(
            &mut world,
            3,
            1,
            VehicleAction::Enter {
                vehicle: old.vehicle,
            },
            now,
        );
        assert_eq!(winner.driver.unwrap().vehicle, old.vehicle);
        assert_ne!(winner.driver.unwrap().epoch, old.epoch);
        assert_eq!(loser.vehicle_reply.unwrap().result, VehicleResult::Full);
        assert!(loser.driver.is_none());
        assert_eq!(world.snapshot()[0].driver, Some(2));
    }
    #[test]
    fn only_current_driver_with_current_epoch_and_new_sequence_can_move() {
        let now = Instant::now();
        let mut world = VehicleWorld::default();
        let grant = spawn(&mut world, now);
        let mut motion = VehicleMotion {
            vehicle: grant.vehicle,
            epoch: grant.epoch,
            sequence: 1,
            pose: VehiclePose {
                position: [2.0, 0.0, 0.0],
                speed: 5.0,
                ..VehiclePose::default()
            },
        };
        let input = |motion| Pose {
            vehicle_motion: Some(motion),
            ..Pose::default()
        };
        world.apply(2, input(motion), now);
        assert_eq!(world.snapshot()[0].pose.position, [0.; 3]);
        world.apply(1, input(motion), now);
        assert_eq!(world.snapshot()[0].pose.position, [2., 0., 0.]);
        motion.pose.position[0] = 3.;
        world.apply(1, input(motion), now);
        assert_eq!(world.snapshot()[0].pose.position[0], 2.);
        motion.sequence = 2;
        motion.epoch += 1;
        world.apply(1, input(motion), now);
        assert_eq!(world.snapshot()[0].pose.position[0], 2.);
        motion.epoch = grant.epoch;
        motion.vehicle += 1;
        world.apply(1, input(motion), now);
        assert_eq!(world.snapshot()[0].pose.position[0], 2.);
        motion.vehicle = grant.vehicle;
        motion.pose.position[0] = f32::NAN;
        world.apply(1, input(motion), now);
        assert_eq!(world.snapshot()[0].pose.position[0], 2.);
        motion.pose.position[0] = 100.;
        world.apply(1, input(motion), now);
        assert_eq!(world.snapshot()[0].pose.position[0], 2.);
        command(&mut world, 1, 2, VehicleAction::Leave, now);
        command(
            &mut world,
            2,
            1,
            VehicleAction::Enter {
                vehicle: grant.vehicle,
            },
            now,
        );
        motion.pose.position[0] = 3.;
        world.apply(1, input(motion), now);
        assert_eq!(world.snapshot()[0].pose.position[0], 2.);
    }
    #[test]
    fn disconnect_parks_car_and_keeps_passenger_and_late_join_snapshot() {
        let now = Instant::now();
        let mut world = VehicleWorld::default();
        let grant = spawn(&mut world, now);
        let rider = world.apply(
            2,
            Pose {
                ride_request: Some(crate::RideRequest {
                    sequence: 1,
                    owner: Some(grant.vehicle),
                }),
                ..Pose::default()
            },
            now,
        );
        assert_eq!(rider.ride.unwrap().owner, grant.vehicle);
        world.reconcile(&[peer(2), peer(3)], now);
        let car = world.snapshot()[0];
        assert_eq!(car.driver, None);
        assert_eq!(car.pose.speed, 0.);
        assert_eq!(car.passengers[0], Some(2));
        assert!(world.decorate(2, Pose::default()).ride.is_some());
        assert!(command(
            &mut world,
            3,
            1,
            VehicleAction::Enter {
                vehicle: grant.vehicle
            },
            now
        )
        .driver
        .is_some());
        world.reconcile(&[], now);
        assert_eq!(world.snapshot()[0].passengers, [None; 3]);
    }
    #[test]
    fn boarding_checks_distance_interior_speed_and_current_seat() {
        let now = Instant::now();
        let mut world = VehicleWorld::default();
        let grant = spawn(&mut world, now);
        command(&mut world, 1, 2, VehicleAction::Leave, now);
        for (seq, position, interior, expected) in [
            (1, [7., 0., 0.], 0, VehicleResult::TooFar),
            (2, [0.; 3], 1, VehicleResult::Interior),
        ] {
            let out = world.apply(
                2,
                Pose {
                    position,
                    interior,
                    vehicle_request: Some(VehicleRequest {
                        sequence: seq,
                        action: VehicleAction::Enter {
                            vehicle: grant.vehicle,
                        },
                    }),
                    ..Pose::default()
                },
                now,
            );
            assert_eq!(out.vehicle_reply.unwrap().result, expected);
        }
        world
            .cars
            .get_mut(&grant.vehicle)
            .unwrap()
            .record
            .pose
            .speed = 6.;
        assert_eq!(
            command(
                &mut world,
                2,
                3,
                VehicleAction::Enter {
                    vehicle: grant.vehicle
                },
                now
            )
            .vehicle_reply
            .unwrap()
            .result,
            VehicleResult::Moving
        );
        world
            .cars
            .get_mut(&grant.vehicle)
            .unwrap()
            .record
            .pose
            .speed = 0.;
        command(
            &mut world,
            2,
            4,
            VehicleAction::Enter {
                vehicle: grant.vehicle,
            },
            now,
        );
        assert_eq!(
            command(
                &mut world,
                2,
                5,
                VehicleAction::Enter {
                    vehicle: grant.vehicle
                },
                now
            )
            .vehicle_reply
            .unwrap()
            .result,
            VehicleResult::Busy
        );
    }
    #[test]
    fn replayed_commands_cannot_reenter_or_duplicate_spawn_and_wrap_is_supported() {
        let now = Instant::now();
        let mut world = VehicleWorld::default();
        let grant = spawn(&mut world, now);
        command(&mut world, 1, 2, VehicleAction::Leave, now);
        assert!(command(
            &mut world,
            1,
            1,
            VehicleAction::Enter {
                vehicle: grant.vehicle
            },
            now
        )
        .driver
        .is_none());
        assert!(command(
            &mut world,
            1,
            2,
            VehicleAction::Enter {
                vehicle: grant.vehicle
            },
            now
        )
        .driver
        .is_none());
        assert_eq!(world.snapshot().len(), 1);
        assert!(newer(1, u32::MAX));
        assert!(!newer(u32::MAX, 1));
        assert!(!newer(0, u32::MAX));
    }
    #[test]
    fn capacity_spawn_rate_and_parked_expiry_are_bounded() {
        let now = Instant::now();
        let mut world = VehicleWorld::default();
        let grant = spawn(&mut world, now);
        assert_eq!(
            command(
                &mut world,
                1,
                2,
                VehicleAction::Spawn {
                    model: 1,
                    pose: VehiclePose::default()
                },
                now
            )
            .vehicle_reply
            .unwrap()
            .result,
            VehicleResult::Busy
        );
        for i in 1..MAX_VEHICLES {
            let t = now + SPAWN_INTERVAL * (i as u32);
            command(
                &mut world,
                1,
                i as u32 + 2,
                VehicleAction::Spawn {
                    model: 0,
                    pose: VehiclePose::default(),
                },
                t,
            );
        }
        assert_eq!(world.snapshot().len(), MAX_VEHICLES);
        assert_eq!(
            command(
                &mut world,
                1,
                100,
                VehicleAction::Spawn {
                    model: 0,
                    pose: VehiclePose::default()
                },
                now + Duration::from_secs(100)
            )
            .vehicle_reply
            .unwrap()
            .result,
            VehicleResult::Limit
        );
        world.reconcile(&[peer(1)], now + PARKED_LIFETIME + Duration::from_secs(100));
        assert_eq!(world.snapshot().len(), 1, "occupied cars must not expire");
        assert_ne!(world.snapshot()[0].id, grant.vehicle);
        world.reconcile(&[], now + PARKED_LIFETIME + Duration::from_secs(101));
        world.reconcile(&[], now + PARKED_LIFETIME * 2 + Duration::from_secs(102));
        assert!(world.snapshot().is_empty());
    }
    #[test]
    fn passenger_seats_are_unique_and_cannot_be_forged() {
        let now = Instant::now();
        let mut world = VehicleWorld::default();
        let grant = spawn(&mut world, now);
        for player in 2..=4 {
            let out = world.apply(
                player,
                Pose {
                    ride_request: Some(crate::RideRequest {
                        sequence: 1,
                        owner: Some(grant.vehicle),
                    }),
                    ..Pose::default()
                },
                now,
            );
            assert_eq!(out.ride.unwrap().seat, (player - 1) as u8);
        }
        let out = world.apply(
            5,
            Pose {
                ride: Some(PassengerSeat {
                    owner: grant.vehicle,
                    seat: 1,
                }),
                ride_request: Some(crate::RideRequest {
                    sequence: 1,
                    owner: Some(grant.vehicle),
                }),
                ..Pose::default()
            },
            now,
        );
        assert!(out.ride.is_none());
        assert_eq!(out.ride_reply.unwrap().result, RideResult::Full);
    }
}
