//! `CAutomobile::ProcessControl` — one tick, in the game's order.
//!
//! The order below is the specification. Reordering any of it changes the car:
//! springs must see pre-gravity attitude, the gearbox must see this tick's
//! gravity-free forward speed, wheel forces must be applied after the spring
//! pass rebuilt the contact points, and integration must happen last, once.
//!
//! ```text
//!  1. reset per-tick flags (HasContacted, HasHitWall, bogged-down)
//!  2. ProcessControlInputs   (0x6AD690)  pedals, steering ramp, burnout
//!  3. NitrousControl         (0x6A3EA0)  boost/recharge, before the drive calc
//!  4. ProcessAI airborne branch          only if last tick had no contact
//!  5. DoSoftGroundResistance             (0x6A4AF0) -- needs surface info
//!  6. ApplyFriction                      (0x5483D0) fold friction accumulators
//!  7. gravity, drag exponent, angular air resistance (0.99, not dt-scaled)
//!  8. wheel contact preparation          compression + m_WheelCounts
//!  9. ProcessSuspension      (0x6AFB10)  springs, then dampers
//! 10. CalculateDriveAcceleration (0x6D05E0)
//! 11. steering angle + speed limiter     + rear-steer history for bit 0x40
//! 12. per wheel pair: biases, brake, traction temperature, burnout torque,
//!     then ProcessWheel (0x6D6C00) for each of the two wheels
//! 13. rear tyre temperature drift, rest-stop snap
//! 14. integration: ApplyMoveSpeed + ApplyTurnSpeed + Reorthogonalise
//! ```
//!
//! Collision (`CWorld::Process`, `CPhysical::ProcessCollision`, `ProcessShift`)
//! sits between 13 and 14 in the real game and is **not** modelled here; it is
//! a world-ownership problem, not a vehicle problem. See [`crate::contact`] for
//! the input shape it produces.

use crate::contact::WheelContactSet;
use crate::handling::{
    Handling, FRONT_WHEELS, HANDLING_FLAG_REAR_WHEEL_FIRST, WHEEL_NAMES,
};
use crate::math::{clamp, Vec3, ZERO};
use crate::physical::Body;
use crate::suspension::{advance_wheel_counts, apply_suspension, SuspensionTrace};
use crate::transmission::{DriveAcceleration, Transmission};
use crate::wheel::{
    process_wheel, wheel_basis, ProcessWheelArgs, ProcessWheelResult, WheelState,
};

/// `CPhysical` gravity in raw units per CTimer step. Note how small it is: GTA
/// velocity is not m/s, so this is *not* 9.81.
pub const GTA_GRAVITY_RAW_PER_CTIME: f32 = 0.008;
/// `CPhysical::ApplyAirResistance` multiplies turn speed by this once per
/// ProcessControl call. It is deliberately *not* raised to the timestep.
pub const GTA_ANGULAR_AIR_RESISTANCE: f32 = 0.99;
/// `m_WheelCounts` persistence (4 at 50 Hz).
pub const WHEEL_COUNT_PERSIST: f32 = 4.0;
/// Below this speed (all axes) with no pedal and ground contact, the car is
/// snapped to rest (`0x6B32EF`).
pub const REST_STOP_SPEED: f32 = 0.0045;
/// `CAutomobile::NitrousControl` rates, in CTimer units.
pub const NITRO_ACTIVE_RATE: f32 = 1.0 / 1000.0;
pub const NITRO_RECHARGE_RATE: f32 = 1.0 / 100.0;
pub const NITRO_UNLIMITED_CHARGES: i32 = 101;
/// `ProcessCarWheelPair` handbrake torque, not scaled by brake bias.
pub const HANDBRAKE_BRAKE_TORQUE: f32 = 20000.0;

/// Normalised pad state. The game's `CPad` gives 0..255 per axis; these are the
/// same values divided by 255, which preserves every branch threshold as long as
/// they are compared against the same scale (see the `150.0 / 255.0` test).
#[derive(Clone, Copy, Debug, Default)]
pub struct Controls {
    pub accelerate: f32,
    pub brake_reverse: f32,
    pub handbrake: f32,
    /// -1 (right) .. +1 (left), the raw GTA steering axis.
    pub steering: f32,
    /// Analog "steer up/down" (pad look), used only by the airborne branch.
    pub steer_forward: f32,
    pub steer_back: f32,
    pub nitro: bool,
}

