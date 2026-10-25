//! `tHandlingData` — the per-vehicle record from `data/handling.cfg`.
//!
//! GTA reads 33 whitespace-separated fields per line
//! (`cHandlingDataMgr::LoadHandlingData` -> `tHandlingData::InitFromData`), then
//! `InitFromData` itself rewrites a couple of them:
//!
//! * `engineAcceleration` is stored as `cfg * 0.4`, and it is *that* value the
//!   transmission divides by 4 (4WD) or 2 (RWD/FWD). Applying the 0.4 twice,
//!   or forgetting it, is the single most common "my car accelerates at half /
//!   double the rate" bug. [`Handling::raw_engine_acceleration`] keeps the file
//!   value so the mistake is visible.
//! * `dragMult` becomes `raw_drag_factor * drag_coefficient` at use sites; see
//!   [`crate::vehicle`] for the exponent form.
//!
//! Everything else is a straight copy. Field *order* below matches the Python
//! reference, which was checked field-by-field against the game's parse order.
//! Where a name is ambiguous the handling.cfg comment order is used.
//!
//! Provenance: `tHandlingData` from the US 1.0 decompilation as published by the
//! gta-reversed project, plus measurements on stock model 411 (Infernus).

use crate::math::Vec3;

/// Wheel indices are always in this order in GTA: front-left, rear-left,
/// front-right, rear-right. Not a layout choice — every array is indexed this
/// way, including `m_aWheelState` and the collision line list.
pub const WHEEL_FL: usize = 0;
pub const WHEEL_RL: usize = 1;
pub const WHEEL_FR: usize = 2;
pub const WHEEL_RR: usize = 3;
pub const FRONT_WHEELS: [usize; 2] = [WHEEL_FL, WHEEL_FR];
pub const REAR_WHEELS: [usize; 2] = [WHEEL_RL, WHEEL_RR];
pub const WHEEL_NAMES: [&str; 4] = ["front-left", "rear-left", "front-right", "rear-right"];

// --- eVehicleHandlingFlags / model flags used by the automobile path --------
/// `HANDLING_1ST_GEAR` (0x1): first-gear acceleration multiplier is x5.
pub const HANDLING_FLAG_1ST_GEAR: u32 = 0x0000_0001;
/// `HANDLING_2ND_GEAR` (0x2): multiplier is x4 (otherwise x3).
pub const HANDLING_FLAG_2ND_GEAR: u32 = 0x0000_0002;
/// `HANDLING_REARWHEEL_STEER` (0x40): the handbrake steers the rear axle
/// instead of locking it (Monster). Reached through `ProcessCarWheelPair`.
pub const HANDLING_FLAG_REARWHEEL_STEER: u32 = 0x0000_0040;
/// `HANDLING_NOS` (0x80000): a permanent NOS install; disables the rear-traction
/// temperature multiplier used for non-NOS cars.
pub const HANDLING_FLAG_NOS: u32 = 0x0008_0000;
/// `HANDLING_REARWHEEL_DRIVE` (0x800000): process the rear wheel pair first.
pub const HANDLING_FLAG_REAR_WHEEL_FIRST: u32 = 0x0080_0000;
/// `HANDLING_USE_MAXSP_LIMIT` (0x1000000): `maxVelocity` is a hard cap and the
/// gear table is built differently.
pub const HANDLING_FLAG_USE_MAXSP_LIMIT: u32 = 0x0100_0000;
/// `HANDLING_OFFROAD_ABILITY` / `_2` — only used by `ProcessWheel` surface logic.
pub const HANDLING_FLAG_OFFROAD_ABILITY: u32 = 0x0010_0000;
pub const HANDLING_FLAG_OFFROAD_ABILITY2: u32 = 0x0020_0000;
/// `VEHICLE_HANDLING_MODEL_FORCE_GROUND_CLEARANCE` (0x40000000), a *model*
/// flag: `CAutomobile::SetupSuspensionLines` clamps the body spheres above the
/// road while allocating lines for such a model.
pub const MODEL_FLAG_FORCE_GROUND_CLEARANCE: u32 = 0x4000_0000;

