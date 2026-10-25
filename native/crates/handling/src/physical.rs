//! `CPhysical` — GTA's rigid body.
//!
//! It is much simpler than a modern physics engine and that is the point:
//!
//! * Rotation is *not* a tensor. There is one scalar, `turn_mass`, standing in
//!   for inertia about every axis. `ApplyTurnForce` is
//!   `ω += (r × F) / turn_mass`.
//! * A force at a point therefore changes both linear and angular velocity,
//!   with the angular part scaled by `1 / turn_mass` and the linear part by
//!   `1 / mass`.
//! * `GetSpeed(point)` is `v + ω × (point - centreOfMass)`.
//! * "Effective mass" at a point along a direction is
//!   `1 / (|r × d|² / turn_mass + 1 / mass)`. Every contact solver uses it.
//! * Friction forces accumulate in a *separate* pair of accumulators
//!   (`friction_move_speed` / `friction_turn_speed`) that are folded into the
//!   main velocity once per tick by [`Body::apply_friction`]. If you add
//!   friction straight into `move_speed`, contact response changes.
//! * The stored position is the **model origin**, not the centre of mass.
//!   Integration therefore moves the origin by `v * dt` and then re-offsets it
//!   by the rotating COM arm. Skipping that second step makes cars orbit a
//!   point slightly ahead of themselves when spinning.
//!
//! Source: `CPhysical::ApplyMoveForce` 0x542B50, `ApplyTurnForce` 0x542A50,
//! `GetSpeed`/`GetMass` inline, `ApplyFriction` 0x5483D0,
//! `ApplyMoveSpeed` 0x547B80, `ApplyTurnSpeed` 0x542E20,
//! `CMatrix::Reorthogonalise` 0x59B6A0.

use crate::handling::Handling;
use crate::math::{Basis, Vec3, ZERO};

#[derive(Clone, Debug)]
pub struct Body {
    /// Model origin (not COM) in world space.
    pub position: Vec3,
    /// right / forward / up, kept orthonormal.
    pub basis: Basis,
    /// `m_vecMoveSpeed` — raw units per CTimer step.
    pub move_speed: Vec3,
    /// `m_vecTurnSpeed` — radians per CTimer step.
    pub turn_speed: Vec3,
    /// Friction accumulator, folded in by [`Body::apply_friction`].
    pub friction_move_speed: Vec3,
    /// Friction accumulator, folded in by [`Body::apply_friction`].
    pub friction_turn_speed: Vec3,
    /// Per-tick COM override, e.g. the less-than-three-wheels balance offset
    /// that `CAutomobile::ProcessAI` computes. `None` = use handling's COM.
    pub active_center_of_mass: Option<Vec3>,
}

impl Default for Body {
    fn default() -> Self {
        Self {
            position: ZERO,
            basis: Basis::default(),
            move_speed: ZERO,
            turn_speed: ZERO,
            friction_move_speed: ZERO,
            friction_turn_speed: ZERO,
            active_center_of_mass: None,
        }
    }
}

impl Body {
    pub fn new(position: Vec3, basis: Basis) -> Self {
        Self {
            position,
            basis: basis.reorthogonalise(),
            ..Default::default()
        }
    }

    pub fn right(&self) -> Vec3 {
        self.basis.right
    }

    pub fn forward(&self) -> Vec3 {
        self.basis.forward
    }

    pub fn up(&self) -> Vec3 {
        self.basis.up
    }

    pub fn forward_speed(&self) -> f32 {
        self.move_speed.dot(self.forward())
    }

    pub fn speed(&self) -> f32 {
        self.move_speed.length()
    }

    /// Local COM for this tick (override wins).
    pub fn center_of_mass_local(&self, h: &Handling) -> Vec3 {
        self.active_center_of_mass.unwrap_or(h.center_of_mass)
    }

    /// World-space COM offset from the model origin.
    pub fn center_of_mass_world_offset(&self, h: &Handling) -> Vec3 {
        self.basis.transform_local(self.center_of_mass_local(h))
    }

    pub fn center_of_mass_position(&self, h: &Handling) -> Vec3 {
        self.position + self.center_of_mass_world_offset(h)
    }

    /// `CPhysical::GetSpeed` for a point given relative to the model origin.
    pub fn get_speed(&self, point: Vec3, h: &Handling) -> Vec3 {
        let distance = point - self.center_of_mass_world_offset(h);
        let turn = self.turn_speed + self.friction_turn_speed;
        let mov = self.move_speed + self.friction_move_speed;
        mov + turn.cross(distance)
    }

    /// `GetMass(wheelContactPoint, direction)` as used by `ProcessWheel`:
    /// the point is model-relative and used *directly*, without subtracting the
    /// COM first. Suspension uses [`Body::contact_effective_mass`] instead —
    /// the two conventions differ and mixing them shifts grip.
    pub fn wheel_effective_mass(&self, point: Vec3, direction: Vec3, h: &Handling) -> f32 {
        let arm = point.cross(direction);
        1.0 / (arm.dot(arm) / h.turn_mass.max(1e-12) + h.inverse_mass())
    }

    /// `GetMass` for suspension/contact points, where the arm is measured from
    /// the centre of mass.
    pub fn contact_effective_mass(&self, point: Vec3, direction: Vec3, h: &Handling) -> f32 {
        let distance = point - self.center_of_mass_world_offset(h);
        let arm = distance.cross(direction);
        1.0 / (arm.dot(arm) / h.turn_mass.max(1e-12) + h.inverse_mass())
    }