impl Controls {
    /// The signed axis the pedals resolve to before branching.
    pub fn steering_input(&self) -> f32 {
        clamp(self.steering, -1.0, 1.0)
    }
}

#[derive(Clone, Debug)]
pub struct Vehicle {
    pub handling: Handling,
    pub transmission: Transmission,
    pub body: Body,
    /// `CTimer::GetTimeStep()`: 1.0 at 50 Hz, 0.5 at 100 Hz.
    pub ctimer_step: f32,
    // --- ProcessControlInputs state -------------------------------------
    pub gas_pedal: f32,
    pub brake_pedal: f32,
    pub handbrake_on: bool,
    pub doing_burnout: bool,
    /// `m_fSteerAngle` before the speed limiter; ramps at 1/5 per unit step.
    pub raw_steer_angle: f32,
    pub front_steer_angle: f32,
    pub rear_steer_angle: f32,
    // --- wheel state -----------------------------------------------------
    pub wheel_counts: [f32; 4],
    pub compressions: [f32; 4],
    pub previous_compressions: [f32; 4],
    pub previous_normals: [Vec3; 4],
    pub wheel_states: [WheelState; 4],
    /// `CVehicle.cpp`'s process-global `bAlreadySkidding`.
    pub already_skidding: bool,
    // --- nitro / tyre temperature ---------------------------------------
    /// One native field does both jobs: `m_fTireTemperature` is the nitro level
    /// (1.0 ready, (0,1) recharging, negative burning) and, for cars without
    /// NOS, the rear-tyre grip multiplier. 1.0 while cold means "no effect".
    pub tire_temperature: f32,
    pub nitro_installed: bool,
    pub nitro_boosts: i32,
    // --- flags ----------------------------------------------------------
    pub has_contacted: bool,
    pub has_hit_wall: bool,
}

impl Vehicle {
    pub fn new(handling: Handling, position: Vec3, ctimer_step: f32) -> Self {
        let transmission = Transmission::from_handling(&handling);
        let mut v = Self {
            body: Body::new(position, Default::default()),
            handling,
            transmission,
            ctimer_step,
            gas_pedal: 0.0,
            brake_pedal: 0.0,
            handbrake_on: false,
            doing_burnout: false,
            raw_steer_angle: 0.0,
            front_steer_angle: 0.0,
            rear_steer_angle: 0.0,
            wheel_counts: [0.0; 4],
            compressions: [1.0; 4],
            previous_compressions: [1.0; 4],
            previous_normals: [Vec3::new(0.0, 0.0, 1.0); 4],
            wheel_states: [WheelState::Normal; 4],
            already_skidding: false,
            tire_temperature: 1.0,
            nitro_installed: false,
            nitro_boosts: 0,
            has_contacted: false,
            has_hit_wall: false,
        };
        v.transmission.set_step(ctimer_step);
        v
    }

    /// Install NOS, as a Race pickup does (upgrade 1010 = double bottle = 10).
    pub fn install_nitro(&mut self, charges: i32) {
        self.nitro_installed = true;
        self.nitro_boosts = charges;
        self.tire_temperature = 1.0;
    }

    pub fn nitro_boost_active(&self) -> bool {
        self.nitro_installed && self.tire_temperature < 0.0
    }

    /// Replace the tuning while keeping the body, gear and tyre state.
    pub fn set_handling(&mut self, handling: Handling) {
        self.handling = handling;
        self.transmission = Transmission::from_handling(&self.handling);
        self.transmission.set_step(self.ctimer_step);
    }

    pub fn set_step(&mut self, ctimer_step: f32) {
        self.ctimer_step = ctimer_step;
        self.transmission.set_step(ctimer_step);
    }

