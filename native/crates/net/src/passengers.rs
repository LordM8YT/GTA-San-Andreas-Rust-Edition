//! Host-reserved passenger seats in each connected peer's personal vehicle.
use crate::{Peer, Pose, VehiclePose};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassengerSeat {
    pub owner: u32,
    pub seat: u8,
}
impl PassengerSeat {
    pub fn valid(self) -> bool {
        (1..=3).contains(&self.seat)
    }
    pub fn feet(self, car: VehiclePose) -> [f32; 3] {
        let (side, forward) = match self.seat {
            1 => (0.45, 0.35),
            2 => (-0.45, -0.75),
            _ => (0.45, -0.75),
        };
        [
            car.position[0] + car.yaw.cos() * side + car.yaw.sin() * forward,
            car.position[1] + 0.2,
            car.position[2] - car.yaw.sin() * side + car.yaw.cos() * forward,
        ]
        .map(|value| value.clamp(-20000.0, 20000.0))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RideRequest {
    pub sequence: u32,
    pub owner: Option<u32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RideReply {
    pub sequence: u32,
    pub result: RideResult,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RideResult {
    Seated,
    Left,
    Full,
    TooFar,
    Missing,
    Moving,
    Driving,
    Interior,
    OwnCar,
    Lost,
}
impl RideResult {
    pub fn label(self) -> &'static str {
        match self {
            Self::Seated => "Passenger seat reserved. F to leave.",
            Self::Left => "You left the passenger seat.",
            Self::Full => "All passenger seats are occupied.",
            Self::TooFar => "Move closer to the other player's car.",
            Self::Missing => "That player's car is unavailable.",
            Self::Moving => "The car is moving too fast to board.",
            Self::Driving => "Leave your own car before riding as a passenger.",
            Self::Interior => "You and the vehicle are in different interiors.",
            Self::OwnCar => "Use F to drive your own car.",
            Self::Lost => "Your passenger ride ended: the vehicle changed or left.",
        }
    }
}
#[derive(Clone, Copy)]
struct Reserved {
    seat: PassengerSeat,
    model: u16,
    interior: u8,
    position: [f32; 3],
}
#[derive(Default)]
pub(crate) struct SeatBook {
    seats: HashMap<u32, Reserved>,
    replies: HashMap<u32, RideReply>,
}
fn distance_squared(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.into_iter().zip(b).map(|(a, b)| (a - b).powi(2)).sum()
}
fn vehicle(peers: &[Peer], id: u32) -> Option<(VehiclePose, u16)> {
    let peer = peers.iter().find(|peer| peer.id == id)?;
    Some((peer.pose.vehicle?, peer.pose.car_model))
}
impl SeatBook {
    pub fn reconcile(&mut self, peers: &[Peer]) {
        self.replies
            .retain(|id, _| peers.iter().any(|peer| peer.id == *id));
        self.seats.retain(|id, reserved| {
            if !peers.iter().any(|peer| peer.id == *id) {
                return false;
            }
            let keep = vehicle(peers, reserved.seat.owner).is_some_and(|(car, model)| {
                let keep = model == reserved.model
                    && car.interior == reserved.interior
                    && distance_squared(car.position, reserved.position) <= 30.0 * 30.0;
                reserved.position = car.position;
                keep
            });
            if !keep {
                if let Some(reply) = self.replies.get_mut(id) {
                    reply.result = RideResult::Lost;
                }
            }
            keep
        });
    }
    fn reserve(&mut self, id: u32, pose: Pose, owner: u32, peers: &[Peer]) -> RideResult {
        if owner == id {
            return RideResult::OwnCar;
        }
        if pose.driving {
            return RideResult::Driving;
        }
        let Some((car, model)) = vehicle(peers, owner) else {
            return RideResult::Missing;
        };
        if pose.interior != car.interior {
            return RideResult::Interior;
        }
        if distance_squared(pose.position, car.position) > 6.0 * 6.0 {
            return RideResult::TooFar;
        }
        if car.speed.abs() > 5.0 {
            return RideResult::Moving;
        }
        let Some(seat) = (1..=3).find(|seat| {
            !self.seats.iter().any(|(peer, reserved)| {
                *peer != id && reserved.seat.owner == owner && reserved.seat.seat == *seat
            })
        }) else {
            return RideResult::Full;
        };
        self.seats.insert(
            id,
            Reserved {
                seat: PassengerSeat { owner, seat },
                model,
                interior: car.interior,
                position: car.position,
            },
        );
        RideResult::Seated
    }
    pub fn apply(&mut self, id: u32, mut pose: Pose, peers: &[Peer]) -> Pose {
        if let Some(request) = pose.ride_request.take() {
            let previous = self.replies.get(&id).map_or(0, |reply| reply.sequence);
            let difference = request.sequence.wrapping_sub(previous);
            if request.sequence != 0 && difference != 0 && difference < 0x8000_0000 {
                let result = if let Some(owner) = request.owner {
                    self.reserve(id, pose, owner, peers)
                } else {
                    self.seats.remove(&id);
                    RideResult::Left
                };
                self.replies.insert(
                    id,
                    RideReply {
                        sequence: request.sequence,
                        result,
                    },
                );
            }
        }
        self.decorate(id, pose, peers)
    }
    pub fn decorate(&self, id: u32, mut pose: Pose, peers: &[Peer]) -> Pose {
        // Client-provided grants/replies never establish a reservation.
        pose.ride = self.seats.get(&id).map(|reserved| reserved.seat);
        pose.ride_reply = self.replies.get(&id).copied();
        pose.ride_request = None;
        if let Some(seat) = pose.ride {
            if let Some((car, _)) = vehicle(peers, seat.owner) {
                pose.position = seat.feet(car);
                pose.yaw = car.yaw;
                pose.speed = car.speed;
                pose.interior = car.interior;
                pose.driving = false;
                pose.moving = false;
            }
        }
        pose
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn peers() -> Vec<Peer> {
        (0..5)
            .map(|id| Peer {
                id,
                name: format!("Player{id}"),
                pose: Pose {
                    vehicle: (id == 0).then_some(VehiclePose::default()),
                    ..Pose::default()
                },
            })
            .collect()
    }
    fn request(
        book: &mut SeatBook,
        peers: &[Peer],
        id: u32,
        sequence: u32,
        owner: Option<u32>,
    ) -> Pose {
        book.apply(
            id,
            Pose {
                ride_request: Some(RideRequest { sequence, owner }),
                ..peers[id as usize].pose
            },
            peers,
        )
    }
    #[test]
    fn seats_are_unique_full_car_rejects_and_leave_frees_a_seat() {
        let peers = peers();
        let mut book = SeatBook::default();
        for id in 1..=3 {
            assert_eq!(
                request(&mut book, &peers, id, 1, Some(0))
                    .ride
                    .unwrap()
                    .seat,
                id as u8
            );
        }
        assert_eq!(
            request(&mut book, &peers, 4, 1, Some(0))
                .ride_reply
                .unwrap()
                .result,
            RideResult::Full
        );
        assert!(request(&mut book, &peers, 2, 2, None).ride.is_none());
        assert_eq!(
            request(&mut book, &peers, 4, 2, Some(0)).ride.unwrap().seat,
            2
        );
    }
    #[test]
    fn boarding_validates_distance_speed_interior_driver_and_owner() {
        for (id, owner, change, result) in [
            (1, 0, 0, RideResult::TooFar),
            (1, 0, 1, RideResult::Moving),
            (1, 0, 2, RideResult::Interior),
            (1, 0, 3, RideResult::Driving),
            (0, 0, 4, RideResult::OwnCar),
            (1, 99, 4, RideResult::Missing),
        ] {
            let mut peers = peers();
            let mut book = SeatBook::default();
            match change {
                0 => peers[1].pose.position[0] = 6.1,
                1 => peers[0].pose.vehicle.as_mut().unwrap().speed = 5.1,
                2 => peers[1].pose.interior = 1,
                3 => peers[1].pose.driving = true,
                _ => (),
            }
            let pose = request(&mut book, &peers, id, 1, Some(owner));
            assert_eq!(pose.ride_reply.unwrap().result, result);
            assert!(pose.ride.is_none());
        }
    }
    #[test]
    fn client_cannot_grant_seats_and_replayed_requests_do_not_reboard() {
        let peers = peers();
        let mut book = SeatBook::default();
        let spoof = book.apply(
            1,
            Pose {
                ride: Some(PassengerSeat { owner: 0, seat: 1 }),
                ride_reply: Some(RideReply {
                    sequence: 1,
                    result: RideResult::Seated,
                }),
                ..Pose::default()
            },
            &peers,
        );
        assert!(spoof.ride.is_none() && spoof.ride_reply.is_none());
        request(&mut book, &peers, 1, 1, Some(0));
        request(&mut book, &peers, 1, 2, None);
        assert!(request(&mut book, &peers, 1, 1, Some(0)).ride.is_none());
        assert!(request(&mut book, &peers, 1, 2, Some(0)).ride.is_none());
        assert!(request(&mut book, &peers, 1, 3, Some(0)).ride.is_some());
        book.replies.get_mut(&1).unwrap().sequence = u32::MAX;
        assert!(request(&mut book, &peers, 1, 1, None).ride.is_none());
    }
    #[test]
    fn driver_motion_moves_passenger_but_preserves_their_own_parked_car() {
        let mut peers = peers();
        let mut book = SeatBook::default();
        peers[1].pose.vehicle = Some(VehiclePose {
            position: [-20.0, 0.0, 0.0],
            ..VehiclePose::default()
        });
        request(&mut book, &peers, 1, 1, Some(0));
        let car = peers[0].pose.vehicle.as_mut().unwrap();
        car.position = [10.0, 2.0, 5.0];
        car.yaw = 1.0;
        car.speed = 15.0;
        book.reconcile(&peers);
        let pose = book.decorate(1, peers[1].pose, &peers);
        assert_eq!(
            pose.position,
            pose.ride.unwrap().feet(peers[0].pose.vehicle.unwrap())
        );
        assert_eq!(pose.vehicle, peers[1].pose.vehicle);
        assert_eq!(pose.speed, 15.0);
        assert!(!pose.driving && !pose.moving);
    }
    #[test]
    fn missing_replaced_teleported_and_interior_changed_cars_revoke_grants() {
        for change in 0..5 {
            let mut peers = peers();
            let mut book = SeatBook::default();
            request(&mut book, &peers, 1, 1, Some(0));
            match change {
                0 => {
                    peers.remove(0);
                }
                1 => peers[0].pose.vehicle = None,
                2 => peers[0].pose.car_model = 1,
                3 => peers[0].pose.vehicle.as_mut().unwrap().position[0] = 31.0,
                _ => peers[0].pose.vehicle.as_mut().unwrap().interior = 1,
            }
            book.reconcile(&peers);
            let pose = book.decorate(1, Pose::default(), &peers);
            assert!(pose.ride.is_none());
            assert_eq!(pose.ride_reply.unwrap().result, RideResult::Lost);
            assert!(request(&mut book, &self::peers(), 1, 1, Some(0))
                .ride
                .is_none());
        }
    }
}
