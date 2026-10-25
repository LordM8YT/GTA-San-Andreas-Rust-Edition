//! `cTransmission` — the gearbox, and the acceleration curve that makes GTA
//! cars feel like GTA cars.
//!
//! Two functions matter, and both are ported here in full:
//!
//! * `cHandlingDataMgr::ConvertDataToGameUnits` builds the gear table once per
//!   handling record: it converts km/h to GTA's per-step units and walks the
//!   top speed down in 0.01 steps until drag and gear acceleration balance.
//!   That walk is why a car's real top speed sits below `maxVelocity`.
//! * `cTransmission::CalculateDriveAcceleration` (0x6D05E0) runs every tick.
//!   It is a small state machine over `(velocity, current gear, gas pedal)`
//!   returning a raw acceleration — a velocity delta per tick, not per second.
//!   It also carries two history values, `a6`/`a7` in the source, that smooth
//!   gear changes. They persist across ticks and are half of what people mean
//!   by "the transmission feels wrong".
//!
//! Read [`Transmission::calculate_drive_acceleration`] top to bottom; comments
//! mark the branches that are easy to get backwards.

use crate::handling::{Handling, HANDLING_FLAG_1ST_GEAR, HANDLING_FLAG_2ND_GEAR};
use crate::math::clamp;

/// `1 / (50 * 50)`: handling.cfg acceleration -> game units.
pub const GTA_ACCEL_CONST: f32 = 1.0 / (50.0 * 50.0);
/// `0.277778 / 50`: km/h -> GTA velocity units per CTimer step.
pub const GTA_VELOCITY_CONST: f32 = 0.277778 / 50.0;
/// Free-wheel inertia growth per unit of pedal when no driven wheel touches.
pub const GTA_FREE_ACCELERATION: f32 = 0.1;
/// US 1.0 computes the gear-change window as `maxV / gears * (1 - 0.6667)`.
/// The decompiled source prints `/3`; the binary loads 0.6667 from `.rdata`
/// (0x871D50) and that is what moves the settled idle `a6` to the
/// telemetry-observed 0.2631288.
pub const GTA_TRANSMISSION_MAX_CHANGE_FRAC: f32 = 1.0 - 0.6667;
/// `a7 = a * (1 - 0.85) + 0.85 * a7` — the smoother on the inertia term.
pub const GTA_TRANSMISSION_SMOOTHER: f32 = 0.85;
pub const GTA_NITROS_MULTIPLIER: f32 = 2.0;
pub const GTA_NITROS_INERTIA_MULTIPLIER: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gear {
    /// Velocity at which this gear tops out.
    pub max_velocity: f32,
    /// Shift up when velocity exceeds this.
    pub change_up_velocity: f32,
    /// Shift down when velocity falls below this.
    pub change_down_velocity: f32,
}

#[derive(Clone, Debug)]
pub struct Transmission {
    pub number_of_gears: usize,
    pub drive_type: char,
    pub handling_flags: u32,
    /// `engineAcceleration / (4 for 4WD, 2 otherwise)`.
    pub engine_acceleration: f32,
    pub engine_inertia: f32,
    pub max_velocity: f32,
    pub max_flat_velocity: f32,
    pub max_reverse_velocity: f32,
    /// `gears[0]` is reverse, `gears[1..=n]` are forward gears.
    pub gears: Vec<Gear>,
    pub current_gear: usize,
    /// `a6` — normalised position inside the current gear window.
    pub inertia_value1: f32,
    /// `a7` — smoothed multiplier actually applied to acceleration.
    pub inertia_value2: f32,
    /// `CTimer::GetTimeStep()` as seen by the native call. 1.0 at 50 Hz,
    /// 0.5 at the 100 Hz rate this project runs. Not a struct member in the
    /// original (it reads the global); kept here so the call is pure.
    pub step: f32,
}