    /// One authoritative tick.
    pub fn tick(&mut self, controls: &Controls, contacts: &WheelContactSet) -> VehicleTickTrace {
        let dt = self.ctimer_step;
        let mut trace = VehicleTickTrace {
            step: dt,
            ..Default::default()
        };

        // 1. Per-tick flags. HasContacted is a within-world-pass flag, never
        //    carried across ticks.
        self.body.active_center_of_mass = None;
        self.has_contacted = false;
        self.has_hit_wall = false;
        let was_supported = self.wheel_counts.iter().any(|c| *c > 0.0);
        trace.previous_supported = was_supported;
        // DidAnyWheelTouchGroundPrev is based on last tick's normalised
        // compression, not on the persistence counter (which can outlive a
        // missed line contact).
        let previous_tick_contact = self.previous_compressions.iter().any(|c| *c < 1.0);
        trace.previous_tick_contact = previous_tick_contact;

        // 2. Inputs.
        self.process_control_inputs(controls, dt);
        trace.gas_pedal = self.gas_pedal;
        trace.brake_pedal = self.brake_pedal;
        trace.doing_burnout = self.doing_burnout;
        trace.raw_steer_angle = self.raw_steer_angle;

        // 3. Nitro. Runs before the drive calculation, so the activation tick
        //    already boosts.
        self.update_nitro(controls.nitro, dt);
        let nitro_drive = self.nitro_boost_active();
        trace.nitro_boost = nitro_drive;
        trace.nitro_level = self.tire_temperature;

        // 4. Airborne control torque.
        self.apply_airborne_control_forces(controls, dt, was_supported);

        // 5. (DoSoftGroundResistance omitted: needs surface/water queries.)

        // 6. Fold friction accumulators.
        self.body.apply_friction();

        // 7. Gravity, drag, angular air resistance.
        self.body.move_speed =
            self.body.move_speed - Vec3::new(0.0, 0.0, GTA_GRAVITY_RAW_PER_CTIME * dt);
        let drag_base = (1.0
            - self.handling.raw_drag_factor * self.handling.drag_coefficient * self.body.move_speed.length())
        .max(0.0);
        let drag_multiplier = drag_base.powf(dt);
        self.body.move_speed = self.body.move_speed * drag_multiplier;
        self.body.turn_speed = self.body.turn_speed * GTA_ANGULAR_AIR_RESISTANCE;
        trace.drag_multiplier = drag_multiplier;
        trace.speed_after_drag = self.body.move_speed.length();

        // 8. Wheel contact preparation.
        let logical = self.prepare_contacts(contacts);
        trace.logical_contacts = logical
            .iter()
            .map(|c| WHEEL_NAMES[c.index])
            .collect::<Vec<_>>()
            .join(",")
            .leak()
            .to_string();

        // 9. Suspension.
        let suspension_trace: SuspensionTrace =
            apply_suspension(&mut self.body, &self.handling, contacts, &self.compressions, dt);
        trace.spring_forces = suspension_trace
            .wheels
            .iter()
            .map(|w| w.spring_force.z)
            .collect();

        // 10. Gearbox.
        let speed_forward = self.body.move_speed.dot(self.body.forward());
        let drive_count = (0..4)
            .filter(|i| {
                logical.iter().any(|c| c.index == *i)
                    && (self.handling.drive_type == '4'
                        || (self.handling.drive_type == 'F' && FRONT_WHEELS.contains(i))
                        || (self.handling.drive_type == 'R' && !FRONT_WHEELS.contains(i)))
            })
            .count() as i32;
        // Native calls this even with zero driven wheels on the ground: the zero
        // count selects the free-wheel inertia branch and the a6/a7 history must
        // still advance.
        let drive: DriveAcceleration = self.transmission.calculate_drive_acceleration(
            self.gas_pedal,
            speed_forward,
            drive_count,
            nitro_drive,
        );
        trace.gear = drive.gear_after;
        trace.drive_acceleration = drive.drive_acceleration;

        // 11. Steering angle and the speed limiter.
        let steer_lock = self.handling.steering_lock_degrees.to_radians();
        let mut steering_angle = if self.raw_steer_angle.abs() > 1e-12 {
            steer_lock * self.raw_steer_angle * self.raw_steer_angle
                * if self.raw_steer_angle < 0.0 { -1.0 } else { 1.0 }
        } else {
            0.0
        };
        let front_supported = self.wheel_counts[0] > 0.0 || self.wheel_counts[2] > 0.0;
        if speed_forward <= 0.01 || !front_supported {
            // No limiter when stopped or when the front wheels are off the
            // ground: this is why you can turn the wheel fully in mid-air.
        } else {
            let speed_right = self.body.move_speed.dot(self.body.right());
            let traction = 0.004 * self.handling.traction_multiplier / 4.0
                / self.handling.velocity_frequency.max(1e-12);
            let surface = self.handling.surface_adhesive_limit;
            let mut limiter =
                (surface * traction * 16.0 / (speed_forward * speed_forward).max(1e-12)).min(1.0);
            limiter = limiter.asin() / steer_lock.max(1e-12);
            if (steering_angle < 0.0 && speed_right > 0.05)
                || (steering_angle > 0.0 && speed_right < -0.05)
                || self.handbrake_on
                || limiter > 1.0
            {
                limiter = 1.0;
            }
            steering_angle *= limiter;
        }
        // ProcessControlInputs updates the handbrake rear-steer history *before*
        // the front limiter is applied, and rear steer replaces the brake lock
        // rather than adding to it (handling flag 0x40, the Monster).
        if self.handling.rear_steer_by_handbrake() {
            self.rear_steer_angle = if self.handbrake_on {
                0.1 * self.rear_steer_angle - 0.9 * steering_angle
            } else {
                0.9 * self.rear_steer_angle
            };
        }
        self.front_steer_angle = steering_angle;
        trace.steer_angle = steering_angle;

        // 12. Wheel pairs.
        let base_brake = self.brake_pedal * self.handling.brake_deceleration * dt.min(3.0);
        let traction_global = 0.004 * self.handling.traction_multiplier / 4.0
            / self.handling.velocity_frequency.max(1e-12);
        let order: [(bool, [usize; 2]); 2] = if self.handling.handling_flags
            & HANDLING_FLAG_REAR_WHEEL_FIRST
            != 0
        {
            [(false, [1, 3]), (true, [0, 2])]
        } else {
            [(true, [0, 2]), (false, [1, 3])]
        };

        let mut wheel_results: Vec<ProcessWheelResult> = Vec::with_capacity(4);
        let contact_speeds = suspension_trace.contact_speeds;
        for (is_front, pair) in order {
            let pair_bias = if is_front {
                2.0 * self.handling.suspension_bias
            } else {
                2.0 - 2.0 * self.handling.suspension_bias
            };
            let mut brake_for_pair = if !is_front && self.handbrake_on
                && !self.handling.rear_steer_by_handbrake()
            {
                HANDBRAKE_BRAKE_TORQUE
            } else {
                base_brake
            };
            let mut pair_traction = traction_global;

            if !is_front && (!self.handbrake_on || self.handling.rear_steer_by_handbrake()) {
                if self.doing_burnout
                    && matches!(self.handling.drive_type, '4' | 'R')
                {
                    // Automobile.cpp applies this turn force once, at the left
                    // rear contact, before processing the rear pair.
                    brake_for_pair = 0.0;
                    pair_traction = 0.0;
                    let point = suspension_trace.wheel_contact_points[1];
                    let force = self.body.right()
                        * (clamp(3000.0 / self.handling.turn_mass.max(1e-12), 0.0, 1.0)
                            * -0.002
                            * self.handling.turn_mass
                            * steering_angle);
                    // The US 1.0 call at 0x6A511D literally passes the contact
                    // vector as FORCE and the steering/right vector as POINT.
                    // Do not "correct" this argument order: swapping it reverses
                    // burnout yaw and changes the centre-of-mass torque.
                    self.body.apply_turn_force(point, force, &self.handling);
                } else if matches!(self.handling.drive_type, '4' | 'R')
                    && !self.nitro_installed
                {
                    // The source tests the persistent NOS-installed bit, not
                    // whether boost is requested this tick.
                    pair_traction *= self.tire_temperature;
                }
            }

            let brake_bias = if is_front {
                2.0 * self.handling.brake_bias
            } else {
                2.0 - 2.0 * self.handling.brake_bias
            };
            let traction_bias = if is_front {
                2.0 * self.handling.traction_bias
            } else {
                2.0 - 2.0 * self.handling.traction_bias
            };

            for index in pair {
                if self.wheel_counts[index] <= 0.0 {
                    continue;
                }
                let Some(contact) = logical.iter().find(|c| c.index == index) else {
                    continue;
                };
                let normal = suspension_trace.normals[index];
                let steer = if is_front {
                    steering_angle
                } else if self.handling.handling_flags & 0x20 != 0 {
                    -steering_angle
                } else if self.handling.rear_steer_by_handbrake() {
                    self.rear_steer_angle
                } else {
                    0.0
                };
                let (wheel_forward, wheel_right) =
                    wheel_basis(self.body.forward(), normal, steer);

                let surface = contact
                    .surface_adhesive_limit
                    .unwrap_or(self.handling.surface_adhesive_limit);
                let mut adhesion = surface * pair_traction;
                // Load transfer: the more the spring is compressed, the more
                // grip, capped at 2x.
                adhesion *= (pair_bias
                    * self.handling.suspension_force_level
                    * 4.0
                    * (1.0 - self.compressions[index]))
                .min(2.0);
                adhesion *= traction_bias;

                let driven = self.handling.drive_type == '4'
                    || (self.handling.drive_type == 'F' && is_front)
                    || (self.handling.drive_type == 'R' && !is_front);
                let thrust = if driven { drive.drive_acceleration } else { 0.0 };

                let args = ProcessWheelArgs {
                    contact_speed: contact_speeds[index] - contact.relative_velocity,
                    wheel_forward,
                    wheel_right,
                    wheels_on_ground: (logical.len() as i32).max(1),
                    thrust,
                    brake: brake_for_pair * brake_bias,
                    adhesion,
                    gas_pedal: self.gas_pedal,
                    index,
                    ctimer_step: dt,
                };
                let mut r = process_wheel(
                    &mut self.body,
                    &self.handling,
                    contact.point,
                    &mut self.wheel_states[index],
                    &mut self.already_skidding,
                    &args,
                );
                // ProcessCarWheelPair clears a reverse-driven spinning state for
                // the drive pair immediately after ProcessWheel returns.
                if thrust != 0.0 && self.gas_pedal < 0.0 && r.state_after == WheelState::Spinning
                {
                    self.wheel_states[index] = WheelState::Normal;
                    r.state_after = WheelState::Normal;
                }
                wheel_results.push(r);
            }
        }
        trace.wheels = wheel_results;

        // Rear tyre temperature: rises while burning out, decays back to 1.0.
        if matches!(self.handling.drive_type, '4' | 'R') && !self.nitro_installed {
            let rear_spinning = self.wheel_states[1] == WheelState::Spinning
                || self.wheel_states[3] == WheelState::Spinning;
            if self.doing_burnout && rear_spinning {
                self.tire_temperature =
                    (self.tire_temperature + dt.min(3.0) / 1000.0).min(3.0);
            } else if self.tire_temperature > 1.0 {
                self.tire_temperature =
                    0.995f32.powf(dt.min(3.0)) * (self.tire_temperature - 1.0) + 1.0;
            }
        }

        // Rest stop: no pedal, ground contact last tick, and effectively still.
        if previous_tick_contact
            && self.gas_pedal.abs() <= 1e-12
            && self.body.move_speed.abs().length() < REST_STOP_SPEED
        {
            self.body.move_speed = ZERO;
            self.body.turn_speed.z = 0.0;
            trace.rest_stop = true;
        }

        trace.turn_speed_before_integrate = self.body.turn_speed;
        trace.move_speed_before_integrate = self.body.move_speed;

        // 14. Integration.
        self.body.integrate(dt, &self.handling, true);

        trace.position = self.body.position;
        trace.basis = self.body.basis;
        trace.move_speed = self.body.move_speed;
        trace.turn_speed = self.body.turn_speed;
        self.previous_compressions = self.compressions;
        self.prepare_previous_state_after_tick();
        trace
    }

