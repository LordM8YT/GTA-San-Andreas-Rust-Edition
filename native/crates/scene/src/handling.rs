//! Per-model driving feel from the player's own `data/handling.cfg`.
//!
//! Only the column meanings listed in the file's own header are used. Units
//! and every scale below are SARE's own choices, tuned by watching how each
//! car drives, not taken from the game's code.
use crate::vehicle::{Handling, ROLLING_RESISTANCE};
use std::path::Path;

/// Share of a row's maximum velocity (read as km/h) a car reaches flat out.
const TOP_SPEED_SCALE: f32 = 0.9;
/// Engine acceleration column to m/s².
const ENGINE_SCALE: f32 = 0.30;
/// Engine inertia column that halves a car's pull.
const INERTIA_HALVING: f32 = 50.0;
/// Mass the engine pull is relative to, in kg.
const REFERENCE_MASS: f32 = 1500.0;
/// Seconds of throttle lag per unit of engine inertia.
const LAG_PER_INERTIA: f32 = 0.012;
/// Aerodynamic drag per unit of the drag column, m⁻¹.
const DRAG_PER_UNIT: f32 = 0.0004;
/// Braking column to m/s².
const BRAKE_SCALE: f32 = 1.2;
/// Grip of a car with traction 0.75, the common saloon value.
const SALOON_GRIP: f32 = 4.6;
/// Squared radius of gyration, m², that makes an average saloon's turn mass 1.
const SALOON_GYRATION: f32 = 2.8;
/// Wheelbase the centre-of-mass offset is measured against, m.
const WHEELBASE: f32 = 2.7;
/// How far traction bias moves grip between the axles.
const BIAS_GAIN: f32 = 1.2;
/// Mid-air turning for cars from handling.cfg, rad/s².
const AIR_CONTROL: f32 = 1.5;
/// handlingFlags bit for vehicles without a handbrake.
const NO_HANDBRAKE: u32 = 0x10;

/// Handling for a vehicle model, or `None` if the files lack it.
pub fn for_model(game: &Path, model: &str) -> Option<Handling> {
    let read = |file: &str| {
        sa_assets::game_path::resolve(game, file)
            .ok()
            .and_then(|path| std::fs::read_to_string(path).ok())
    };
    let id = handling_id(&read("data/vehicles.ide")?, model)?;
    let line = read("data/handling.cfg")?
        .lines()
        .find(|line| line.split_whitespace().next() == Some(id.as_str()))?
        .to_owned();
    from_line(&line)
}

/// The handling identifier column of a `cars` row in vehicles.ide.
fn handling_id(ide: &str, model: &str) -> Option<String> {
    ide.lines().find_map(|line| {
        let fields: Vec<_> = line.split(',').map(str::trim).collect();
        (fields.len() > 5 && fields[1].eq_ignore_ascii_case(model) && fields[3] == "car")
            .then(|| fields[4].to_ascii_uppercase())
    })
}