impl Transmission {
    /// `cHandlingDataMgr::ConvertDataToGameUnits` for one automobile.
    pub fn from_handling(h: &Handling) -> Self {
        let gears_count = h.number_of_gears;
        assert!(
            (1..=5).contains(&gears_count),
            "SA transmission supports 1..=5 gears"
        );
        let drive_split = if h.drive_type == '4' { 4.0 } else { 2.0 };
        let acceleration_before_split = h.engine_acceleration * GTA_ACCEL_CONST;
        let acceleration_limit = acceleration_before_split / 6.0;
        let converted_max = h.max_velocity * GTA_VELOCITY_CONST;

        // Walk the nominal top speed down until drag stops letting the engine
        // accelerate: the same loop the native runs.
        let mut max_flat = converted_max;
        while max_flat > 0.0 {
            max_flat -= 0.01;
            if h.drag_coefficient >= 0.01 {
                if max_flat * max_flat * (h.drag_coefficient / 2.0) / 1000.0 <= acceleration_limit {
                    break;
                }
            } else {
                let reciprocal = 1.0 / (max_flat * max_flat * h.drag_coefficient + 1.0);
                if (reciprocal - 1.0) * -max_flat <= acceleration_limit {
                    break;
                }
            }
        }

        let (max_velocity, max_flat_velocity, max_reverse) = if h.uses_maxsp_limit() {
            (converted_max, max_flat / 1.2, (-max_flat / 1.2 / 4.0).min(-0.2))
        } else {
            (max_flat * 1.2, max_flat, (-max_flat * 0.3).min(-0.2))
        };

        let average_half = 0.5 * max_velocity / gears_count as f32;
        let max_gear = max_velocity - average_half;
        let mut gears = vec![
            Gear {
                max_velocity: 0.0,
                change_up_velocity: 0.0,
                change_down_velocity: 0.0,
            };
            gears_count + 1
        ];
        for gear_number in 1..=gears_count {
            let gear_max = gear_number as f32 * max_gear / gears_count as f32 + average_half;
            let previous = gears[gear_number - 1];
            let difference = gear_max - previous.max_velocity;
            let up = if gear_number >= gears_count {
                max_velocity
            } else {
                0.6667 * difference + previous.max_velocity
            };
            gears[gear_number].max_velocity = gear_max;
            gears[gear_number].change_up_velocity = up;
            if gear_number < gears_count {
                gears[gear_number + 1].change_down_velocity =
                    0.42 * difference + previous.max_velocity;
            }
        }
        // Reverse is gear 0; first is gear 1 with an explicit down-shift floor.
        gears[0] = Gear {
            max_velocity: max_reverse,
            change_up_velocity: -0.01,
            change_down_velocity: max_reverse,
        };
        gears[1] = Gear {
            max_velocity: gears[1].max_velocity,
            change_up_velocity: gears[1].change_up_velocity,
            change_down_velocity: -0.01,
        };

        Self {
            number_of_gears: gears_count,
            drive_type: h.drive_type,
            handling_flags: h.handling_flags,
            engine_acceleration: acceleration_before_split / drive_split,
            engine_inertia: h.engine_inertia,
            max_velocity,
            max_flat_velocity,
            max_reverse_velocity: max_reverse,
            gears,
            current_gear: 1,
            inertia_value1: 0.0,
            inertia_value2: 0.0,
            step: 1.0,
        }
    }

    /// 1.0 = original 50 Hz, 0.5 = the 100 Hz rate this project simulates at.
    pub fn set_step(&mut self, step: f32) {
        assert!(step > 0.0 && step.is_finite());
        self.step = step;
    }

    /// GTA constructs `a6`/`a7` as zero, but a car parked on the ground since
    /// map load has been running the grounded branch every tick, so its history
    /// is settled rather than zero. Spawning from zero history makes the first
    /// second of acceleration wrong. This iterates the grounded recurrence at
    /// rest, gas pedal 0, all wheels down, until it stops moving.
    pub fn initialize_grounded_idle(&mut self) {
        let mut a6 = 0.0f32;
        let mut a7 = 0.0f32;
        let gear = 1usize;
        for _ in 0..500 {
            let max_change =
                self.max_velocity / self.number_of_gears as f32 * GTA_TRANSMISSION_MAX_CHANGE_FRAC;
            // velocity 0 in gear 1
            let current_down = max_change;
            let up_down = max_change + self.gears[1].change_up_velocity;
            if up_down.abs() < 1e-9 {
                break;
            }
            let ratio = current_down / up_down;
            let inertia_multiplier = ratio - a6;
            a6 = ratio;
            let a = clamp(1.0 - inertia_multiplier * self.engine_inertia, 0.1, 1.0);
            let next = a * (1.0 - GTA_TRANSMISSION_SMOOTHER) + GTA_TRANSMISSION_SMOOTHER * a7;
            if (next - a7).abs() < 1e-9 {
                a7 = next;
                break;
            }
            a7 = next;
        }
        let _ = gear;
        self.current_gear = 1;
        self.inertia_value1 = a6;
        self.inertia_value2 = a7;
    }

