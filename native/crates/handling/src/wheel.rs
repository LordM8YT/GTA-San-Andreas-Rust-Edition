//! `CVehicle::ProcessWheel` (0x6D6C00) — one tyre, per tick.
//!
//! This function is the whole grip model. It receives the wheel's contact
//! point, its local forward/right axes, the per-tick thrust and brake the
//! gearbox and pedals produced, and an `adhesion` budget, and returns a velocity
//! correction along those axes. It is deliberately *not* a spring: it decides a
//! target correction, clips it against adhesion, and applies it as a mass-scaled
//! force so that `ApplyMoveForce`'s division by mass cancels out.
//!
//! The four wheel states matter because they latch:
//!
//! * `Normal`
//! * `Spinning` — driven wheel exceeding the traction limit.
//! * `Skidding` — sliding but not driven.
//! * `Fixed` — locked/wedged (brake larger than the available adhesion).
//!
//! Once a wheel enters any non-`Normal` state the process-global
//! `bAlreadySkidding` latch is set, which raises the effective traction-loss
//! multiplier to 1.0 (i.e. removes the loss) for this and later wheels in the
//! same tick, and negative traction temperature effects stop applying. Cars that
//! "stick again after a slide" but never "dig in" are usually missing the latch
//! entirely or clearing it every tick.

use crate::handling::{Handling, FRONT_WHEELS};
use crate::math::{clamp, Vec3};
use crate::physical::Body;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WheelState {
    #[default]
    Normal,
    Spinning,
    Skidding,
    Fixed,
}

impl WheelState {
    pub fn as_str(self) -> &'static str {
        match self {
            WheelState::Normal => "normal",
            WheelState::Spinning => "spinning",
            WheelState::Skidding => "skidding",
            WheelState::Fixed => "fixed",
        }
    }
}

/// Per-wheel local frame: contact forward/right after steering.
pub fn wheel_basis(forward: Vec3, normal: Vec3, steer_angle: f32) -> (Vec3, Vec3) {
    let mut wheel_fwd = (forward - normal * forward.dot(normal)).normalized(forward);
    let mut wheel_right = wheel_fwd.cross(normal).normalized(Vec3::new(1.0, 0.0, 0.0));
    if steer_angle.abs() > 1e-12 {
        let (sin, cos) = steer_angle.sin_cos();
        let base_fwd = wheel_fwd;
        let base_right = wheel_right;
        wheel_fwd = base_fwd * cos - base_right * sin;
        wheel_right = base_fwd * sin + base_right * cos;
    }
    (wheel_fwd, wheel_right)
}

pub struct ProcessWheelArgs {
    pub contact_speed: Vec3,
    pub wheel_forward: Vec3,
    pub wheel_right: Vec3,
    pub wheels_on_ground: i32,
    pub thrust: f32,
    pub brake: f32,
    pub adhesion: f32,
    pub gas_pedal: f32,
    pub index: usize,
    pub ctimer_step: f32,
}

#[derive(Clone, Debug, Default)]
pub struct ProcessWheelResult {
    pub index: usize,
    pub state_before: WheelState,
    pub state_after: WheelState,
    pub already_skidding_before: bool,
    pub already_skidding_after: bool,
    pub adhesion: f32,
    pub longitudinal: f32,
    pub lateral: f32,
    pub total: Vec3,
    pub linear_delta: Vec3,
    pub angular_delta: Vec3,
}