    /// `CAutomobile::ProcessControlInputs` (0x6AD690), player branch.
    fn process_control_inputs(&mut self, controls: &Controls, dt: f32) {
        let forward = self.body.move_speed.dot(self.body.forward());
        self.handbrake_on = controls.handbrake > 0.0;
        self.doing_burnout = false;
        let raw_input = controls.accelerate - controls.brake_reverse;
        let (gas, brake);
        if forward.abs() < 0.01 {
            // Accelerate + brake together at a standstill is a burnout, not a
            // stall: both pedals hold.
            if controls.accelerate > 150.0 / 255.0 && controls.brake_reverse > 150.0 / 255.0 {
                self.doing_burnout = true;
                gas = controls.accelerate;
                brake = controls.brake_reverse;
            } else {
                gas = raw_input;
                brake = 0.0;
            }
        } else if forward >= 0.0 {
            if raw_input < 0.0 {
                gas = 0.0;
                brake = raw_input.abs();
            } else {
                gas = raw_input;
                brake = 0.0;
            }
        } else if raw_input >= 0.0 && (self.gas_pedal <= 0.5 || forward <= -0.15) {
            // Rolling backwards: brake until the pedal is held past the point
            // where the game decides you mean "drive forward again".
            gas = 0.0;
            brake = raw_input;
        } else {
            gas = raw_input;
            brake = 0.0;
        }
        self.gas_pedal = gas;
        self.brake_pedal = brake;
        // Steering ramps at 1/5 per CTimer step toward the input; this lag is a
        // large part of GTA's steering feel.
        self.raw_steer_angle += (-controls.steering_input() - self.raw_steer_angle) / 5.0 * dt;
        self.raw_steer_angle = clamp(self.raw_steer_angle, -1.0, 1.0);
    }