    /// `cTransmission::CalculateDriveAcceleration`, 0x6D05E0.
    ///
    /// * `velocity` — forward component of `move_speed`, raw units.
    /// * `all_wheels_on_ground` — count of *driven* wheels on the ground this
    ///   tick (0..=4). Native calls this even when the count is 0, which is what
    ///   selects the free-wheel inertia branch.
    /// * Returns a velocity delta to add along forward, per tick.
    pub fn calculate_drive_acceleration(
        &mut self,
        gas_pedal: f32,
        velocity: f32,
        all_wheels_on_ground: i32,
        nitro: bool,
    ) -> DriveAcceleration {
        let current_velocity = velocity;
        let gear_before = self.current_gear;
        let before_a6 = self.inertia_value1;
        let before_a7 = self.inertia_value2;

        if current_velocity < self.max_reverse_velocity {
            return self.result(gear_before, before_a6, before_a7, 0.0, nitro, "below-reverse");
        }
        let mut all_ground = all_wheels_on_ground;
        let mut pointers = true;
        let cheat_multiplier = 1.0f32;
        // The NITROS cheat is threaded through the call: every accelerate branch
        // loads 2.0 when boosting, so reverse gets the same x2. A gear walk
        // inside the same call clears it, which is why a shift tick never boosts.
        let mut handling_nitros = nitro;

        loop {
            if current_velocity > self.max_velocity {
                break;
            }
            let gear = self.gears[self.current_gear.min(self.gears.len() - 1)];
            let mut accelerate = false;
            let mut lower = false;
            if current_velocity > gear.change_up_velocity {
                if self.current_gear == 0 && gas_pedal <= 0.0 {
                    accelerate = true;
                } else {
                    self.current_gear = (self.current_gear + 1).min(self.gears.len() - 1);
                }
            } else {
                if current_velocity >= gear.change_down_velocity
                    || self.current_gear == 0
                    || (self.current_gear == 1 && gas_pedal >= 0.0)
                {
                    accelerate = true;
                }
                lower = true;
            }

            if accelerate {
                let nitros_multiplier = if handling_nitros { GTA_NITROS_MULTIPLIER } else { 1.0 };
                let speed_multiplier = if self.number_of_gears == 1 {
                    1.0
                } else if self.current_gear >= 1 {
                    let mut gear_number = 1.0
                        - (self.current_gear as f32 - 1.0) / (self.number_of_gears as f32 - 1.0);
                    gear_number *= gear_number;
                    let base = if self.handling_flags & HANDLING_FLAG_1ST_GEAR != 0 {
                        5.0
                    } else if self.handling_flags & HANDLING_FLAG_2ND_GEAR != 0 {
                        4.0
                    } else {
                        3.0
                    };
                    gear_number * base + 1.0
                } else {
                    // Reverse: 4.5 and — contrary to a common reading — nitro
                    // doubles it like any other gear. The signed (negative) gas
                    // pedal is what makes it push backwards.
                    4.5
                };

                let mut acceleration = speed_multiplier
                    * (cheat_multiplier * self.engine_acceleration)
                    * nitros_multiplier
                    * 0.4
                    * gas_pedal
                    * self.step;

                if pointers {
                    if all_ground != 0 {
                        let max_change = self.max_velocity / self.number_of_gears as f32
                            * GTA_TRANSMISSION_MAX_CHANGE_FRAC;
                        let (current_down, up_down) = if self.current_gear == 0 {
                            (
                                max_change - current_velocity,
                                max_change - self.gears[0].change_down_velocity,
                            )
                        } else if self.current_gear == 1 {
                            (
                                max_change + current_velocity,
                                max_change + self.gears[1].change_up_velocity,
                            )
                        } else {
                            (
                                current_velocity - gear.change_down_velocity,
                                gear.change_up_velocity - gear.change_down_velocity,
                            )
                        };
                        let ratio = current_down / up_down;
                        let mut inertia_multiplier = ratio - self.inertia_value1;
                        if handling_nitros {
                            inertia_multiplier *= GTA_NITROS_INERTIA_MULTIPLIER;
                        }
                        let a = clamp(1.0 - inertia_multiplier * self.engine_inertia, 0.1, 1.0);
                        self.inertia_value1 = ratio;
                        self.inertia_value2 = a * (1.0 - GTA_TRANSMISSION_SMOOTHER)
                            + GTA_TRANSMISSION_SMOOTHER * self.inertia_value2;
                        acceleration *= self.inertia_value2;
                    } else {
                        // No driven wheel on the ground: the engine free-revs and
                        // builds inertia, which is what makes a car accelerate
                        // harder again on landing.
                        self.inertia_value1 += gas_pedal.abs() / self.engine_inertia.max(1e-12)
                            * GTA_FREE_ACCELERATION;
                        self.inertia_value1 = self.inertia_value1.min(1.0);
                        self.inertia_value2 = 0.1;
                    }
                }

                let gear_max = gear.max_velocity;
                let cheat_max = cheat_multiplier * gear_max;
                if gear_max >= 0.0 || current_velocity >= cheat_max {
                    if gear_max <= 0.0 || current_velocity <= cheat_max {
                        return self.result(
                            gear_before,
                            before_a6,
                            before_a7,
                            acceleration,
                            nitro,
                            "accelerate",
                        );
                    }
                    let change = current_velocity - cheat_max;
                    acceleration *= 1.0 - (change / 0.05).min(1.0);
                    return self.result(
                        gear_before,
                        before_a6,
                        before_a7,
                        acceleration,
                        nitro,
                        "gear-top",
                    );
                } else {
                    let change = cheat_max - current_velocity;
                    acceleration *= 1.0 - (change / 0.05).min(1.0);
                    return self.result(
                        gear_before,
                        before_a6,
                        before_a7,
                        acceleration,
                        nitro,
                        "gear-top",
                    );
                }
            }

            if lower {
                self.current_gear = self.current_gear.saturating_sub(1);
            }
            pointers = false;
            all_ground = 0;
            handling_nitros = false;
            if current_velocity < self.max_reverse_velocity {
                break;
            }
        }
        // Walking the whole gear stack without accelerating returns zero: the
        // car is outside every gear window (above max velocity, or below
        // reverse), so the engine contributes nothing this tick.
        self.result(
            gear_before,
            before_a6,
            before_a7,
            0.0,
            nitro,
            "walk-exhausted",
        )
    }

