//! Xbox-style gamepad state and analog stick handling.
use gilrs::{Axis, Button, Gamepad};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Input {
    pub move_x: f32,
    pub move_y: f32,
    pub look_x: f32,
    pub look_y: f32,
    pub throttle: f32,
    pub jump: bool,
    pub sprint: bool,
    pub handbrake: bool,
    pub menu_up: bool,
    pub menu_down: bool,
    pub menu_left: bool,
    pub menu_right: bool,
    pub menu_accept: bool,
    pub menu_back: bool,
    pub pause: bool,
    pub map: bool,
    pub enter_exit: bool,
}

const STICK_DEADZONE: f32 = 0.16;
const TRIGGER_DEADZONE: f32 = 0.08;

fn stick_axis(value: f32) -> f32 {
    let value = value.clamp(-1.0, 1.0);
    if value.abs() <= STICK_DEADZONE {
        0.0
    } else {
        value.signum() * (value.abs() - STICK_DEADZONE) / (1.0 - STICK_DEADZONE)
    }
}

fn normalize_trigger(value: f32) -> f32 {
    let value = value.clamp(0.0, 1.0);
    if value <= TRIGGER_DEADZONE {
        0.0
    } else {
        (value - TRIGGER_DEADZONE) / (1.0 - TRIGGER_DEADZONE)
    }
}

fn trigger_value(gamepad: &Gamepad, button: Button) -> f32 {
    let value = gamepad.button_data(button).map_or(0.0, |data| data.value());
    normalize_trigger(value)
}
fn movement_axes(x: f32, y: f32) -> (f32, f32) {
    (stick_axis(x), stick_axis(y))
}

pub fn read(gamepad: &Gamepad) -> Input {
    let axis = |axis| gamepad.axis_data(axis).map_or(0.0, |data| data.value());
    let (move_x, move_y) = movement_axes(axis(Axis::LeftStickX), axis(Axis::LeftStickY));
    Input {
        move_x,
        move_y,
        look_x: stick_axis(axis(Axis::RightStickX)),
        look_y: -stick_axis(axis(Axis::RightStickY)),
        throttle: trigger_value(gamepad, Button::RightTrigger2)
            - trigger_value(gamepad, Button::LeftTrigger2),
        jump: gamepad.is_pressed(Button::South),
        sprint: gamepad.is_pressed(Button::West),
        handbrake: gamepad.is_pressed(Button::LeftTrigger),
        menu_up: gamepad.is_pressed(Button::DPadUp),
        menu_down: gamepad.is_pressed(Button::DPadDown),
        menu_left: gamepad.is_pressed(Button::DPadLeft),
        menu_right: gamepad.is_pressed(Button::DPadRight),
        menu_accept: gamepad.is_pressed(Button::South),
        menu_back: gamepad.is_pressed(Button::East),
        pause: gamepad.is_pressed(Button::Start),
        map: gamepad.is_pressed(Button::Select),
        enter_exit: gamepad.is_pressed(Button::North),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn left_stick_directions_match_keyboard_movement() {
        // gilrs uses positive Y for up; runtime uses positive Y input for W.
        assert_eq!(movement_axes(0.0, 1.0), (0.0, 1.0));
        assert_eq!(movement_axes(0.0, -1.0), (0.0, -1.0));
        assert_eq!(movement_axes(1.0, 0.0), (1.0, 0.0));
        assert_eq!(movement_axes(-1.0, 0.0), (-1.0, 0.0));
        assert_eq!(movement_axes(0.1, -0.1), (0.0, 0.0));
    }

    #[test]
    fn trigger_deadzone_suppresses_drift_and_rescales_travel() {
        assert_eq!(normalize_trigger(0.0), 0.0);
        assert_eq!(normalize_trigger(0.07), 0.0);
        assert!((normalize_trigger(1.0) - 1.0).abs() < f32::EPSILON);
        assert!(normalize_trigger(0.5) > 0.0 && normalize_trigger(0.5) < 0.5);
    }

    #[test]
    fn stick_deadzone_suppresses_drift_and_rescales_motion() {
        assert_eq!(stick_axis(0.0), 0.0);
        assert_eq!(stick_axis(0.15), 0.0);
        assert_eq!(stick_axis(-0.16), 0.0);
        assert!((stick_axis(1.0) - 1.0).abs() < f32::EPSILON);
        assert!((stick_axis(-1.0) + 1.0).abs() < f32::EPSILON);
        assert!(stick_axis(0.5) > 0.0 && stick_axis(0.5) < 0.5);
    }
}