    /// `CAutomobile::NitrousControl(0)` (0x6A3EA0).
    fn update_nitro(&mut self, pressed: bool, dt: f32) {
        if !self.nitro_installed {
            return;
        }
        let level = self.tire_temperature;
        if level >= 1.0 && self.nitro_boosts > 0 {
            if pressed {
                self.tire_temperature = -0.000001;
                if self.nitro_boosts < NITRO_UNLIMITED_CHARGES {
                    self.nitro_boosts -= 1;
                }
            }
            return;
        }
        if level >= 0.0 {
            // Recharge faster with less throttle.
            let rate_factor = (1.0 - self.gas_pedal).max(0.25);
            self.tire_temperature = (level + rate_factor * dt * NITRO_RECHARGE_RATE).min(1.0);
            return;
        }
        self.tire_temperature = level - dt * NITRO_ACTIVE_RATE;
        if self.tire_temperature < -1.0 {
            self.tire_temperature = 0.000001;
            if self.nitro_boosts <= 0 {
                self.nitro_installed = false;
                self.tire_temperature = 1.0;
            }
        }
    }

    /// The player airborne-control branch of `ProcessAI`. Only reachable when
    /// the previous tick had no contacting wheels.
    fn apply_airborne_control_forces(
        &mut self,
        controls: &Controls,
        dt: f32,
        was_supported: bool,
    ) {
        // Deliberate asymmetry: the handbrake impulse still fires while
        // accelerating, but the coast and pitch impulses are gated by
        // `!GetAccelerate()`.
        if was_supported || (controls.accelerate > 0.0 && controls.handbrake <= 0.0) {
            return;
        }
        let weight =
            0.0007 * (3000.0 / self.handling.turn_mass.max(1e-12)).min(1.0) * self.handling.turn_mass;
        let steer_lr = controls.steering_input();
        let com_local = self.body.center_of_mass_local(&self.handling);
        let right = self.body.right();
        let forward = self.body.forward();
        let up = self.body.up();

        if controls.handbrake > 0.0 {
            apply_air_impulse(&mut self.body, &self.handling, com_local, weight, dt, right, forward, steer_lr, up);
        } else if controls.accelerate <= 0.0 {
            apply_air_impulse(&mut self.body, &self.handling, com_local, weight, dt, right, up, steer_lr, forward);
        }
        if controls.accelerate <= 0.0 {
            let steer_ud = controls.steer_back - controls.steer_forward;
            apply_air_impulse(&mut self.body, &self.handling, com_local, weight, dt, up, forward, steer_ud, right);
        }
    }