    /// `CPhysical::ApplyMoveForce` — linear only.
    pub fn apply_move_force(&mut self, force: Vec3, h: &Handling) -> Vec3 {
        let delta = force * h.inverse_mass();
        self.move_speed = self.move_speed + delta;
        delta
    }

    /// `CPhysical::ApplyTurnForce` — angular only. `point` is model-relative
    /// *and* the COM is subtracted here, which is the source convention.
    pub fn apply_turn_force(&mut self, force: Vec3, point: Vec3, h: &Handling) -> Vec3 {
        let distance = point - self.center_of_mass_world_offset(h);
        let delta = distance.cross(force) * (1.0 / h.turn_mass.max(1e-12));
        self.turn_speed = self.turn_speed + delta;
        delta
    }

    /// Both halves of a force at a point (`CPhysical::ApplyForce`).
    pub fn apply_force(&mut self, force: Vec3, point: Vec3, h: &Handling) -> (Vec3, Vec3) {
        let linear = self.apply_move_force(force, h);
        let angular = self.apply_turn_force(force, point, h);
        (linear, angular)
    }

    /// Friction variant: accumulates instead of applying immediately.
    pub fn apply_friction_force(&mut self, force: Vec3, point: Vec3, h: &Handling) -> (Vec3, Vec3) {
        let linear = force * h.inverse_mass();
        let distance = point - self.center_of_mass_world_offset(h);
        let angular = distance.cross(force) * (1.0 / h.turn_mass.max(1e-12));
        self.friction_move_speed = self.friction_move_speed + linear;
        self.friction_turn_speed = self.friction_turn_speed + angular;
        (linear, angular)
    }

    /// `CPhysical::ApplyFriction` — fold the accumulators in, then clear.
    pub fn apply_friction(&mut self) {
        self.move_speed = self.move_speed + self.friction_move_speed;
        self.turn_speed = self.turn_speed + self.friction_turn_speed;
        self.friction_move_speed = ZERO;
        self.friction_turn_speed = ZERO;
    }

    /// `ApplyMoveSpeed` + `ApplyTurnSpeed` for one world pass.
    ///
    /// `reorthogonalize` exists only for diagnostics: the game always does it.
    pub fn integrate(&mut self, ctimer_step: f32, h: &Handling, reorthogonalize: bool) {
        self.position = self.position + self.move_speed * ctimer_step;
        let turn_step = self.turn_speed * ctimer_step;
        let rotated = self.basis.rotate_by(turn_step);
        let negative_com = -self.center_of_mass_world_offset(h);
        self.position = self.position + turn_step.cross(negative_com);
        self.basis = if reorthogonalize {
            rotated.reorthogonalise()
        } else {
            rotated
        };
    }

    /// Apply an immediate rotation step without moving (used by the collision
    /// phase, which rotates the hull by this tick's impulse torque so contact
    /// attitude shows up on the same frame).
    pub fn rotate_basis_by(&mut self, step: Vec3) {
        self.basis = self.basis.rotate_by(step).reorthogonalise();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn force_splits_between_linear_and_angular() {
        let h = Handling::default();
        let mut b = Body::default();
        let point = Vec3::new(0.0, 1.0, 0.0);
        let force = Vec3::new(1.0, 0.0, 0.0);
        let (lin, ang) = b.apply_force(force, point, &h);
        assert!((lin.x - 1.0 / 1400.0).abs() < 1e-9);
        // r x F with r = (0,1,0) - com(0,0,-0.25) = (0,1,0.25)
        // (0,1,0.25) x (1,0,0) = (1*0 - 0.25*0, 0.25*1 - 0*0, 0*0 - 1*1) = (0, 0.25, -1)
        assert!((ang.y - 0.25 / 2725.3).abs() < 1e-9);
        assert!((ang.z + 1.0 / 2725.3).abs() < 1e-9);
    }

    #[test]
    fn effective_mass_is_bounded_by_mass() {
        let h = Handling::default();
        let b = Body::default();
        let straight_down = Vec3::new(0.0, 0.0, -1.0);
        // Through the COM: pure linear, so exactly the mass.
        let m = b.contact_effective_mass(Vec3::new(0.0, 0.0, -0.25), straight_down, &h);
        assert!((m - 1400.0).abs() < 1e-2, "{m}");
        // Off the COM: lower effective mass.
        let m2 = b.contact_effective_mass(Vec3::new(1.0, 0.0, 0.0), straight_down, &h);
        assert!(m2 < m);
    }

    #[test]
    fn friction_accumulates_then_applies() {
        let h = Handling::default();
        let mut b = Body::default();
        b.apply_friction_force(Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), &h);
        assert_eq!(b.move_speed, ZERO);
        assert!(b.friction_move_speed.x > 0.0);
        b.apply_friction();
        assert!(b.move_speed.x > 0.0);
        assert_eq!(b.friction_move_speed, ZERO);
    }

    #[test]
    fn integration_keeps_basis_orthonormal() {
        let h = Handling::default();
        let mut b = Body {
            turn_speed: Vec3::new(0.01, 0.02, 0.03),
            ..Body::default()
        };
        for _ in 0..2000 {
            b.integrate(0.5, &h, true);
        }
        assert!((b.basis.up.length() - 1.0).abs() < 1e-4);
        assert!(b.basis.right.dot(b.basis.up).abs() < 1e-3);
    }
}
