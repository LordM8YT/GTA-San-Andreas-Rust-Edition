//! Experimental 3D car body: a rigid box on four spring wheels, so cars roll
//! down hills, glance off walls, tip, flip and land on their roof. Built from
//! textbook rigid-body and raycast-suspension dynamics with SARE's own
//! constants; it reads the same `Handling` as the flat model.
use super::{
    to_model, Car, AIR_TILT_RATE, BURNOUT_SPEED, BURNOUT_TURN, BURNOUT_WHEEL_SPEED,
    ROLLING_RESISTANCE,
};
use crate::collision::CollisionWorld;
use glam::{Quat, Vec3};

const GRAVITY: f32 = 9.81;
/// Wheel mounts as shares of the body's half width and half length:
/// rear-left, front-left, rear-right, front-right.
const WHEELS: [(f32, f32); 4] = [(-0.75, -0.56), (-0.75, 0.56), (0.75, -0.56), (0.75, 0.56)];
/// How far past its resting length a wheel still reaches for the road, m.
const DROOP: f32 = 0.45;
/// Sideways force per m/s of tyre slip on one wheel, as car acceleration.
const TYRE_STIFFNESS: f32 = 2.75;
/// Body panels touching the ground: spring and damping per corner.
const BODY_SPRING: f32 = 600.0;
const BODY_DAMPING: f32 = 25.0;
const BODY_FRICTION: f32 = 0.6;
/// Wall hits: share of the closing speed bounced back, and scraping friction.
const WALL_BOUNCE: f32 = 0.2;
const WALL_FRICTION: f32 = 0.3;
/// Share of the ride height at which tyre forces act on the body.
const ROLL_CENTRE: f32 = 0.5;
/// Fastest the body may spin, rad/s.
const MAX_SPIN: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rigid {
    orientation: Quat,
    velocity: Vec3,
    /// World angular velocity, rad/s.
    spin: Vec3,
    /// The car's public pose and motion as last written, to notice changes
    /// made from outside (spawning, crashes, `stop`).
    seen: [f32; 7],
    seen_position: Vec3,
}

/// Body orientation from the flat model's angles: `pitch` lifts the nose
/// and `roll` is the side grade, as in `model_rotation`.
fn orientation(yaw: f32, pitch: f32, roll: f32) -> Quat {
    let bank = (roll.tan() * pitch.cos()).atan();
    Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-pitch) * Quat::from_rotation_z(bank)
}
/// Yaw, pitch and side grade of a body orientation.
fn angles(q: Quat) -> (f32, f32, f32) {
    let forward = q * Vec3::Z;
    let yaw = forward.x.atan2(forward.z);
    let pitch = forward.y.clamp(-1.0, 1.0).asin();
    let mut rest = (Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-pitch)).inverse() * q;
    if rest.w < 0.0 {
        rest = -rest;
    }
    let bank = 2.0 * rest.z.atan2(rest.w);
    let roll = (bank.clamp(-1.5, 1.5).tan() / pitch.cos().max(0.05)).atan();
    (yaw, pitch, roll)
}

impl Rigid {
    pub(super) fn new(car: &Car) -> Self {
        let mut body = Self {
            orientation: orientation(car.yaw, car.pitch, car.roll),
            velocity: Vec3::ZERO,
            spin: Vec3::ZERO,
            seen: [f32::NAN; 7],
            seen_position: car.position,
        };
        body.adopt(car);
        body.seen = Self::state(car);
        body
    }
    /// Stop all motion, spin included.
    pub(super) fn halt(&mut self) {
        self.velocity = Vec3::ZERO;
        self.spin = Vec3::ZERO;
    }
    fn state(car: &Car) -> [f32; 7] {
        [
            car.yaw,
            car.pitch,
            car.roll,
            car.speed,
            car.lateral_speed,
            car.yaw_rate,
            car.vertical_speed,
        ]
    }
    /// Take over pose or motion that something else set on the car.
    fn adopt(&mut self, car: &Car) {
        let now = Self::state(car);
        if now[1..3] != self.seen[1..3] {
            self.orientation = orientation(car.yaw, car.pitch, car.roll);
            self.spin = Vec3::ZERO;
        } else if now[0] != self.seen[0] {
            // A new heading keeps the body's tilt, even upside down.
            self.orientation =
                (Quat::from_rotation_y(now[0] - self.seen[0]) * self.orientation).normalize();
        }
        if car.position != self.seen_position {
            // Moved by something else: a teleport or a push out of another car.
            self.spin = Vec3::ZERO;
        }
        if now[3..5] != self.seen[3..5] {
            self.velocity = car.forward() * car.speed
                + Vec3::new(car.yaw.cos(), 0.0, -car.yaw.sin()) * car.lateral_speed
                + Vec3::Y * self.velocity.y;
        }
        if now[5] != self.seen[5] {
            self.spin.y = car.yaw_rate;
        }
        if now[6] != self.seen[6] {
            self.velocity.y = car.vertical_speed;
        }
    }
    /// Body-frame inertia per unit mass of a box the car's size.
    fn inertia(car: &Car) -> Vec3 {
        let [x, z, height] = car.size;
        let y = height * 0.5;
        Vec3::new(
            (y * y + z * z) / 3.0,
            (x * x + z * z) / 3.0 * car.handling.yaw_inertia,
            (x * x + y * y) / 3.0,
        )
    }
    fn turn(&self, inertia: Vec3, torque: Vec3) -> Vec3 {
        self.orientation * ((self.orientation.inverse() * torque) / inertia)
    }
    #[cfg(test)]
    pub(super) fn flip_for_test(&mut self) {
        self.orientation *= Quat::from_rotation_z(std::f32::consts::PI);
    }
    pub(super) fn model_orientation(&self) -> Quat {
        self.orientation * Quat::from_rotation_y(std::f32::consts::PI)
    }
}