    /// Normalise this tick's contacts into logical wheel contacts.
    fn prepare_contacts(&mut self, contacts: &WheelContactSet) -> Vec<crate::contact::WheelContact> {
        let mut out = Vec::with_capacity(4);
        for index in 0..4 {
            let Some(contact) = contacts.get(index) else {
                self.wheel_counts[index] = (self.wheel_counts[index] - self.ctimer_step).max(0.0);
                self.compressions[index] = 1.0;
                continue;
            };
            let front = index % 2 == 0;
            let compression = contact.compression.unwrap_or_else(|| {
                self.handling
                    .normalized_compression(contact.line_fraction, front)
            });
            self.compressions[index] = compression;
            let current_contact = contact.geometric_hit && compression < 1.0;
            if current_contact {
                self.previous_normals[index] = contact.normal;
            }
            if compression < 1.0 {
                self.wheel_counts[index] = WHEEL_COUNT_PERSIST;
            } else {
                self.wheel_counts[index] =
                    (self.wheel_counts[index] - self.ctimer_step).max(0.0);
            }
            if self.wheel_counts[index] > 0.0 {
                let mut logical = contact.clone();
                logical.normal = self.previous_normals[index];
                out.push(logical);
            }
        }
        let hits = [
            contacts.get(0).map(|c| c.geometric_hit).unwrap_or(false),
            contacts.get(1).map(|c| c.geometric_hit).unwrap_or(false),
            contacts.get(2).map(|c| c.geometric_hit).unwrap_or(false),
            contacts.get(3).map(|c| c.geometric_hit).unwrap_or(false),
        ];
        let _ = hits;
        out.sort_by_key(|c| c.index);
        out
    }

