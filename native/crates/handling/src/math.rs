//! Vectors and the `CMatrix` basis, with GTA:SA's exact re-orthonormalisation.
//!
//! GTA's `CMatrix` stores three orthogonal unit vectors — right, forward, up —
//! and rotates them by *adding a cross product* rather than composing a
//! rotation matrix. Two consequences that ports get wrong:
//!
//! 1. `CMatrix::Reorthogonalise` (0x59B6A0) does **not** Gram-Schmidt the
//!    existing right vector. It rebuilds `up = right × forward`,
//!    `right = forward × up`, `forward = up × right`. That order is observable
//!    after `ApplyTurnSpeed` and must be kept ([`Basis::reorthogonalise`]).
//! 2. A rotation step is `v' = v + ω × v`, never `R(ω) * v`. For small steps
//!    they agree; over thousands of ticks they do not.

use std::ops::{Add, Div, Mul, Neg, Sub};

pub type Real = f32;

pub const ZERO: Vec3 = Vec3::new(0.0, 0.0, 0.0);
pub const GTA_UP: Vec3 = Vec3::new(0.0, 0.0, 1.0);
pub const GTA_FORWARD: Vec3 = Vec3::new(0.0, 1.0, 0.0);
pub const GTA_RIGHT: Vec3 = Vec3::new(1.0, 0.0, 0.0);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    pub x: Real,
    pub y: Real,
    pub z: Real,
}

impl Vec3 {
    pub const fn new(x: Real, y: Real, z: Real) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, o: Vec3) -> Real {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Vec3) -> Vec3 {
        Vec3::new(
            self.y * o.z - self.z * o.y,
            self.z * o.x - self.x * o.z,
            self.x * o.y - self.y * o.x,
        )
    }

    pub fn length(self) -> Real {
        self.length_squared().sqrt()
    }

    pub fn length_squared(self) -> Real {
        self.dot(self)
    }

    /// Normalise, returning `fallback` for a zero/non-finite vector.
    ///
    /// The native code has no guard and would emit NaN; the fallback keeps a
    /// degenerate contact from poisoning the whole body.
    pub fn normalized(self, fallback: Vec3) -> Vec3 {
        let len = self.length();
        if len > 1e-12 && len.is_finite() {
            self * (1.0 / len)
        } else {
            fallback
        }
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }

    pub fn abs(self) -> Vec3 {
        Vec3::new(self.x.abs(), self.y.abs(), self.z.abs())
    }

    pub fn lerp(self, o: Vec3, t: Real) -> Vec3 {
        self * (1.0 - t) + o * t
    }

    pub fn planar_length(self) -> Real {
        (self.x * self.x + self.y * self.y).sqrt()
    }
}

