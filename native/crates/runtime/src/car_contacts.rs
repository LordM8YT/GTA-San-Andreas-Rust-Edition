//! Contacts between vehicles, and between vehicles and the player on foot.
//!
//! Every car is a box on the ground plane: the player's own car (driven or
//! parked), traffic, and other players' cars. Boxes are tested with the
//! separating axis test, which is a handful of dot products per pair, so
//! even a full street of traffic costs next to nothing on weak PCs.
use super::State;
use glam::{Vec2, Vec3};

/// Radius of CJ's body when pushed out of a car.
const PED_RADIUS: f32 = 0.35;

/// A body's box on the ground: centre, heading and half extents
/// (width, length), plus the height range it occupies.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Footprint {
    pub center: Vec3,
    pub yaw: f32,
    pub half: Vec2,
    pub bottom: f32,
    pub top: f32,
}

impl Footprint {
    /// A car box from its ground position, heading and measured size.
    pub fn car(ground: Vec3, yaw: f32, size: [f32; 3]) -> Self {
        Self {
            center: ground,
            yaw,
            half: Vec2::new(size[0], size[1]),
            bottom: ground.y - 0.5,
            top: ground.y + size[2],
        }
    }
    /// A person standing at `feet`.
    pub fn person(feet: Vec3) -> Self {
        Self {
            center: feet,
            yaw: 0.0,
            half: Vec2::splat(0.3),
            bottom: feet.y,
            top: feet.y + 1.8,
        }
    }
    pub fn forward(&self) -> Vec3 {
        Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos())
    }
    pub fn side(&self) -> Vec3 {
        Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin())
    }
    fn vertical_overlap(&self, other: &Self) -> bool {
        self.bottom < other.top && other.bottom < self.top
    }
    fn radius_along(&self, axis: Vec3) -> f32 {
        self.half.x * self.side().dot(axis).abs() + self.half.y * self.forward().dot(axis).abs()
    }
    /// The smallest horizontal move that takes `self` out of `other`, if
    /// they overlap.
    pub fn push_out_of(&self, other: &Self) -> Option<Vec3> {
        if !self.vertical_overlap(other) {
            return None;
        }
        let offset = other.center - self.center;
        let offset = Vec3::new(offset.x, 0.0, offset.z);
        let mut best: Option<(f32, Vec3)> = None;
        for axis in [self.side(), self.forward(), other.side(), other.forward()] {
            let distance = offset.dot(axis);
            let overlap = self.radius_along(axis) + other.radius_along(axis) - distance.abs();
            if overlap <= 0.0 {
                return None;
            }
            if best.is_none_or(|(depth, _)| overlap < depth) {
                let away = if distance > 0.0 { -axis } else { axis };
                best = Some((overlap, away));
            }
        }
        best.map(|(depth, away)| away * depth)
    }
    /// How far along a ray from `origin` (unit `direction`) it first
    /// enters this box, if within `range`.
    pub fn ray_hit(&self, origin: Vec3, direction: Vec3, range: f32) -> Option<f32> {
        let offset = origin - self.center;
        let (mut near, mut far) = (0.0_f32, range);
        for (start, step, low, high) in [
            (
                offset.dot(self.side()),
                direction.dot(self.side()),
                -self.half.x,
                self.half.x,
            ),
            (
                offset.dot(self.forward()),
                direction.dot(self.forward()),
                -self.half.y,
                self.half.y,
            ),
            (origin.y, direction.y, self.bottom, self.top),
        ] {
            if step.abs() < 1e-6 {
                if start < low || start > high {
                    return None;
                }
                continue;
            }
            let (a, b) = ((low - start) / step, (high - start) / step);
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
        Some(near)
    }
    /// The smallest horizontal move that takes a round body of `radius` at
    /// `point` out of this box.
    pub fn push_point_out(&self, point: Vec3, radius: f32, height: f32) -> Option<Vec3> {
        if point.y >= self.top || point.y + height <= self.bottom {
            return None;
        }
        let offset = point - self.center;
        let local = Vec2::new(offset.dot(self.side()), offset.dot(self.forward()));
        let closest = local.clamp(-self.half, self.half);
        let to_point = local - closest;
        let push = if to_point == Vec2::ZERO {
            // Inside the box: leave through the nearest side.
            let room = self.half - local.abs();
            if room.x < room.y {
                Vec2::new(local.x.signum() * (room.x + radius), 0.0)
            } else {
                Vec2::new(0.0, local.y.signum() * (room.y + radius))
            }
        } else {
            let distance = to_point.length();
            if distance >= radius {
                return None;
            }
            to_point / distance * (radius - distance)
        };
        Some(self.side() * push.x + self.forward() * push.y)
    }
}