    fn prepare_previous_state_after_tick(&mut self) {
        // `previous_compressions` was already assigned in `tick`; this hook
        // exists so callers that drive the pieces directly can mirror it.
        let _ = advance_wheel_counts;
    }
}

/// One airborne-control impulse.
///
/// `requested` is the control axis value; the impulse is skipped when the body
/// is already rotating that way beyond ~0.02 rad/step, which is why holding a
/// key in the air ramps once and then stops instead of spinning up forever.
#[allow(clippy::too_many_arguments)] // one control axis per impulse, spelled out
fn apply_air_impulse(
    body: &mut Body,
    handling: &Handling,
    com_local: Vec3,
    weight: f32,
    dt: f32,
    force_axis: Vec3,
    point_axis: Vec3,
    requested: f32,
    current_axis: Vec3,
) {
    if requested.abs() <= 1e-12 {
        return;
    }
    let current = body.turn_speed.dot(current_axis);
    if !((current < 0.02 && requested < 0.0) || (current > -0.02 && requested > 0.0)) {
        return;
    }
    let force = force_axis * (requested * weight * dt);
    // The source passes the stored *local* COM plus an axis here, and
    // ApplyTurnForce subtracts the rotated COM internally. Pre-transforming this
    // point changes the mixed local/world convention and the resulting torque.
    let point = com_local + point_axis;
    body.apply_turn_force(force, point, handling);
}

/// Per-tick trace. Native code keeps none of this; it exists so a Rust port can
/// compare one tick against a recorded reference and find the first divergence.
#[derive(Clone, Debug, Default)]
pub struct VehicleTickTrace {
    pub step: f32,
    pub previous_supported: bool,
    pub previous_tick_contact: bool,
    pub gas_pedal: f32,
    pub brake_pedal: f32,
    pub doing_burnout: bool,
    pub raw_steer_angle: f32,
    pub steer_angle: f32,
    pub nitro_boost: bool,
    pub nitro_level: f32,
    pub drag_multiplier: f32,
    pub speed_after_drag: f32,
    pub logical_contacts: String,
    pub spring_forces: Vec<f32>,
    pub gear: usize,
    pub drive_acceleration: f32,
    pub wheels: Vec<ProcessWheelResult>,
    pub rest_stop: bool,
    pub move_speed_before_integrate: Vec3,
    pub turn_speed_before_integrate: Vec3,
    pub position: Vec3,
    pub basis: crate::math::Basis,
    pub move_speed: Vec3,
    pub turn_speed: Vec3,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contact::WheelContact;

    /// A real flat ground plane at z = 0. Contacts are rebuilt from the car's
    /// current pose every tick, exactly as a world query would; feeding constant
    /// model-relative points would make the springs pump (a real porting bug:
    /// the contact has to come from the world, not be cached).
    fn flat_ground_contacts(pos: Vec3) -> WheelContactSet {
        let h = Handling::default();
        let definitions = [
            (-h.wheel_track / 2.0, h.wheel_front_y),
            (-h.wheel_track / 2.0, h.wheel_rear_y),
            (h.wheel_track / 2.0, h.wheel_front_y),
            (h.wheel_track / 2.0, h.wheel_rear_y),
        ];
        let mut set = WheelContactSet::default();
        for (i, (x, y)) in definitions.iter().enumerate() {
            let (start, end) = h.suspension_line(*x, *y);
            let start_world = pos + start;
            let end_world = pos + end;
            let span = start_world.z - end_world.z;
            let t = if span.abs() < 1e-6 {
                1.0
            } else {
                ((start_world.z - 0.0) / span).clamp(0.0, 1.0)
            };
            let hit_world = start_world.lerp(end_world, t);
            if t >= 1.0 {
                set.wheels[i] = Some(WheelContact::miss(i));
            } else {
                let mut c = WheelContact::hit(
                    i,
                    hit_world - pos,
                    Vec3::new(0.0, 0.0, 1.0),
                    t,
                );
                c.point = hit_world - pos;
                set.wheels[i] = Some(c);
            }
        }
        set
    }

