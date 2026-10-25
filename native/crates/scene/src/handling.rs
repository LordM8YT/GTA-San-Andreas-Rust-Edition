//! Per-model driving feel from the player's own `data/handling.cfg`, scaled
//! onto SARE's car model so the default taxi still drives as before.
use crate::vehicle::Handling;
use std::path::Path;

/// The original taxi values SARE's defaults were tuned against.
const BASE_MASS: f32 = 1450.0;
const BASE_TOP_SPEED: f32 = 180.0;
const BASE_ACCELERATION: f32 = 20.0;
const BASE_BRAKE: f32 = 7.0;
const BASE_DRAG: f32 = 2.2;
const BASE_TRACTION: f32 = 0.75;
const BASE_TRACTION_LOSS: f32 = 0.85;

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
fn from_line(line: &str) -> Option<Handling> {
    let fields: Vec<_> = line.split_whitespace().collect();
    if fields.len() < 21 {
        return None;
    }
    let number = |index: usize| fields[index].parse::<f32>().ok().filter(|v| v.is_finite());
    let base = Handling::default();
    let ratio =
        |value: f32, reference: f32, low: f32, high: f32| (value / reference).clamp(low, high);
    let mass = number(1)?.max(1.0);
    let handling = Handling {
        top_speed: base.top_speed * ratio(number(12)?, BASE_TOP_SPEED, 0.4, 1.8),
        // GTA:SA's engine force is affected by vehicle mass as well as the
        // model's engine acceleration. Keep the taxi baseline unchanged.
        acceleration: base.acceleration
            * ratio(number(13)?, BASE_ACCELERATION, 0.3, 2.5)
            * ratio(BASE_MASS, mass, 0.7, 1.4),
        brake_deceleration: base.brake_deceleration * ratio(number(17)?, BASE_BRAKE, 0.4, 2.0),
        // Both traction multiplier and traction loss contribute to the grip
        // envelope; the taxi reference preserves SARE's current baseline.
        tire_grip: base.tire_grip
            * ratio(number(8)?, BASE_TRACTION, 0.5, 1.6)
            * ratio(number(9)?, BASE_TRACTION_LOSS, 0.6, 1.4),
        aerodynamic_drag: base.aerodynamic_drag * ratio(number(3)?, BASE_DRAG, 0.4, 2.5),
        steering_lock: number(20)?.to_radians().clamp(0.3, 0.9),
        ..base
    };
    handling.validate().ok()?;
    Some(handling)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn faster_cars_get_more_speed_and_taxi_matches_defaults() {
        let taxi = "TAXI 1450.0 4056.4 2.2 0.0 0.3 -0.25 75 0.75 0.85 0.51 5 180.0 20.0 10.0 F P 7.0 0.60 0 30.0 1.4 0.1 0.0 0.28 -0.17 0.5 0.0 0.2 0.25 0.5 20000 0 200000 0 1 0";
        let infernus = "INFERNUS 1400.0 2725.3 1.5 0.0 0.0 -0.25 70 0.70 0.80 0.50 5 240.0 30.0 10.0 4 P 11.0 0.51 0 30.0 1.2 0.19 0.0 0.25 -0.10 0.5 0.4 0.37 0.72 95000 40002004 C04000 1 1 1";
        let taxi = from_line(taxi).unwrap();
        let base = Handling::default();
        assert_eq!(taxi.top_speed, base.top_speed);
        assert_eq!(taxi.acceleration, base.acceleration);
        assert_eq!(taxi.tire_grip, base.tire_grip);
        let infernus = from_line(infernus).unwrap();
        assert!(infernus.top_speed > taxi.top_speed);
        assert!(infernus.acceleration > taxi.acceleration);
        assert!(infernus.brake_deceleration > taxi.brake_deceleration);
        assert!(infernus.aerodynamic_drag < taxi.aerodynamic_drag);
        assert!(infernus.tire_grip < taxi.tire_grip);
        assert!(from_line("SHORT 1 2 3").is_none());
    }
    #[test]
    fn finds_the_handling_id_of_a_car_model() {
        let ide = "cars\n420, taxi, taxi, car, TAXI, TAXI, null, taxi, 10, 0, 0, -1, 0.7, 0.7, 0\n\
                   522, nrg500, nrg500, bike, NRG500, NRG500, bikes, bike, 10, 0, 0, 16, 0.6, 0.6, 1\nend";
        assert_eq!(handling_id(ide, "TAXI").as_deref(), Some("TAXI"));
        assert_eq!(handling_id(ide, "nrg500"), None);
    }
}