/// One automobile handling record, in game units.
///
/// Defaults are the stock INFERNUS row (model 411), so `Handling::default()`
/// is a driveable car and tests need no fixture file.
#[derive(Clone, Debug, PartialEq)]
pub struct Handling {
    pub model_id: i32,
    /// `m_fMass` — kg-ish. `ApplyMoveForce` divides by this.
    pub mass: f32,
    /// `m_fTurnMass` — the *scalar* rotational inertia GTA uses instead of a
    /// tensor. `ApplyTurnForce` divides by this. Usually ~2x mass.
    pub turn_mass: f32,
    pub elasticity: f32,
    pub drag_coefficient: f32,
    /// `0.0005` for cars; multiplied by `drag_coefficient * |v|` inside the
    /// per-tick drag multiplier (see `vehicle::drag_multiplier`).
    pub raw_drag_factor: f32,
    /// Local-space centre of mass (metres from the model origin).
    pub center_of_mass: Vec3,
    /// `percentSubmerged`, unused by the automobile ground path but kept for
    /// completeness.
    pub percent_submerged: f32,
    pub traction_multiplier: f32,
    /// `tractionLoss` — grip multiplier applied once a wheel is already sliding.
    pub traction_loss: f32,
    pub traction_bias: f32,
    pub brake_bias: f32,
    pub brake_deceleration: f32,
    /// Degrees in the file; code converts.
    pub steering_lock_degrees: f32,
    pub suspension_force_level: f32,
    pub suspension_damping_level: f32,
    pub suspension_high_speed_damping: f32,
    pub suspension_bias: f32,
    pub suspension_anti_dive: f32,
    pub number_of_gears: usize,
    /// `"4"`, `"F"` or `"R"`.
    pub drive_type: char,
    /// Post-`InitFromData` value: `raw * 0.4`.
    pub engine_acceleration: f32,
    /// The handling.cfg value, kept so the 0.4 conversion cannot be applied
    /// twice (or zero times) by accident.
    pub raw_engine_acceleration: f32,
    pub engine_inertia: f32,
    /// km/h in the file; converted by the transmission.
    pub max_velocity: f32,
    pub model_flags: u32,
    pub handling_flags: u32,
    pub velocity_frequency: f32,
    pub surface_adhesive_limit: f32,
    pub wheel_friction: f32,
    /// `suspension_upper_limit - suspension_lower_limit` for the reference
    /// model; per-axle values are derived in [`Handling::axle_line_length`].
    pub suspension_spring_length: f32,
    pub suspension_line_length: f32,
    pub wheel_track: f32,
    pub wheel_front_y: f32,
    pub wheel_rear_y: f32,
    pub suspension_line_start_z: f32,
    pub wheel_radius: f32,
    pub wheel_radius_front: f32,
    pub wheel_radius_rear: f32,
    pub suspension_upper_limit: f32,
    pub suspension_lower_limit: f32,
    /// `CColModel` bound box, from the model's collision file. Used by the
    /// roll-on-to-wheels branch and by camera framing.
    pub collision_bound_max_x: f32,
    pub collision_bound_max_z: f32,
    pub collision_bound_width: f32,
    pub collision_bound_length: f32,
    pub collision_bound_height: f32,
    pub rail_track_resistance: f32,
}

impl Default for Handling {
    /// Stock INFERNUS (model 411) — the values every constant in this crate was
    /// measured against.
    fn default() -> Self {
        Self {
            model_id: 411,
            mass: 1400.0,
            // stock value is 2725.3; the native field is f32, so it stores as
            // 2725.300048828125.
            turn_mass: 2725.3,
            elasticity: 0.05,
            drag_coefficient: 1.5,
            raw_drag_factor: 0.0005,
            center_of_mass: Vec3::new(0.0, 0.0, -0.25),
            percent_submerged: 70.0,
            traction_multiplier: 0.70,
            traction_loss: 0.80,
            traction_bias: 0.50,
            brake_bias: 0.51,
            brake_deceleration: 11.0,
            steering_lock_degrees: 30.0,
            suspension_force_level: 1.20,
            suspension_damping_level: 0.19,
            suspension_high_speed_damping: 0.0,
            suspension_bias: 0.50,
            suspension_anti_dive: 0.40,
            number_of_gears: 5,
            drive_type: '4',
            engine_acceleration: 12.0,
            raw_engine_acceleration: 30.0,
            engine_inertia: 10.0,
            max_velocity: 240.0,
            model_flags: 0x4000_2004,
            handling_flags: 0x00C0_4000,
            velocity_frequency: 1.0,
            surface_adhesive_limit: 4.5,
            wheel_friction: 0.90,
            suspension_spring_length: 0.35,
            suspension_line_length: 0.70,
            wheel_track: 1.91,
            wheel_front_y: 1.7133,
            wheel_rear_y: -1.7132,
            suspension_line_start_z: -0.10,
            wheel_radius: 0.35,
            wheel_radius_front: 0.35,
            wheel_radius_rear: 0.35,
            suspension_upper_limit: 0.25,
            suspension_lower_limit: -0.10,
            collision_bound_max_x: 1.187_034_3,
            collision_bound_max_z: 0.685_905_15,
            collision_bound_width: 2.382_349_7,
            collision_bound_length: 5.550_418,
            collision_bound_height: 1.301_431_4,
            rail_track_resistance: 0.003,
        }
    }
}

