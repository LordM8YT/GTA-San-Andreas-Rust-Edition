//! Loads the original GTA:SA per-model handling data.
//! Raw values are retained while SI adapter values bridge to the current simulator;
//! the adapter is transitional until the original force model is fully ported.
use crate::vehicle::Handling;
use std::path::Path;

/// Reference values used only by the temporary adapter for the legacy simulator.
const BASE_BRAKE: f32 = 7.0;
const BASE_TRACTION: f32 = 0.75;

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
/// brake bias, ABS, steering lock, ...
///
/// The gear count is the car's real one, so a stock five-speed shifts through
/// five bands. Add `"gears": 1` to a resource profile to keep the single-band
/// prototype feel.
fn from_line(line: &str) -> Option<Handling> {
    let fields: Vec<_> = line.split_whitespace().collect();
    if fields.len() < 32 {
        return None;
    }
    let number = |index: usize| fields.get(index)?.parse::<f32>().ok().filter(|v| v.is_finite());
    let code = |index: usize, values: &[(&str, u8)]| {
        let value = *fields.get(index)?;
        values.iter().find_map(|(name, code)| (*name == value).then_some(*code))
    };
    let base = Handling::default();

    // Keep the original values intact. SARE's simulation is SI-based, while
    // GTA stores maxVelocity in km/h and engineAcceleration in its own scale.
    let mass = number(1)?;
    let turn_mass = number(2)?;
    let drag = number(3)?;
    let centre_of_mass_x = number(4)?;
    let centre_of_mass_y = number(5)?;
    let centre_of_mass_z = number(6)?;
    let traction_multiplier = number(8)?;
    let traction_loss = number(9)?;
    let traction_bias = number(10)?;
    let gears = number(11)?.round().clamp(1.0, 6.0);
    let max_velocity = number(12)?;
    let engine_acceleration = number(13)?;
    let engine_inertia = number(14)?;
    let drive_type = code(15, &[("F", 0), ("R", 1), ("4", 2)])?;
    let engine_type = code(16, &[("P", 0), ("D", 1), ("E", 2)])?;
    let brake_deceleration = number(17)?;
    let brake_bias = number(18)?;
    let abs = match fields.get(19)? {
        &"0" => 0.0,
        &"1" => 1.0,
        _ => return None,
    };
    let steering_lock = number(20)?;
    let suspension_force_level = number(21)?;
    let suspension_damping = number(22)?;
    let suspension_high_speed_damping = number(23)?;
    let suspension_upper_limit = number(24)?;
    let suspension_lower_limit = number(25)?;
    let suspension_front_rear_bias = number(26)?;
    let suspension_anti_dive_multiplier = number(27)?;

    // GTA's transmission uses a drag-limited maxFlat, not handling.cfg's
    // maxVelocity directly. These calculations follow the documented SA
    // ConvertDataToGameUnits relationship; convert its per-tick speed to m/s.
    let drive_divisor = if drive_type == 2 { 4.0 } else { 2.0 };
    let drive_acceleration = engine_acceleration * 0.4 / 2500.0 / drive_divisor;
    let mut max_flat = max_velocity * 0.277_778 / 50.0;
    while max_flat > 0.01
        && max_flat * max_flat * drag / 2.0 / 1000.0 > drive_acceleration / 6.0
    {
        max_flat -= 0.01;
    }
    let top_speed = (max_flat.max(0.01) * 1.2 * 50.0).clamp(5.0, 90.0);
    let acceleration = (drive_acceleration * 50.0 * 50.0).clamp(0.5, 20.0);

    let handling = Handling {
        top_speed,
        acceleration,
        reverse_acceleration: (acceleration * 0.5).clamp(0.5, 10.0),
        // These are still adapter values for the legacy vehicle integrator.
        // The raw GTA values below are authoritative inputs for the ongoing port.
        brake_deceleration: (base.brake_deceleration
            * (brake_deceleration / BASE_BRAKE).clamp(0.4, 2.0))
            .clamp(1.0, 25.0),
        tire_grip: (base.tire_grip
            * (traction_multiplier / BASE_TRACTION).clamp(0.5, 1.6))
            .clamp(1.0, 12.0),
        steering_lock: steering_lock.to_radians().clamp(0.1, 0.9),
        gears,
        suspension_spring: (base.suspension_spring * suspension_force_level).clamp(40.0, 250.0),
        suspension_damping: (base.suspension_damping * (1.0 + suspension_damping / 20.0))
            .clamp(8.0, 40.0),
        aerodynamic_drag: (drag * 0.001).clamp(0.0, 0.02),
        gta_mass: mass,
        gta_turn_mass: turn_mass,
        gta_drag_multiplier: drag,
        gta_centre_of_mass_x: centre_of_mass_x,
        gta_centre_of_mass_y: centre_of_mass_y,
        gta_centre_of_mass_z: centre_of_mass_z,
        gta_traction_loss: traction_loss,
        gta_traction_bias: traction_bias,
        gta_drive_type: f32::from(drive_type),
        gta_engine_type: f32::from(engine_type),
        gta_engine_inertia: engine_inertia,
        gta_brake_bias: brake_bias,
        gta_abs: abs,
        gta_suspension_force_level: suspension_force_level,
        gta_suspension_high_speed_damping: suspension_high_speed_damping,
        gta_suspension_upper_limit: suspension_upper_limit,
        gta_suspension_lower_limit: suspension_lower_limit,
        gta_suspension_front_rear_bias: suspension_front_rear_bias,
        gta_suspension_anti_dive_multiplier: suspension_anti_dive_multiplier,
        ..base
    };
    handling.validate().ok()?;
    Some(handling)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handling_rows_convert_units_and_preserve_original_values() {
        let taxi = "TAXI 1450.0 4056.4 2.2 0.0 0.3 -0.25 75 0.75 0.85 0.51 5 180.0 20.0 10.0 F P 7.0 0.60 0 30.0 1.4 0.1 0.0 0.28 -0.17 0.5 0.0 0.2 0.25 0.5 20000 0 200000 0 1 0";
        let infernus = "INFERNUS 1400.0 2725.3 1.5 0.0 0.0 -0.25 70 0.70 0.80 0.50 5 240.0 30.0 10.0 4 P 11.0 0.51 0 30.0 1.2 0.19 0.0 0.25 -0.10 0.5 0.4 0.37 0.72 95000 40002004 C04000 1 1 1";
        let taxi = from_line(taxi).unwrap();
        assert!((20.0..30.0).contains(&taxi.top_speed), "{}", taxi.top_speed);
        assert!((3.9..4.1).contains(&taxi.acceleration), "{}", taxi.acceleration);
        assert_eq!(taxi.gta_mass, 1450.0);
        assert_eq!(taxi.gta_turn_mass, 4056.4);
        assert_eq!(taxi.gta_centre_of_mass_x, 0.0);
        assert_eq!(taxi.gta_centre_of_mass_y, 0.3);
        assert_eq!(taxi.gta_centre_of_mass_z, -0.25);
        assert_eq!(taxi.gta_drive_type, 0);
        assert_eq!(taxi.gta_engine_type, 0);
        assert_eq!(taxi.gta_traction_loss, 0.85);
        assert_eq!(taxi.gta_traction_bias, 0.51);
        assert_eq!(taxi.gta_abs, 0.0);
        let infernus = from_line(infernus).unwrap();
        assert!(infernus.top_speed > taxi.top_speed);
        assert!((2.9..3.1).contains(&infernus.acceleration), "{}", infernus.acceleration);
        assert!(infernus.brake_deceleration > taxi.brake_deceleration);
        assert!(from_line("SHORT 1 2 3").is_none());
    }
    #[test]
    fn the_row_gear_count_reaches_the_gearbox() {
        let infernus = "INFERNUS 1400.0 2725.3 1.5 0.0 0.0 -0.25 70 0.70 0.80 0.50 5 240.0 30.0 10.0 4 P 11.0 0.51 0 30.0 1.2 0.19 0.0 0.25 -0.10 0.5 0.4 0.37 0.72 95000 40002004 C04000 1 1 1";
        let infernus = from_line(infernus).unwrap();
        assert_eq!(infernus.gears, 5.0);
        assert_eq!(infernus.gta_drive_type, 2);
        assert!(infernus.top_speed > from_line("TAXI 1450.0 4056.4 2.2 0.0 0.3 -0.25 75 0.75 0.85 0.51 5 180.0 20.0 10.0 F P 7.0 0.60 0 30.0 1.4 0.1 0.0 0.28 -0.17 0.5 0.0 0.2 0.25 0.5 20000 0 200000 0 1 0").unwrap().top_speed);
        let four = "SENTINEL 1400.0 2725.3 1.5 0.0 0.0 -0.25 70 0.70 0.8 0.50 4 240.0 30.0 10.0 R P 11.0 0.51 0 30.0 1.2 0.19 0.0 0.25 -0.10 0.5 0.4 0.37 0.72 95000 0 0 1 1 1";
        assert_eq!(from_line(four).unwrap().gears, 4.0);
        let clamped = "ODD 1400.0 2725.3 1.5 0.0 0.0 -0.25 70 0.70 0.8 0.50 9 240.0 30.0 10.0 4 P 11.0 0.51 0 30.0 1.2 0.19 0.0 0.25 -0.10 0.5 0.4 0.37 0.72 95000 0 0 1 1 1";
        assert_eq!(from_line(clamped).unwrap().gears, 6.0);
    }
    #[test]
    fn finds_the_handling_id_of_a_car_model() {
        let ide = "cars\n420, taxi, taxi, car, TAXI, TAXI, null, taxi, 10, 0, 0, -1, 0.7, 0.7, 0\n\
                   522, nrg500, nrg500, bike, NRG500, NRG500, bikes, bike, 10, 0, 0, 16, 0.6, 0.6, 1\nend";
        assert_eq!(handling_id(ide, "TAXI").as_deref(), Some("TAXI"));
        assert_eq!(handling_id(ide, "nrg500"), None);
    }
}
