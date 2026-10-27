//! Race pickups from MTA maps: nitro, repair and vehicle change. Each player
//! takes them with their own car, so they work alone and on servers alike.
use crate::ORIGIN;
use glam::Vec3;
use sa_scene::{vehicle::Car, MapPickup, PickupKind};

/// How close the car's centre has to come, in metres.
const REACH: f32 = 4.5;
/// Seconds before a taken pickup can be taken again when the map leaves it open.
const DEFAULT_RESPAWN: f32 = 5.0;
/// Seconds of boost in a full nitro tank.
const NITRO_TANK: f32 = 10.0;
/// Extra forward acceleration while the nitro burns, m/s².
const NITRO_ACCELERATION: f32 = 9.0;
/// Nitro pushes the car this far past its normal top speed.
const NITRO_TOP_SPEED: f32 = 1.3;

#[derive(Default)]
pub(crate) struct Pickups {
    /// Each pickup with the seconds left until it can be taken again.
    items: Vec<(MapPickup, f32)>,
    /// Seconds of nitro left in the current car.
    pub nitro: f32,
}
impl Pickups {
    pub fn new(pickups: &[MapPickup]) -> Self {
        Self {
            items: pickups.iter().map(|p| (*p, 0.0)).collect(),
            nitro: 0.0,
        }
    }
    /// Counts down respawns and takes every ready pickup the car touches.
    /// Nitro and repair act on the car at once; vehicle changes are returned
    /// for the caller, which owns the car catalog.
    pub fn update(&mut self, car: &mut Car, dt: f32) -> Vec<i32> {
        let mut changes = Vec::new();
        let at = sa_position(car.position);
        for (pickup, wait) in &mut self.items {
            *wait = (*wait - dt).max(0.0);
            if *wait > 0.0 || Vec3::from(pickup.position).distance(at) > REACH {
                continue;
            }
            *wait = if pickup.respawn_ms == 0 {
                DEFAULT_RESPAWN
            } else {
                pickup.respawn_ms as f32 / 1000.0
            };
            match pickup.kind {
                PickupKind::Nitro => self.nitro = NITRO_TANK,
                PickupKind::Repair => car.damage = Default::default(),
                PickupKind::VehicleChange(id) => changes.push(id),
            }
        }
        changes
    }
    /// Burns nitro while `held`, pushing the car forward.
    pub fn boost(&mut self, car: &mut Car, held: bool, dt: f32) {
        if !held || self.nitro <= 0.0 || car.damage.wrecked || car.speed < 0.0 {
            return;
        }
        self.nitro = (self.nitro - dt).max(0.0);
        let limit = car.handling.top_speed * NITRO_TOP_SPEED;
        if car.speed < limit {
            car.speed = (car.speed + NITRO_ACCELERATION * dt).min(limit);
        }
    }
}
/// Runtime space to San Andreas world coordinates.
fn sa_position(local: Vec3) -> Vec3 {
    Vec3::new(ORIGIN[0] + local.x, ORIGIN[1] - local.z, local.y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn car_at(sa: [f32; 3]) -> Car {
        Car::new(Vec3::new(sa[0] - ORIGIN[0], sa[2], ORIGIN[1] - sa[1]), 0.5)
    }

    #[test]
    fn pickups_act_once_and_respawn() {
        let pickups = [
            MapPickup {
                kind: PickupKind::Nitro,
                position: [10.0, 20.0, 5.0],
                respawn_ms: 0,
            },
            MapPickup {
                kind: PickupKind::Repair,
                position: [11.0, 20.0, 5.0],
                respawn_ms: 1000,
            },
            MapPickup {
                kind: PickupKind::VehicleChange(411),
                position: [12.0, 21.0, 5.0],
                respawn_ms: 0,
            },
            MapPickup {
                kind: PickupKind::Nitro,
                position: [500.0, 20.0, 5.0],
                respawn_ms: 0,
            },
        ];
        let mut state = Pickups::new(&pickups);
        let mut car = car_at([11.0, 20.0, 5.5]);
        car.damage.health = 100.0;
        assert_eq!(state.update(&mut car, 0.016), vec![411]);
        assert_eq!(state.nitro, NITRO_TANK);
        assert_eq!(car.damage, Default::default());
        // Taken pickups wait before they act again.
        car.damage.health = 100.0;
        assert!(state.update(&mut car, 0.5).is_empty());
        assert_eq!(car.damage.health, 100.0);
        state.update(&mut car, 0.6);
        assert_eq!(car.damage, Default::default());
    }

    #[test]
    fn nitro_burns_only_while_held_and_caps_speed() {
        let mut state = Pickups {
            nitro: 1.0,
            ..Default::default()
        };
        let mut car = car_at([0.0, 0.0, 0.0]);
        car.speed = 10.0;
        state.boost(&mut car, false, 0.5);
        assert_eq!((car.speed, state.nitro), (10.0, 1.0));
        state.boost(&mut car, true, 0.5);
        assert!(car.speed > 10.0 && state.nitro == 0.5);
        car.speed = car.handling.top_speed * NITRO_TOP_SPEED;
        state.boost(&mut car, true, 0.5);
        assert_eq!(car.speed, car.handling.top_speed * NITRO_TOP_SPEED);
        assert_eq!(state.nitro, 0.0);
    }
}
