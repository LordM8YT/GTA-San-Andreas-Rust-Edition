//! Server-granted passenger seats in shared vehicles.
use crate::VehiclePose;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassengerSeat {
    /// Stable vehicle ID, never a player ID.
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
            Self::TooFar => "Move closer to the vehicle.",
            Self::Missing => "That vehicle is unavailable.",
            Self::Moving => "The car is moving too fast to board.",
            Self::Driving => "Leave your own car before riding as a passenger.",
            Self::Interior => "You and the vehicle are in different interiors.",
            Self::OwnCar => "Use F to drive your own car.",
            Self::Lost => "Your passenger ride ended.",
        }
    }
}