/// What a footprint belongs to, so a push moves the right thing.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Owner {
    Traffic(usize),
    Remote,
    OwnParked,
}

impl State {
    /// Footprints of every car other than the one the player drives.
    fn other_cars(&self) -> Vec<(Owner, Footprint, Vec3)> {
        let mut cars = Vec::new();
        for (index, car) in self.traffic.cars.iter().enumerate() {
            cars.push((
                Owner::Traffic(index),
                Footprint::car(car.position, car.yaw, car.size),
                Vec3::new(car.yaw.sin(), 0.0, car.yaw.cos()) * car.speed,
            ));
        }
        for car in &self.remote_traffic {
            cars.push((Owner::Remote, car.footprint(), car.velocity()));
        }
        for actor in self.remote_actors.iter().filter(|a| a.car_visible) {
            if let Some(car) = actor.current.vehicle {
                let forward = Vec3::new(car.yaw.sin(), 0.0, car.yaw.cos());
                cars.push((
                    Owner::Remote,
                    Footprint::car(Vec3::from_array(car.position), car.yaw, actor.car_size),
                    forward * car.speed,
                ));
            }
        }
        if !self.driving {
            if let Some((car, _)) = &self.car {
                cars.push((
                    Owner::OwnParked,
                    Footprint::car(car.position - Vec3::Y * car.clearance, car.yaw, car.size),
                    Vec3::ZERO,
                ));
            }
        }
        cars
    }

