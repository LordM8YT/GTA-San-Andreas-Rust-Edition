//! Lightweight vehicle dynamics with tire slip and steering inertia.
//! Position is the model origin; speed and slip are in metres per second.
use crate::collision::CollisionWorld;
use glam::{Affine3A, Quat, Vec3};
pub use sa_assets::{Panel, VehiclePart};
/// Render the original model's -Z front in the simulated +Z-forward body frame.
pub fn model_rotation(yaw: f32, pitch: f32, roll: f32) -> Quat {
    // Simulation stores independent forward/side grade angles. Euler bank
    // after pitch would multiply side grade by 1/cos(pitch) without this correction.
    let bank = (roll.tan() * pitch.cos()).atan();
    Quat::from_rotation_y(yaw + std::f32::consts::PI)
        * Quat::from_rotation_x(pitch)
        * Quat::from_rotation_z(-bank)
}
/// How far a door swings open, in radians.
pub const DOOR_SWING: f32 = 1.2;
/// One drawable piece of a car model and how it moves. Positions are in
/// the model space of `Car` meshes (-Z front, -X left).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigPiece {
    pub part: VehiclePart,
    /// The `_dam` look of a part, drawn instead of the intact one once the
    /// part is dented.
    pub damaged: bool,
    /// Door hinge or wheel hub.
    pub pivot: Vec3,
    /// Tyre radius of a wheel, so it turns as fast as the car rolls.
    pub radius: f32,
}
/// Bit of a part in `Look::dents`; zero for parts without a damaged look.
pub fn dent_bit(part: VehiclePart) -> u16 {
    match part {
        VehiclePart::Panel(panel) => 1 << panel as u16,
        VehiclePart::Door(door) => 1 << (7 + u16::from(door.min(3))),
        _ => 0,
    }
}
/// What one car's moving parts show this frame.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Look {
    /// Metres rolled, which turns the wheels.
    pub travel: f32,
    /// Front wheel angle in radians, the same sign as `Car` steering.
    pub steer: f32,
    /// How far each door is open, 0 shut to 1 wide: front left, front
    /// right, rear left, rear right.
    pub doors: [f32; 4],
    /// Parts showing their damaged look, by `dent_bit`.
    pub dents: u16,
    /// Burnt out after an explosion.
    pub burnt: bool,
}
impl RigPiece {
    /// Model-space placement of this piece for `look`, or `None` while the
    /// other variant of the part (intact or damaged) is shown instead.
    pub fn transform(&self, look: &Look) -> Option<Affine3A> {
        let bit = dent_bit(self.part);
        if self.damaged != (look.dents & bit != 0) {
            return None;
        }
        let turn = match self.part {
            VehiclePart::Wheel { front, .. } => {
                let steer = if front {
                    Quat::from_rotation_y(look.steer)
                } else {
                    Quat::IDENTITY
                };
                // Models face -Z, so rolling forwards turns the top of the
                // tyre towards -Z.
                steer * Quat::from_rotation_x(-look.travel / self.radius.max(0.1))
            }
            VehiclePart::Door(door) => {
                // Hinges are at the front edge; left doors swing out to -X.
                let side = if door % 2 == 0 { -1.0 } else { 1.0 };
                Quat::from_rotation_y(side * look.doors[usize::from(door.min(3))] * DOOR_SWING)
            }
            _ => return Some(Affine3A::IDENTITY),
        };
        Some(
            Affine3A::from_rotation_translation(turn, self.pivot)
                * Affine3A::from_translation(-self.pivot),
        )
    }
}
/// Rolling resistance in m/s^2 the car always pays, before aerodynamic drag.
const ROLLING_RESISTANCE: f32 = 0.28;
/// Health a car starts with, as in SA.
pub const FULL_HEALTH: f32 = 1000.0;
/// Below this the engine catches fire and the car burns until it blows up.
pub const ON_FIRE: f32 = 250.0;
/// Health lost per second while burning: a burning car explodes in about
/// five seconds.
const BURN_RATE: f32 = 50.0;
/// Closing speed in m/s a crash needs before it hurts the car.
const CRASH_THRESHOLD: f32 = 4.0;
/// Health lost per m/s of closing speed above the threshold.
const CRASH_DAMAGE: f32 = 10.0;
/// Closing speed that dents the panels on the side that hit.
const DENT_SPEED: f32 = 6.0;
/// What has happened to a car's body.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Damage {
    pub health: f32,
    pub dents: u16,
    /// Blown up: burnt black and undriveable.
    pub wrecked: bool,
}
impl Default for Damage {
    fn default() -> Self {
        Self {
            health: FULL_HEALTH,
            dents: 0,
            wrecked: false,
        }
    }
}
impl Damage {
    pub fn burning(&self) -> bool {
        !self.wrecked && self.health < ON_FIRE
    }
    /// A crash at `impact` m/s closing speed. `towards` points from the car
    /// to what it hit, in model space (-Z front, -X left). Returns the
    /// health lost.
    pub fn crash(&mut self, impact: f32, towards: Vec3) -> f32 {
        if self.wrecked || !impact.is_finite() {
            return 0.0;
        }
        if impact >= DENT_SPEED {
            let flat = Vec3::new(towards.x, 0.0, towards.z).normalize_or_zero();
            let (front, left) = (-flat.z, -flat.x);
            let mut parts = Vec::new();
            if front > 0.5 {
                parts.extend([Panel::BumperFront, Panel::Bonnet].map(VehiclePart::Panel));
                if impact >= DENT_SPEED * 2.0 {
                    parts.extend(
                        [Panel::WingLeft, Panel::WingRight, Panel::Windscreen]
                            .map(VehiclePart::Panel),
                    );
                }
            } else if front < -0.5 {
                parts.extend([Panel::BumperRear, Panel::Boot].map(VehiclePart::Panel));
            }
            if left > 0.5 {
                parts.extend([
                    VehiclePart::Door(0),
                    VehiclePart::Door(2),
                    VehiclePart::Panel(Panel::WingLeft),
                ]);
            } else if left < -0.5 {
                parts.extend([
                    VehiclePart::Door(1),
                    VehiclePart::Door(3),
                    VehiclePart::Panel(Panel::WingRight),
                ]);
            }
            for part in parts {
                self.dents |= dent_bit(part);
            }
        }
        self.hurt((impact - CRASH_THRESHOLD).max(0.0) * CRASH_DAMAGE)
    }
    /// Bullets or a blast: health only. Returns the health lost.
    pub fn hurt(&mut self, amount: f32) -> f32 {
        if self.wrecked || !amount.is_finite() || amount <= 0.0 {
            return 0.0;
        }
        let before = self.health;
        self.health = (self.health - amount).max(0.0);
        before - self.health
    }
    /// Burn down a car on fire. Returns true the moment it explodes.
    pub fn update(&mut self, dt: f32) -> bool {
        if self.wrecked {
            return false;
        }
        if self.burning() {
            self.health = (self.health - BURN_RATE * dt.max(0.0)).max(0.0);
        }
        if self.health <= 0.0 {
            self.wrecked = true;
            // Everything that can buckle has buckled.
            self.dents = u16::MAX;
            return true;
        }
        false
    }
}
/// World direction `towards` turned into a car's model space.
pub fn to_model(yaw: f32, towards: Vec3) -> Vec3 {
    Quat::from_rotation_y(-(yaw + std::f32::consts::PI)) * towards
}
/// Native tuning values; omitted JSON fields retain the original prototype defaults.
#[derive(Clone, Copy, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Handling {
    /// Current simulation parameters, in SI units.
    pub acceleration: f32,
    pub reverse_acceleration: f32,
    pub brake_deceleration: f32,
    pub top_speed: f32,
    pub reverse_speed: f32,
    pub tire_grip: f32,
    pub steering_lock: f32,
    pub steering_rate: f32,
    /// Gear count from handling.cfg.
    pub gears: f32,
    pub suspension_spring: f32,
    pub suspension_damping: f32,
    pub aerodynamic_drag: f32,
    /// Original GTA:SA handling.cfg values retained for a faithful physics port.
    /// Mass and turn mass are in the source file's units.
    /// 1 when values came from the original handling.cfg; 0 for hand-tuned fallback profiles.
    pub gta_handling_loaded: f32,
    /// Original handling.cfg bit flags (stored exactly as a numeric value).
    pub gta_handling_flags: f64,
    pub gta_mass: f32,
    pub gta_turn_mass: f32,
    pub gta_drag_multiplier: f32,
    pub gta_traction_multiplier: f32,
    pub gta_centre_of_mass_x: f32,
    pub gta_centre_of_mass_y: f32,
    pub gta_centre_of_mass_z: f32,
    pub gta_traction_loss: f32,
    pub gta_traction_bias: f32,
    /// Encoded original values: drive F=0/R=1/4=2; engine P=0/D=1/E=2.
    pub gta_drive_type: f32,
    pub gta_engine_type: f32,
    pub gta_engine_inertia: f32,
    pub gta_brake_bias: f32,
    pub gta_abs: f32,
    pub gta_suspension_force_level: f32,
    pub gta_suspension_high_speed_damping: f32,
    pub gta_suspension_upper_limit: f32,
    pub gta_suspension_lower_limit: f32,
    pub gta_suspension_front_rear_bias: f32,
    pub gta_suspension_anti_dive_multiplier: f32,
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
            gears: 1.0,
            suspension_spring: 140.0,
            suspension_damping: 22.0,
            aerodynamic_drag: 0.0035,
            gta_handling_loaded: 0.0,
            gta_handling_flags: 0.0,
            gta_mass: 1500.0,
            gta_turn_mass: 3000.0,
            gta_drag_multiplier: 2.0,
            gta_traction_multiplier: 0.75,
            gta_centre_of_mass_x: 0.0,
            gta_centre_of_mass_y: 0.0,
            gta_centre_of_mass_z: -0.2,
            gta_traction_loss: 0.8,
            gta_traction_bias: 0.5,
            gta_drive_type: 0.0,
            gta_engine_type: 0.0,
            gta_engine_inertia: 10.0,
            gta_brake_bias: 0.5,
            gta_abs: 0.0,
            gta_suspension_force_level: 1.0,
            gta_suspension_high_speed_damping: 0.0,
            gta_suspension_upper_limit: 0.3,
            gta_suspension_lower_limit: -0.15,
            gta_suspension_front_rear_bias: 0.5,
            gta_suspension_anti_dive_multiplier: 0.0,
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
            ("gears", self.gears, 1.0, 6.0),
            ("gta_handling_loaded", self.gta_handling_loaded, 0.0, 1.0),
            ("gta_mass", self.gta_mass, 1.0, 100000.0),
            ("gta_centre_of_mass_x", self.gta_centre_of_mass_x, -10.0, 10.0),
            ("gta_centre_of_mass_y", self.gta_centre_of_mass_y, -10.0, 10.0),
            ("gta_centre_of_mass_z", self.gta_centre_of_mass_z, -10.0, 10.0),
            ("gta_drive_type", self.gta_drive_type, 0.0, 2.0),
            ("gta_engine_type", self.gta_engine_type, 0.0, 2.0),
            ("gta_abs", self.gta_abs, 0.0, 1.0),
            ("gta_turn_mass", self.gta_turn_mass, 1.0, 1000000.0),
            ("gta_drag_multiplier", self.gta_drag_multiplier, 0.0, 100.0),
            ("gta_traction_multiplier", self.gta_traction_multiplier, 0.0, 5.0),
            ("gta_traction_loss", self.gta_traction_loss, 0.0, 5.0),
            ("gta_traction_bias", self.gta_traction_bias, 0.0, 1.0),
            ("gta_engine_inertia", self.gta_engine_inertia, 0.0, 100.0),
            ("gta_brake_bias", self.gta_brake_bias, 0.0, 1.0),
            ("gta_suspension_force_level", self.gta_suspension_force_level, 0.0, 20.0),
            ("gta_suspension_high_speed_damping", self.gta_suspension_high_speed_damping, 0.0, 100.0),
            ("gta_suspension_upper_limit", self.gta_suspension_upper_limit, -5.0, 5.0),
            ("gta_suspension_lower_limit", self.gta_suspension_lower_limit, -5.0, 5.0),
            ("gta_suspension_front_rear_bias", self.gta_suspension_front_rear_bias, 0.0, 1.0),
            ("gta_suspension_anti_dive_multiplier", self.gta_suspension_anti_dive_multiplier, 0.0, 10.0),
        ] {
            anyhow::ensure!(
                value.is_finite() && (low..=high).contains(&value),
                "vehicle handling {name} must be finite and between {low} and {high}"
            );
        }
        anyhow::ensure!(
            self.gta_handling_flags.is_finite()
                && (0.0..=u32::MAX as f64).contains(&self.gta_handling_flags),
            "vehicle handling gta_handling_flags must be a valid 32-bit flag value"
        );
        Ok(())
    }
}
impl Handling {
    /// Whole gears to pull through, clamped to the validated range.
    pub fn gear_count(&self) -> f32 {
        self.gears.round().clamp(1.0, 6.0)
    }
    /// Speed where the engine balances rolling and aerodynamic drag.
    ///
    /// `top_speed` is an asymptote the car never reaches because drag grows
    /// with the square of speed, so the lower gear bands are spaced against
    /// this speed instead. Otherwise a middle gear could top out below its own
    /// shift point and the car would stick there.
    pub fn reachable_top_speed(&self) -> f32 {
        let engine = self.acceleration - ROLLING_RESISTANCE;
        if engine <= 0.0 {
            return self.top_speed * 0.5;
        }
        let drag = self.aerodynamic_drag + self.acceleration / self.top_speed.powi(2);
        (engine / drag.max(1e-6))
            .sqrt()
            .clamp(self.top_speed * 0.2, self.top_speed)
    }
    /// GTA's transmission band endpoint in m/s. Gear zero is reverse; gears
    /// one through N use the original equal-interval table. Hand-tuned fallback
    /// profiles retain the legacy reachable-speed ceiling.
    pub fn gear_band_top(&self, gear: u8) -> f32 {
        let count = self.gear_count();
        let gear = gear.min(count as u8);
        if gear == 0 && self.gta_handling_loaded > 0.5 {
            // GTA's gear-zero band is reverse, not a positive half-band.
            return -self.reverse_speed;
        }
        if f32::from(gear) >= count {
            return self.top_speed;
        }
        let ceiling = if self.gta_handling_loaded > 0.5 {
            self.top_speed
        } else {
            self.reachable_top_speed()
        };
        let gear = f32::from(gear);
        let half = ceiling / (2.0 * count);
        gear * (ceiling - half) / count + half
    }