    fn result(
        &self,
        gear_before: usize,
        before_a6: f32,
        before_a7: f32,
        acceleration: f32,
        nitro: bool,
        branch: &'static str,
    ) -> DriveAcceleration {
        DriveAcceleration {
            gear_before,
            gear_after: self.current_gear,
            drive_acceleration: acceleration,
            inertia_value1_before: before_a6,
            inertia_value1_after: self.inertia_value1,
            inertia_value2_before: before_a7,
            inertia_value2_after: self.inertia_value2,
            nitro,
            branch,
        }
    }
}

/// One call's result. Native returns only `acceleration`; the rest is kept for
/// tracing and differential tests.
#[derive(Clone, Copy, Debug)]
pub struct DriveAcceleration {
    pub gear_before: usize,
    pub gear_after: usize,
    pub drive_acceleration: f32,
    pub inertia_value1_before: f32,
    pub inertia_value1_after: f32,
    pub inertia_value2_before: f32,
    pub inertia_value2_after: f32,
    pub nitro: bool,
    pub branch: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infernus_gear_table() {
        let h = Handling::default();
        let t = Transmission::from_handling(&h);
        // 240 km/h converts to 1.3333 raw, but `maxVelocity` ends up being
        // 1.2x the *drag-limited* flat speed: the game's real top speed is
        // lower than the handling.cfg number, and that is faithful.
        assert!((t.max_velocity - 1.228).abs() < 0.02, "{}", t.max_velocity);
        assert!(t.max_velocity < 240.0 * 0.277778 / 50.0);
        assert!(t.max_flat_velocity < t.max_velocity);
        assert_eq!(t.gears.len(), 6);
        assert!(t.gears[0].max_velocity < 0.0);
        for w in t.gears[1..].windows(2) {
            assert!(w[1].max_velocity > w[0].max_velocity);
            assert!(w[1].change_up_velocity > w[0].change_up_velocity);
        }
        assert_eq!(t.gears[5].change_up_velocity, t.max_velocity);
    }

    #[test]
    fn standing_start_shifts_up_and_converges() {
        let h = Handling::default();
        let mut t = Transmission::from_handling(&h);
        t.set_step(0.5);
        let mut v = 0.0f32;
        let mut shift_seen = false;
        for _ in 0..4000 {
            let r = t.calculate_drive_acceleration(1.0, v, 4, false);
            v += r.drive_acceleration;
            if r.gear_after != r.gear_before {
                shift_seen = true;
            }
            assert!(v < 2.0, "velocity ran away: {v}");
        }
        assert!(shift_seen, "car never shifted");
        assert_eq!(t.current_gear, 5);
        assert!(v > 0.5, "car never moved: {v}");
    }

    #[test]
    fn zero_gas_coasts() {
        let h = Handling::default();
        let mut t = Transmission::from_handling(&h);
        t.set_step(0.5);
        let r = t.calculate_drive_acceleration(0.0, 0.4, 4, false);
        assert_eq!(r.drive_acceleration, 0.0);
        assert!(t.current_gear >= 1);
    }

    #[test]
    fn grounded_idle_history_settles() {
        let h = Handling::default();
        let mut t = Transmission::from_handling(&h);
        t.initialize_grounded_idle();
        assert!(t.inertia_value1 > 0.1 && t.inertia_value1 < 1.0);
        assert!(t.inertia_value2 > 0.0 && t.inertia_value2 <= 1.0);
    }
}
