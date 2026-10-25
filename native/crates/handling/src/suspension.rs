//! `CAutomobile::ProcessSuspension` (0x6AFB10) — two passes over four springs.
//!
//! Pass 1 applies a **spring** force along the contact normal, proportional to
//! how far the suspension line still has to travel:
//!
//! ```text
//! limit = (1 - compression) * min(step, 3) * mass * forceLevel * bias * 0.016
//! F     = normal * limit
//! ```
//!
//! Pass 2 applies **damping** along that normal, using the contact point's
//! speed *after* the springs were applied, and it is clamped twice: the timestep
//! term to ±0.25, and the resulting force to 99.9% of the spring force from
//! pass 1. The second clamp is what stops a hard landing from injecting energy.
//!
//! Details that are easy to lose and that visibly change the car:
//!
//! * `bias` is `suspensionBias` for the front axle and `1 - suspensionBias` for
//!   the rear, so a value of 0.5 is neutral and 0.6 stiffens the front.
//! * The damping direction is *not* always the contact normal: if the normal's
//!   z is above 0.35 the code uses `-normal`, otherwise straight `down`. This is
//!   the difference between a car that settles on a slope and one that skates.
//! * A wheel whose compression is `>= 1.0` is skipped by the springs but keeps
//!   its four-tick logical contact count, so `ProcessWheel` may still run on it.
//! * The wheel contact points used for the contact-speed array are rebuilt after
//!   the spring pass, from the suspension-line end for uncompressed wheels and
//!   from the actual hit for compressed ones.
//!
//! The roll-on-to-wheels branch at the end handles a car resting on its side:
//! if both wheels of one side are fully extended and the other side is loaded
//! while the car is nearly stopped, GTA pushes it back over.

use crate::contact::WheelContactSet;
use crate::handling::{Handling, FRONT_WHEELS};
use crate::math::{clamp, Basis, Vec3, ZERO};
use crate::physical::Body;

/// `CPhysical` damping clamps, from the source.
pub const GTA_DAMPING_LIMIT_IN_FRAME: f32 = 0.25;
pub const GTA_DAMPING_LIMIT_OF_SPRING_FORCE: f32 = 0.999;

#[derive(Clone, Debug, Default)]
pub struct WheelSpringState {
    pub point: Vec3,
    pub normal: Vec3,
    pub compression: f32,
    pub spring_limit: f32,
    pub spring_force: Vec3,
    pub damping_speed: f32,
    pub damping_force: f32,
}

#[derive(Clone, Debug, Default)]
pub struct SuspensionTrace {
    pub wheels: [WheelSpringState; 4],
    /// Contact speeds for `ProcessWheel`, rebuilt after the spring pass.
    pub contact_speeds: [Vec3; 4],
    /// Contact points after the pass (model-relative).
    pub wheel_contact_points: [Vec3; 4],
    pub normals: [Vec3; 4],
    pub points: [Vec3; 4],
    pub roll_on_to_wheels: bool,
}