impl Car {
    pub(super) fn integrate_rigid(
        &mut self,
        world: &CollisionWorld,
        throttle: f32,
        steer: f32,
        handbrake: bool,
        handling: f32,
        dt: f32,
    ) {
        let Some(mut body) = self.rigid else { return };
        body.adopt(self);
        let tuning = self.handling;
        let handling = handling.clamp(0.5, 1.5);
        let throttle = throttle.clamp(-1.0, 1.0);
        let steer = steer.clamp(-1.0, 1.0);
        let roll_key = handbrake;
        let handbrake = handbrake && tuning.handbrake > 0.5;
        let q = body.orientation;
        let (up, forward, side) = (q * Vec3::Y, q * Vec3::Z, q * Vec3::X);
        let inertia = Rigid::inertia(self);
        let centre = self.position;
        let along = body.velocity.dot(forward);

        // Suspension: each wheel looks straight down for the road.
        let [half_width, half_length, height] = self.size;
        let wheelbase = 2.0 * WHEELS[1].1 * half_length;
        let shift = (tuning.front_weight - 0.5) * wheelbase;
        let mut wheels = [None; 4];
        if up.y > 0.3 {
            for (wheel, &(x, z)) in wheels.iter_mut().zip(&WHEELS) {
                let mount = centre + q * Vec3::new(x * half_width, 0.0, z * half_length - shift);
                let Some(ground) = world.ground_below(mount, mount.y + 0.3) else {
                    continue;
                };
                let length = (mount.y - ground) / up.y;
                if length > self.clearance + DROOP {
                    continue;
                }
                let rate = (body.velocity + body.spin.cross(mount - centre)).dot(up);
                let compression = self.clearance - length;
                let stop = (self.clearance * 0.35 - length).max(0.0);
                let load = (GRAVITY / 4.0
                    + tuning.suspension_spring / 4.0 * compression
                    + tuning.suspension_spring * 2.0 * stop
                    - tuning.suspension_damping / 4.0 * rate)
                    .max(0.0);
                *wheel = Some((mount - up * length, load, z > 0.0));
            }
        }
        let contacts = wheels.iter().flatten().count();
        let grounded = contacts >= 2;
        let normal = match wheels {
            [Some(a), Some(b), Some(c), Some(d)] => {
                let ahead = (b.0 + d.0 - a.0 - c.0) * 0.5;
                let across = (c.0 + d.0 - a.0 - b.0) * 0.5;
                let n = ahead.cross(across).normalize_or(Vec3::Y);
                if n.y < 0.0 {
                    -n
                } else {
                    n
                }
            }
            _ => Vec3::Y,
        };

        // Steering, pedals and engine as in the flat model.
        let speed_lock =
            tuning.steering_lock / (1.0 + (along.abs() / tuning.steer_fade_speed).powi(2));
        let lateral = body.velocity.dot(side);
        let relief = if steer * lateral > 0.0 {
            (lateral.abs() / (along.abs() * 0.5 + 1.0)).min(1.0)
        } else {
            0.0
        };
        let lock = speed_lock + (tuning.steering_lock - speed_lock) * relief;
        self.steering_angle += (steer * lock - self.steering_angle)
            .clamp(-tuning.steering_rate * dt, tuning.steering_rate * dt);
        self.burning_out =
            self.burnout_input && !self.damage.wrecked && grounded && along.abs() < BURNOUT_SPEED;
        let braking = !self.burning_out && throttle * along < -0.15;
        let pedal = if self.burning_out {
            1.0
        } else {
            throttle.max(0.0)
        };
        if tuning.engine_lag > 0.0 {
            let rate = if pedal > self.engine_throttle {
                1.0
            } else {
                3.0
            } / tuning.engine_lag;
            self.engine_throttle += (pedal - self.engine_throttle) * (1.0 - (-rate * dt).exp());
        } else {
            self.engine_throttle = pedal;
        }
        let engine = if self.burning_out || braking {
            0.0
        } else if throttle >= 0.0 {
            self.engine_throttle
                * tuning.acceleration
                * (1.0 - (along.max(0.0) / tuning.top_speed).powi(2)).max(0.0)
        } else {
            throttle
                * tuning.reverse_acceleration
                * (1.0 - (-along / tuning.reverse_speed).powi(2)).max(0.0)
        };
        let driven = |front: bool| match tuning.drive.round() as u8 {
            0 => front,
            1 => !front,
            _ => true,
        };
        let count = |test: &dyn Fn(bool) -> bool| {
            wheels.iter().flatten().filter(|w| test(w.2)).count().max(1) as f32
        };
        let driven_count = count(&|front| driven(front));
        let front_count = count(&|front| front);
        let rear_count = count(&|front| !front);

        // Forces as accelerations of the whole car, and their torques.
        let mut force = Vec3::NEG_Y * GRAVITY
            - body.velocity * body.velocity.length() * tuning.aerodynamic_drag;
        let mut torque = Vec3::ZERO;
        let push = |at: Vec3, f: Vec3, force: &mut Vec3, torque: &mut Vec3| {
            *force += f;
            *torque += (at - centre).cross(f);
        };
        let mut rolled = 0.0;
        for &(point, load, front) in wheels.iter().flatten() {
            let heading = if front {
                Quat::from_axis_angle(up, self.steering_angle) * forward
            } else {
                forward
            };
            let ahead = (heading - normal * heading.dot(normal)).normalize_or_zero();
            let across = normal.cross(ahead);
            let velocity = body.velocity + body.spin.cross(point - centre);
            let (rolling, sliding) = (velocity.dot(ahead), velocity.dot(across));
            rolled += rolling;
            let axle = if front {
                tuning.front_grip
            } else {
                2.0 - tuning.front_grip
            };
            let limit = 2.0 * tuning.tire_grip * handling * axle / GRAVITY * load;
            // Force that would stop this wheel's share of the car rolling.
            let hold = |most: f32| (-rolling / (4.0 * dt)).clamp(-most, most);
            let mut long = if self.burning_out {
                hold(tuning.brake_deceleration / 4.0)
            } else if handbrake && (!front || along.abs() < 0.5) {
                // The handbrake locks the rear wheels; parked, it holds the car.
                hold(limit * 0.8)
            } else if braking {
                let share = if front {
                    tuning.brake_front / front_count
                } else {
                    (1.0 - tuning.brake_front) / rear_count
                };
                hold(tuning.brake_deceleration * throttle.abs() * share)
            } else {
                let drive = if driven(front) {
                    engine / driven_count
                } else {
                    0.0
                };
                drive + hold(ROLLING_RESISTANCE / 4.0 * load / (GRAVITY / 4.0))
            };
            long = long.clamp(-limit, limit);
            let mut budget = (limit * limit - long * long)
                .max((0.35 * limit).powi(2))
                .sqrt();
            if handbrake && !front {
                budget *= 0.35;
            }
            let mut lat = -TYRE_STIFFNESS * sliding * (load / (GRAVITY / 4.0)).min(2.0);
            if lat.abs() > budget {
                // A sliding tyre grips less than a rolling one.
                let loss = tuning.traction_loss;
                lat = lat.signum() * budget * loss;
            }
            push(point, normal * load, &mut force, &mut torque);
            // Tyre forces act as if near the centre of mass, as through stiff
            // anti-roll bars: hard cornering leans the body without flipping it.
            let lever = (self.clearance * ROLL_CENTRE - tuning.cg_offset).max(0.0);
            push(
                point + up * lever,
                ahead * long + across * lat,
                &mut force,
                &mut torque,
            );
        }

        // Body corners that reach the ground: roof landings, scraping, rollovers.
        // The roof can only touch down once the car is well tipped.
        let roof = (up.y < 0.5).then_some(height - self.clearance);
        for x in [-half_width, half_width] {
            for z in [-half_length, half_length] {
                for y in std::iter::once(0.25 - self.clearance).chain(roof) {
                    let corner = centre + q * Vec3::new(x, y, z);
                    // Only surfaces below the centre count, so a low ceiling
                    // never lifts the car.
                    let reach = (corner.y + 0.5).min(centre.y);
                    let Some(ground) = world.ground_below(corner, reach) else {
                        continue;
                    };
                    let depth = ground - corner.y;
                    if depth <= 0.0 {
                        continue;
                    }
                    let velocity = body.velocity + body.spin.cross(corner - centre);
                    let press = (BODY_SPRING * depth - BODY_DAMPING * velocity.y).max(0.0);
                    let slide = velocity - Vec3::Y * velocity.y;
                    let rub = (-slide * 8.0).clamp_length_max(BODY_FRICTION * press);
                    push(corner, Vec3::Y * press + rub, &mut force, &mut torque);
                }
            }
        }

        // Steering in mid-air: turn, tip the nose, or roll with the handbrake key.
        if contacts == 0 && tuning.air_control > 0.0 {
            let rate = AIR_TILT_RATE * tuning.air_control;
            if roll_key {
                torque += forward * steer * rate * inertia.z;
            } else {
                torque += up * steer * tuning.air_control * inertia.y;
            }
            torque += side * self.air_pitch_input * rate * inertia.x;
        }

        body.velocity += force * dt;
        let local = q.inverse() * body.spin;
        let gyro = q * local.cross(local * inertia);
        body.spin += body.turn(inertia, torque - gyro) * dt;
        if self.burning_out {
            // Spinning wheels let the car pivot slowly on the spot.
            let yaw = body.spin.dot(up);
            body.spin += up * (steer * BURNOUT_TURN - yaw) * (1.0 - (-4.0 * dt).exp());
        }
        body.spin = body.spin.clamp_length_max(MAX_SPIN);
        self.position += body.velocity * dt;
        body.orientation = (Quat::from_scaled_axis(body.spin * dt) * q).normalize();

        // Walls: slide along them and bounce off a little instead of stopping dead.
        let flat = Vec3::new(forward.x, 0.0, forward.z).normalize_or(Vec3::Z);
        let feet = self.position - Vec3::Y * self.clearance;
        let mut deepest = Vec3::ZERO;
        let mut hit = feet;
        let radius = half_width.max(0.3);
        let reach = (half_length - radius).max(0.0);
        let probes = (2.0 * reach / radius).ceil().max(1.0) as usize;
        for index in 0..=probes {
            let probe = feet + flat * (index as f32 / probes as f32 * 2.0 - 1.0) * reach;
            let out = world.resolve_body(probe, radius, height) - probe;
            if out.length_squared() > deepest.length_squared() {
                deepest = out;
                hit = probe;
            }
        }
        if deepest.length_squared() > 1e-8 {
            self.position += deepest;
            let n = Vec3::new(deepest.x, 0.0, deepest.z).normalize_or_zero();
            let r = hit - n * radius + Vec3::Y * 0.5 - (self.position - deepest);
            let velocity = body.velocity + body.spin.cross(r);
            let closing = velocity.dot(n);
            if closing < 0.0 && n != Vec3::ZERO {
                let frame = body;
                let reach = |dir: Vec3| 1.0 + dir.dot(frame.turn(inertia, r.cross(dir)).cross(r));
                let j = -(1.0 + WALL_BOUNCE) * closing / reach(n);
                body.velocity += n * j;
                body.spin += frame.turn(inertia, r.cross(n)) * j;
                let scrape = velocity - n * closing;
                if let Some(t) = scrape.try_normalize() {
                    let jt = (scrape.length() / reach(t)).min(WALL_FRICTION * j);
                    body.velocity -= t * jt;
                    body.spin -= frame.turn(inertia, r.cross(t)) * jt;
                }
                self.damage.crash(-closing, to_model(self.yaw, -n));
            }
        }

        // Publish the body as the car's pose and motion.
        let (yaw, pitch, roll) = angles(body.orientation);
        self.yaw = yaw;
        self.pitch = pitch;
        self.roll = roll;
        self.speed = body.velocity.dot(self.forward());
        self.lateral_speed = body.velocity.dot(Vec3::new(yaw.cos(), 0.0, -yaw.sin()));
        self.yaw_rate = body.spin.y;
        self.vertical_speed = body.velocity.y;
        self.airborne = contacts == 0;
        if contacts > 0 {
            self.travel += if self.burning_out {
                BURNOUT_WHEEL_SPEED
            } else {
                rolled / contacts as f32
            } * dt;
        }
        body.seen = Rigid::state(self);
        body.seen_position = self.position;
        self.rigid = Some(body);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn angles_round_trip_through_the_body_orientation() {
        for (yaw, pitch, roll) in [(0.0, 0.0, 0.0), (1.2, 0.2, -0.1), (-2.5, -0.3, 0.25)] {
            let (y, p, r) = angles(orientation(yaw, pitch, roll));
            assert!((y - yaw).abs() < 1e-4 && (p - pitch).abs() < 1e-4 && (r - roll).abs() < 1e-4);
            let model = crate::vehicle::model_rotation(yaw, pitch, roll);
            let body = orientation(yaw, pitch, roll) * Quat::from_rotation_y(std::f32::consts::PI);
            assert!(model.dot(body).abs() > 0.9999);
        }
    }
}