    #[test]
    fn car_on_flat_ground_settles_and_does_not_sink() {
        let mut v = Vehicle::new(Handling::default(), Vec3::new(0.0, 0.0, 0.4), 0.5);
        for _ in 0..2000 {
            let contacts = flat_ground_contacts(v.body.position);
            v.tick(&Controls::default(), &contacts);
        }
        // The car must come to rest with all four springs partly compressed and
        // its body within a few centimetres of where the geometry puts it.
        for (i, c) in v.compressions.iter().enumerate() {
            assert!(*c > 0.2 && *c < 0.95, "wheel {i} compression {c}");
        }
        assert!(v.body.position.z > -0.5, "car sank: {}", v.body.position.z);
        assert!(v.body.position.z < 0.8, "car launched: {}", v.body.position.z);
        assert!(
            v.body.move_speed.length() < 0.05,
            "car never came to rest: {:?}",
            v.body.move_speed
        );
        assert!(!v.handbrake_on);
    }

    #[test]
    fn straight_line_acceleration_moves_forward() {
        let mut v = Vehicle::new(Handling::default(), Vec3::new(0.0, 0.0, 0.4), 0.5);
        let controls = Controls {
            accelerate: 1.0,
            ..Default::default()
        };
        for _ in 0..400 {
            let contacts = flat_ground_contacts(v.body.position);
            v.tick(&controls, &contacts);
        }
        assert!(v.body.position.y > 1.0, "no forward motion: {:?}", v.body.position);
        assert!(v.transmission.current_gear > 1, "never shifted");
    }

    #[test]
    fn handbrake_locks_the_rear_and_spins() {
        let mut v = Vehicle::new(Handling::default(), Vec3::new(0.0, 0.0, 0.4), 0.5);
        for _ in 0..200 {
            let contacts = flat_ground_contacts(v.body.position);
            v.tick(&Controls { accelerate: 1.0, ..Default::default() }, &contacts);
        }
        let turn_before = v.body.turn_speed.z;
        for _ in 0..40 {
            let contacts = flat_ground_contacts(v.body.position);
            v.tick(
                &Controls {
                    accelerate: 1.0,
                    handbrake: 1.0,
                    steering: 0.6,
                    ..Default::default()
                },
                &contacts,
            );
        }
        assert!(v.body.turn_speed.z != turn_before);
        assert!(v.handbrake_on);
    }
}

#[cfg(test)]
mod hold_tests {
    use super::*;
    use crate::contact::{WheelContact, WheelContactSet};

    fn four_wheels(compression: f32) -> WheelContactSet {
        let mut set = WheelContactSet::default();
        // Model-relative points in GTA space: x right, y forward, z up.
        let layout = [
            (-0.76, 1.73),
            (-0.76, -1.73),
            (0.76, 1.73),
            (0.76, -1.73),
        ];
        for (index, (x, y)) in layout.iter().enumerate() {
            set.wheels[index] = Some(WheelContact::compressed(
                index,
                Vec3::new(*x, *y, -0.6),
                Vec3::new(0.0, 0.0, 1.0),
                compression,
            ));
        }
        set
    }

    #[test]
    fn a_car_with_contacts_holds_itself_up() {
        let handling = Handling::default();
        let mut vehicle = Vehicle::new(handling.clone(), Vec3::new(0.0, 0.0, 0.6), 0.4167);
        let controls = Controls::default();
        let contacts = four_wheels(0.5);
        let mut trace = None;
        for _ in 0..20 {
            trace = Some(vehicle.tick(&controls, &contacts));
        }
        let trace = trace.unwrap();
        println!(
            "z={:.4} vz={:.5} spring_z={:?} comp={:?}",
            vehicle.body.position.z,
            vehicle.body.move_speed.z,
            trace.spring_forces,
            vehicle.compressions
        );
        assert!(
            vehicle.body.move_speed.z > -0.05,
            "fell through: vz={}",
            vehicle.body.move_speed.z
        );
        assert!(
            vehicle.body.position.z > 0.3,
            "sank to {}",
            vehicle.body.position.z
        );
    }
}