/// Run both spring passes.
///
/// * `compressions` — normalised 0..1 compression per wheel (see
///   [`Handling::normalized_compression`]); `1.0` means "not touching".
/// * `previous_normals` — last tick's normals, used to keep contact normals
///   from flipping; updated in place through the returned trace.
pub fn apply_suspension(
    body: &mut Body,
    handling: &Handling,
    contacts: &WheelContactSet,
    compressions: &[f32; 4],
    ctimer_step: f32,
) -> SuspensionTrace {
    let mut trace = SuspensionTrace::default();
    let mut normals = [Vec3::new(0.0, 0.0, 1.0); 4];
    let mut points = [ZERO; 4];
    let down = -body.up();

    // --- locality pass: which wheels have a usable point this tick ----------
    for index in 0..4 {
        if let Some(contact) = contacts.get(index) {
            normals[index] = contact.normal;
            // ProcessSuspension deliberately uses a zero point for a wheel whose
            // current compression is >= 1, even though its persistence counter
            // keeps it alive for the wheel pass.
            if compressions[index] < 1.0 {
                points[index] = contact.point;
            }
        }
    }

    // --- pass 1: springs ----------------------------------------------------
    for index in 0..4 {
        let compression = compressions[index];
        let Some(contact) = contacts.get(index) else {
            continue;
        };
        if compression >= 1.0 {
            continue;
        }
        let mut normal = normals[index];
        // Keep the normal pointing at the car.
        if down.dot(normal) > 0.0 {
            normal = -normal;
            normals[index] = normal;
        }
        let bias = if FRONT_WHEELS.contains(&index) {
            handling.suspension_bias
        } else {
            1.0 - handling.suspension_bias
        };
        let timestep = ctimer_step.min(3.0);
        let limit = (1.0 - compression)
            * (timestep * handling.mass)
            * handling.suspension_force_level
            * bias
            * 0.016;
        let force = normal * limit;
        trace.wheels[index].point = points[index];
        trace.wheels[index].normal = normal;
        trace.wheels[index].compression = compression;
        trace.wheels[index].spring_limit = limit;
        trace.wheels[index].spring_force = force;
        let _ = contact;
        body.apply_force(force, points[index], handling);
    }

    // Post-spring point speeds: damping uses these, so springs and dampers of
    // the same tick see each other.
    let mut post_spring = [ZERO; 4];
    for index in 0..4 {
        post_spring[index] = body.get_speed(points[index], handling);
    }

    // --- pass 2: dampers ----------------------------------------------------
    for index in 0..4 {
        let compression = compressions[index];
        let Some(contact) = contacts.get(index) else {
            continue;
        };
        if compression >= 1.0 {
            continue;
        }
        let direction = if normals[index].z > 0.35 {
            -normals[index]
        } else {
            down
        };
        let post_spring_relative = post_spring[index] - contact.relative_velocity;
        let collision_position_speed = post_spring_relative.dot(direction);
        // ApplySpringDampening recomputes GetSpeed(point) internally; unlike the
        // argument above, that internal speed is not made relative to a moving
        // collision entity in the source. Keep the asymmetry.
        let point_speed = body.get_speed(points[index], handling);
        let collision_point_speed = point_speed.dot(direction);

        let mut damping_timestep =
            ctimer_step.min(3.0) * handling.suspension_damping_level;
        damping_timestep = clamp(
            damping_timestep,
            -GTA_DAMPING_LIMIT_IN_FRAME,
            GTA_DAMPING_LIMIT_IN_FRAME,
        );
        let mut damping_speed = -(damping_timestep * collision_position_speed);
        // Never let damping *add* speed along the normal.
        if damping_speed > 0.0 && damping_speed + collision_point_speed > 0.0 {
            damping_speed = if collision_point_speed >= 0.0 {
                0.0
            } else {
                -collision_point_speed
            };
        } else if damping_speed < 0.0 && damping_speed + collision_point_speed < 0.0 {
            damping_speed = if collision_point_speed <= 0.0 {
                0.0
            } else {
                -collision_point_speed
            };
        }

        let effective_mass = body.contact_effective_mass(points[index], direction, handling);
        let mut damping_force = effective_mass * damping_speed;
        let force_limit = trace.wheels[index].spring_limit.abs() * GTA_DAMPING_LIMIT_OF_SPRING_FORCE;
        if damping_force > force_limit {
            damping_force = force_limit;
        }
        trace.wheels[index].damping_speed = damping_speed;
        trace.wheels[index].damping_force = damping_force;
        body.apply_force(direction * damping_force, points[index], handling);
    }

    // --- rebuild wheel contact points and the contact-speed array -----------
    let definitions = [
        (-handling.wheel_track / 2.0, handling.wheel_front_y),
        (-handling.wheel_track / 2.0, handling.wheel_rear_y),
        (handling.wheel_track / 2.0, handling.wheel_front_y),
        (handling.wheel_track / 2.0, handling.wheel_rear_y),
    ];
    for index in 0..4 {
        let (x, y) = definitions[index];
        if compressions[index] >= 1.0 {
            // Uncompressed: use the suspension line end.
            let (_start, end) = handling.suspension_line(x, y);
            trace.wheel_contact_points[index] = body.basis.transform_local(end);
        } else if let Some(contact) = contacts.get(index) {
            trace.wheel_contact_points[index] = contact.point;
        }
    }
    for index in 0..4 {
        trace.contact_speeds[index] = body.get_speed(trace.wheel_contact_points[index], handling);
    }

    // --- roll-on-to-wheels --------------------------------------------------
    let forward_speed = body.forward_speed().abs();
    if forward_speed < 0.02 {
        let mut roll_direction = 0.0f32;
        if compressions[0] == 1.0
            && compressions[1] == 1.0
            && (compressions[2] < 1.0 || compressions[3] < 1.0)
        {
            roll_direction = 1.0;
        } else if compressions[2] == 1.0
            && compressions[3] == 1.0
            && (compressions[0] < 1.0 || compressions[1] < 1.0)
        {
            roll_direction = -1.0;
        }
        if roll_direction != 0.0 {
            // Deliberately unnormalised: the source keeps the horizontal
            // projection length, and both the alignment gate and the lateral
            // force use it.
            let roll_right = body.forward().cross(crate::math::GTA_UP);
            let alignment = roll_right.dot(body.right()).abs();
            if alignment < 0.6 {
                trace.roll_on_to_wheels = true;
                let force_magnitude = 0.0025 * handling.turn_mass;
                body.apply_turn_force(
                    body.up() * (roll_direction * force_magnitude),
                    body.right() * handling.collision_bound_max_x,
                    handling,
                );
                body.apply_move_force(
                    roll_right * (-roll_direction * 0.0025 * handling.mass),
                    handling,
                );
                for index in 0..4 {
                    trace.contact_speeds[index] =
                        body.get_speed(trace.wheel_contact_points[index], handling);
                }
            }
        }
    }

    trace.normals = normals;
    trace.points = points;
    trace
}

/// `m_WheelCounts` persistence: a wheel that touched ground keeps a logical
/// contact for four ticks at 50 Hz (eight at 100 Hz), which is why a car does
/// not lose grip the instant a tyre skips over a seam.
pub const WHEELS_COUNT_PERSIST_TICKS_50HZ: f32 = 4.0;

pub fn advance_wheel_counts(counts: &mut [f32; 4], hits: [bool; 4], ctimer_step: f32) {
    for i in 0..4 {
        if hits[i] {
            counts[i] = WHEELS_COUNT_PERSIST_TICKS_50HZ;
        } else {
            counts[i] = (counts[i] - ctimer_step).max(0.0);
        }
    }
}

/// `GetBasis` helper for tests: orthonormal basis from a yaw/pitch/roll triple.
pub fn basis_from_euler(pitch: f32, roll: f32, yaw: f32) -> Basis {
    let (sp, cp) = pitch.sin_cos();
    let (sr, cr) = roll.sin_cos();
    let (sy, cy) = yaw.sin_cos();
    let forward = Vec3::new(cy * cp, sy * cp, sp);
    let right = Vec3::new(cy * sr * sp + sy * cr, sy * sr * sp - cy * cr, -sr * cp);
    let up = Vec3::new(-(cy * cr * sp) + sy * sr, -(sy * cr * sp) - cy * sr, cr * cp);
    Basis::new(right, forward, up).reorthogonalise()
}