/// Parses a standard car row: name, mass, turn mass, drag, centre of mass
/// (3), submerged, traction multiplier, loss, bias, gears, max velocity,
/// engine acceleration, inertia, drive, engine type, brake deceleration,
/// brake bias, ABS, steering lock, suspension (7), seat offset, damage,
/// value, model flags, handling flags, lights (2), anim group.
pub(crate) fn from_line(line: &str) -> Option<Handling> {
    let fields: Vec<_> = line.split_whitespace().collect();
    if fields.len() < 33 {
        return None;
    }
    let number = |index: usize| fields[index].parse::<f32>().ok().filter(|v| v.is_finite());
    let base = Handling::default();
    let mass = number(1)?.max(1.0);
    let turn_mass = number(2)?.max(1.0);
    let drag = number(3)?.max(0.0);
    let inertia = number(14)?.max(0.0);
    let flags = u32::from_str_radix(fields[32], 16).unwrap_or(0);

    let acceleration = (ENGINE_SCALE * number(13)? / (1.0 + inertia / INERTIA_HALVING)
        * (REFERENCE_MASS / mass).powf(0.3).clamp(0.4, 1.1))
    .clamp(0.5, 20.0);
    let target = (TOP_SPEED_SCALE * number(12)? / 3.6).clamp(5.0, 85.0);
    // The engine pull fades towards `top_speed`; pick that point so the pull
    // still covers rolling resistance and drag at the target speed.
    let mut aerodynamic_drag = (DRAG_PER_UNIT * drag).clamp(0.0, 0.02);
    let spare = |drag: f32| acceleration - ROLLING_RESISTANCE - drag * target * target;
    if spare(aerodynamic_drag) < 0.1 * acceleration {
        aerodynamic_drag = ((0.9 * acceleration - ROLLING_RESISTANCE) / (target * target)).max(0.0);
    }
    let top_speed =
        (target * (acceleration / spare(aerodynamic_drag).max(0.01)).sqrt()).clamp(5.0, 90.0);
    let drive = match fields[15] {
        "R" => 1.0,
        "4" => 2.0,
        _ => 0.0,
    };

    let handling = Handling {
        top_speed,
        acceleration,
        reverse_acceleration: (0.6 * acceleration).clamp(0.5, 10.0),
        reverse_speed: (0.25 * target).clamp(1.0, 12.0),
        brake_deceleration: (BRAKE_SCALE * number(17)?).clamp(1.0, 25.0),
        tire_grip: (SALOON_GRIP * (number(8)? / 0.75).max(0.0).sqrt()).clamp(3.2, 6.5),
        steering_lock: number(20)?.to_radians().clamp(0.3, 0.9),
        aerodynamic_drag,
        traction_loss: (0.4 + 0.55 * number(9)?).clamp(0.6, 0.95),
        front_grip: (1.0 + BIAS_GAIN * (number(10)? - 0.5)).clamp(0.4, 1.6),
        drive,
        brake_front: number(18)?.clamp(0.2, 0.8),
        engine_lag: (LAG_PER_INERTIA * inertia).clamp(0.0, 1.5),
        yaw_inertia: (turn_mass / (mass * SALOON_GYRATION)).clamp(0.5, 4.0),
        front_weight: (0.5 + number(5)? / WHEELBASE).clamp(0.35, 0.65),
        cg_offset: number(6)?.clamp(-0.3, 0.3),
        handbrake: if flags & NO_HANDBRAKE != 0 { 0.0 } else { 1.0 },
        air_control: AIR_CONTROL,
        ..base
    };
    handling.validate().ok()?;
    Some(handling)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub(crate) const TAXI: &str = "TAXI 1450.0 4056.4 2.2 0.0 0.3 -0.25 75 0.80 0.75 0.45 5 180.0 19.0 10.0 F P 9.1 0.60 0 35.0 1.4 0.1 0.0 0.25 -0.15 0.54 0.0 0.2 0.51 20000 0 200000 0 1 0";
    pub(crate) const INFERNUS: &str = "INFERNUS 1400.0 2725.3 1.5 0.0 0.0 -0.25 70 0.70 0.8 0.50 5 240.0 30.0 10.0 4 P 11.0 0.51 0 30.0 1.2 0.19 0.0 0.25 -0.10 0.5 0.4 0.37 0.72 95000 40002004 C04000 1 1 1";
    pub(crate) const VOODOO: &str = "VOODOO 1800.0 4411.5 2.0 0.0 -0.1 -0.2 70 0.95 0.80 0.45 5 160.0 23.0 5.0 R P 6.50 0.50 0 30.0 1.0 0.08 0.0 0.20 -0.25 0.5 0.6 0.26 0.41 30000 0 2410008 1 1 0";
    pub(crate) const LINERUN: &str = "LINERUN 3800.0 19953.2 5.0 0.0 0.0 -0.2 90 0.95 0.65 0.4 5 120.0 25.0 30.0 R D 8.0 0.30 0 25.0 1.6 0.06 0.0 0.40 -0.20 0.5 0.0 0.65 0.25 35000 6000 200 0 1 2";
    pub(crate) const RHINO: &str = "RHINO 25000.0 250000.0 5.0 0.0 0.0 0.0 90 2.50 0.8 0.5 4 80.0 40.0 150.0 4 D 5.0 0.50 0 35.0 0.4 0.02 0.0 0.35 -0.10 0.5 0.0 0.22 0.09 110000 40008008 308840 0 1 24";
    #[test]
    fn real_rows_order_the_cars_like_the_game() {
        let [taxi, infernus, voodoo, linerun, rhino] =
            [TAXI, INFERNUS, VOODOO, LINERUN, RHINO].map(|row| from_line(row).unwrap());
        // Flat out the supercar is quickest, the truck and the tank slowest.
        assert!(infernus.acceleration > voodoo.acceleration);
        assert!(voodoo.acceleration > taxi.acceleration);
        assert!(taxi.acceleration > linerun.acceleration);
        assert!(linerun.acceleration > rhino.acceleration);
        assert!(infernus.top_speed > taxi.top_speed && taxi.top_speed > rhino.top_speed);
        assert!(infernus.brake_deceleration > taxi.brake_deceleration);
        // Heavy vehicles answer the pedal and the wheel more slowly.
        assert!(linerun.engine_lag > taxi.engine_lag && rhino.engine_lag > linerun.engine_lag);
        assert!(rhino.yaw_inertia > linerun.yaw_inertia);
        assert!(linerun.yaw_inertia > taxi.yaw_inertia);
        assert!(taxi.yaw_inertia > infernus.yaw_inertia);
        // Drive layouts come from the row.
        assert_eq!((taxi.drive, voodoo.drive, infernus.drive), (0.0, 1.0, 2.0));
        // The tank's huge traction stays inside the range a car can use.
        assert!(rhino.tire_grip <= 6.5 && rhino.tire_grip > taxi.tire_grip);
        // Cars from handling.cfg can turn a little in mid-air.
        assert!(taxi.air_control > 0.0);
        assert!(from_line("SHORT 1 2 3").is_none());
    }
    #[test]
    fn the_engine_reaches_the_row_speed_against_drag() {
        for row in [TAXI, INFERNUS, VOODOO, LINERUN, RHINO] {
            let car = from_line(row).unwrap();
            let max_kmh: f32 = row.split_whitespace().nth(12).unwrap().parse().unwrap();
            let target = TOP_SPEED_SCALE * max_kmh / 3.6;
            let pull = car.acceleration * (1.0 - (target / car.top_speed).powi(2));
            let resist = ROLLING_RESISTANCE + car.aerodynamic_drag * target * target;
            assert!((pull - resist).abs() < 0.01, "{row}: {pull} vs {resist}");
        }
    }
    #[test]
    fn the_no_handbrake_flag_disables_the_handbrake() {
        let taxi = from_line(TAXI).unwrap();
        assert_eq!(taxi.handbrake, 1.0);
        let no_handbrake = TAXI.replace(" 200000 0 1 0", " 200010 0 1 0");
        assert_eq!(from_line(&no_handbrake).unwrap().handbrake, 0.0);
    }
    #[test]
    fn finds_the_handling_id_of_a_car_model() {
        let ide = "cars\n420, taxi, taxi, car, TAXI, TAXI, null, taxi, 10, 0, 0, -1, 0.7, 0.7, 0\n\
                   522, nrg500, nrg500, bike, NRG500, NRG500, bikes, bike, 10, 0, 0, 16, 0.6, 0.6, 1\nend";
        assert_eq!(handling_id(ide, "TAXI").as_deref(), Some("TAXI"));
        assert_eq!(handling_id(ide, "nrg500"), None);
    }
}
