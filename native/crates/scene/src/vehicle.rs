//! Lightweight vehicle dynamics with tire slip and steering inertia.
//! Position is the model origin; speed and slip are in metres per second.
use crate::collision::CollisionWorld;
use glam::Vec3;
pub struct Car {
    pub position: Vec3,
    pub yaw: f32,
    pub speed: f32,
    pub clearance: f32,
    vertical_speed: f32,
    lateral_speed: f32,
    steering_angle: f32,
    yaw_rate: f32,
}
impl Car {
    pub fn new(position: Vec3, clearance: f32) -> Self {
        Self {
            position,
            yaw: 0.0,
            speed: 0.0,
            clearance,
            vertical_speed: 0.0,
            lateral_speed: 0.0,
            steering_angle: 0.0,
            yaw_rate: 0.0,
        }
    }
    pub fn forward(&self) -> Vec3 {
        Vec3::new(self.yaw.sin(), 0.0, self.yaw.cos())
    }
    pub fn stop(&mut self) {
        self.speed = 0.0;
        self.lateral_speed = 0.0;
        self.yaw_rate = 0.0;
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
        handbrake: bool,
        handling: f32,
        dt: f32,
    ) {
        let dt = dt.clamp(0.0, 0.05);
        if dt == 0.0 {
            return;
        }
        // Fixed maximum integration interval keeps grip consistent across frame rates.
        let steps = (dt / (1.0 / 120.0)).ceil() as usize;
        for _ in 0..steps {
            self.integrate(
                world,
                throttle,
                steer,
                handbrake,
                handling,
                dt / steps as f32,
            );
        }
    }
    fn integrate(
        &mut self,
        world: &CollisionWorld,
        throttle: f32,
        steer: f32,
        handbrake: bool,
        handling: f32,
        dt: f32,
    ) {
        let handling = handling.clamp(0.5, 1.5);
        let throttle = throttle.clamp(-1.0, 1.0);
        let side = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        let mut velocity = self.forward() * self.speed + side * self.lateral_speed;
        let grounded = world
            .ground_below(self.position, self.position.y - self.clearance + 0.45)
            .is_some_and(|ground| self.position.y - self.clearance <= ground + 0.45);
        let grip = if grounded { 1.0 } else { 0.0 };
        // Steering lock falls with speed; wheel input and body rotation both have inertia.
        let lock = 0.55 / (1.0 + (self.speed.abs() / 16.0).powi(2));
        let target_steering = steer.clamp(-1.0, 1.0) * lock;
        self.steering_angle += (target_steering - self.steering_angle).clamp(-1.7 * dt, 1.7 * dt);
        let braking = throttle * self.speed < -0.15;
        let acceleration = if braking {
            -self.speed.signum() * 10.5 * throttle.abs()
        } else if throttle >= 0.0 {
            throttle * 6.2 * (1.0 - (self.speed.max(0.0) / 48.0).powi(2)).max(0.0)
        } else {
            throttle * 3.8 * (1.0 - (-self.speed / 10.0).powi(2)).max(0.0)
        };
        let old_speed = self.speed;
        self.speed += acceleration * grip * dt;
        if braking && self.speed.signum() != old_speed.signum() {
            self.speed = 0.0;
        }
        let resistance =
            (0.28 + 0.0035 * self.speed * self.speed + if handbrake { 7.0 * grip } else { 0.0 })
                * dt;
        self.speed = self.speed.signum() * (self.speed.abs() - resistance).max(0.0);
        velocity += self.forward() * (self.speed - old_speed);
        let desired_yaw_rate = self.speed / 2.7 * self.steering_angle.tan();
        let max_yaw_rate =
            (8.5 * handling / self.speed.abs().max(3.0)) * if handbrake { 1.55 } else { 1.0 };
        let target_yaw_rate = desired_yaw_rate.clamp(-max_yaw_rate, max_yaw_rate) * grip;
        self.yaw_rate += (target_yaw_rate - self.yaw_rate) * (1.0 - (-5.0 * dt).exp());
        let old_yaw = self.yaw;
        self.yaw += self.yaw_rate * dt;
        if !self.body_clear(world, self.position) {
            self.yaw = old_yaw;
            self.yaw_rate = 0.0;
        }
        let side = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        self.speed = velocity.dot(self.forward());
        self.lateral_speed = velocity.dot(side);
        // Limited tire force preserves momentum through corners. The handbrake
        // reduces rear grip instead of making the car stop instantly.
        let lateral_acceleration = (-self.lateral_speed * 7.0)
            .clamp(-8.5 * handling, 8.5 * handling)
            * grip
            * if handbrake { 0.16 } else { 1.0 };
        self.lateral_speed += lateral_acceleration * dt;
        let motion = (self.forward() * self.speed + side * self.lateral_speed) * dt;
        let steps = ((motion.length() / 0.15).ceil() as usize).clamp(1, 20);
        for _ in 0..steps {
            let desired = self.position + motion / steps as f32;
            if !self.body_clear(world, desired) {
                self.stop();
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
        for _ in 0..120 {
            car.step(&world, 0.0, 1.0, false, 1.0, 1.0 / 60.0);
            assert!(car.body_clear(&world, car.position));
        }
        assert!(car.yaw.abs() < 0.04);
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
        for _ in 0..90 {
            car.step(&world, -1.0, 0.0, false, 1.0, 1.0 / 60.0);
        }
        assert!(car.speed.abs() < 4.0);
    }
    #[test]
    fn brake_slows_before_reverse_and_handbrake_preserves_slip() {
        let world = CollisionWorld::from_batches(&[floor(-500.0, 500.0, 0.0)]);
        let mut car = Car::new(Vec3::Y * 0.6, 0.6);
        car.speed = 15.0;
        car.step(&world, -1.0, 0.0, false, 1.0, 0.05);
        assert!(car.speed > 14.0 && car.speed < 15.0);
        for _ in 0..180 {
            car.step(&world, -1.0, 0.0, false, 1.0, 1.0 / 60.0);
        }
        assert!(car.speed < -1.0);
        let mut normal = Car::new(Vec3::Y * 0.6, 0.6);
        let mut sliding = Car::new(Vec3::Y * 0.6, 0.6);
        normal.speed = 18.0;
        sliding.speed = 18.0;
        for _ in 0..30 {
            normal.step(&world, 0.0, 1.0, false, 1.0, 1.0 / 60.0);
            sliding.step(&world, 0.0, 1.0, true, 1.0, 1.0 / 60.0);
        }
        assert!(sliding.lateral_speed.abs() > normal.lateral_speed.abs() + 0.5);
        assert!(sliding.speed > 5.0);
    }
    #[test]
    fn driving_is_consistent_across_frame_rates_and_air_has_no_traction() {
        let world = CollisionWorld::from_batches(&[floor(-500.0, 500.0, 0.0)]);
        let simulate = |hz: usize| {
            let mut car = Car::new(Vec3::Y * 0.6, 0.6);
            for _ in 0..hz * 2 {
                car.step(&world, 1.0, 0.35, false, 1.0, 1.0 / hz as f32);
            }
            car
        };
        let slow = simulate(30);
        let fast = simulate(120);
        assert!(slow.position.distance(fast.position) < 0.1);
        assert!((slow.yaw - fast.yaw).abs() < 0.01);
        let mut airborne = Car::new(Vec3::Y * 50.0, 0.6);
        airborne.speed = 12.0;
        airborne.step(&world, 1.0, 1.0, false, 1.0, 0.05);
        assert_eq!(airborne.yaw, 0.0);
        assert!(airborne.speed <= 12.0 && airborne.position.y < 50.0);
    }
}