impl Handling {
    /// The stock INFERNUS row from `data/handling.cfg`, exactly as shipped.
    /// Handy as a parser fixture.
    pub const STOCK_INFERNUS_LINE: &'static str = "INFERNUS 1400.0 2725.3 1.5 0.0 0.0 -0.25 70 0.70 0.8 0.50 \
         5 240.0 30.0 10.0 4 P 11.0 0.51 0 30.0 1.2 0.19 0.0 0.25 -0.10 0.5 0.4 0.37 0.72 95000 40002004 C04000 1 1 1";

    /// `tHandlingData::InitFromData` — parse one handling.cfg row.
    ///
    /// Field indices are the file's, which is *not* the C++ struct order:
    /// 0 name, 1 mass, 2 turn mass, 3 drag, 4..6 centre of mass,
    /// 7 percent submerged, 8 traction multiplier, 9 traction loss,
    /// 10 traction bias, 11 gear count, 12 max velocity (km/h),
    /// 13 engine acceleration, 14 engine inertia, 15 drive type,
    /// 16 engine type, 17 brake deceleration, 18 brake bias, 19 ABS,
    /// 20 steering lock (deg), 21 suspension force, 22 suspension damping,
    /// 23 suspension high-speed damping, 24 upper limit, 25 lower limit,
    /// 26 suspension bias, 27 anti-dive, 28 collision damage multiplier,
    /// 29 ?, 30 monetary value, 31 model flags (hex), 32 handling flags (hex).
    #[allow(clippy::field_reassign_with_default)] // 40 fields, assigned from the row
    pub fn from_cfg_line(line: &str) -> Result<Self, String> {
        let body = line.split('#').next().unwrap_or("");
        let f: Vec<&str> = body.split_whitespace().collect();
        if f.len() < 33 {
            return Err(format!("handling.cfg row needs >= 33 fields, got {}", f.len()));
        }
        let num = |i: usize| -> Result<f32, String> {
            f[i].parse::<f32>().map_err(|e| format!("field {}: {e}", i))
        };
        let raw_accel = num(13)?;
        let spring_upper = num(24)?;
        let spring_lower = num(25)?;
        let mut h = Handling::default();
        h.model_id = 0; // caller sets the model id from vehicles.ide
        h.mass = num(1)?;
        h.turn_mass = num(2)?;
        h.drag_coefficient = num(3)?;
        h.center_of_mass = Vec3::new(num(4)?, num(5)?, num(6)?);
        h.percent_submerged = num(7)?;
        h.traction_multiplier = num(8)?;
        h.traction_loss = num(9)?;
        h.traction_bias = num(10)?;
        h.number_of_gears = num(11)? as usize;
        h.max_velocity = num(12)?;
        h.raw_engine_acceleration = raw_accel;
        h.engine_acceleration = raw_accel * 0.4;
        h.engine_inertia = num(14)?;
        h.drive_type = f[15].chars().next().unwrap_or('4').to_ascii_uppercase();
        h.brake_deceleration = num(17)?;
        h.brake_bias = num(18)?;
        h.steering_lock_degrees = num(20)?;
        h.suspension_force_level = num(21)?;
        h.suspension_damping_level = num(22)?;
        h.suspension_high_speed_damping = num(23)?;
        h.suspension_upper_limit = spring_upper;
        h.suspension_lower_limit = spring_lower;
        h.suspension_spring_length = spring_upper - spring_lower;
        h.suspension_bias = num(26)?;
        h.suspension_anti_dive = num(27)?;
        h.model_flags = parse_handling_flag(f[31])?;
        h.handling_flags = parse_handling_flag(f[32])?;
        Ok(h)
    }

    /// `1.0 / mass`, which the native stores separately (`m_fMassRecpr`).
    pub fn inverse_mass(&self) -> f32 {
        1.0 / self.mass.max(1e-12)
    }

    /// Wheel radius for one axle, from `vehicles.ide` (`m_fWheelSizeFront/Rear`).
    /// `CAutomobile::SetupSuspensionLines` calls `GetSizeOfWheel`.
    pub fn axle_wheel_radius(&self, front: bool) -> f32 {
        if front {
            self.wheel_radius_front
        } else {
            self.wheel_radius_rear
        }
    }

