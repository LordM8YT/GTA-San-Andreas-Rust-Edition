//! Contact inputs: what the world hands the vehicle each tick.
//!
//! This crate does not generate contacts. GTA does it in `CWorld` /
//! `CCollision` (line tests against the sector list for wheels, sphere/triangle
//! narrowphase for the body), and that machinery is a project of its own. What
//! matters for faithfulness is the *shape* of the data and the two conventions
//! below, because getting them wrong changes every force:
//!
//! * A wheel contact point is stored **model-relative** (native:
//!   `m_wheelColPoint.m_vecPoint - GetPosition()`), and `ProcessWheel` passes
//!   it to `GetMass` without subtracting the centre of mass.
//! * A body contact point is a **world-frame offset from the model origin**
//!   (native: `worldHit - position`), and the collision helpers subtract the COM
//!   themselves.
//!
//! A wheel that misses must still be reported, with `line_fraction = 1.0`, so
//! that the four-tick wheel-count persistence (`m_WheelCounts`) can be
//! evaluated separately from geometry. Reporting only hits makes cars fall
//! through the road on the tick after a lip.

use crate::math::{Vec3, ZERO};

/// One wheel's collision-line result, in native wheel order.
#[derive(Clone, Debug)]
pub struct WheelContact {
    pub index: usize,
    /// Model-relative contact point.
    pub point: Vec3,
    pub normal: Vec3,
    /// Pre-normalisation fraction along the suspension line: 0 at the top,
    /// 1 at the bottom (fully compressed).
    pub line_fraction: f32,
    /// Normalised compression (`0.0` fully extended, `1.0` fully compressed)
    /// when the caller already knows it. GTA derives it from `line_fraction`
    /// using its own suspension-line geometry; a caller with a mesh world can
    /// measure it directly and set this instead.
    pub compression: Option<f32>,
    /// False when the line missed; `line_fraction` is then 1.0.
    pub geometric_hit: bool,
    /// Surface adhesive limit from the map's material, if known.
    pub surface_adhesive_limit: Option<f32>,
    pub surface_material: Option<i32>,
    /// Velocity of a moving surface/entity at the contact point.
    pub relative_velocity: Vec3,
}

impl WheelContact {
    pub fn hit(index: usize, point: Vec3, normal: Vec3, line_fraction: f32) -> Self {
        Self {
            index,
            point,
            normal,
            line_fraction,
            compression: None,
            geometric_hit: true,
            surface_adhesive_limit: None,
            surface_material: None,
            relative_velocity: ZERO,
        }
    }

    /// A contact whose compression is already known, in the caller's units.
    pub fn compressed(index: usize, point: Vec3, normal: Vec3, compression: f32) -> Self {
        let mut contact = Self::hit(index, point, normal, compression);
        contact.compression = Some(compression.clamp(0.0, 1.0));
        contact
    }

    pub fn miss(index: usize) -> Self {
        Self {
            index,
            point: ZERO,
            normal: Vec3::new(0.0, 0.0, 1.0),
            line_fraction: 1.0,
            compression: None,
            geometric_hit: false,
            surface_adhesive_limit: None,
            surface_material: None,
            relative_velocity: ZERO,
        }
    }
}

/// Four suspension lines against an infinite plane at `plane_z`.
///
/// This is the shape a flat world hands the kernel, built from the handling's
/// own line geometry exactly as `CAutomobile::SetupSuspensionLines` places it.
/// Tests and replays that have no map use it so their contacts are the same
/// ones a flat road would produce; a real world raycasts its own triangles.
pub fn plane_contacts(
    handling: &crate::handling::Handling,
    position: Vec3,
    basis: crate::math::Basis,
    plane_z: f32,
) -> WheelContactSet {
    let half_track = handling.wheel_track * 0.5;
    // Native order: front-left, rear-left, front-right, rear-right.
    let layout = [
        (-half_track, handling.wheel_front_y),
        (-half_track, handling.wheel_rear_y),
        (half_track, handling.wheel_front_y),
        (half_track, handling.wheel_rear_y),
    ];
    let mut wheels: [Option<WheelContact>; 4] = Default::default();
    for (index, (x, y)) in layout.iter().enumerate() {
        let (start, end) = handling.suspension_line(*x, *y);
        let world_start = position + basis.transform_local(start);
        let world_end = position + basis.transform_local(end);
        let drop = world_start.z - world_end.z;
        if drop.abs() <= 1.0e-6 {
            wheels[index] = Some(WheelContact::miss(index));
            continue;
        }
        // The line runs from the mount to the line end; the plane is crossed
        // where z(lambda) = plane_z.
        let fraction = (world_start.z - plane_z) / drop;
        if !(0.0..=1.0).contains(&fraction) {
            wheels[index] = Some(WheelContact::miss(index));
            continue;
        }
        let hit = world_start + (world_end - world_start) * fraction;
        let local = basis.transform_world_to_local(hit - position);
        wheels[index] = Some(WheelContact::hit(
            index,
            local,
            Vec3::new(0.0, 0.0, 1.0),
            fraction,
        ));
    }
    WheelContactSet { wheels }
}

/// A body contact from the map or another entity.
#[derive(Clone, Debug)]
pub struct BodyContact {
    /// World-frame offset from the model origin (`world_hit - position`).
    pub point: Vec3,
    pub normal: Vec3,
    pub elasticity: f32,
    pub friction: f32,
    pub depth: f32,
    pub surface_material: Option<i32>,
    /// Set for moving entities; static map geometry leaves it false.
    pub entity_is_vehicle: bool,
    pub entity_linear_velocity: Vec3,
    pub entity_angular_velocity: Vec3,
    pub entity_center_of_mass_offset: Vec3,
    pub entity_mass: f32,
    pub entity_turn_mass: f32,
}

impl Default for BodyContact {
    fn default() -> Self {
        Self {
            point: ZERO,
            normal: Vec3::new(0.0, 0.0, 1.0),
            elasticity: 0.0,
            friction: 0.0,
            depth: 0.0,
            surface_material: None,
            entity_is_vehicle: false,
            entity_linear_velocity: ZERO,
            entity_angular_velocity: ZERO,
            entity_center_of_mass_offset: ZERO,
            entity_mass: 99999.0,
            entity_turn_mass: 99999.0,
        }
    }
}

/// A complete set of four wheel contacts for one tick.
#[derive(Clone, Debug, Default)]
pub struct WheelContactSet {
    pub wheels: [Option<WheelContact>; 4],
}

impl WheelContactSet {
    pub fn from_slice(contacts: &[WheelContact]) -> Self {
        let mut set = Self::default();
        for c in contacts {
            if c.index < 4 {
                set.wheels[c.index] = Some(c.clone());
            }
        }
        set
    }

    pub fn get(&self, index: usize) -> Option<&WheelContact> {
        self.wheels.get(index).and_then(|c| c.as_ref())
    }
}