    /// Keep the player's car and CJ out of every other car. Only local bodies
    /// move: other players resolve their own cars, and traffic stays on its
    /// lane but pulls up after a knock.
    pub(super) fn resolve_vehicle_contacts(&mut self) {
        if self.passenger.is_some() {
            return;
        }
        let others = self.other_cars();
        if self.driving {
            let (Some(world), Some((car, _))) = (&self.collision, &mut self.car) else {
                return;
            };
            let mut moved = Vec3::ZERO;
            let mut crimes = Vec::new();
            for (owner, other, velocity) in &others {
                if *owner == Owner::OwnParked {
                    continue;
                }
                let mine =
                    Footprint::car(car.position - Vec3::Y * car.clearance, car.yaw, car.size);
                let Some(push) = mine.push_out_of(other) else {
                    continue;
                };
                // Only the speed into the other car counts, so resting
                // against it does no further harm.
                let normal = push.normalize_or_zero();
                let impact = (*velocity - car.velocity()).dot(normal).max(0.0);
                let before = car.position;
                car.collide(world, push, *velocity);
                moved += car.position - before;
                if let Owner::Traffic(index) = owner {
                    if let Some(hit) = self.traffic.cars.get_mut(*index) {
                        hit.knock(impact, normal);
                        // Only the player driving into a car is a crime, not
                        // a police car pulling up against them.
                        if impact > 2.0 && car.speed.abs() > 3.0 {
                            crimes.push((hit.police.is_some(), hit.position));
                        }
                    }
                }
            }
            // The chase camera follows the body it was placed behind.
            self.position += moved;
            // People in the way are knocked down, harder the faster the car.
            let speed = car.velocity().length();
            let mine = Footprint::car(car.position - Vec3::Y * car.clearance, car.yaw, car.size);
            let hits: Vec<(usize, Vec3)> = self
                .crowd
                .walkers
                .iter()
                .enumerate()
                .filter(|(_, walker)| walker.dead_for.is_none())
                .filter_map(|(index, walker)| {
                    mine.push_point_out(walker.feet, PED_RADIUS, 1.8)
                        .map(|away| (index, away))
                })
                .collect();
            for (index, away) in hits {
                self.crowd.run_over(index, speed, away);
            }
            for (police, at) in crimes {
                let crime = if police {
                    crate::wanted::Crime::HitPoliceCar
                } else {
                    crate::wanted::Crime::DamageCar
                };
                self.report_crime(crime, at);
            }
        } else if self.walking {
            let Some(player) = &mut self.player else {
                return;
            };
            for (_, other, _) in &others {
                if let Some(push) = other.push_point_out(player.feet, PED_RADIUS, 1.8) {
                    player.feet += Vec3::new(push.x, 0.0, push.z);
                }
            }
            self.position = player.eye();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SALOON: [f32; 3] = [0.95, 2.4, 1.5];

    #[test]
    fn shots_hit_a_car_box_and_miss_beside_or_over_it() {
        let car = Footprint::car(Vec3::ZERO, 0.0, SALOON);
        // From the side at door height.
        let hit = car.ray_hit(Vec3::new(-10.0, 1.0, 0.0), Vec3::X, 50.0);
        assert!((hit.unwrap() - (10.0 - 0.95)).abs() < 1e-4, "{hit:?}");
        // Too far away, beside it, or over the roof.
        assert_eq!(car.ray_hit(Vec3::new(-10.0, 1.0, 0.0), Vec3::X, 5.0), None);
        assert_eq!(car.ray_hit(Vec3::new(-10.0, 1.0, 3.0), Vec3::X, 50.0), None);
        assert_eq!(car.ray_hit(Vec3::new(-10.0, 2.0, 0.0), Vec3::X, 50.0), None);
        // Shooting down onto the bonnet from above.
        let down = car.ray_hit(Vec3::new(0.0, 6.0, 2.0), Vec3::NEG_Y, 50.0);
        assert!((down.unwrap() - 4.5).abs() < 1e-4, "{down:?}");
    }

    #[test]
    fn separate_cars_do_not_touch() {
        let a = Footprint::car(Vec3::ZERO, 0.0, SALOON);
        let b = Footprint::car(Vec3::new(3.0, 0.0, 0.0), 0.0, SALOON);
        assert_eq!(a.push_out_of(&b), None);
    }

    #[test]
    fn a_rear_end_pushes_back_along_the_road() {
        let a = Footprint::car(Vec3::ZERO, 0.0, SALOON);
        let b = Footprint::car(Vec3::new(0.0, 0.0, 4.0), 0.0, SALOON);
        let push = a.push_out_of(&b).unwrap();
        assert!(push.z < 0.0 && push.x.abs() < 1e-5, "{push}");
        assert!((push.length() - 0.8).abs() < 1e-4);
        // After the push the cars only just touch.
        let moved = Footprint {
            center: a.center + push * 1.001,
            ..a
        };
        assert_eq!(moved.push_out_of(&b), None);
    }

    #[test]
    fn a_t_bone_pushes_sideways_out_of_the_crossing_car() {
        // A crosses B's path from the side, nose first into B's door.
        let a = Footprint::car(
            Vec3::new(-3.0, 0.0, 0.0),
            std::f32::consts::FRAC_PI_2,
            SALOON,
        );
        let b = Footprint::car(Vec3::ZERO, 0.0, SALOON);
        let push = a.push_out_of(&b).unwrap();
        assert!(push.x < 0.0, "{push}");
        assert!((push.length() - 0.35).abs() < 1e-4, "{push}");
    }

    #[test]
    fn rotated_cars_that_only_seem_close_do_not_touch() {
        // Diagonal neighbours: their axis-aligned bounds overlap but the boxes don't.
        let a = Footprint::car(Vec3::ZERO, std::f32::consts::FRAC_PI_4, SALOON);
        let b = Footprint::car(
            Vec3::new(2.6, 0.0, -2.6),
            std::f32::consts::FRAC_PI_4,
            SALOON,
        );
        assert_eq!(a.push_out_of(&b), None);
    }

    #[test]
    fn a_car_far_above_does_not_collide() {
        let a = Footprint::car(Vec3::ZERO, 0.0, SALOON);
        let b = Footprint::car(Vec3::new(0.0, 8.0, 0.0), 0.0, SALOON);
        assert_eq!(a.push_out_of(&b), None);
    }

    #[test]
    fn cj_is_pushed_out_of_a_car_he_walks_into() {
        let car = Footprint::car(Vec3::ZERO, 0.0, SALOON);
        // Beside the door, slightly inside the body.
        let push = car
            .push_point_out(Vec3::new(1.1, 0.0, 0.0), PED_RADIUS, 1.8)
            .unwrap();
        assert!(push.x > 0.0 && (1.1 + push.x - (0.95 + PED_RADIUS)).abs() < 1e-4);
        // Deep inside: leaves through the nearest side.
        let push = car
            .push_point_out(Vec3::new(0.0, 0.0, 2.0), PED_RADIUS, 1.8)
            .unwrap();
        assert!(push.z > 0.0 && push.x.abs() < 1e-5);
        // Clear of the car, or standing on its roof: nothing to do.
        assert_eq!(
            car.push_point_out(Vec3::new(2.0, 0.0, 0.0), PED_RADIUS, 1.8),
            None
        );
        assert_eq!(
            car.push_point_out(Vec3::new(0.0, 1.6, 0.0), PED_RADIUS, 1.8),
            None
        );
    }
}