impl Add for Vec3 {
    type Output = Vec3;
    fn add(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl Sub for Vec3 {
    type Output = Vec3;
    fn sub(self, o: Vec3) -> Vec3 {
        Vec3::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl Mul<Real> for Vec3 {
    type Output = Vec3;
    fn mul(self, s: Real) -> Vec3 {
        Vec3::new(self.x * s, self.y * s, self.z * s)
    }
}

impl Div<Real> for Vec3 {
    type Output = Vec3;
    fn div(self, s: Real) -> Vec3 {
        Vec3::new(self.x / s, self.y / s, self.z / s)
    }
}

impl Neg for Vec3 {
    type Output = Vec3;
    fn neg(self) -> Vec3 {
        Vec3::new(-self.x, -self.y, -self.z)
    }
}

/// `CMatrix` basis vectors in GTA order: right, forward, up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Basis {
    pub right: Vec3,
    pub forward: Vec3,
    pub up: Vec3,
}

impl Default for Basis {
    fn default() -> Self {
        Self {
            right: GTA_RIGHT,
            forward: GTA_FORWARD,
            up: GTA_UP,
        }
    }
}

impl Basis {
    pub fn new(right: Vec3, forward: Vec3, up: Vec3) -> Self {
        Self {
            right,
            forward,
            up,
        }
    }

    /// Build from a row-major 3x3 matrix whose *columns* are the basis vectors.
    pub fn from_rows(m: [[Real; 3]; 3]) -> Self {
        Self::new(
            Vec3::new(m[0][0], m[1][0], m[2][0]),
            Vec3::new(m[0][1], m[1][1], m[2][1]),
            Vec3::new(m[0][2], m[1][2], m[2][2]),
        )
    }

    /// Local -> world. Same multiply/add grouping as the scalar helper so a
    /// transliterated port produces identical rounding.
    pub fn transform_local(self, local: Vec3) -> Vec3 {
        Vec3::new(
            (self.right.x * local.x + self.forward.x * local.y) + self.up.x * local.z,
            (self.right.y * local.x + self.forward.y * local.y) + self.up.y * local.z,
            (self.right.z * local.x + self.forward.z * local.y) + self.up.z * local.z,
        )
    }

    /// World -> local. Only valid for an orthonormal basis (which
    /// [`Basis::reorthogonalise`] guarantees).
    pub fn transform_world_to_local(self, world: Vec3) -> Vec3 {
        Vec3::new(
            self.right.dot(world),
            self.forward.dot(world),
            self.up.dot(world),
        )
    }

    /// `CMatrix::Reorthogonalise`, 0x59B6A0 — see the module docs for why the
    /// order matters.
    pub fn reorthogonalise(self) -> Self {
        let up = self.right.cross(self.forward).normalized(GTA_UP);
        let right = self.forward.cross(up).normalized(GTA_RIGHT);
        let forward = up.cross(right);
        Self {
            right,
            forward,
            up,
        }
    }

    /// Rotate the basis by one angular step: `v' = v + ω × v` for each axis.
    /// This is what `CPhysical::ApplyTurnSpeed` (0x542E20) does.
    pub fn rotate_by(self, turn_step: Vec3) -> Self {
        Self {
            right: self.right + turn_step.cross(self.right),
            forward: self.forward + turn_step.cross(self.forward),
            up: self.up + turn_step.cross(self.up),
        }
    }

    /// Column-major 3x3 matrix, for handing the pose to a renderer.
    pub fn to_cols(self) -> [[Real; 3]; 3] {
        [
            [self.right.x, self.right.y, self.right.z],
            [self.forward.x, self.forward.y, self.forward.z],
            [self.up.x, self.up.y, self.up.z],
        ]
    }
}

/// GTA's clamping helper (`std::clamp` in the source).
pub fn clamp(v: Real, lo: Real, hi: Real) -> Real {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// Wrap an angle into `(-pi, pi]`. Used everywhere a heading error is taken;
/// forgetting it makes long spins jump 2π.
pub fn normalize_angle(a: Real) -> Real {
    let two_pi = std::f32::consts::TAU;
    let mut v = a;
    while v > std::f32::consts::PI {
        v -= two_pi;
    }
    while v <= -std::f32::consts::PI {
        v += two_pi;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorthogonalise_uses_source_order() {
        // A basis deliberately skewed off-orthogonal by a rotation step.
        let b = Basis::default().rotate_by(Vec3::new(0.0, 0.0, 0.05));
        let r = b.reorthogonalise();
        assert!((r.right.length() - 1.0).abs() < 1e-5);
        assert!((r.forward.length() - 1.0).abs() < 1e-5);
        assert!((r.up.length() - 1.0).abs() < 1e-5);
        assert!(r.right.dot(r.forward).abs() < 1e-5);
        assert!(r.right.dot(r.up).abs() < 1e-5);
        assert!(r.forward.dot(r.up).abs() < 1e-5);
        // up is rebuilt from right x forward, so it is exactly parallel to it.
        let expected_up = b.right.cross(b.forward).normalized(GTA_UP);
        assert!((r.up - expected_up).length() < 1e-5);
    }

    #[test]
    fn transform_round_trip() {
        let b = Basis::default().rotate_by(Vec3::new(0.01, 0.02, 0.03)).reorthogonalise();
        let local = Vec3::new(1.0, -2.0, 0.5);
        let back = b.transform_world_to_local(b.transform_local(local));
        assert!((back - local).length() < 1e-4);
    }

    #[test]
    fn angle_wrap() {
        assert!((normalize_angle(3.0 * std::f32::consts::PI)).abs() - std::f32::consts::PI < 1e-4);
        assert!(normalize_angle(-0.5) < 0.0);
    }
}