/// Port of `CVehicle::ProcessWheel`.
///
/// `point` is the model-relative contact point and is passed to
/// `wheel_effective_mass` unchanged — see the note in [`crate::physical`].
pub fn process_wheel(
    body: &mut Body,
    handling: &Handling,
    point: Vec3,
    state: &mut WheelState,
    already_skidding: &mut bool,
    args: &ProcessWheelArgs,
) -> ProcessWheelResult {
    let mut result = ProcessWheelResult {
        index: args.index,
        state_before: *state,
        already_skidding_before: *already_skidding,
        ..Default::default()
    };

    let contact_speed_fwd = args.wheel_forward.dot(args.contact_speed);
    let contact_speed_right = args.wheel_right.dot(args.contact_speed);
    let braking = args.brake != 0.0;
    let driving = !braking && args.thrust != 0.0;

    // Vehicle.cpp multiplies adhesion by the raw CTimer step. Unlike the
    // suspension and damping helpers, this line has no 3.0 clamp; keep the
    // distinction.
    let mut adhesion = args.adhesion * args.ctimer_step;

    if *state != WheelState::Normal {
        *already_skidding = true;
        adhesion *= handling.traction_loss;
        if *state == WheelState::Spinning {
            adhesion *= 1.0 - args.gas_pedal.abs() * 0.2;
        }
    }

    let ground = args.wheels_on_ground.max(1) as f32;
    let mut state_value = WheelState::Normal;
    let mut right = -contact_speed_right / ground;
    let mut fwd;
    if driving {
        fwd = args.thrust;
        right = clamp(right, -adhesion, adhesion);
    } else if contact_speed_fwd != 0.0 {
        fwd = -contact_speed_fwd / ground;
        let mut local_brake = args.brake;
        // Rolling resistance when coasting with no pedal: the source converts
        // wheelFriction/mass into a brake-like limit.
        if !braking && args.gas_pedal.abs() < 0.01 {
            local_brake = handling.wheel_friction / handling.mass.max(1e-12);
        }
        if local_brake > adhesion {
            if contact_speed_fwd.abs() > 0.005 {
                state_value = WheelState::Fixed;
            }
        } else {
            fwd = clamp(fwd, -local_brake, local_brake);
        }
    } else {
        fwd = 0.0;
    }

    let speed_sq = right * right + fwd * fwd;
    if speed_sq > adhesion * adhesion {
        if state_value != WheelState::Fixed {
            let mut traction_limit = 0.3;
            // The front tyres get twice the slip allowance once moving
            // forwards; this is what makes understeer feel like GTA's.
            if contact_speed_fwd > 0.15 && FRONT_WHEELS.contains(&args.index) {
                traction_limit *= 2.0;
            }
            state_value = if driving && traction_limit * adhesion < fwd.abs() {
                WheelState::Spinning
            } else {
                WheelState::Skidding
            };
        }
        let mut traction_loss = if *already_skidding {
            1.0
        } else {
            handling.traction_loss
        };
        if !*already_skidding && state_value == WheelState::Spinning {
            traction_loss *= 1.0 - args.gas_pedal.abs() * 0.2;
        }
        let length = speed_sq.sqrt();
        if length > 0.0 {
            fwd *= adhesion * traction_loss / length;
            right *= adhesion * traction_loss / length;
        }
    }

    let total = args.wheel_forward * fwd + args.wheel_right * right;
    let mut angular_total = total;
    let mut separate_turn = false;
    // Anti-dive: the braking/driving part of the tyre force is applied at the
    // contact point with a *reduced*, differently-directed moment, which is how
    // GTA stops cars from nose-diving under hard braking.
    if handling.suspension_anti_dive > 0.0 {
        if braking {
            separate_turn = true;
            angular_total =
                angular_total - args.wheel_forward * (handling.suspension_anti_dive * fwd);
        } else if driving {
            separate_turn = true;
            angular_total =
                angular_total - args.wheel_forward * (0.5 * handling.suspension_anti_dive * fwd);
        }
    }

    let speed = total.length();
    let turn_speed = if separate_turn {
        angular_total.length()
    } else {
        speed
    };
    let mut linear_delta = Vec3::default();
    let mut angular_delta = Vec3::default();
    if speed > 0.0 {
        let direction = total * (1.0 / speed);
        let turn_direction = if separate_turn {
            angular_total.normalized(direction)
        } else {
            direction
        };
        // Multiply by mass and let ApplyMoveForce divide it back out: the
        // resulting velocity delta is exactly `speed` along `direction`.
        linear_delta = body.apply_move_force(direction * (speed * handling.mass), handling);
        let turn_force = turn_speed * body.wheel_effective_mass(point, turn_direction, handling);
        angular_delta = body.apply_turn_force(turn_direction * turn_force, point, handling);
    }

    *state = state_value;
    result.state_after = state_value;
    result.already_skidding_after = *already_skidding;
    result.adhesion = adhesion;
    result.longitudinal = fwd;
    result.lateral = right;
    result.total = total;
    result.linear_delta = linear_delta;
    result.angular_delta = angular_delta;
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::GTA_UP;

    #[test]
    fn wheel_basis_is_perpendicular_to_normal() {
        let (fwd, right) = wheel_basis(Vec3::new(0.0, 1.0, 0.0), GTA_UP, 0.0);
        assert!(fwd.dot(GTA_UP).abs() < 1e-6);
        assert!(right.dot(GTA_UP).abs() < 1e-6);
        assert!((fwd.length() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn thrust_becomes_velocity_delta_of_that_size() {
        let h = Handling::default();
        let mut b = Body::default();
        let mut st = WheelState::Normal;
        let mut skid = false;
        let args = ProcessWheelArgs {
            contact_speed: Vec3::default(),
            wheel_forward: Vec3::new(0.0, 1.0, 0.0),
            wheel_right: Vec3::new(1.0, 0.0, 0.0),
            wheels_on_ground: 4,
            thrust: 0.001,
            brake: 0.0,
            adhesion: 1.0,
            gas_pedal: 1.0,
            index: 1,
            ctimer_step: 0.5,
        };
        let r = process_wheel(
            &mut b,
            &h,
            Vec3::new(0.0, -1.7, -0.25),
            &mut st,
            &mut skid,
            &args,
        );
        assert!((r.linear_delta.length() - r.total.length()).abs() < 1e-6);
        assert!(b.move_speed.y > 0.0);
    }

    #[test]
    fn coasting_with_no_pedal_is_damped_by_rolling_resistance() {
        let h = Handling::default();
        let mut b = Body {
            move_speed: Vec3::new(0.0, 0.5, 0.0),
            ..Body::default()
        };
        let mut st = WheelState::Normal;
        let mut skid = false;
        let args = ProcessWheelArgs {
            contact_speed: Vec3::new(0.0, 0.5, 0.0),
            wheel_forward: Vec3::new(0.0, 1.0, 0.0),
            wheel_right: Vec3::new(1.0, 0.0, 0.0),
            wheels_on_ground: 4,
            thrust: 0.0,
            brake: 0.0,
            adhesion: 1.0,
            gas_pedal: 0.0,
            index: 0,
            ctimer_step: 0.5,
        };
        let r = process_wheel(&mut b, &h, Vec3::default(), &mut st, &mut skid, &args);
        assert!(r.linear_delta.y < 0.0, "coasting should slow the car");
    }
}
