//! Lightweight vehicle dynamics with tire slip and steering inertia.
//! Position is the model origin; speed and slip are in metres per second.
use crate::collision::CollisionWorld;
use glam::{Quat, Vec3};
/// Render the original model's -Z front in the simulated +Z-forward body frame.
pub fn model_rotation(yaw: f32, pitch: f32, roll: f32) -> Quat {
    // Simulation stores independent forward/side grade angles. Euler bank
    // after pitch would multiply side grade by 1/cos(pitch) without this correction.
    let bank = (roll.tan() * pitch.cos()).atan();
    Quat::from_rotation_y(yaw + std::f32::consts::PI)
        * Quat::from_rotation_x(pitch)
        * Quat::from_rotation_z(-bank)
}
/// Native tuning values; omitted JSON fields retain the original prototype defaults.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Handling {
    pub acceleration: f32,
    pub reverse_acceleration: f32,
    pub brake_deceleration: f32,
    pub top_speed: f32,
    pub reverse_speed: f32,
    pub tire_grip: f32,
    pub steering_lock: f32,
    pub steering_rate: f32,
    pub suspension_spring: f32,
    pub suspension_damping: f32,
    pub aerodynamic_drag: f32,
}
impl Default for Handling {
    fn default() -> Self {
        Self {
            acceleration: 6.2,
            reverse_acceleration: 3.8,
            brake_deceleration: 10.5,
            top_speed: 48.0,
            reverse_speed: 10.0,
            tire_grip: 4.6,
            steering_lock: 0.55,
            steering_rate: 1.7,
            suspension_spring: 140.0,
            suspension_damping: 22.0,
            aerodynamic_drag: 0.0035,
        }
    }
}
impl Handling {
    pub fn validate(&self) -> anyhow::Result<()> {
        for (name, value, low, high) in [
            ("acceleration", self.acceleration, 0.5, 20.0),
            ("reverse_acceleration", self.reverse_acceleration, 0.5, 10.0),
            ("brake_deceleration", self.brake_deceleration, 1.0, 25.0),
            ("top_speed", self.top_speed, 5.0, 90.0),
            ("reverse_speed", self.reverse_speed, 1.0, 25.0),
            ("tire_grip", self.tire_grip, 1.0, 12.0),
            ("steering_lock", self.steering_lock, 0.1, 0.9),
            ("steering_rate", self.steering_rate, 0.3, 4.0),
            ("suspension_spring", self.suspension_spring, 40.0, 250.0),
            ("suspension_damping", self.suspension_damping, 8.0, 40.0),
            ("aerodynamic_drag", self.aerodynamic_drag, 0.0, 0.02),
        ] {
            anyhow::ensure!(
                value.is_finite() && (low..=high).contains(&value),
                "vehicle handling {name} must be finite and between {low} and {high}"
            );
        }
        Ok(())
    }
}
pub struct Car {
    pub handling: Handling,
    pub position: Vec3,
    pub yaw: f32,
    pub speed: f32,
    pub clearance: f32,
    pub pitch: f32,
    pub roll: f32,
    vertical_speed: f32,
    lateral_speed: f32,
    steering_angle: f32,
    yaw_rate: f32,
}
impl Car {
    pub fn new(position: Vec3, clearance: f32) -> Self {
        Self {
            handling: Handling::default(),
            position,
            yaw: 0.0,
            speed: 0.0,
            clearance,
            pitch: 0.0,
            roll: 0.0,
            vertical_speed: 0.0,
            lateral_speed: 0.0,
            steering_angle: 0.0,
            yaw_rate: 0.0,
        }
    }
    pub fn with_handling(mut self, handling: Handling) -> Self {
        self.handling = handling;
        self
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
        let dt = dt.clamp(0.0, 0.25);
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
        let tuning = self.handling;
        let throttle = throttle.clamp(-1.0, 1.0);
        let side = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        let mut velocity = self.forward() * self.speed + side * self.lateral_speed;
        // Probe from each wheel's expected height on the tilted body. A centre-
        // height ray misses the uphill axle and biases suspension toward the low axle.
        const TRAVEL: f32 = 0.75;
        let expected_height = |x: f32, z: f32| {
            self.position.y - self.clearance + z * self.pitch.tan() + x * self.roll.tan()
        };
        // Four tire contact points avoid losing all traction at the centre of a ledge.
        let contacts = [-0.72, 0.72]
            .into_iter()
            .flat_map(|x| [-1.35, 1.35].map(move |z| (x, z)))
            .map(|(x, z)| {
                world.ground_below(
                    self.position + side * x + self.forward() * z,
                    expected_height(x, z) + TRAVEL,
                )
            })
            .collect::<Vec<_>>();
        // A wheel past a ramp lip or ledge keeps the body's current plane. Using
        // the ground far below it pitched the nose down and let the suspension
        // damper cancel the climb, so the car dropped instead of jumping.
        let (supports, heights): (Vec<_>, Vec<_>) = contacts
            .iter()
            .enumerate()
            .map(|(i, ground)| {
                let x = if i < 2 { -0.72 } else { 0.72 };
                let z = if i % 2 == 0 { -1.35 } else { 1.35 };
                let expected = expected_height(x, z);
                let support = ground.filter(|g| expected - g <= TRAVEL);
                (support, support.unwrap_or(expected))
            })
            .unzip();
        let supports: Vec<f32> = supports.into_iter().flatten().collect();
        let grounded = supports.len() >= 2;
        let grip = if grounded {
            supports.len() as f32 / 4.0
        } else {
            0.0
        };
        // Steering lock falls with speed; wheel input and body rotation both have inertia.
        let lock = tuning.steering_lock / (1.0 + (self.speed.abs() / 16.0).powi(2));
        let target_steering = steer.clamp(-1.0, 1.0) * lock;
        self.steering_angle += (target_steering - self.steering_angle)
            .clamp(-tuning.steering_rate * dt, tuning.steering_rate * dt);
        let braking = throttle * self.speed < -0.15;
        let acceleration = if braking {
            -self.speed.signum() * tuning.brake_deceleration * throttle.abs()
        } else if throttle >= 0.0 {
            throttle
                * tuning.acceleration
                * (1.0 - (self.speed.max(0.0) / tuning.top_speed).powi(2)).max(0.0)
        } else {
            throttle
                * tuning.reverse_acceleration
                * (1.0 - (-self.speed / tuning.reverse_speed).powi(2)).max(0.0)
        };
        let old_speed = self.speed;
        self.speed += acceleration * grip * dt;
        if braking && self.speed.signum() != old_speed.signum() {
            self.speed = 0.0;
        }
        let resistance = (0.28
            + tuning.aerodynamic_drag * self.speed * self.speed
            + if handbrake { 7.0 * grip } else { 0.0 })
            * dt;
        self.speed = self.speed.signum() * (self.speed.abs() - resistance).max(0.0);
        velocity += self.forward() * (self.speed - old_speed);
        // Axle slip generates both lateral force and yaw torque. Forces are bounded
        // by a friction circle so braking and cornering share the available grip.
        let tire_budget =
            (tuning.tire_grip * handling).powi(2) - (acceleration * grip * 0.5).powi(2);
        let front_limit = tire_budget.max(0.5).sqrt() * grip;
        let rear_limit = tuning.tire_grip * handling * grip * if handbrake { 0.12 } else { 1.0 };
        let front_slip =
            self.lateral_speed + 1.35 * self.yaw_rate - self.speed * self.steering_angle.tan();
        let rear_slip = self.lateral_speed - 1.35 * self.yaw_rate;
        let front_force = (-front_slip * 5.5).clamp(-front_limit, front_limit);
        let rear_force = (-rear_slip * 5.5).clamp(-rear_limit, rear_limit);
        let lateral_acceleration = front_force + rear_force;
        velocity += side * lateral_acceleration * dt;
        self.yaw_rate += (1.35 * (front_force - rear_force) / 2.2 - self.yaw_rate * 0.22) * dt;
        self.yaw_rate = self.yaw_rate.clamp(-2.0, 2.0);
        let old_yaw = self.yaw;
        self.yaw += self.yaw_rate * dt;
        if !self.body_clear(world, self.position) {
            self.yaw = old_yaw;
            self.yaw_rate = 0.0;
        }
        let side = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        self.speed = velocity.dot(self.forward());
        self.lateral_speed = velocity.dot(side);
        let height = |index: usize| heights[index];
        let slope_pitch = (((height(1) + height(3)) - (height(0) + height(2))) * 0.5 / 2.7).atan();
        let slope_roll = (((height(2) + height(3)) - (height(0) + height(1))) * 0.5 / 1.44).atan();
        let blend = 1.0 - (-8.0 * dt).exp();
        self.pitch +=
            ((slope_pitch + acceleration * grip * 0.012).clamp(-0.6, 0.6) - self.pitch) * blend;
        self.roll +=
            ((slope_roll + lateral_acceleration * 0.016).clamp(-0.5, 0.5) - self.roll) * blend;
        let motion = (self.forward() * self.speed + side * self.lateral_speed) * dt;
        let before_motion = self.position;
        let steps = ((motion.length() / 0.15).ceil() as usize).clamp(1, 20);
        for _ in 0..steps {
            let desired = self.position + motion / steps as f32;
            if !self.body_clear(world, desired) {
                let bottom = self.position.y - self.clearance;
                let high = [-0.7, 0.7]
                    .into_iter()
                    .flat_map(|x| [-2.25, 2.25].map(move |z| (x, z)))
                    .filter_map(|(x, z)| {
                        world.ground_below(desired + side * x + self.forward() * z, bottom + 0.25)
                    })
                    .fold(bottom, f32::max);
                let raised = Vec3::new(desired.x, high + self.clearance, desired.z);
                if high > bottom + 0.01 && high - bottom <= 0.25 && self.body_clear(world, raised) {
                    self.position = raised;
                    self.vertical_speed = self.vertical_speed.max(0.0);
                    continue;
                }
                self.stop();
                break;
            }
            self.position = desired;
        }
        if grounded {
            let ground = heights.iter().sum::<f32>() / heights.len() as f32;
            let moved = self.position - before_motion;
            let ground_velocity = (moved.dot(self.forward()) * slope_pitch.tan()
                + moved.dot(side) * slope_roll.tan())
                / dt;
            let ground = ground + ground_velocity * dt;
            let normal_offset = self.clearance
                * (1.0 + slope_pitch.tan().powi(2) + slope_roll.tan().powi(2)).sqrt();
            let target = ground + normal_offset;
            // A damped suspension settles instead of snapping to one centre ray.
            // Ground support cancels gravity before damping. Damping a velocity
            // already reduced by gravity creates a frame-rate-dependent height offset.
            self.vertical_speed += ((target - self.position.y) * tuning.suspension_spring
                - (self.vertical_speed - ground_velocity) * tuning.suspension_damping)
                * dt;
            self.position.y += self.vertical_speed * dt;
            if self.position.y < target {
                self.position.y = target;
                self.vertical_speed = self.vertical_speed.max(ground_velocity);
            }
        } else {
            self.vertical_speed -= 9.81 * dt;
            self.position.y += self.vertical_speed * dt;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Batch, Vertex};
    #[test]
    fn handling_rejects_unknown_nonfinite_and_out_of_range_values() {
        let partial: Handling = serde_json::from_str(r#"{"acceleration":9.0}"#).unwrap();
        assert_eq!(partial.top_speed, Handling::default().top_speed);
        partial.validate().unwrap();
        assert!(serde_json::from_str::<Handling>(r#"{"accelleration":9}"#).is_err());
        let mut invalid = partial;
        invalid.tire_grip = f32::NAN;
        assert!(invalid.validate().is_err());
        let original = serde_json::to_value(Handling::default()).unwrap();
        for key in original.as_object().unwrap().keys() {
            let mut invalid = original.clone();
            invalid[key] = serde_json::json!(-1.0);
            assert!(
                serde_json::from_value::<Handling>(invalid)
                    .unwrap()
                    .validate()
                    .is_err(),
                "{key}"
            );
        }
    }
    #[test]
    fn tuning_changes_acceleration_and_braking_without_losing_support() {
        let world = CollisionWorld::from_batches(&[floor(-1000.0, 1000.0, 0.0)]);
        let tuning = Handling {
            acceleration: 12.0,
            brake_deceleration: 20.0,
            ..Handling::default()
        };
        let mut original = Car::new(Vec3::Y * 0.6, 0.6);
        let mut custom = Car::new(Vec3::Y * 0.6, 0.6).with_handling(tuning);
        for _ in 0..120 {
            original.step(&world, 1.0, 0.0, false, 1.0, 1.0 / 120.0);
            custom.step(&world, 1.0, 0.0, false, 1.0, 1.0 / 120.0);
        }
        assert!(custom.speed > original.speed * 1.5);
        original.speed = 15.0;
        custom.speed = 15.0;
        for _ in 0..60 {
            original.step(&world, -1.0, 0.0, false, 1.0, 1.0 / 120.0);
            custom.step(&world, -1.0, 0.0, false, 1.0, 1.0 / 120.0);
        }
        assert!(custom.speed < original.speed - 3.0);
        for _ in 0..120 {
            custom.step(&world, 0.0, 0.0, false, 1.0, 1.0 / 120.0);
        }
        assert!(
            (custom.position.y - 0.6).abs() < 0.01,
            "position {:?}, pitch {}, speed {}, vertical {}",
            custom.position,
            custom.pitch,
            custom.speed,
            custom.vertical_speed
        );
        assert_eq!(custom.handling, tuning);
    }
    fn mesh(points: &[[f32; 3]]) -> Batch {
        Batch {
            key: "test".into(),
            alpha: false,
            animated: false,
            uv_animation: None,
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
    fn ramp(slope: f32) -> Batch {
        mesh(&[
            [-20.0, -100.0 * slope, -100.0],
            [20.0, -100.0 * slope, -100.0],
            [20.0, 100.0 * slope, 100.0],
            [-20.0, -100.0 * slope, -100.0],
            [20.0, 100.0 * slope, 100.0],
            [-20.0, 100.0 * slope, 100.0],
        ])
    }
    #[test]
    fn both_axles_support_the_car_on_uphill_and_downhill_roads() {
        for slope in [-0.25_f32, 0.25] {
            let world = CollisionWorld::from_batches(&[ramp(slope)]);
            let mut car = Car::new(Vec3::Y * 0.6, 0.6);
            for _ in 0..120 {
                car.step(&world, 0.0, 0.0, false, 1.0, 1.0 / 120.0);
            }
            assert!(
                (car.position.y - 0.6).abs() < 0.04,
                "one axle sank the body on slope {slope}: {:?}",
                car.position
            );
            assert!(
                (car.pitch - slope.atan()).abs() < 0.03,
                "wrong road pitch on slope {slope}: {}",
                car.pitch
            );
            for _ in 0..240 {
                car.step(&world, 1.0, 0.0, false, 1.0, 1.0 / 120.0);
            }
            let road_y = car.position.z * slope;
            assert!(
                (car.position.y - car.clearance - road_y).abs() < 0.07,
                "body lost the road at {:?}, road={road_y}",
                car.position
            );
            assert!(car.position.z > 5.0 && car.speed > 5.0);
        }
    }

    #[test]
    fn banked_roads_follow_the_road_plane_at_different_headings() {
        let gradient = Vec3::new(0.25, 0.0, 0.2);
        let point = |x: f32, z: f32| [x, x * gradient.x + z * gradient.z, z];
        let world = CollisionWorld::from_batches(&[mesh(&[
            point(-100.0, -100.0),
            point(100.0, -100.0),
            point(100.0, 100.0),
            point(-100.0, -100.0),
            point(100.0, 100.0),
            point(-100.0, 100.0),
        ])]);
        for yaw in [0.0, 0.8, -1.7] {
            let mut car = Car::new(Vec3::Y * 0.6, 0.6);
            car.yaw = yaw;
            let forward_grade = gradient.dot(car.forward());
            let side_grade = gradient.dot(Vec3::new(yaw.cos(), 0.0, -yaw.sin()));
            for _ in 0..240 {
                car.step(&world, 0.0, 0.0, false, 1.0, 1.0 / 120.0);
            }
            let expected_height = 0.6 * (1.0 + gradient.length_squared()).sqrt();
            assert!((car.position.y - expected_height).abs() < 0.04);
            assert!((car.pitch - forward_grade.atan()).abs() < 0.03);
            assert!((car.roll - side_grade.atan()).abs() < 0.03);
        }
    }
    #[test]
    fn rendered_wheel_plane_matches_combined_road_grade_and_bank() {
        for gradient in [Vec3::new(0.25, 0.0, 0.2), Vec3::new(-0.2, 0.0, 0.3)] {
            let point = |x: f32, z: f32| [x, x * gradient.x + z * gradient.z, z];
            let world = CollisionWorld::from_batches(&[mesh(&[
                point(-100.0, -100.0),
                point(100.0, -100.0),
                point(100.0, 100.0),
                point(-100.0, -100.0),
                point(100.0, 100.0),
                point(-100.0, 100.0),
            ])]);
            for yaw in [0.0, 0.8, -1.7, std::f32::consts::PI] {
                let mut car = Car::new(Vec3::Y * 0.6, 0.6);
                car.yaw = yaw;
                for _ in 0..240 {
                    car.step(&world, 0.0, 0.0, false, 1.0, 1.0 / 120.0);
                }
                let rotation = model_rotation(car.yaw, car.pitch, car.roll);
                for x in [-0.72, 0.72] {
                    for z in [-1.35, 1.35] {
                        let wheel = rotation * Vec3::new(x, -car.clearance, z) + car.position;
                        let gap = wheel.y - wheel.x * gradient.x - wheel.z * gradient.z;
                        assert!(
                            gap.abs() < 0.0001,
                            "rendered wheel left road plane: yaw={yaw}, gap={gap}"
                        );
                    }
                }
            }
        }
    }
    #[test]
    fn crossing_a_hill_crest_keeps_support_and_a_real_drop_still_falls() {
        let strip = |z0: f32, y0: f32, z1: f32, y1: f32| {
            mesh(&[
                [-20.0, y0, z0],
                [20.0, y0, z0],
                [20.0, y1, z1],
                [-20.0, y0, z0],
                [20.0, y1, z1],
                [-20.0, y1, z1],
            ])
        };
        let world = CollisionWorld::from_batches(&[
            strip(-20.0, 0.0, 0.0, 0.0),
            strip(0.0, 0.0, 12.0, 3.0),
            strip(12.0, 3.0, 24.0, 0.0),
            strip(24.0, 0.0, 60.0, 0.0),
        ]);
        let mut car = Car::new(Vec3::new(0.0, 0.6, -5.0), 0.6);
        for _ in 0..720 {
            car.step(&world, 1.0, 0.0, false, 1.0, 1.0 / 120.0);
            if let Some(road) = world.ground_below(car.position, car.position.y + 1.0) {
                let gap = car.position.y - car.clearance - road;
                assert!(
                    gap > -0.4 && gap < 0.6,
                    "crest suspension lost road: gap={gap}, {:?}",
                    car.position
                );
            }
        }
        assert!(
            car.position.z > 25.0,
            "crest stopped car at {:?}",
            car.position
        );
        let world =
            CollisionWorld::from_batches(&[floor(-20.0, 0.0, 0.0), floor(0.0, 100.0, -5.0)]);
        let mut car = Car::new(Vec3::new(0.0, 0.6, -3.0), 0.6);
        car.speed = 10.0;
        // The body stays level while the rear axle is still on the ledge.
        for _ in 0..40 {
            car.step(&world, 0.0, 0.0, false, 1.0, 1.0 / 120.0);
        }
        assert!(
            (car.position.y - 0.6).abs() < 0.05 && car.pitch.abs() < 0.05,
            "front wheels over a ledge pulled the body down: {:?}, pitch {}",
            car.position,
            car.pitch
        );
        for _ in 0..50 {
            car.step(&world, 0.0, 0.0, false, 1.0, 1.0 / 120.0);
        }
        assert!(
            car.position.z > 1.0 && car.position.y < 0.5 && car.position.y > -4.0,
            "suspension snapped to lower floor instead of falling: {:?}",
            car.position
        );
    }
    #[test]
    fn a_ramp_launches_the_car_instead_of_pulling_it_down_at_the_lip() {
        // 20 m ramp rising 5 m, ending in open air 5 m above the landing floor.
        let ramp = mesh(&[
            [-20.0, 0.0, 0.0],
            [20.0, 0.0, 0.0],
            [20.0, 5.0, 20.0],
            [-20.0, 0.0, 0.0],
            [20.0, 5.0, 20.0],
            [-20.0, 5.0, 20.0],
        ]);
        let world =
            CollisionWorld::from_batches(&[floor(-60.0, 0.0, 0.0), ramp, floor(20.0, 200.0, 0.0)]);
        let mut car = Car::new(Vec3::new(0.0, 0.6, -30.0), 0.6);
        car.speed = 20.0;
        let mut peak = f32::NEG_INFINITY;
        let mut airborne_distance = 0.0_f32;
        for _ in 0..600 {
            car.step(&world, 1.0, 0.0, false, 1.0, 1.0 / 120.0);
            assert!(car.position.is_finite());
            peak = peak.max(car.position.y - car.clearance);
            if car.position.z > 20.0 && car.position.y - car.clearance > 1.0 {
                airborne_distance = car.position.z - 20.0;
            }
        }
        // Leaving at roughly 20 m/s on a 1:4 grade carries the car well above
        // the lip and tens of metres downrange before it lands and drives on.
        assert!(peak > 5.4, "car did not rise past the ramp lip: {peak}");
        assert!(
            airborne_distance > 8.0,
            "car fell at the lip after {airborne_distance} m"
        );
        assert!(
            car.position.z > 60.0 && (car.position.y - 0.6).abs() < 0.2,
            "car did not land and continue: {:?}",
            car.position
        );
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
            uv_animation: None,
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
    fn low_curb_is_traversable_but_tall_wall_still_blocks() {
        let curb = mesh(&[
            [-20.0, 0.0, 3.0],
            [-20.0, 0.15, 3.0],
            [20.0, 0.15, 3.0],
            [-20.0, 0.0, 3.0],
            [20.0, 0.15, 3.0],
            [20.0, 0.0, 3.0],
        ]);
        let world =
            CollisionWorld::from_batches(&[floor(-100.0, 3.0, 0.0), floor(3.0, 100.0, 0.15), curb]);
        let mut car = Car::new(Vec3::Y * 0.6, 0.6);
        for _ in 0..180 {
            car.step(&world, 1.0, 0.0, false, 1.0, 1.0 / 60.0);
        }
        assert!(
            car.position.z > 10.0,
            "curb stopped car at {:?}",
            car.position
        );
        assert!(car.position.y >= 0.74 && car.position.is_finite());
    }
    #[test]
    fn steering_direction_reverses_when_backing_and_slow_frames_keep_elapsed_time() {
        let world = CollisionWorld::from_batches(&[floor(-500.0, 500.0, 0.0)]);
        let mut forward = Car::new(Vec3::Y * 0.6, 0.6);
        forward.speed = 10.0;
        let mut reverse = Car::new(Vec3::Y * 0.6, 0.6);
        reverse.speed = -5.0;
        for _ in 0..60 {
            forward.step(&world, 0.0, 0.4, false, 1.0, 1.0 / 60.0);
            reverse.step(&world, 0.0, 0.4, false, 1.0, 1.0 / 60.0);
        }
        assert!(forward.yaw > 0.1 && reverse.yaw < -0.1);
        let mut slow = Car::new(Vec3::Y * 0.6, 0.6);
        let mut fast = Car::new(Vec3::Y * 0.6, 0.6);
        for _ in 0..10 {
            slow.step(&world, 1.0, 0.0, false, 1.0, 0.1);
        }
        for _ in 0..120 {
            fast.step(&world, 1.0, 0.0, false, 1.0, 1.0 / 120.0);
        }
        assert!(slow.position.distance(fast.position) < 0.01);
        assert!(forward.roll.abs() > 0.001 && forward.roll.abs() < 0.26);
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
