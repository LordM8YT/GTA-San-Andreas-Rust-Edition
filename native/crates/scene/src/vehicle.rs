//! Initial arcade vehicle controller. Position is the model origin.
use crate::collision::CollisionWorld;
use glam::Vec3;
pub struct Car {
    pub position: Vec3,
    pub yaw: f32,
    pub speed: f32,
    pub clearance: f32,
    vertical_speed: f32,
}
impl Car {
    pub fn new(position: Vec3, clearance: f32) -> Self {
        Self {
            position,
            yaw: 0.0,
            speed: 0.0,
            clearance,
            vertical_speed: 0.0,
        }
    }
    pub fn forward(&self) -> Vec3 {
        Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos())
    }
    fn body_clear(&self, world: &CollisionWorld, position: Vec3) -> bool {
        let feet = position - Vec3::Y * self.clearance;
        [-1.3, 0.0, 1.3].into_iter().all(|offset| {
            let probe = feet + self.forward() * offset;
            world.resolve_body(probe, 0.9, 1.5).distance_squared(probe) < 0.0001
        })
    }
    /// Find a nearby supported footprint, with walls and roof clearance.
    pub fn spawn_near(world: &CollisionWorld, eye: Vec3, yaw: f32, clearance: f32) -> Option<Self> {
        let forward = Vec3::new(yaw.sin(), 0.0, yaw.cos());
        let side = Vec3::new(yaw.cos(), 0.0, -yaw.sin());
        for offset in [
            forward * 5.0,
            side * 5.0,
            -side * 5.0,
            -forward * 5.0,
            forward * 8.0,
        ] {
            let target = eye + offset;
            let mut low = f32::INFINITY;
            let mut high = f32::NEG_INFINITY;
            let mut supported = true;
            for x in [-0.7, 0.0, 0.7] {
                for z in [-1.3, 0.0, 1.3] {
                    if let Some(ground) =
                        world.ground_below(target + side * x + forward * z, eye.y - 1.2)
                    {
                        low = low.min(ground);
                        high = high.max(ground);
                    } else {
                        supported = false;
                    }
                }
            }
            if !supported || high - low > 0.8 || low < eye.y - 6.0 {
                continue;
            }
            let mut car = Self::new(Vec3::new(target.x, high + clearance, target.z), clearance);
            car.yaw = yaw;
            let feet = car.position - Vec3::Y * clearance;
            let roof_clear = [-1.3, 0.0, 1.3].into_iter().all(|z| {
                world
                    .ceiling_above(feet + forward * z, high + 0.05)
                    .is_none_or(|roof| roof >= high + 1.5)
            });
            if roof_clear && car.body_clear(world, car.position) {
                return Some(car);
            }
        }
        None
    }
    pub fn exit_player(&self, world: &CollisionWorld) -> Option<crate::collision::Player> {
        let side = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        for direction in [side, -side, -self.forward(), self.forward()] {
            let desired = self.position + direction * 2.5;
            if let Some(player) = world.standing_at(desired, self.position.y - self.clearance + 0.6)
            {
                // Do not teleport through a wall to an otherwise clear floor.
                let from = self.position + Vec3::Y * 0.6;
                if world.clip_camera(from, player.eye()).distance(player.eye()) < 0.01 {
                    return Some(player);
                }
            }
        }
        None
    }
    pub fn step(
        &mut self,
        world: &CollisionWorld,
        throttle: f32,
        steer: f32,
        brake: bool,
        handling: f32,
        dt: f32,
    ) {
        let dt = dt.clamp(0.0, 0.05);
        if dt == 0.0 {
            return;
        }
        let handling = handling.clamp(0.5, 1.5);
        self.speed = (self.speed + throttle.clamp(-1.0, 1.0) * 8.0 * handling * dt)
            .clamp(-10.0 * handling, 35.0 * handling);
        let deceleration = if brake { 24.0 } else { 1.2 };
        self.speed = self.speed.signum() * (self.speed.abs() - deceleration * dt).max(0.0);
        let old_yaw = self.yaw;
        self.yaw += steer.clamp(-1.0, 1.0)
            * self.speed.signum()
            * (self.speed.abs() / 8.0).min(1.0)
            * handling
            * 1.2
            * dt;
        if !self.body_clear(world, self.position) {
            self.yaw = old_yaw;
        }
        let motion = self.forward() * self.speed * dt;
        let steps = ((motion.length() / 0.15).ceil() as usize).clamp(1, 20);
        for _ in 0..steps {
            let desired = self.position + motion / steps as f32;
            if !self.body_clear(world, desired) {
                self.speed = 0.0;
                break;
            }
            self.position = desired;
        }
        self.vertical_speed -= 17.0 * dt;
        self.position.y += self.vertical_speed * dt;
        if let Some(ground) =
            world.ground_below(self.position, self.position.y - self.clearance + 0.45)
        {
            if self.position.y - self.clearance <= ground + 0.4 {
                self.position.y = ground + self.clearance;
                self.vertical_speed = 0.0;
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Batch, Vertex};
    fn mesh(points: &[[f32; 3]]) -> Batch {
        Batch {
            key: "test".into(),
            alpha: false,
            animated: false,
            vertices: points
                .iter()
                .map(|p| Vertex {
                    position: *p,
                    ..Default::default()
                })
                .collect(),
        }
    }
    fn floor(z0: f32, z1: f32, y: f32) -> Batch {
        mesh(&[
            [-20.0, y, z0],
            [20.0, y, z0],
            [20.0, y, z1],
            [-20.0, y, z0],
            [20.0, y, z1],
            [-20.0, y, z1],
        ])
    }
    #[test]
    fn spawn_needs_full_support_and_clear_roof_and_tries_other_sides() {
        let eye = Vec3::Y * 1.6;
        let wall = mesh(&[
            [-3.0, 0.0, 5.0],
            [-3.0, 4.0, 5.0],
            [3.0, 4.0, 5.0],
            [-3.0, 0.0, 5.0],
            [3.0, 4.0, 5.0],
            [3.0, 0.0, 5.0],
        ]);
        let world = CollisionWorld::from_batches(&[floor(-20.0, 20.0, 0.0), wall]);
        let placed = Car::spawn_near(&world, eye, 0.0, 0.6).unwrap();
        assert!(placed.position.x > 4.0 && placed.body_clear(&world, placed.position));
        let narrow = CollisionWorld::from_batches(&[floor(4.9, 5.1, 0.0)]);
        assert!(Car::spawn_near(&narrow, eye, 0.0, 0.6).is_none());
        let low_roof =
            CollisionWorld::from_batches(&[floor(-20.0, 20.0, 0.0), floor(-20.0, 20.0, 1.2)]);
        assert!(Car::spawn_near(&low_roof, eye, 0.0, 0.6).is_none());
        let empty = CollisionWorld::from_batches(&[]);
        assert!(Car::spawn_near(&empty, eye, 0.0, 0.6).is_none());
    }
    #[test]
    fn steering_does_not_rotate_the_body_through_a_nearby_wall() {
        let wall = mesh(&[
            [0.94, 0.0, -20.0],
            [0.94, 4.0, -20.0],
            [0.94, 4.0, 20.0],
            [0.94, 0.0, -20.0],
            [0.94, 4.0, 20.0],
            [0.94, 0.0, 20.0],
        ]);
        let world = CollisionWorld::from_batches(&[floor(-20.0, 20.0, 0.0), wall]);
        let mut car = Car::new(Vec3::Y * 0.6, 0.6);
        car.speed = 8.0;
        car.step(&world, 0.0, 1.0, false, 1.0, 0.05);
        assert_eq!(car.yaw, 0.0);
        assert!(car.position.z > 0.0 && car.body_clear(&world, car.position));
    }
    #[test]
    fn car_accelerates_brakes_and_stops_at_wall() {
        let geometry = Batch {
            key: "test".into(),
            alpha: false,
            animated: false,
            vertices: [
                [-100.0, 0.0, -100.0],
                [100.0, 0.0, -100.0],
                [100.0, 0.0, 100.0],
                [-100.0, 0.0, -100.0],
                [100.0, 0.0, 100.0],
                [-100.0, 0.0, 100.0],
                [-5.0, 0.0, 8.0],
                [-5.0, 4.0, 8.0],
                [5.0, 4.0, 8.0],
                [-5.0, 0.0, 8.0],
                [5.0, 4.0, 8.0],
                [5.0, 0.0, 8.0],
            ]
            .map(|p| Vertex {
                position: p,
                ..Default::default()
            })
            .to_vec(),
        };
        let world = CollisionWorld::from_batches(&[geometry]);
        let mut car = Car::new(Vec3::Y * 0.6, 0.6);
        for _ in 0..240 {
            car.step(&world, 1.0, 0.0, false, 1.0, 1.0 / 60.0);
        }
        assert!(car.position.z > 1.0 && car.position.z < 7.11);
        assert!(car.position.y > 0.5);
        assert!(car.speed < 1.0);
        car.position.z = 0.0;
        car.speed = 15.0;
        for _ in 0..30 {
            car.step(&world, 0.0, 0.0, true, 1.0, 1.0 / 60.0);
        }
        assert!(car.speed < 4.0);
    }
}