    /// GTA's upshift fraction (0.6667) applied to the current adapter band.
    /// The band endpoint remains transitional until the drive-force curve is ported.
    pub fn gear_up_shift_speed(&self, gear: u8) -> f32 {
        if self.gear_count() <= 1.0 {
            return self.top_speed;
        }
        let lower = self.gear_band_top(gear.saturating_sub(1));
        let upper = self.gear_band_top(gear);
        lower + 0.6667 * (upper - lower)
    }

    /// GTA's downshift fraction (0.42) applied to the current adapter band.
    pub fn gear_down_shift_speed(&self, gear: u8) -> f32 {
        if self.gear_count() <= 1.0 {
            return -0.5;
        }
        let lower = self.gear_band_top(gear.saturating_sub(2));
        let upper = self.gear_band_top(gear.saturating_sub(1));
        lower + 0.42 * (upper - lower)
    }
}
/// Body size used until a model is measured: an average saloon.
pub const DEFAULT_SIZE: [f32; 3] = [0.95, 2.4, 1.5];
/// Half width, half length and height of a model from its vertices. Models
/// face -Z, so length runs along Z. Odd or empty models fall back to sane bounds.
pub fn measure(points: impl IntoIterator<Item = [f32; 3]>) -> [f32; 3] {
    let (mut low, mut high) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for p in points {
        let p = Vec3::from_array(p);
        low = low.min(p);
        high = high.max(p);
    }
    if !low.is_finite() || !high.is_finite() {
        return DEFAULT_SIZE;
    }
    let extent = high - low;
    [
        (extent.x / 2.0).clamp(0.3, 2.5),
        (extent.z / 2.0).clamp(0.6, 10.0),
        extent.y.clamp(0.5, 5.0),
    ]
}
/// Estimate normalized load at the four contact points (rear-left, front-left,
/// rear-right, front-right). This is a first-order load-transfer model, not a
/// literal port of GTA's suspension solver. Positive longitudinal acceleration
/// transfers load rearward; lateral acceleration transfers it across the body.
fn wheel_normal_loads(
    supported: [bool; 4],
    longitudinal_acceleration: f32,
    lateral_acceleration: f32,
    cg_height: f32,
) -> [f32; 4] {
    const WHEELBASE: f32 = 2.7;
    const TRACK: f32 = 1.44;
    const GRAVITY: f32 = 9.81;
    let count = supported
        .iter()
        .filter(|&&is_supported| is_supported)
        .count();
    if count == 0 {
        return [0.0; 4];
    }
    let mut loads = supported.map(|is_supported| {
        if is_supported {
            1.0 / count as f32
        } else {
            0.0
        }
    });

    // Index layout follows the wheel probe order: rear-left, front-left,
    // rear-right, front-right.
    let rear = [0, 2];
    let front = [1, 3];
    let left = [0, 1];
    let right = [2, 3];
    let transfer =
        |indices: [usize; 2], loads: &[f32; 4]| indices.iter().map(|&i| loads[i]).sum::<f32>();
    let front_count = front.iter().filter(|&&i| supported[i]).count();
    let rear_count = rear.iter().filter(|&&i| supported[i]).count();
    if front_count > 0 && rear_count > 0 {
        let front_load = transfer(front, &loads);
        let rear_load = transfer(rear, &loads);
        let moved = (longitudinal_acceleration * cg_height / (GRAVITY * WHEELBASE))
            .clamp(-rear_load, front_load);
        for i in front {
            if supported[i] {
                loads[i] -= moved / front_count as f32;
            }
        }
        for i in rear {
            if supported[i] {
                loads[i] += moved / rear_count as f32;
            }
        }
    }
    let left_count = left.iter().filter(|&&i| supported[i]).count();
    let right_count = right.iter().filter(|&&i| supported[i]).count();
    if left_count > 0 && right_count > 0 {
        let left_load = transfer(left, &loads);
        let right_load = transfer(right, &loads);
        let moved =
            (lateral_acceleration * cg_height / (GRAVITY * TRACK)).clamp(-left_load, right_load);
        for i in left {
            if supported[i] {
                loads[i] += moved / left_count as f32;
            }
        }
        for i in right {
            if supported[i] {
                loads[i] -= moved / right_count as f32;
            }
        }
    }
    let total = loads.iter().sum::<f32>();
    if total > 0.0 {
        for load in &mut loads {
            *load = load.max(0.0) / total;
        }
    }
    loads
}
pub struct Car {
    pub handling: Handling,
    pub position: Vec3,
    pub yaw: f32,
    pub speed: f32,
    /// Selected gear, from 1. Always 1 for a single-band car.
    pub gear: u8,
    pub clearance: f32,
    pub pitch: f32,
    pub roll: f32,
    /// Half width, half length and height of the body, for car-to-car and
    /// car-to-ped contacts.
    pub size: [f32; 3],
    pub damage: Damage,
    /// Metres rolled, for turning the wheels.
    travel: f32,
    vertical_speed: f32,
    lateral_speed: f32,
    steering_angle: f32,
    yaw_rate: f32,
    /// Grade of the last road surface under all four wheels.
    ground_pitch: f32,
    ground_roll: f32,
    airborne: bool,
    /// Stays set through a slide until both axles have recovered their grip.
    skid_latched: bool,
    /// GTA cTransmission's two per-car inertia smoothing values.
    transmission_a6: f32,
    transmission_a7: f32,
    transmission_initialized: bool,
    /// GTA wheel states: 0 normal, 1 spinning, 2 skidding, 3 fixed/locked.
    wheel_states: [u8; 4],
}
impl Car {
    pub fn new(position: Vec3, clearance: f32) -> Self {
        Self {
            handling: Handling::default(),
            position,
            yaw: 0.0,
            speed: 0.0,
            gear: 1,
            clearance,
            pitch: 0.0,
            roll: 0.0,
            size: DEFAULT_SIZE,
            damage: Damage::default(),
            travel: 0.0,
            vertical_speed: 0.0,
            lateral_speed: 0.0,
            steering_angle: 0.0,
            yaw_rate: 0.0,
            ground_pitch: 0.0,
            ground_roll: 0.0,
            airborne: false,
            skid_latched: false,
            transmission_a6: 0.0,
            transmission_a7: 1.0,
            transmission_initialized: false,
            wheel_states: [0; 4],
        }
    }
    pub fn with_handling(mut self, handling: Handling) -> Self {
        self.handling = handling;
        self
    }
    pub fn with_size(mut self, size: [f32; 3]) -> Self {
        self.size = size;
        self
    }
    /// Push the car out of another vehicle or obstacle by `push` (horizontal)
    /// and take the impact: the closing speed along the contact normal is
    /// reflected with a little bounce, like a soft crash in SA.
    pub fn collide(&mut self, world: &CollisionWorld, push: Vec3, obstacle_velocity: Vec3) {
        let push = Vec3::new(push.x, 0.0, push.z);
        let normal = push.normalize_or_zero();
        if normal == Vec3::ZERO {
            return;
        }
        if self.body_clear(world, self.position + push) {
            self.position += push;
        }
        let side = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        let mut velocity = self.forward() * self.speed + side * self.lateral_speed;
        let closing = (velocity - obstacle_velocity).dot(normal);
        if closing < 0.0 {
            self.damage.crash(-closing, to_model(self.yaw, -normal));
            velocity -= normal * closing * 1.3;
            // Glancing blows spin the body a little.
            self.yaw_rate += self.forward().cross(normal).y * closing * 0.15;
            self.yaw_rate = self.yaw_rate.clamp(-2.0, 2.0);
        }
        self.speed = velocity.dot(self.forward());
        self.lateral_speed = velocity.dot(side);
    }
    /// Wheels, steering and dents as they should be drawn; doors are up to
    /// whoever opens them.
    pub fn look(&self) -> Look {
        Look {
            travel: self.travel,
            steer: self.steering_angle,
            dents: self.damage.dents,
            burnt: self.damage.wrecked,
            ..Look::default()
        }
    }
    /// An explosion under the car throws it up a little.
    pub fn blast(&mut self) {
        self.vertical_speed = self.vertical_speed.max(5.0);
    }
    /// World velocity of the body, ignoring vertical motion.
    pub fn velocity(&self) -> Vec3 {
        let side = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        self.forward() * self.speed + side * self.lateral_speed
    }
    /// Sideways tyre slip in m/s, zero while airborne. Drives skid marks.
    pub fn slip(&self) -> f32 {
        if self.airborne {
            0.0
        } else {
            self.lateral_speed.abs()
        }
    }
    pub fn airborne(&self) -> bool {
        self.airborne
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
    /// Shift by road speed only: up at 60% of the current band, down at 40% of
    /// the one below, so a car sitting on a boundary cannot hunt between gears.
    /// 60% keeps every shift point below the speed where drag cancels the
    /// gear's pull, even for a car running on two wheels.
    fn shift_gears(&mut self, tuning: &Handling, throttle: f32) {
        let count = tuning.gear_count() as u8;
        if tuning.gta_handling_loaded <= 0.5 {
            if self.speed <= 0.0 {
                self.gear = 1;
                return;
            }
            while self.gear < count && self.speed > tuning.gear_up_shift_speed(self.gear) {
                self.gear += 1;
            }
            while self.gear > 1 && self.speed < tuning.gear_down_shift_speed(self.gear) {
                self.gear -= 1;
            }
            return;
        }

        // GTA has a distinct reverse gear at index zero. It remains selected
        // while reversing or while reverse torque is being used to stop forward
        // motion, and returns to first once forward throttle is requested.
        if self.gear == 0 {
            if self.speed > -0.5 && throttle >= 0.0 {
                self.gear = 1;
            } else {
                return;
            }
        }
        if self.speed < 0.0 || (self.speed <= 0.0 && throttle < 0.0) {
            self.gear = 0;
            return;
        }
        if self.speed <= 0.0 {
            self.gear = 1;
            return;
        }
        while self.gear < count && self.speed > tuning.gear_up_shift_speed(self.gear) {
            self.gear += 1;
        }
        while self.gear > 1 && self.speed < tuning.gear_down_shift_speed(self.gear) {
            self.gear -= 1;
        }
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
        // A burnt-out wreck only rolls to a stop.
        let (throttle, steer, handbrake) = if self.damage.wrecked {
            (0.0, 0.0, true)
        } else {
            (throttle, steer, handbrake)
        };
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
    fn support_force(&self, target: f32, ground_velocity: f32) -> f32 {
        (9.81 + (target - self.position.y) * self.handling.suspension_spring
            - (self.vertical_speed - ground_velocity) * self.handling.suspension_damping)
            .max(0.0)
    }

    /// Apply GTA:SA's per-wheel velocity corrections and turn forces.
    /// This replaces the legacy axle friction-circle model for cars loaded from
    /// the original handling.cfg. Material adhesion is still a provisional
    /// tarmac value until SARE's collision surfaces expose GTA surface types.
    fn apply_gtasa_wheel_forces(
        &mut self,
        velocity: &mut Vec3,
        supported: [bool; 4],
        drive_wheels: [bool; 4],
        throttle: f32,
        braking: bool,
        handbrake: bool,
        acceleration: f32,
        handling: f32,
        dt: f32,
    ) -> f32 {
        let tuning = self.handling;
        let forward = self.forward();
        let side = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        let before = *velocity;
        let wheels_on_ground = supported.iter().filter(|&&is_supported| is_supported).count();
        if wheels_on_ground == 0 {
            self.wheel_states = [0; 4];
            return 0.0;
        }
        let wheels_on_ground = wheels_on_ground as f32;
        let centre_of_mass =
            side * -tuning.gta_centre_of_mass_x + forward * -tuning.gta_centre_of_mass_z;
        const WHEEL_OFFSETS: [(f32, f32); 4] = [
            (-0.72, -1.35), // rear left
            (-0.72, 1.35),  // front left
            (0.72, -1.35),  // rear right
            (0.72, 1.35),   // front right
        ];
        let mut already_skidding = false;

        for (index, &(x, z)) in WHEEL_OFFSETS.iter().enumerate() {
            if !supported[index] {
                self.wheel_states[index] = 0;
                continue;
            }
            let front_wheel = index == 1 || index == 3;
            let rear_wheel = !front_wheel;
            let point = side * x + forward * z - centre_of_mass;
            let contact_velocity =
                *velocity + Vec3::new(self.yaw_rate * point.z, 0.0, -self.yaw_rate * point.x);
            let mut wheel_forward = forward;
            let mut wheel_right = side;
            if front_wheel {
                let (sin_steer, cos_steer) = self.steering_angle.sin_cos();
                let rotated_forward = cos_steer * wheel_forward - sin_steer * wheel_right;
                wheel_right = sin_steer * wheel_forward + cos_steer * wheel_right;
                wheel_forward = rotated_forward;
            }
            let contact_speed_forward = contact_velocity.dot(wheel_forward);
            let contact_speed_right = contact_velocity.dot(wheel_right);
            let was_skidding = self.wheel_states[index] != 0;
            let traction_bias = if front_wheel {
                (2.0 * tuning.gta_traction_bias).clamp(0.0, 2.0)
            } else {
                (2.0 - 2.0 * tuning.gta_traction_bias).clamp(0.0, 2.0)
            };
            let suspension_bias = if front_wheel {
                2.0 * tuning.gta_suspension_front_rear_bias
            } else {
                2.0 - 2.0 * tuning.gta_suspension_front_rear_bias
            };
            // Compression is not yet exposed by SARE's collision query, so use
            // the mid-travel value until per-wheel suspension is ported.
            let suspension_adhesion =
                (suspension_bias * tuning.gta_suspension_force_level * 2.0).clamp(0.0, 2.0);
            let mut adhesion = tuning.gta_traction_multiplier
                * 2.5
                * dt
                * handling
                * traction_bias
                * suspension_adhesion;
            if was_skidding {
                already_skidding = true;
                adhesion *= tuning.gta_traction_loss;
            }
            self.wheel_states[index] = 0;

            let wheel_braking = braking || (handbrake && rear_wheel);
            let driving =
                !wheel_braking && throttle != 0.0 && drive_wheels[index];
            let mut brake = if handbrake && rear_wheel {
                // 20,000 game-speed units per tick, converted to SI velocity change.
                20_000.0 * dt * 2_500.0
            } else if braking {
                tuning.brake_deceleration * throttle.abs() * dt
            } else {
                0.0
            };
            let mut right = if contact_speed_right != 0.0 {
                -contact_speed_right / wheels_on_ground
            } else {
                0.0
            };
            let mut fwd = 0.0;
            if driving {
                fwd = acceleration * dt;
                right = right.clamp(-adhesion, adhesion);
            } else if contact_speed_forward != 0.0 {
                fwd = -contact_speed_forward / wheels_on_ground;
                if !braking && throttle.abs() < 0.01 && !(handbrake && rear_wheel) {
                    // GTA's global wheel friction is 0.9; convert its per-tick
                    // correction to m/s for this SI integrator.
                    brake = 0.9 / tuning.gta_mass * dt * 2_500.0;
                }
                if brake > adhesion {
                    if contact_speed_forward.abs() > 0.005 {
                        self.wheel_states[index] = 3;
                    }
                } else {
                    fwd = fwd.clamp(-brake, brake);
                }
            }

            let speed_squared = right * right + fwd * fwd;
            if speed_squared > adhesion * adhesion {
                if self.wheel_states[index] != 3 {
                    self.wheel_states[index] = if driving && 0.3 * adhesion < fwd.abs() {
                        1
                    } else {
                        2
                    };
                }
                let loss = if already_skidding {
                    1.0
                } else {
                    tuning.gta_traction_loss
                };
                let length = speed_squared.sqrt();
                fwd *= adhesion * loss / length;
                right *= adhesion * loss / length;
            }

            if self.wheel_states[index] != 0 {
                already_skidding = true;
            }
            if fwd == 0.0 && right == 0.0 {
                continue;
            }

            let total_speed = fwd * wheel_forward + right * wheel_right;
            let mut turn_direction = total_speed;
            if wheel_braking {
                turn_direction -= tuning.gta_suspension_anti_dive_multiplier * wheel_forward * fwd;
            } else if driving {
                turn_direction -=
                    0.5 * tuning.gta_suspension_anti_dive_multiplier * wheel_forward * fwd;
            }
            let speed = total_speed.length();
            let turn_speed = turn_direction.length();
            let direction = total_speed / speed.max(1e-6);
            let turn_direction = turn_direction / turn_speed.max(1e-6);
            let cross_direction = point.z * direction.x - point.x * direction.z;
            let effective_mass = 1.0
                / (cross_direction * cross_direction / tuning.gta_turn_mass
                    + 1.0 / tuning.gta_mass);
            let turn_cross = point.z * turn_direction.x - point.x * turn_direction.z;
            *velocity += total_speed;
            self.yaw_rate += turn_cross * turn_speed * effective_mass / tuning.gta_turn_mass;
        }

        ((*velocity - before).dot(side) / dt).clamp(-100.0, 100.0)
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
        let wheels = [-0.72_f32, 0.72]
            .into_iter()
            .flat_map(|x| [-1.35_f32, 1.35].map(move |z| (x, z)))
            .collect::<Vec<_>>();
        let contacts = wheels
            .iter()
            .map(|&(x, z)| {
                world.ground_below(
                    self.position + side * x + self.forward() * z,
                    expected_height(x, z) + TRAVEL,
                )
            })
            .collect::<Vec<_>>();
        // Wheel offsets and ground heights of the wheels carrying the car.
        let supports: Vec<(f32, f32, f32)> = wheels
            .iter()
            .zip(&contacts)
            .filter_map(|(&(x, z), ground)| {
                ground
                    .filter(|g| expected_height(x, z) - g <= TRAVEL)
                    .map(|g| (x, z, g))
            })
            .collect();
        // Past a ramp lip or ledge the next surface is far below a wheel. Its
        // height says nothing about the road the car is leaving: following it
        // pitched the nose down and made the damper cancel the climb, so the
        // car dropped at the lip instead of jumping.
        const REACH: f32 = 1.0;
        let over_air = wheels
            .iter()
            .zip(&contacts)
            .any(|(&(x, z), ground)| ground.is_none_or(|g| expected_height(x, z) - g > REACH));
        let height = |index: usize| contacts[index].unwrap_or(self.position.y - self.clearance);
        let measured_pitch =
            (((height(1) + height(3)) - (height(0) + height(2))) * 0.5 / 2.7).atan();
        let measured_roll =
            (((height(2) + height(3)) - (height(0) + height(1))) * 0.5 / 1.44).atan();
        // With a wheel over open air keep the grade of the road being left; after
        // a flight that grade is stale, so level out until all wheels find ground.
        let (slope_pitch, slope_roll) = if !over_air {
            (measured_pitch, measured_roll)
        } else if self.airborne {
            (0.0, 0.0)
        } else {
            (self.ground_pitch, self.ground_roll)
        };
        self.ground_pitch = slope_pitch;
        self.ground_roll = slope_roll;
        // The sampled road plane sets both suspension load and tire grip.
        let grade = if over_air {
            (slope_pitch.tan(), slope_roll.tan())
        } else {
            (0.0, 0.0)
        };
        let ground = supports
            .iter()
            .map(|(x, z, ground)| ground - z * grade.0 - x * grade.1)
            .sum::<f32>()
            / supports.len().max(1) as f32;
        let normal_offset =
            self.clearance * (1.0 + slope_pitch.tan().powi(2) + slope_roll.tan().powi(2)).sqrt();
        let target = ground + normal_offset;
        let grounded = supports.len() >= 2;
        let road_velocity = self.speed * slope_pitch.tan() + self.lateral_speed * slope_roll.tan();
        let grip = if grounded && self.support_force(target, road_velocity) > 0.0 {
            supports.len() as f32 / 4.0
        } else {
            0.0
        };
        let supported_wheels = [
            supports.iter().any(|(x, z, _)| *x < 0.0 && *z < 0.0),
            supports.iter().any(|(x, z, _)| *x < 0.0 && *z > 0.0),
            supports.iter().any(|(x, z, _)| *x > 0.0 && *z < 0.0),
            supports.iter().any(|(x, z, _)| *x > 0.0 && *z > 0.0),
        ];
        let drive_wheels = match tuning.gta_drive_type as u8 {
            0 => [false, true, false, true], // front-wheel drive
            1 => [true, false, true, false], // rear-wheel drive
            2 => [true; 4],                  // four-wheel drive
            _ => [true; 4],
        };
        let drive_wheel_count = drive_wheels.iter().filter(|&&drive| drive).count();
        let supported_drive_count = drive_wheels
            .iter()
            .zip(supported_wheels)
            .filter(|(drive, supported)| **drive && *supported)
            .count();
        let drive_grip = if tuning.gta_handling_loaded > 0.5 {
            supported_drive_count as f32 / drive_wheel_count.max(1) as f32
        } else {
            grip
        };
        // Steering lock falls with speed; wheel input and body rotation both have inertia.
        let lock = tuning.steering_lock / (1.0 + (self.speed.abs() / 16.0).powi(2));
        let target_steering = steer.clamp(-1.0, 1.0) * lock;
        self.steering_angle += (target_steering - self.steering_angle)
            .clamp(-tuning.steering_rate * dt, tuning.steering_rate * dt);
        // A gearbox pulls against the current gear's band; with one gear the
        // band is the whole top-speed range, exactly as the prototype had it.
        if tuning.gears > 1.0 {
            self.shift_gears(&tuning, throttle);
        }
        let band_top = if tuning.gears > 1.0 {
            tuning.gear_band_top(self.gear)
        } else {
            tuning.top_speed
        };
        let braking = throttle * self.speed < -0.15;
        let gta_transmission = tuning.gta_handling_loaded > 0.5;
        let mut transmission_inertia = 1.0;
        let gear_acceleration_multiplier = if !gta_transmission {
            1.0
        } else if tuning.gear_count() <= 1.0 {
            0.4
        } else {
            let gear_number =
                (1.0 - (f32::from(self.gear) - 1.0) / (tuning.gear_count() - 1.0)).max(0.0);
            let handling_flags = tuning.gta_handling_flags as u32;
            let boost = if handling_flags & 0x1 != 0 {
                5.0
            } else if handling_flags & 0x2 != 0 {
                4.0
            } else {
                3.0
            };
            (gear_number * gear_number * boost + 1.0) * 0.4
        };
        if gta_transmission {
            // Port of cTransmission::CalculateDriveAcceleration's inertia
            // smoothing (a6/a7). Speeds are SI here, but the ratio is unitless.
            let gear_count = tuning.gear_count();
            let max_velocity_change = tuning.top_speed / gear_count / 3.0;
            let (current_down_diff, up_down_diff) = if self.speed < 0.0 {
                (
                    max_velocity_change + self.speed.abs(),
                    max_velocity_change + tuning.reverse_speed,
                )
            } else if self.gear <= 1 {
                (
                    max_velocity_change + self.speed.max(0.0),
                    max_velocity_change + tuning.gear_up_shift_speed(1),
                )
            } else {
                (
                    self.speed - tuning.gear_down_shift_speed(self.gear),
                    tuning.gear_up_shift_speed(self.gear)
                        - tuning.gear_down_shift_speed(self.gear),
                )
            };
            let ratio = if up_down_diff.abs() > 1e-5 {
                (current_down_diff / up_down_diff).clamp(-2.0, 2.0)
            } else {
                1.0
            };
            if !self.transmission_initialized {
                // GTA's inertia variables are already settled after a car has
                // been sitting on the ground since map load.
                self.transmission_a6 = ratio;
                self.transmission_a7 = 1.0;
                self.transmission_initialized = true;
            }
            if drive_grip > 0.0 {
                let inertia_delta = ratio - self.transmission_a6;
                let engine_response =
                    (1.0 - inertia_delta * tuning.gta_engine_inertia).clamp(0.1, 1.0);
                self.transmission_a6 = ratio;
                self.transmission_a7 =
                    engine_response * 0.15 + 0.85 * self.transmission_a7;
            } else {
                self.transmission_a6 += throttle.abs()
                    / tuning.gta_engine_inertia.max(0.001)
                    * (dt * 50.0)
                    * 0.1;
                self.transmission_a6 = self.transmission_a6.min(1.0);
                self.transmission_a7 = 0.1;
            }
            transmission_inertia = self.transmission_a7;
        }
        let acceleration = if braking {
            -self.speed.signum() * tuning.brake_deceleration * throttle.abs()
        } else if throttle >= 0.0 {
            let engine_force =
                throttle * tuning.acceleration * gear_acceleration_multiplier * transmission_inertia;
            if gta_transmission {
                // GTA tapers drive force only after this gear's MaxVelocity;
                // 0.05 game-units/tick equals 2.5 m/s at 50 Hz.
                let excess_speed = (self.speed.max(0.0) - band_top).max(0.0);
                engine_force * (1.0 - (excess_speed / 2.5).min(1.0))
            } else {
                engine_force
                    * (1.0 - (self.speed.max(0.0) / band_top).powi(2)).max(0.0)
            }
        } else if gta_transmission {
            let excess_speed = (-self.speed - tuning.reverse_speed).max(0.0);
            throttle
                * tuning.reverse_acceleration
                * 1.8
                * transmission_inertia
                * (1.0 - (excess_speed / 2.5).min(1.0))
        } else {
            throttle
                * tuning.reverse_acceleration
                * (1.0 - (-self.speed / tuning.reverse_speed).powi(2)).max(0.0)
        };
        let lateral_acceleration;
        let body_acceleration = if braking {
            acceleration * grip
        } else {
            acceleration * drive_grip
        };
        if gta_transmission {
            lateral_acceleration = self.apply_gtasa_wheel_forces(
                &mut velocity,
                supported_wheels,
                drive_wheels,
                throttle,
                braking,
                handbrake,
                acceleration,
                handling,
                dt,
            );
            // CPhysical::ApplyAirResistance: drag scales with game velocity
            // (units/tick), then is raised to the timestep. Convert SI speed
            // back to game units/tick for the formula.
            let game_speed = velocity.length() / 50.0;
            let timestep = dt * 50.0;
            let drag_factor =
                (1.0 - 0.0005 * tuning.gta_drag_multiplier * game_speed)
                    .max(0.0)
                    .powf(timestep);
            velocity *= drag_factor;
        } else {
            let old_speed = self.speed;
            let longitudinal_grip = if braking { grip } else { drive_grip };
            self.speed += acceleration * longitudinal_grip * dt;
            if braking && self.speed.signum() != old_speed.signum() {
                self.speed = 0.0;
            }
            let resistance = (ROLLING_RESISTANCE
                + tuning.aerodynamic_drag * self.speed * self.speed
                + if handbrake { 7.0 * grip } else { 0.0 })
                * dt;
            self.speed = self.speed.signum() * (self.speed.abs() - resistance).max(0.0);
            velocity += self.forward() * (self.speed - old_speed);
            // Approximate per-wheel normal load and load transfer while retaining
            // SARE's friction-circle tire response. A lifted wheel contributes no grip;
            // acceleration and cornering unload the appropriate wheels/axle.
            let approximate_lateral_acceleration = self.speed * self.yaw_rate;
            // Geometric probes can still see the road during takeoff. Only
            // wheels with active suspension support should contribute tire load.
            let loaded_wheels = supported_wheels.map(|supported| supported && grip > 0.0);
            let wheel_loads = wheel_normal_loads(
                loaded_wheels,
                acceleration,
                approximate_lateral_acceleration,
                (self.clearance * 0.8).clamp(0.25, 0.8),
            );
            // Tire capacity is load-sensitive: two equally loaded wheels sum to one
            // axle's baseline capacity; unloading a wheel reduces that axle's grip.
            let front_load_capacity = wheel_loads[1].sqrt() + wheel_loads[3].sqrt();
            let rear_load_capacity = wheel_loads[0].sqrt() + wheel_loads[2].sqrt();
            let tire_budget =
                (tuning.tire_grip * handling).powi(2) - (acceleration * longitudinal_grip * 0.5).powi(2);
            let front_traction_bias = if gta_transmission {
                (2.0 * tuning.gta_traction_bias).clamp(0.0, 2.0)
            } else {
                1.0
            };
            let rear_traction_bias = if gta_transmission {
                (2.0 - 2.0 * tuning.gta_traction_bias).clamp(0.0, 2.0)
            } else {
                1.0
            };
            let mut front_limit =
                tire_budget.max(0.5).sqrt() * front_load_capacity * front_traction_bias;
            let mut rear_limit =
                tuning.tire_grip * handling * rear_load_capacity * rear_traction_bias;
            if handbrake {
                rear_limit *= 0.12;
            }
            let front_slip =
                self.lateral_speed + 1.35 * self.yaw_rate - self.speed * self.steering_angle.tan();
            let rear_slip = self.lateral_speed - 1.35 * self.yaw_rate;
            // Latch a skid when the requested axle force exceeds its load-dependent
            // limit. Keep rear grip reduced until both axles settle, rather than
            // allowing the model to snap instantly back to full grip each tick.
            if grounded
                && ((front_slip * 5.5).abs() > front_limit || (rear_slip * 5.5).abs() > rear_limit)
            {
                self.skid_latched = true;
            }
            if grounded && front_slip.abs() < 0.35 && rear_slip.abs() < 0.35 {
                self.skid_latched = false;
            }
            if self.skid_latched && !handbrake {
                if gta_transmission {
                    // handling.cfg tractionLoss applies once a wheel is skidding;
                    // this axle-level adapter uses it until the skid state recovers.
                    front_limit *= tuning.gta_traction_loss;
                    rear_limit *= tuning.gta_traction_loss;
                } else {
                    rear_limit *= 0.82;
                }
            }
            let front_force = (-front_slip * 5.5).clamp(-front_limit, front_limit);
            let rear_force = (-rear_slip * 5.5).clamp(-rear_limit, rear_limit);
            lateral_acceleration = front_force + rear_force;
            velocity += side * lateral_acceleration * dt;
            self.yaw_rate +=
                (1.35 * (front_force - rear_force) / 2.2 - self.yaw_rate * 0.22) * dt;
            self.yaw_rate = self.yaw_rate.clamp(-2.0, 2.0);
        }
        let old_yaw = self.yaw;
        self.yaw += self.yaw_rate * dt;
        if !self.body_clear(world, self.position) {
            self.yaw = old_yaw;
            self.yaw_rate = 0.0;
        }
        let side = Vec3::new(self.yaw.cos(), 0.0, -self.yaw.sin());
        self.speed = velocity.dot(self.forward());
        self.lateral_speed = velocity.dot(side);
        let blend = 1.0 - (-8.0 * dt).exp();
        self.pitch +=
            ((slope_pitch + body_acceleration * 0.012).clamp(-0.6, 0.6) - self.pitch) * blend;
        self.roll +=
            ((slope_roll + lateral_acceleration * 0.016).clamp(-0.5, 0.5) - self.roll) * blend;
        let motion = (self.forward() * self.speed + side * self.lateral_speed) * dt;
        if grounded {
            self.travel += self.speed * dt;
        }
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
                // Hit a wall or a building: the car takes the knock.
                let impact =
                    (self.speed * self.speed + self.lateral_speed * self.lateral_speed).sqrt();
                self.damage
                    .crash(impact, to_model(self.yaw, motion.normalize_or_zero()));
                self.stop();
                break;
            }
            self.position = desired;
        }
        if grounded {
            let moved = self.position - before_motion;
            let ground_velocity = (moved.dot(self.forward()) * slope_pitch.tan()
                + moved.dot(side) * slope_roll.tan())
                / dt;
            let target = target + ground_velocity * dt;
            // A damped suspension settles instead of snapping to one centre ray.
            // Ground support cancels gravity before damping. Damping a velocity
            // already reduced by gravity creates a frame-rate-dependent height offset.
            // Tires can push against the road, but cannot pull the body down.
            // Preserve upward momentum over crests, even while the road remains
            // within suspension reach. Gravity alone brings an unloaded car back.
            let support = self.support_force(target, ground_velocity);
            self.vertical_speed += (support - 9.81) * dt;
            self.position.y += self.vertical_speed * dt;
            if self.position.y < target {
                self.position.y = target;
                self.vertical_speed = self.vertical_speed.max(ground_velocity);
            }
            self.airborne = support == 0.0;
        } else {
            self.vertical_speed -= 9.81 * dt;
            self.position.y += self.vertical_speed * dt;
            self.airborne = true;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Batch, Vertex};

    #[test]
    fn wheel_loads_transfer_under_acceleration() {
        let loads = wheel_normal_loads([true; 4], 6.0, 0.0, 0.5);
        assert!((loads.iter().sum::<f32>() - 1.0).abs() < 1e-5);
        assert!(loads[0] + loads[2] > loads[1] + loads[3]);
        assert_eq!(wheel_normal_loads([false; 4], 0.0, 0.0, 0.5), [0.0; 4]);
    }

    #[test]
    fn skid_latch_stays_set_during_slip_and_clears_after_recovery() {
        let world = CollisionWorld::from_batches(&[floor(-100.0, 100.0, 0.0)]);
        let mut car = Car::new(Vec3::Y * 0.6, 0.6);
        car.speed = 10.0;
        car.lateral_speed = 3.0;
        car.step(&world, 0.0, 0.0, false, 1.0, 1.0 / 120.0);
        assert!(car.skid_latched);

        car.lateral_speed = 0.0;
        car.yaw_rate = 0.0;
        car.step(&world, 0.0, 0.0, false, 1.0, 1.0 / 120.0);
        assert!(!car.skid_latched);
    }
    #[test]
    fn a_crash_stops_the_closing_speed_and_bounces_a_little() {
        let world = CollisionWorld::from_batches(&[]);
        let mut car = Car::new(Vec3::new(0.0, 1.0, 0.0), 0.5);
        car.speed = 20.0;
        // Rear-ended a parked car ahead: pushed back along -Z.
        car.collide(&world, Vec3::new(0.0, 0.0, -0.4), Vec3::ZERO);
        assert!((car.position.z + 0.4).abs() < 1e-5);
        assert!(car.speed < 0.0 && car.speed > -8.0, "{}", car.speed);
        // Moving away already: the contact leaves the speed alone.
        car.speed = -3.0;
        car.collide(&world, Vec3::new(0.0, 0.0, -0.1), Vec3::ZERO);
        assert_eq!(car.speed, -3.0);
        // Following a car that drives at the same speed: no impact.
        car.speed = 10.0;
        car.collide(&world, Vec3::new(0.0, 0.0, -0.1), Vec3::new(0.0, 0.0, 10.0));
        assert!((car.speed - 10.0).abs() < 1e-4);
    }
    #[test]
    fn crashes_dent_the_side_that_hit_and_cost_health() {
        let mut damage = Damage::default();
        assert_eq!(damage.crash(3.0, Vec3::NEG_Z), 0.0, "a bump does nothing");
        assert_eq!(damage.dents, 0);
        // Nose first into a wall at 72 km/h.
        let lost = damage.crash(20.0, Vec3::NEG_Z);
        assert_eq!(lost, 160.0);
        for panel in [Panel::BumperFront, Panel::Bonnet, Panel::Windscreen] {
            assert_ne!(
                damage.dents & dent_bit(VehiclePart::Panel(panel)),
                0,
                "{panel:?}"
            );
        }
        assert_eq!(damage.dents & dent_bit(VehiclePart::Panel(Panel::Boot)), 0);
        // Hit on the left: the left doors buckle, not the right.
        damage.crash(8.0, Vec3::NEG_X);
        assert_ne!(damage.dents & dent_bit(VehiclePart::Door(0)), 0);
        assert_ne!(damage.dents & dent_bit(VehiclePart::Door(2)), 0);
        assert_eq!(damage.dents & dent_bit(VehiclePart::Door(1)), 0);
    }
    #[test]
    fn a_burning_car_explodes_once_and_stays_a_wreck() {
        let mut damage = Damage::default();
        damage.hurt(700.0);
        assert!(!damage.burning() && !damage.update(1.0));
        damage.hurt(100.0);
        assert!(damage.burning());
        let mut seconds = 0.0;
        while !damage.update(0.1) {
            seconds += 0.1;
            assert!(seconds < 10.0, "never exploded");
        }
        assert!((3.5..5.5).contains(&seconds), "{seconds}");
        assert!(damage.wrecked && !damage.burning());
        assert!(!damage.update(0.1), "explodes only once");
        assert_eq!(damage.hurt(100.0), 0.0);
    }
    #[test]
    fn wheels_roll_and_steer_and_doors_swing_out_on_their_side() {
        let wheel = RigPiece {
            part: VehiclePart::Wheel {
                front: true,
                left: true,
            },
            damaged: false,
            pivot: Vec3::new(-0.8, 0.3, -1.3),
            radius: 0.3,
        };
        let top = wheel.pivot + Vec3::Y * 0.3;
        let rolled = Look {
            travel: 0.3 * std::f32::consts::FRAC_PI_2,
            ..Look::default()
        };
        // A quarter turn forwards brings the top of the tyre to the front (-Z).
        let moved = wheel.transform(&rolled).unwrap().transform_point3(top);
        assert!(
            (moved - (wheel.pivot - Vec3::Z * 0.3)).length() < 1e-4,
            "{moved}"
        );
        let steered = Look {
            steer: 0.4,
            ..Look::default()
        };
        let hub = wheel
            .transform(&steered)
            .unwrap()
            .transform_point3(wheel.pivot);
        assert!((hub - wheel.pivot).length() < 1e-5, "steers about its hub");
        for (door, outward) in [(0_u8, -1.0_f32), (1, 1.0)] {
            let piece = RigPiece {
                part: VehiclePart::Door(door),
                damaged: false,
                pivot: Vec3::new(outward * 0.9, 0.5, -0.6),
                radius: 0.0,
            };
            let mut look = Look::default();
            look.doors[usize::from(door)] = 1.0;
            // The back edge of the door swings away from the car.
            let back = piece.pivot + Vec3::Z * 1.1;
            let open = piece.transform(&look).unwrap().transform_point3(back);
            assert!(open.x * outward > 0.9 + 0.5, "door {door}: {open}");
        }
    }
    #[test]
    fn dented_parts_swap_for_their_damaged_look() {
        let piece = |damaged| RigPiece {
            part: VehiclePart::Panel(Panel::Bonnet),
            damaged,
            pivot: Vec3::ZERO,
            radius: 0.0,
        };
        let intact = Look::default();
        let dented = Look {
            dents: dent_bit(VehiclePart::Panel(Panel::Bonnet)),
            ..Look::default()
        };
        assert!(piece(false).transform(&intact).is_some());
        assert!(piece(true).transform(&intact).is_none());
        assert!(piece(false).transform(&dented).is_none());
        assert!(piece(true).transform(&dented).is_some());
    }
    #[test]
    fn driving_into_a_wall_hurts_the_car() {
        let world = CollisionWorld::from_batches(&[
            floor(-1000.0, 1000.0, 0.0),
            mesh(&[
                [-10.0, -1.0, 10.0],
                [10.0, -1.0, 10.0],
                [10.0, 5.0, 10.0],
                [-10.0, -1.0, 10.0],
                [10.0, 5.0, 10.0],
                [-10.0, 5.0, 10.0],
            ]),
        ]);
        let mut car = Car::new(Vec3::Y * 0.6, 0.6);
        car.speed = 25.0;
        for _ in 0..120 {
            car.step(&world, 1.0, 0.0, false, 1.0, 1.0 / 60.0);
        }
        assert!(car.damage.health < FULL_HEALTH - 100.0, "{:?}", car.damage);
        assert_ne!(
            car.damage.dents & dent_bit(VehiclePart::Panel(Panel::BumperFront)),
            0
        );
        assert!(car.look().travel > 5.0, "the wheels turned on the way");
    }
    #[test]
    fn measured_size_follows_the_model_and_stays_sane() {
        let size = measure([[-1.0, -0.4, -2.5], [1.0, 1.2, 2.5]]);
        assert_eq!(size, [1.0, 2.5, 1.6]);
        assert_eq!(measure(std::iter::empty()), DEFAULT_SIZE);
        assert_eq!(measure([[0.0; 3]]), [0.3, 0.6, 0.5]);
    }
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
            let invalid_value = match key.as_str() {
                "gta_centre_of_mass_x"
                | "gta_centre_of_mass_y"
                | "gta_centre_of_mass_z"
                | "gta_suspension_upper_limit"
                | "gta_suspension_lower_limit" => 1000.0,
                _ => -1.0,
            };
            invalid[key] = serde_json::json!(invalid_value);
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
    fn one_gear_keeps_the_single_band_curve() {
        let tuning = Handling::default();
        assert_eq!(tuning.gear_count(), 1.0);
        // The one band ends at the top speed, so the prototype curve the
        // project was tuned around is untouched.
        assert_eq!(tuning.gear_band_top(1), tuning.top_speed);
        let mut car = Car::new(Vec3::Y * 0.6, 0.6);
        car.handling = tuning;
        car.speed = 30.0;
        car.shift_gears(&tuning, 1.0);
        assert_eq!(car.gear, 1);
    }
    #[test]
    fn five_gears_shift_up_through_the_bands_and_back_down() {
        let mut car = Car::new(Vec3::Y * 0.6, 0.6);
        let tuning = Handling {
            gears: 5.0,
            ..Handling::default()
        };
        car.handling = tuning;
        // Bands widen towards the top and the last one reaches `top_speed`.
        assert!(tuning.gear_band_top(1) < tuning.gear_band_top(2));
        assert_eq!(tuning.gear_band_top(5), tuning.top_speed);
        // Each band pulls past its own shift point, so no gear can stall.
        for gear in 1..5u8 {
            assert!(
                tuning.reachable_top_speed() > 0.6 * tuning.gear_band_top(gear),
                "gear {gear} stalls"
            );
        }
        // Crossing GTA's upshift point selects exactly the next gear.
        for gear in 1..5u8 {
            car.gear = gear;
            car.speed = tuning.gear_up_shift_speed(gear) + 0.05;
            car.shift_gears(&tuning, 1.0);
            assert_eq!(car.gear, gear + 1, "did not leave gear {gear}");
        }
        // Well inside the first band the car drops back to first.
        car.speed = 0.3 * tuning.gear_band_top(1);
        car.shift_gears(&tuning, 1.0);
        assert_eq!(car.gear, 1);
        // Legacy fallback keeps reverse in first; GTA-loaded handling uses gear zero.
        car.speed = -3.0;
        car.shift_gears(&tuning, -1.0);
        assert_eq!(car.gear, 1);
        let gta_tuning = Handling {
            gears: 5.0,
            gta_handling_loaded: 1.0,
            ..Handling::default()
        };
        car.gear = 1;
        car.shift_gears(&gta_tuning, -1.0);
        assert_eq!(car.gear, 0);
    }
    #[test]
    fn a_car_sitting_on_a_shift_point_does_not_hunt_between_gears() {
        let mut car = Car::new(Vec3::Y * 0.6, 0.6);
        let tuning = Handling {
            gears: 5.0,
            ..Handling::default()
        };
        let up = tuning.gear_up_shift_speed(1);
        car.gear = 1;
        car.speed = up + 0.05;
        car.shift_gears(&tuning, 1.0);
        assert_eq!(car.gear, 2);
        // Either side of the first gear's up point stays in second because
        // the down point is lower, preserving the gearbox's shift hysteresis.
        for speed in [up - 0.05f32, up + 0.05, up - 0.05, up + 0.05] {
            car.speed = speed;
            car.shift_gears(&tuning, 1.0);
            assert_eq!(car.gear, 2, "gear hunted at {speed}");
        }
    }
    #[test]
    fn gears_shift_while_driving_and_keep_the_top_speed() {
        let world = CollisionWorld::from_batches(&[floor(-2000.0, 2000.0, 0.0)]);
        let geared = Handling {
            gears: 5.0,
            ..Handling::default()
        };
        let mut single = Car::new(Vec3::Y * 0.6, 0.6);
        let mut shifting = Car::new(Vec3::Y * 0.6, 0.6).with_handling(geared);
        for _ in 0..3600 {
            single.step(&world, 1.0, 0.0, false, 1.0, 1.0 / 120.0);
            shifting.step(&world, 1.0, 0.0, false, 1.0, 1.0 / 120.0);
        }
        assert_eq!(shifting.gear, 5, "never reached top gear");
        assert!(shifting.speed > 25.0, "speed {}", shifting.speed);
        // The top band is the whole range, so the geared car still ends up at
        // the same drag-limited speed; it is only slower on the way there.
        assert!(
            (shifting.speed - single.speed).abs() < 0.15 * single.speed.max(1.0),
            "geared {} vs single {}",
            shifting.speed,
            single.speed
        );
    }
    #[test]
    fn a_half_grounded_car_still_reaches_top_gear() {
        // Grip at half means only two wheels carry the car, so every gear pulls
        // less. The shift points have to stay below that reduced pull or the
        // car would sit in a middle gear forever.
        let world = CollisionWorld::from_batches(&[floor(-2000.0, 2000.0, 0.0)]);
        let mut car = Car::new(Vec3::Y * 0.6, 0.6).with_handling(Handling {
            gears: 5.0,
            ..Handling::default()
        });
        for _ in 0..3600 {
            car.step(&world, 1.0, 0.0, false, 0.5, 1.0 / 120.0);
        }
        assert_eq!(
            car.gear, 5,
            "stuck in gear {} at {} m/s",
            car.gear, car.speed
        );
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
    fn crossing_a_hill_crest_can_leave_the_road_and_a_real_drop_still_falls() {
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
            strip(24.0, 0.0, 600.0, 0.0),
        ]);
        let mut car = Car::new(Vec3::new(0.0, 0.6, -5.0), 0.6);
        let mut crest_air = false;
        for _ in 0..720 {
            car.step(&world, 1.0, 0.0, false, 1.0, 1.0 / 120.0);
            if let Some(road) = world.ground_below(car.position, car.position.y + 1.0) {
                let gap = car.position.y - car.clearance - road;
                assert!(
                    gap > -0.4 && gap < 5.0,
                    "crest penetrated road or launched excessively: gap={gap}, {:?}",
                    car.position
                );
                crest_air |= car.position.z > 12.0 && gap > 0.6;
            }
        }
        assert!(crest_air, "fast crest crossing remained glued to the road");
        assert!((car.position.y - 0.6).abs() < 0.1, "car failed to land");
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
    fn suspension_cannot_pull_an_upward_moving_car_down() {
        let world = CollisionWorld::from_batches(&[floor(-100.0, 100.0, 0.0)]);
        for hz in [30.0, 60.0, 120.0] {
            let mut car = Car::new(Vec3::Y * 0.8, 0.6);
            car.vertical_speed = 5.0;
            car.step(&world, 0.0, 0.0, false, 1.0, 1.0 / hz);
            assert!(
                (car.vertical_speed - (5.0 - 9.81 / hz)).abs() < 0.001,
                "suspension cancelled launch momentum at {hz} Hz: {}",
                car.vertical_speed
            );
            assert!(car.position.y > 0.8);
        }
    }
    #[test]
    fn unloaded_tires_cannot_accelerate_brake_or_steer_during_takeoff() {
        let world = CollisionWorld::from_batches(&[floor(-100.0, 100.0, 0.0)]);
        for throttle in [-1.0, 1.0] {
            let mut coast = Car::new(Vec3::Y * 0.8, 0.6);
            coast.speed = 10.0;
            coast.vertical_speed = 5.0;
            let mut driven = Car::new(coast.position, coast.clearance);
            driven.speed = coast.speed;
            driven.vertical_speed = coast.vertical_speed;
            for _ in 0..6 {
                coast.step(&world, 0.0, 0.0, false, 1.0, 1.0 / 120.0);
                driven.step(&world, throttle, 1.0, true, 1.0, 1.0 / 120.0);
            }
            assert!(
                (driven.speed - coast.speed).abs() < 0.001,
                "unloaded tires changed speed: {} versus {}",
                driven.speed,
                coast.speed
            );
            assert!(driven.yaw.abs() < 0.0001, "unloaded tires turned the body");
            assert!(driven.position.distance(coast.position) < 0.001);
        }
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
        for _ in 0..720 {
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
        assert!(
            peak < 9.0,
            "car was launched far above its climb rate: {peak}"
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