    /// Suspension line length for one axle: `start.z - end.z`.
    pub fn axle_line_length(&self, front: bool) -> f32 {
        self.suspension_spring_length + self.axle_wheel_radius(front)
    }

    /// Reconstruct the local suspension line for one wheel, as
    /// `CAutomobile::SetupSuspensionLines` does. The end point carries the
    /// axle's own wheel radius. Front axle is `y >= 0`.
    pub fn suspension_line(&self, x: f32, y: f32) -> (Vec3, Vec3) {
        let wheel_dummy_z = self.suspension_line_start_z - self.suspension_upper_limit;
        let radius = self.axle_wheel_radius(y >= 0.0);
        (
            Vec3::new(x, y, wheel_dummy_z + self.suspension_upper_limit),
            Vec3::new(x, y, wheel_dummy_z + self.suspension_lower_limit - radius),
        )
    }

    /// Collision-line fraction -> normalised compression (`0.0` fully extended,
    /// `1.0` fully compressed), with Automobile.cpp's wheel-radius
    /// normalisation (`1 - springLength / lineLength`).
    pub fn normalized_compression(&self, line_fraction: f32, front: bool) -> f32 {
        let length = self.axle_line_length(front).max(1e-12);
        let wheel_radius = 1.0 - self.suspension_spring_length / length;
        (line_fraction - wheel_radius) / (1.0 - wheel_radius).max(1e-12)
    }

    pub fn gear_count(&self) -> usize {
        self.number_of_gears
    }

    pub fn is_four_wheel_drive(&self) -> bool {
        self.drive_type == '4'
    }

    pub fn force_ground_clearance(&self) -> bool {
        self.model_flags & MODEL_FLAG_FORCE_GROUND_CLEARANCE != 0
    }

    pub fn uses_maxsp_limit(&self) -> bool {
        self.handling_flags & HANDLING_FLAG_USE_MAXSP_LIMIT != 0
    }

    pub fn rear_wheel_first(&self) -> bool {
        self.handling_flags & HANDLING_FLAG_REAR_WHEEL_FIRST != 0
    }

    pub fn rear_steer_by_handbrake(&self) -> bool {
        self.handling_flags & HANDLING_FLAG_REARWHEEL_STEER != 0
    }
}

/// handling.cfg writes flags as bare hex (`C04000`, `40002004`) — *not* as
/// decimal, and without a `0x` prefix. Parsing them with `f32`/decimal silently
/// produces a completely different car.
pub fn parse_handling_flag(text: &str) -> Result<u32, String> {
    let t = text.trim();
    let (neg, digits) = if let Some(rest) = t.strip_prefix('-') {
        (true, rest)
    } else if let Some(rest) = t.strip_prefix('+') {
        (false, rest)
    } else {
        (false, t)
    };
    let digits = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
        .unwrap_or(digits);
    let value = u32::from_str_radix(digits, 16)
        .map_err(|e| format!("bad hex flag {text:?}: {e}"))?;
    Ok(if neg { (!value).wrapping_add(1) } else { value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_infernus_parses_to_defaults() {
        let h = Handling::from_cfg_line(Handling::STOCK_INFERNUS_LINE).expect("parse");
        assert_eq!(h.mass, 1400.0);
        assert_eq!(h.turn_mass, 2725.3);
        assert_eq!(h.number_of_gears, 5);
        assert_eq!(h.drive_type, '4');
        // 30.0 in the file, stored as 12.0 after InitFromData.
        assert_eq!(h.engine_acceleration, 12.0);
        assert_eq!(h.raw_engine_acceleration, 30.0);
        assert_eq!(h.suspension_spring_length, 0.35);
        assert_eq!(h.model_flags, 0x4000_2004);
        assert_eq!(h.handling_flags, 0x00C0_4000);
        assert_eq!(h.traction_loss, 0.8);
        assert_eq!(h.brake_bias, 0.51);
    }

    #[test]
    fn flags_are_hex_not_decimal() {
        assert_eq!(parse_handling_flag("C04000").unwrap(), 0x00C0_4000);
        assert_eq!(parse_handling_flag("40000800").unwrap(), 0x4000_0800);
        assert_eq!(parse_handling_flag("0x40000800").unwrap(), 0x4000_0800);
    }

    #[test]
    fn suspension_line_end_uses_axle_radius() {
        let h = Handling::default();
        let (start, end) = h.suspension_line(-0.955, 1.7133);
        assert!((start.z - (-0.10)).abs() < 1e-6);
        assert!((end.z - (-0.80)).abs() < 1e-6); // -0.10 (start) - 0.25 (upper) - 0.10 (lower) - 0.35 (radius)
    }
}
